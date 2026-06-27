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
//! Index / foreign-key / policy presence is checked best-effort via
//! [`PgExecutor::fetch_strings`] against `pg_indexes`, `pg_constraint`, and
//! `pg_policies`. Only *missing* objects are reported there — Postgres creates
//! many implicit indexes/constraints, so the gate never flags "extra" ones.

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
    /// An index is in the spec but missing from the live table.
    MissingIndex { table: String, index: String },
    /// A foreign key is in the spec but missing from the live table.
    MissingForeignKey { table: String, constraint: String },
    /// A row-level-security policy is in the spec but missing from the live table.
    MissingPolicy { table: String, policy: String },
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

        // Best-effort presence checks for indexes, foreign keys, and policies.
        if !table.indexes.is_empty() {
            let sql = format!(
                "SELECT indexname FROM pg_indexes WHERE schemaname = '{}' AND tablename = '{}'",
                escape_literal(&table.schema),
                escape_literal(&table.name),
            );
            let live_indexes: BTreeSet<String> =
                exec.fetch_strings(&sql).await?.into_iter().collect();
            for idx in &table.indexes {
                if !live_indexes.contains(&idx.name) {
                    drift.push(Drift::MissingIndex {
                        table: qualified.clone(),
                        index: idx.name.clone(),
                    });
                }
            }
        }

        if !table.foreign_keys.is_empty() {
            let sql = format!(
                "SELECT c.conname FROM pg_constraint c \
                 JOIN pg_class t ON c.conrelid = t.oid \
                 JOIN pg_namespace n ON t.relnamespace = n.oid \
                 WHERE c.contype = 'f' AND n.nspname = '{}' AND t.relname = '{}'",
                escape_literal(&table.schema),
                escape_literal(&table.name),
            );
            let live_fks: BTreeSet<String> = exec.fetch_strings(&sql).await?.into_iter().collect();
            for fk in &table.foreign_keys {
                if !live_fks.contains(&fk.name) {
                    drift.push(Drift::MissingForeignKey {
                        table: qualified.clone(),
                        constraint: fk.name.clone(),
                    });
                }
            }
        }

        if !table.policies.is_empty() {
            let sql = format!(
                "SELECT policyname FROM pg_policies WHERE schemaname = '{}' AND tablename = '{}'",
                escape_literal(&table.schema),
                escape_literal(&table.name),
            );
            let live_policies: BTreeSet<String> =
                exec.fetch_strings(&sql).await?.into_iter().collect();
            for policy in &table.policies {
                if !live_policies.contains(&policy.name) {
                    drift.push(Drift::MissingPolicy {
                        table: qualified.clone(),
                        policy: policy.name.clone(),
                    });
                }
            }
        }
    }

    Ok(DriftResult { checked, drift })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::LiveColumn;
    use crate::spec::{ColumnSpec, ForeignKeySpec, IndexColumn, IndexSpec, PgType, PolicySpec};

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
    /// `fetch_strings` routes by the catalog table referenced in the query.
    #[derive(Default)]
    struct FakeExec {
        columns: BTreeMap<String, Vec<LiveColumn>>,
        indexes: Vec<String>,
        fks: Vec<String>,
        policies: Vec<String>,
    }

    impl PgExecutor for FakeExec {
        async fn command(&self, _sql: &str) -> Result<(), PgError> {
            Ok(())
        }
        async fn fetch_strings(&self, sql: &str) -> Result<Vec<String>, PgError> {
            if sql.contains("pg_indexes") {
                Ok(self.indexes.clone())
            } else if sql.contains("pg_constraint") {
                Ok(self.fks.clone())
            } else if sql.contains("pg_policies") {
                Ok(self.policies.clone())
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
        // Live database has none of the expected named objects.
        let exec = FakeExec {
            columns,
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
        let exec = FakeExec {
            columns,
            indexes: vec!["idx_events_org".into(), "events_pkey".into()],
            fks: vec!["fk_events_org".into()],
            policies: vec!["p_tenant_isolation".into()],
        };
        let result = check_drift(&exec, &[s]).await.unwrap();
        assert!(result.is_clean(), "drift: {:?}", result.drift);
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
    }
}
