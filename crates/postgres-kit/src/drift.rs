//! Drift gate (feature `drift`): compare a set of expected [`PgTableSpec`]s
//! against the live schema introspected via [`crate::PgExecutor`]. Read-only — it
//! reports divergence, it never mutates. Intended to run in CI to catch a
//! deployed schema that no longer matches the code's model. Mirrors the
//! clickhouse-kit drift gate.
//!
//! Columns (and their types / nullability) come from [`PgExecutor::fetch_columns`],
//! which a consumer backs with a `pg_catalog` / `information_schema` query. The
//! `data_type` text is expected to be `format_type()`-style (e.g. `uuid`,
//! `integer`, `character varying(255)`, `text[]`, or a user-defined enum's type
//! name); [`canonical_pg_type`] normalizes spelling synonyms so
//! `character varying` and `varchar`, or `timestamp with time zone` and
//! `timestamptz`, compare equal.
//!
//! Foreign keys, indexes, policies, primary keys, unique constraints, and check
//! constraints are checked best-effort via [`PgExecutor::fetch_strings`] against
//! `pg_constraint` / `pg_index` / `pg_policies`, matched by **definition, not
//! name**: a FK by `(from-cols, referenced table, to-cols, on-delete/on-update)`,
//! an index by `(columns/expressions, unique, predicate, method)`, a policy by
//! `(command, roles, using, with-check)`, a unique constraint by
//! `(column set, NULLS NOT DISTINCT)`, a check by normalized expression. This
//! eliminates cosmetic-rename false positives (legacy names, 63-byte truncation,
//! Postgres' default `_fkey` naming). Only *missing* definitions are reported —
//! Postgres creates many implicit objects, so the gate never flags "extra" ones.
//! Enum types are compared separately via [`check_enum_drift`] (by name + value
//! set).

use std::collections::{BTreeMap, BTreeSet};

use crate::client::{PgError, PgExecutor};
use crate::safety::{validate_identifier, SchemaLimits};
use crate::spec::PgTableSpec;

/// A single schema divergence between the expected spec and the live database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    /// The table is expected but does not exist (no live columns).
    MissingTable { table: String },
    /// A column is in the spec but missing from the live table.
    MissingColumn {
        table: String,
        column: String,
        expected_type: String,
    },
    /// A column exists live but is not in the spec.
    ExtraColumn {
        table: String,
        column: String,
        actual_type: String,
    },
    /// A column exists on both sides with a different type.
    TypeMismatch {
        table: String,
        column: String,
        expected_type: String,
        actual_type: String,
    },
    /// A column exists on both sides but its nullability diverges.
    NullabilityMismatch {
        table: String,
        column: String,
        expected_nullable: bool,
        actual_nullable: bool,
    },
    /// An index whose *definition* (columns/expressions, uniqueness, method,
    /// predicate) is in the spec but has no match in the live table. The `index`
    /// field carries the spec's declared name for reporting only — matching is
    /// name-agnostic, so a renamed-but-identical index is **not** flagged.
    MissingIndex { table: String, index: String },
    /// A foreign key whose *definition* (from-columns, referenced table,
    /// to-columns, on-delete/on-update actions) is in the spec but has no match
    /// in the live table. `constraint` is the spec's declared name (reporting
    /// only — matching is name-agnostic).
    MissingForeignKey { table: String, constraint: String },
    /// A row-level-security policy whose *definition* (command, roles, using,
    /// with-check) is in the spec but has no match in the live table. `policy`
    /// is the spec's declared name (reporting only — matching is name-agnostic).
    MissingPolicy { table: String, policy: String },
    /// The primary-key column set diverges between the spec and the live table.
    /// Compared as an unordered column set; only checked when the spec declares
    /// a primary key.
    PrimaryKeyMismatch {
        table: String,
        expected_columns: Vec<String>,
        actual_columns: Vec<String>,
    },
    /// A unique constraint whose *definition* (column set + `NULLS NOT DISTINCT`)
    /// is in the spec but has no match in the live table (name-agnostic).
    MissingUniqueConstraint {
        table: String,
        columns: Vec<String>,
        nulls_not_distinct: bool,
    },
    /// A check constraint whose *normalized expression* is in the spec but has no
    /// match in the live table (name-agnostic).
    MissingCheckConstraint { table: String, expression: String },
    /// An enum type is in the spec but does not exist in the live database.
    MissingEnumType { enum_type: String },
    /// An enum type exists on both sides but its value set diverges (compared as
    /// an unordered set).
    EnumValuesMismatch {
        enum_type: String,
        expected_values: Vec<String>,
        actual_values: Vec<String>,
    },
}

/// Result of a [`check_drift`] pass.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DriftResult {
    /// Schema-qualified names of every table that was checked.
    pub checked: Vec<String>,
    /// All divergences found (empty == schema matches).
    pub drift: Vec<Drift>,
}

impl DriftResult {
    /// Whether the live schema matches every expected spec.
    pub fn is_clean(&self) -> bool {
        self.drift.is_empty()
    }
}

/// Normalize a Postgres type string to a canonical token so spelling synonyms
/// compare equal: `boolean`→`bool`, `integer`→`int4`, `character varying`→
/// `varchar`, `timestamp with time zone`→`timestamptz`, etc. A parenthesized
/// modifier (length/precision) is preserved with its inner whitespace stripped,
/// and an `[]` array suffix is preserved.
pub fn canonical_pg_type(raw: &str) -> String {
    let lower = raw.trim().to_ascii_lowercase();
    let is_array = lower.ends_with("[]");
    let no_array = lower.trim_end_matches("[]").trim();

    // Split off a trailing parenthesized modifier, e.g. `varchar(255)` or
    // `numeric(10, 2)`, normalizing whitespace inside it.
    let (base, modifier) = match no_array.find('(') {
        Some(i) => {
            let m: String = no_array[i..]
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            (no_array[..i].trim(), Some(m))
        }
        None => (no_array, None),
    };

    let canonical_base = match base {
        "bool" | "boolean" => "bool",
        "int2" | "smallint" => "int2",
        "int4" | "int" | "integer" => "int4",
        "int8" | "bigint" => "int8",
        "float4" | "real" => "float4",
        "float8" | "double precision" => "float8",
        "varchar" | "character varying" => "varchar",
        "bpchar" | "char" | "character" => "char",
        "numeric" | "decimal" => "numeric",
        "timestamptz" | "timestamp with time zone" => "timestamptz",
        "timestamp" | "timestamp without time zone" => "timestamp",
        "timetz" | "time with time zone" => "timetz",
        "time" | "time without time zone" => "time",
        "tsvector" => "tsvector",
        "vector" => "vector",
        other => other,
    };

    let mut out = String::with_capacity(canonical_base.len() + 4);
    out.push_str(canonical_base);
    if let Some(m) = modifier {
        out.push_str(&m);
    }
    if is_array {
        out.push_str("[]");
    }
    out
}

/// Escape a value for embedding as a SQL string literal (doubles single quotes).
/// Identifiers placed here are already validated, so this is defense-in-depth.
fn escape_literal(value: &str) -> String {
    value.replace('\'', "''")
}

/// Normalize a definition "signature" for name-agnostic comparison: lowercase,
/// and strip all whitespace and parentheses. Stripping parentheses lets a
/// spec-side predicate/expression (`deleted_at IS NULL`, `lower(email)`) compare
/// equal to the canonical, paren-wrapped form Postgres echoes back from
/// `pg_get_expr` / `pg_get_indexdef` (`(deleted_at IS NULL)`, `lower(email)`).
fn normalize_sig(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '(' && *c != ')')
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The `pg_constraint.confdeltype` / `confupdtype` single-char code for a
/// referential action. A missing action defaults to `NO ACTION` (`'a'`), matching
/// Postgres' own default.
fn ref_action_char(action: Option<crate::spec::ReferentialAction>) -> char {
    use crate::spec::ReferentialAction::*;
    match action {
        None | Some(NoAction) => 'a',
        Some(Restrict) => 'r',
        Some(Cascade) => 'c',
        Some(SetNull) => 'n',
        Some(SetDefault) => 'd',
    }
}

/// Qualify a foreign-key target table to `schema.table`, defaulting the schema
/// to `public`, so it lines up with the `nspname.relname` the catalog reports.
fn qualify_ref_table(table: &str) -> String {
    if table.contains('.') {
        table.to_string()
    } else {
        format!("public.{table}")
    }
}

/// Name-agnostic signature for a foreign key:
/// `from_cols | schema.reftable | to_cols | on_delete | on_update`.
fn fk_signature_spec(fk: &crate::spec::ForeignKeySpec) -> String {
    normalize_sig(&format!(
        "{}|{}|{}|{}|{}",
        fk.columns_from.join(","),
        qualify_ref_table(&fk.table_to),
        fk.columns_to.join(","),
        ref_action_char(fk.on_delete),
        ref_action_char(fk.on_update),
    ))
}

/// Name-agnostic signature for an index:
/// `unique | method | predicate | col1,col2,…`. Each column renders its
/// expression, optional opclass, `DESC`, and `NULLS …` to match
/// `pg_get_indexdef`'s per-column output after [`normalize_sig`].
fn index_signature_spec(idx: &crate::spec::IndexSpec) -> String {
    let cols = idx
        .columns
        .iter()
        .map(|c| {
            let mut t = c.expression.clone();
            if let Some(op) = &c.opclass {
                t.push(' ');
                t.push_str(op);
            }
            if !c.asc {
                t.push_str(" DESC");
            }
            if let Some(n) = &c.nulls {
                t.push_str(" NULLS ");
                t.push_str(n);
            }
            t
        })
        .collect::<Vec<_>>()
        .join(",");
    normalize_sig(&format!(
        "{}|{}|{}|{}",
        idx.unique,
        idx.method,
        idx.where_clause.as_deref().unwrap_or(""),
        cols,
    ))
}

/// The `pg_policies.cmd` token (`all`/`select`/…) for a spec policy command.
/// A missing command defaults to `ALL`, matching Postgres.
fn policy_for_text(for_: Option<crate::spec::PolicyFor>) -> &'static str {
    use crate::spec::PolicyFor::*;
    match for_ {
        None | Some(All) => "all",
        Some(Select) => "select",
        Some(Insert) => "insert",
        Some(Update) => "update",
        Some(Delete) => "delete",
    }
}

/// Name-agnostic signature for a policy: `cmd | roles | using | with_check`.
/// Roles default to `public` (an empty `TO` list) and are sorted so order does
/// not matter.
fn policy_signature_spec(p: &crate::spec::PolicySpec) -> String {
    let mut roles = if p.to.is_empty() {
        vec!["public".to_string()]
    } else {
        p.to.clone()
    };
    roles.sort();
    normalize_sig(&format!(
        "{}|{}|{}|{}",
        policy_for_text(p.for_),
        roles.join(","),
        p.using.as_deref().unwrap_or(""),
        p.with_check.as_deref().unwrap_or(""),
    ))
}

/// Name-agnostic signature for a unique constraint: `sorted_cols | nulls_not_distinct`.
fn unique_signature(columns: &[String], nulls_not_distinct: bool) -> String {
    let mut cols = columns.to_vec();
    cols.sort();
    normalize_sig(&format!("{}|{}", cols.join(","), nulls_not_distinct))
}

/// For each expected table, introspect its live shape and report any drift:
/// missing table, missing/extra column, type mismatch, nullability mismatch,
/// and best-effort missing index / foreign key / policy.
pub async fn check_drift(
    exec: &impl PgExecutor,
    tables: &[PgTableSpec],
) -> Result<DriftResult, PgError> {
    let limits = SchemaLimits::default();
    let mut checked = Vec::with_capacity(tables.len());
    let mut drift = Vec::new();

    for table in tables {
        let qualified = table.qualified_name();
        checked.push(qualified.clone());

        // Validate the names we are about to embed in catalog queries.
        validate_identifier(&table.schema, "schema", &limits)
            .map_err(|e| PgError::Backend(e.to_string()))?;
        validate_identifier(&table.name, "table", &limits)
            .map_err(|e| PgError::Backend(e.to_string()))?;

        let live = exec.fetch_columns(&qualified).await?;
        if live.is_empty() {
            drift.push(Drift::MissingTable {
                table: qualified.clone(),
            });
            continue;
        }

        let live_by_name: BTreeMap<&str, &crate::client::LiveColumn> =
            live.iter().map(|c| (c.name.as_str(), c)).collect();
        let expected_names: BTreeSet<&str> =
            table.columns.iter().map(|c| c.name.as_str()).collect();

        // Expected columns: present-and-matching, wrong-type, wrong-nullability,
        // or missing.
        for col in &table.columns {
            let expected_type = col.ty.to_sql_type();
            match live_by_name.get(col.name.as_str()) {
                None => drift.push(Drift::MissingColumn {
                    table: qualified.clone(),
                    column: col.name.clone(),
                    expected_type,
                }),
                Some(live_col) => {
                    if canonical_pg_type(&live_col.data_type) != canonical_pg_type(&expected_type) {
                        drift.push(Drift::TypeMismatch {
                            table: qualified.clone(),
                            column: col.name.clone(),
                            expected_type,
                            actual_type: live_col.data_type.clone(),
                        });
                    } else if live_col.is_nullable != col.nullable {
                        drift.push(Drift::NullabilityMismatch {
                            table: qualified.clone(),
                            column: col.name.clone(),
                            expected_nullable: col.nullable,
                            actual_nullable: live_col.is_nullable,
                        });
                    }
                }
            }
        }

        // Live columns not in the spec.
        for col in &live {
            if !expected_names.contains(col.name.as_str()) {
                drift.push(Drift::ExtraColumn {
                    table: qualified.clone(),
                    column: col.name.clone(),
                    actual_type: col.data_type.clone(),
                });
            }
        }

        let sch = escape_literal(&table.schema);
        let tbl = escape_literal(&table.name);

        // ── Foreign keys — matched by DEFINITION, not name ──────────────────
        if !table.foreign_keys.is_empty() {
            // `string_agg` over the FK / referenced columns (ordered) yields a
            // structured signature per constraint, independent of `conname`.
            let sql = format!(
                "SELECT \
                   (SELECT string_agg(a.attname, ',' ORDER BY k.ord) \
                      FROM unnest(c.conkey) WITH ORDINALITY AS k(attnum, ord) \
                      JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = k.attnum) \
                   || '|' || rn.nspname || '.' || rt.relname || '|' || \
                   (SELECT string_agg(a.attname, ',' ORDER BY k.ord) \
                      FROM unnest(c.confkey) WITH ORDINALITY AS k(attnum, ord) \
                      JOIN pg_attribute a ON a.attrelid = c.confrelid AND a.attnum = k.attnum) \
                   || '|' || c.confdeltype::text || '|' || c.confupdtype::text \
                 FROM pg_constraint c \
                 JOIN pg_class t ON c.conrelid = t.oid \
                 JOIN pg_namespace n ON t.relnamespace = n.oid \
                 JOIN pg_class rt ON c.confrelid = rt.oid \
                 JOIN pg_namespace rn ON rt.relnamespace = rn.oid \
                 WHERE c.contype = 'f' AND n.nspname = '{sch}' AND t.relname = '{tbl}'",
            );
            let live: BTreeSet<String> = exec
                .fetch_strings(&sql)
                .await?
                .iter()
                .map(|s| normalize_sig(s))
                .collect();
            for fk in &table.foreign_keys {
                if !live.contains(&fk_signature_spec(fk)) {
                    drift.push(Drift::MissingForeignKey {
                        table: qualified.clone(),
                        constraint: fk.name.clone(),
                    });
                }
            }
        }

        // ── Indexes — matched by DEFINITION, not name ───────────────────────
        if !table.indexes.is_empty() {
            // `pg_get_indexdef(idx, col, true)` renders each key column/expression
            // canonically; combined with uniqueness, access method, and predicate
            // it is a name-agnostic signature. Primary-key indexes are excluded
            // (handled separately).
            let sql = format!(
                "SELECT i.indisunique::text || '|' || am.amname || '|' || \
                   coalesce(pg_get_expr(i.indpred, i.indrelid), '') || '|' || \
                   (SELECT string_agg(pg_get_indexdef(i.indexrelid, k.n, true), ',' ORDER BY k.n) \
                      FROM generate_series(1, i.indnkeyatts) AS k(n)) \
                 FROM pg_index i \
                 JOIN pg_class ic ON ic.oid = i.indexrelid \
                 JOIN pg_class tc ON tc.oid = i.indrelid \
                 JOIN pg_namespace n ON tc.relnamespace = n.oid \
                 JOIN pg_am am ON am.oid = ic.relam \
                 WHERE n.nspname = '{sch}' AND tc.relname = '{tbl}' AND NOT i.indisprimary",
            );
            let live: BTreeSet<String> = exec
                .fetch_strings(&sql)
                .await?
                .iter()
                .map(|s| normalize_sig(s))
                .collect();
            for idx in &table.indexes {
                if !live.contains(&index_signature_spec(idx)) {
                    drift.push(Drift::MissingIndex {
                        table: qualified.clone(),
                        index: idx.name.clone(),
                    });
                }
            }
        }

        // ── Policies — matched by DEFINITION, not name ──────────────────────
        if !table.policies.is_empty() {
            let sql = format!(
                "SELECT lower(cmd) || '|' || \
                   array_to_string(ARRAY(SELECT unnest(roles) ORDER BY 1), ',') || '|' || \
                   coalesce(qual, '') || '|' || coalesce(with_check, '') \
                 FROM pg_policies WHERE schemaname = '{sch}' AND tablename = '{tbl}'",
            );
            let live: BTreeSet<String> = exec
                .fetch_strings(&sql)
                .await?
                .iter()
                .map(|s| normalize_sig(s))
                .collect();
            for policy in &table.policies {
                if !live.contains(&policy_signature_spec(policy)) {
                    drift.push(Drift::MissingPolicy {
                        table: qualified.clone(),
                        policy: policy.name.clone(),
                    });
                }
            }
        }

        // ── Primary key — compared as an unordered column set ───────────────
        if !table.primary_key.is_empty() {
            let sql = format!(
                "SELECT a.attname \
                 FROM pg_constraint c \
                 JOIN pg_class t ON c.conrelid = t.oid \
                 JOIN pg_namespace n ON t.relnamespace = n.oid \
                 JOIN unnest(c.conkey) WITH ORDINALITY AS k(attnum, ord) ON true \
                 JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = k.attnum \
                 WHERE c.contype = 'p' AND n.nspname = '{sch}' AND t.relname = '{tbl}' \
                 ORDER BY k.ord",
            );
            let actual = exec.fetch_strings(&sql).await?;
            let expected_set: BTreeSet<&str> =
                table.primary_key.iter().map(|s| s.as_str()).collect();
            let actual_set: BTreeSet<&str> = actual.iter().map(|s| s.as_str()).collect();
            if expected_set != actual_set {
                drift.push(Drift::PrimaryKeyMismatch {
                    table: qualified.clone(),
                    expected_columns: table.primary_key.clone(),
                    actual_columns: actual,
                });
            }
        }

        // ── Unique constraints — matched by column set + NULLS NOT DISTINCT ──
        // Covers both table-level unique constraints and column-level `UNIQUE`.
        let mut expected_uniques: Vec<(Vec<String>, bool)> = Vec::new();
        for uc in &table.unique_constraints {
            expected_uniques.push((uc.columns.clone(), uc.nulls_not_distinct));
        }
        for col in &table.columns {
            if let Some(u) = &col.unique {
                expected_uniques.push((vec![col.name.clone()], u.nulls_not_distinct));
            }
        }
        if !expected_uniques.is_empty() {
            let sql = format!(
                "SELECT \
                   (SELECT string_agg(a.attname, ',' ORDER BY a.attname) \
                      FROM unnest(c.conkey) AS k(attnum) \
                      JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = k.attnum) \
                   || '|' || coalesce(i.indnullsnotdistinct, false)::text \
                 FROM pg_constraint c \
                 JOIN pg_class t ON c.conrelid = t.oid \
                 JOIN pg_namespace n ON t.relnamespace = n.oid \
                 LEFT JOIN pg_index i ON i.indexrelid = c.conindid \
                 WHERE c.contype = 'u' AND n.nspname = '{sch}' AND t.relname = '{tbl}'",
            );
            let live: BTreeSet<String> = exec
                .fetch_strings(&sql)
                .await?
                .iter()
                .map(|s| normalize_sig(s))
                .collect();
            for (cols, nnd) in &expected_uniques {
                if !live.contains(&unique_signature(cols, *nnd)) {
                    drift.push(Drift::MissingUniqueConstraint {
                        table: qualified.clone(),
                        columns: cols.clone(),
                        nulls_not_distinct: *nnd,
                    });
                }
            }
        }

        // ── Check constraints — matched by normalized expression ────────────
        if !table.check_constraints.is_empty() {
            let sql = format!(
                "SELECT pg_get_expr(c.conbin, c.conrelid) \
                 FROM pg_constraint c \
                 JOIN pg_class t ON c.conrelid = t.oid \
                 JOIN pg_namespace n ON t.relnamespace = n.oid \
                 WHERE c.contype = 'c' AND n.nspname = '{sch}' AND t.relname = '{tbl}'",
            );
            let live: BTreeSet<String> = exec
                .fetch_strings(&sql)
                .await?
                .iter()
                .map(|s| normalize_sig(s))
                .collect();
            for cc in &table.check_constraints {
                if !live.contains(&normalize_sig(&cc.value)) {
                    drift.push(Drift::MissingCheckConstraint {
                        table: qualified.clone(),
                        expression: cc.value.clone(),
                    });
                }
            }
        }
    }

    Ok(DriftResult { checked, drift })
}

/// Best-effort, name-agnostic drift check for user-defined enum types: each
/// expected [`EnumTypeSpec`] is compared against the live database by name and
/// value *set* (order-independent). Reports [`Drift::MissingEnumType`] when the
/// type does not exist and [`Drift::EnumValuesMismatch`] when the value sets
/// diverge. Kept a separate entry point from [`check_drift`] (which is
/// table-scoped) so callers opt in without a signature change.
pub async fn check_enum_drift(
    exec: &impl PgExecutor,
    enums: &[crate::spec::EnumTypeSpec],
) -> Result<Vec<Drift>, PgError> {
    let limits = SchemaLimits::default();
    let mut drift = Vec::new();

    for en in enums {
        validate_identifier(&en.schema, "schema", &limits)
            .map_err(|e| PgError::Backend(e.to_string()))?;
        validate_identifier(&en.name, "enum type", &limits)
            .map_err(|e| PgError::Backend(e.to_string()))?;

        let sql = format!(
            "SELECT e.enumlabel \
             FROM pg_enum e \
             JOIN pg_type ty ON ty.oid = e.enumtypid \
             JOIN pg_namespace n ON n.oid = ty.typnamespace \
             WHERE n.nspname = '{}' AND ty.typname = '{}' \
             ORDER BY e.enumsortorder",
            escape_literal(&en.schema),
            escape_literal(&en.name),
        );
        let actual = exec.fetch_strings(&sql).await?;
        if actual.is_empty() {
            drift.push(Drift::MissingEnumType {
                enum_type: en.qualified_name(),
            });
            continue;
        }
        let expected_set: BTreeSet<&str> = en.values.iter().map(|s| s.as_str()).collect();
        let actual_set: BTreeSet<&str> = actual.iter().map(|s| s.as_str()).collect();
        if expected_set != actual_set {
            drift.push(Drift::EnumValuesMismatch {
                enum_type: en.qualified_name(),
                expected_values: en.values.clone(),
                actual_values: actual,
            });
        }
    }

    Ok(drift)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::LiveColumn;
    use crate::spec::{
        ColumnSpec, ForeignKeySpec, IndexColumn, IndexSpec, PgType, PolicyFor, PolicySpec,
    };

    fn col(name: &str, ty: PgType) -> ColumnSpec {
        ColumnSpec::new(name, ty)
    }

    fn spec() -> PgTableSpec {
        PgTableSpec::new(
            "events",
            vec![col("id", PgType::Uuid), col("name", PgType::Text)],
        )
        .primary_key(["id"])
    }

    fn lc(name: &str, data_type: &str, is_nullable: bool) -> LiveColumn {
        LiveColumn {
            name: name.into(),
            data_type: data_type.into(),
            is_nullable,
        }
    }

    /// A canned executor: `fetch_columns` is keyed by qualified table name;
    /// `fetch_strings` routes by a token unique to each catalog query the drift
    /// gate emits. The string vecs hold *live signatures* (or raw column names for
    /// PK / enum), already in the shape the gate compares against.
    #[derive(Default)]
    struct FakeExec {
        columns: BTreeMap<String, Vec<LiveColumn>>,
        indexes: Vec<String>,
        fks: Vec<String>,
        policies: Vec<String>,
        pk: Vec<String>,
        uniques: Vec<String>,
        checks: Vec<String>,
        enums: Vec<String>,
    }

    impl PgExecutor for FakeExec {
        async fn command(&self, _sql: &str) -> Result<(), PgError> {
            Ok(())
        }
        async fn fetch_strings(&self, sql: &str) -> Result<Vec<String>, PgError> {
            // Order matters: the contype tokens are unambiguous, so test them
            // before falling back to the broader catalog-name checks.
            if sql.contains("pg_get_indexdef") {
                Ok(self.indexes.clone())
            } else if sql.contains("pg_policies") {
                Ok(self.policies.clone())
            } else if sql.contains("pg_enum") {
                Ok(self.enums.clone())
            } else if sql.contains("contype = 'f'") {
                Ok(self.fks.clone())
            } else if sql.contains("contype = 'p'") {
                Ok(self.pk.clone())
            } else if sql.contains("contype = 'u'") {
                Ok(self.uniques.clone())
            } else if sql.contains("contype = 'c'") {
                Ok(self.checks.clone())
            } else {
                Ok(vec![])
            }
        }
        async fn fetch_columns(&self, table: &str) -> Result<Vec<LiveColumn>, PgError> {
            Ok(self.columns.get(table).cloned().unwrap_or_default())
        }
    }

    fn exec_with_columns(cols: Vec<LiveColumn>) -> FakeExec {
        let mut columns = BTreeMap::new();
        columns.insert("public.events".to_string(), cols);
        FakeExec {
            columns,
            // The shared `spec()` declares `id` as its primary key.
            pk: vec!["id".into()],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn no_drift_when_schema_matches() {
        let exec = exec_with_columns(vec![lc("id", "uuid", false), lc("name", "text", false)]);
        let result = check_drift(&exec, &[spec()]).await.unwrap();
        assert_eq!(result.checked, vec!["public.events".to_string()]);
        assert!(
            result.is_clean(),
            "expected no drift, got {:?}",
            result.drift
        );
    }

    #[tokio::test]
    async fn reports_missing_table() {
        let exec = FakeExec::default();
        let result = check_drift(&exec, &[spec()]).await.unwrap();
        assert_eq!(
            result.drift,
            vec![Drift::MissingTable {
                table: "public.events".into()
            }]
        );
    }

    #[tokio::test]
    async fn reports_missing_extra_and_mismatch() {
        // `name` missing, `id` wrong type, `extra` not in spec.
        let exec = exec_with_columns(vec![lc("id", "text", false), lc("extra", "integer", false)]);
        let result = check_drift(&exec, &[spec()]).await.unwrap();
        assert!(result.drift.contains(&Drift::TypeMismatch {
            table: "public.events".into(),
            column: "id".into(),
            expected_type: "uuid".into(),
            actual_type: "text".into(),
        }));
        assert!(result.drift.contains(&Drift::MissingColumn {
            table: "public.events".into(),
            column: "name".into(),
            expected_type: "text".into(),
        }));
        assert!(result.drift.contains(&Drift::ExtraColumn {
            table: "public.events".into(),
            column: "extra".into(),
            actual_type: "integer".into(),
        }));
    }

    #[tokio::test]
    async fn type_synonyms_are_normalized() {
        let mut s = PgTableSpec::new(
            "events",
            vec![
                col("id", PgType::Uuid),
                col("name", PgType::Varchar(Some(255))),
                col("created_at", PgType::Timestamptz),
                col("score", PgType::Float8),
                col("tags", PgType::Array(Box::new(PgType::Text))),
            ],
        );
        s.primary_key = vec!["id".into()];
        // Live types use Postgres' canonical spellings (format_type / info_schema).
        let exec = exec_with_columns(vec![
            lc("id", "uuid", false),
            lc("name", "character varying(255)", false),
            lc("created_at", "timestamp with time zone", false),
            lc("score", "double precision", false),
            lc("tags", "text[]", false),
        ]);
        let result = check_drift(&exec, &[s]).await.unwrap();
        assert!(result.is_clean(), "drift: {:?}", result.drift);
    }

    #[tokio::test]
    async fn reports_nullability_mismatch() {
        // Spec `name` is NOT NULL but live is nullable; type matches.
        let exec = exec_with_columns(vec![lc("id", "uuid", false), lc("name", "text", true)]);
        let result = check_drift(&exec, &[spec()]).await.unwrap();
        assert_eq!(
            result.drift,
            vec![Drift::NullabilityMismatch {
                table: "public.events".into(),
                column: "name".into(),
                expected_nullable: false,
                actual_nullable: true,
            }]
        );
    }

    #[tokio::test]
    async fn reports_missing_index_fk_and_policy() {
        let s = PgTableSpec::new(
            "events",
            vec![col("id", PgType::Uuid), col("org_id", PgType::Uuid)],
        )
        .primary_key(["id"])
        .index(IndexSpec::new(
            "idx_events_org",
            [IndexColumn::column("org_id")],
        ))
        .foreign_key(ForeignKeySpec::new(
            "fk_events_org",
            ["org_id"],
            "public.orgs",
            ["id"],
        ))
        .policy(PolicySpec::new("p_tenant_isolation"));

        let mut columns = BTreeMap::new();
        columns.insert(
            "public.events".to_string(),
            vec![lc("id", "uuid", false), lc("org_id", "uuid", false)],
        );
        // Live database has the PK but none of the expected definitions.
        let exec = FakeExec {
            columns,
            pk: vec!["id".into()],
            ..Default::default()
        };
        let result = check_drift(&exec, &[s]).await.unwrap();
        assert!(result.drift.contains(&Drift::MissingIndex {
            table: "public.events".into(),
            index: "idx_events_org".into(),
        }));
        assert!(result.drift.contains(&Drift::MissingForeignKey {
            table: "public.events".into(),
            constraint: "fk_events_org".into(),
        }));
        assert!(result.drift.contains(&Drift::MissingPolicy {
            table: "public.events".into(),
            policy: "p_tenant_isolation".into(),
        }));
    }

    #[tokio::test]
    async fn present_index_fk_and_policy_are_clean() {
        let s = PgTableSpec::new(
            "events",
            vec![col("id", PgType::Uuid), col("org_id", PgType::Uuid)],
        )
        .primary_key(["id"])
        .index(IndexSpec::new(
            "idx_events_org",
            [IndexColumn::column("org_id")],
        ))
        .foreign_key(ForeignKeySpec::new(
            "fk_events_org",
            ["org_id"],
            "public.orgs",
            ["id"],
        ))
        .policy(PolicySpec::new("p_tenant_isolation"));

        let mut columns = BTreeMap::new();
        columns.insert(
            "public.events".to_string(),
            vec![lc("id", "uuid", false), lc("org_id", "uuid", false)],
        );
        // Live signatures (not names) for an org_id btree index, the org FK, and
        // the tenant policy — each matches the spec by definition.
        let exec = FakeExec {
            columns,
            indexes: vec!["false|btree||org_id".into()],
            fks: vec!["org_id|public.orgs|id|a|a".into()],
            policies: vec!["all|public||".into()],
            pk: vec!["id".into()],
            ..Default::default()
        };
        let result = check_drift(&exec, &[s]).await.unwrap();
        assert!(result.is_clean(), "drift: {:?}", result.drift);
    }

    /// The core guarantee: a foreign key / index / policy whose live *name*
    /// differs from the spec (legacy constraint names, 63-char truncation, PG
    /// default `_fkey` naming, non-cascading renames) is NOT drift as long as the
    /// *definition* matches.
    #[tokio::test]
    async fn renamed_but_identical_objects_are_not_drift() {
        let s = PgTableSpec::new(
            "events",
            vec![col("id", PgType::Uuid), col("org_id", PgType::Uuid)],
        )
        .primary_key(["id"])
        .index(
            IndexSpec::new("idx_events_org_id_v2", [IndexColumn::column("org_id")])
                .unique()
                .where_clause("org_id IS NOT NULL"),
        )
        .foreign_key(
            ForeignKeySpec::new("fk_events_org_renamed", ["org_id"], "orgs", ["id"])
                .on_delete(crate::spec::ReferentialAction::Cascade),
        )
        .policy(
            PolicySpec::new("p_old_legacy_name")
                .for_command(PolicyFor::Select)
                .to_roles(["authenticated"])
                .using("org_id = current_org()"),
        );

        let mut columns = BTreeMap::new();
        columns.insert(
            "public.events".to_string(),
            vec![lc("id", "uuid", false), lc("org_id", "uuid", false)],
        );
        // Live objects carry DIFFERENT names but identical definitions. The FK
        // target is unqualified in the spec (`orgs`) yet `public.orgs` live; the
        // predicate / using arrive paren-wrapped as Postgres echoes them.
        let exec = FakeExec {
            columns,
            indexes: vec!["true|btree|(org_id IS NOT NULL)|org_id".into()],
            fks: vec!["org_id|public.orgs|id|c|a".into()],
            policies: vec!["select|authenticated|(org_id = current_org())|".into()],
            pk: vec!["id".into()],
            ..Default::default()
        };
        let result = check_drift(&exec, &[s]).await.unwrap();
        assert!(
            result.is_clean(),
            "renamed-but-identical objects must not drift, got {:?}",
            result.drift
        );
    }

    #[tokio::test]
    async fn reports_primary_key_mismatch() {
        let s = PgTableSpec::new(
            "events",
            vec![col("id", PgType::Uuid), col("org_id", PgType::Uuid)],
        )
        .primary_key(["org_id", "id"]);
        let mut exec =
            exec_with_columns(vec![lc("id", "uuid", false), lc("org_id", "uuid", false)]);
        // Live PK is just `id`.
        exec.pk = vec!["id".into()];
        let result = check_drift(&exec, &[s]).await.unwrap();
        assert!(
            result.drift.contains(&Drift::PrimaryKeyMismatch {
                table: "public.events".into(),
                expected_columns: vec!["org_id".into(), "id".into()],
                actual_columns: vec!["id".into()],
            }),
            "drift: {:?}",
            result.drift
        );
    }

    #[tokio::test]
    async fn primary_key_is_order_agnostic() {
        let s = PgTableSpec::new(
            "events",
            vec![col("id", PgType::Uuid), col("org_id", PgType::Uuid)],
        )
        .primary_key(["org_id", "id"]);
        let mut exec =
            exec_with_columns(vec![lc("id", "uuid", false), lc("org_id", "uuid", false)]);
        exec.pk = vec!["id".into(), "org_id".into()];
        let result = check_drift(&exec, &[s]).await.unwrap();
        assert!(result.is_clean(), "drift: {:?}", result.drift);
    }

    #[tokio::test]
    async fn reports_missing_unique_and_check() {
        let s = PgTableSpec::new(
            "events",
            vec![col("id", PgType::Uuid), col("n", PgType::Int4)],
        )
        .primary_key(["id"])
        .unique_constraint(crate::spec::UniqueConstraintSpec::new("u_n", ["n"]))
        .check(crate::spec::CheckConstraintSpec::new("c_pos", "n > 0"));
        let exec = exec_with_columns(vec![lc("id", "uuid", false), lc("n", "integer", false)]);
        let result = check_drift(&exec, &[s]).await.unwrap();
        assert!(result.drift.contains(&Drift::MissingUniqueConstraint {
            table: "public.events".into(),
            columns: vec!["n".into()],
            nulls_not_distinct: false,
        }));
        assert!(result.drift.contains(&Drift::MissingCheckConstraint {
            table: "public.events".into(),
            expression: "n > 0".into(),
        }));
    }

    #[tokio::test]
    async fn present_unique_and_check_are_clean() {
        let s = PgTableSpec::new(
            "events",
            vec![col("id", PgType::Uuid), col("n", PgType::Int4)],
        )
        .primary_key(["id"])
        .unique_constraint(crate::spec::UniqueConstraintSpec::new("u_n", ["n"]))
        .check(crate::spec::CheckConstraintSpec::new("c_pos", "n > 0"));
        let mut exec = exec_with_columns(vec![lc("id", "uuid", false), lc("n", "integer", false)]);
        exec.uniques = vec!["n|false".into()];
        // Postgres echoes a check expression paren-wrapped: `(n > 0)`.
        exec.checks = vec!["(n > 0)".into()];
        let result = check_drift(&exec, &[s]).await.unwrap();
        assert!(result.is_clean(), "drift: {:?}", result.drift);
    }

    #[tokio::test]
    async fn enum_drift_missing_and_mismatch_and_clean() {
        use crate::spec::EnumTypeSpec;
        // Missing type entirely.
        let missing = FakeExec::default();
        let d = check_enum_drift(&missing, &[EnumTypeSpec::new("status", ["a", "b"])])
            .await
            .unwrap();
        assert_eq!(
            d,
            vec![Drift::MissingEnumType {
                enum_type: "public.status".into()
            }]
        );

        // Value set diverges.
        let mismatch = FakeExec {
            enums: vec!["a".into(), "b".into(), "c".into()],
            ..Default::default()
        };
        let d = check_enum_drift(&mismatch, &[EnumTypeSpec::new("status", ["a", "b"])])
            .await
            .unwrap();
        assert!(matches!(d.as_slice(), [Drift::EnumValuesMismatch { .. }]));

        // Same value set, different declared order → clean.
        let clean = FakeExec {
            enums: vec!["b".into(), "a".into()],
            ..Default::default()
        };
        let d = check_enum_drift(&clean, &[EnumTypeSpec::new("status", ["a", "b"])])
            .await
            .unwrap();
        assert!(d.is_empty(), "enum drift: {d:?}");
    }

    #[test]
    fn canonical_type_normalization() {
        assert_eq!(canonical_pg_type("BOOLEAN"), "bool");
        assert_eq!(canonical_pg_type("integer"), "int4");
        assert_eq!(canonical_pg_type("character varying(255)"), "varchar(255)");
        assert_eq!(canonical_pg_type("numeric(10, 2)"), "numeric(10,2)");
        assert_eq!(canonical_pg_type("timestamp with time zone"), "timestamptz");
        assert_eq!(canonical_pg_type("text[]"), "text[]");
        assert_eq!(canonical_pg_type("status"), "status"); // enum: passthrough
        assert_eq!(canonical_pg_type("tsvector"), "tsvector");
        assert_eq!(canonical_pg_type("vector"), "vector");
        assert_eq!(canonical_pg_type("vector(1536)"), "vector(1536)");
    }
}
