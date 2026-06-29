//! Live-database introspection (feature `introspect`): build a
//! [`PgTableSpec`]/[`EnumTypeSpec`] source of truth directly from a running
//! Postgres via `pg_catalog`. This is the inverse of the DDL/differ path —
//! instead of declaring a schema in code, you read the *actual* schema (with its
//! actual object names) out of a deployed database.
//!
//! The intended use is a **cutover**: to flip schema source-of-truth onto the kit
//! for a database that is today owned by another tool (an ORM, hand-written
//! migrations, …), you introspect the live schema into specs, then feed those
//! specs straight into [`crate::check_drift`] / [`crate::check_enum_drift`]. Since
//! the specs are read from the same catalog the drift gate reads, the result must
//! be drift-clean — proving the kit can take over as a no-op. (The integration
//! test exercises exactly this round-trip.)
//!
//! Everything is read through the driver-agnostic [`PgExecutor::fetch_rows`] seam;
//! the kit never owns a connection. The introspection queries cast every non-text
//! column to `::text`, so an executor implementation reads each cell as an
//! optional string without type-aware rendering.
//!
//! Requires Postgres 15+ — it reads `pg_attribute.attgenerated` (PG12+) and
//! `pg_index.indnullsnotdistinct` (PG15+), matching the Supabase target.
//!
//! What is captured per table: columns (type incl. `tsvector`/`vector`,
//! nullability, defaults, `STORED` generated columns), primary keys, foreign keys
//! (with real names and `ON DELETE`/`ON UPDATE` from `confdeltype`/`confupdtype`),
//! unique constraints (incl. `NULLS NOT DISTINCT`), check constraints, indexes
//! (rendered by `pg_get_indexdef`, including partial-index predicates and access
//! method), RLS policies, and the RLS-enabled flag. Plus user-defined enum types
//! and their value order.

use std::collections::HashSet;

use crate::client::{PgError, PgExecutor};
use crate::safety::{validate_identifier, SchemaLimits};
use crate::spec::{
    CheckConstraintSpec, ColumnSpec, EnumTypeSpec, ForeignKeySpec, IndexColumn, IndexSpec,
    PgTableSpec, PgType, PolicyFor, PolicySpec, ReferentialAction, UniqueConstraintSpec,
};

/// The schema read out of a live database: every table (with its constraints,
/// indexes, and policies) and every user-defined enum type, sorted by qualified
/// name for stable output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntrospectedSchema {
    pub tables: Vec<PgTableSpec>,
    pub enums: Vec<EnumTypeSpec>,
}

/// Escape a value for embedding as a SQL string literal (doubles single quotes).
/// Identifiers placed in literals are already validated; this is defense in depth.
fn escape_literal(value: &str) -> String {
    value.replace('\'', "''")
}

/// A required cell: errors if the column is absent or `NULL`.
fn req(row: &[Option<String>], i: usize) -> Result<String, PgError> {
    row.get(i).and_then(|c| c.clone()).ok_or_else(|| {
        PgError::Backend(format!(
            "introspection: missing required column at index {i}"
        ))
    })
}

/// An optional cell: `None` for an absent column or a SQL `NULL`.
fn opt(row: &[Option<String>], i: usize) -> Option<String> {
    row.get(i).and_then(|c| c.clone())
}

/// Split a `string_agg(..., ',')` cell into its parts. The aggregated values are
/// always validated identifiers (column / role names), so they never contain a
/// comma — a plain split is exact.
fn split_csv(s: &str) -> Vec<String> {
    if s.is_empty() {
        Vec::new()
    } else {
        s.split(',').map(|p| p.to_string()).collect()
    }
}

/// Map a `pg_constraint.confdeltype` / `confupdtype` single-char code to a
/// [`ReferentialAction`]. `'a'` (and anything unexpected) is `NO ACTION`.
fn referential_action_from_char(c: &str) -> ReferentialAction {
    match c {
        "r" => ReferentialAction::Restrict,
        "c" => ReferentialAction::Cascade,
        "n" => ReferentialAction::SetNull,
        "d" => ReferentialAction::SetDefault,
        _ => ReferentialAction::NoAction,
    }
}

/// Map a Postgres type string (`format_type()` output) to a kit [`PgType`].
/// `enum_names` is the set of bare user-defined enum names in the schema. Types
/// outside the closed `PgType` set (domains / other extension types) route through
/// the `Enum(String)` escape hatch, which renders the name verbatim so it still
/// compares equal in the drift gate.
fn map_type(raw: &str, enum_names: &HashSet<String>) -> PgType {
    let t = raw.trim();

    // Arrays: strip the trailing `[]`, map the inner type, re-wrap.
    if let Some(inner) = t.strip_suffix("[]") {
        return PgType::Array(Box::new(map_type(inner, enum_names)));
    }

    // Split off an optional parenthesized modifier, e.g. `varchar(255)`,
    // `numeric(10, 7)`, `vector(1024)`.
    let (base, modifier) = match t.find('(') {
        Some(i) => {
            let m = &t[i + 1..t.rfind(')').unwrap_or(t.len())];
            (t[..i].trim(), Some(m.trim()))
        }
        None => (t, None),
    };
    let base_lower = base.to_ascii_lowercase();

    let native = match base_lower.as_str() {
        "uuid" => Some(PgType::Uuid),
        "text" => Some(PgType::Text),
        "boolean" | "bool" => Some(PgType::Bool),
        "integer" | "int" | "int4" | "serial" => Some(PgType::Int4),
        "bigint" | "int8" | "bigserial" => Some(PgType::Int8),
        "smallint" | "int2" | "smallserial" => Some(PgType::Int2),
        "double precision" | "float8" => Some(PgType::Float8),
        "real" | "float4" => Some(PgType::Float4),
        "date" => Some(PgType::Date),
        "jsonb" => Some(PgType::Jsonb),
        "json" => Some(PgType::Json),
        "bytea" => Some(PgType::Bytea),
        "timestamp with time zone" | "timestamptz" => Some(PgType::Timestamptz),
        "timestamp" | "timestamp without time zone" => Some(PgType::Timestamp),
        "tsvector" => Some(PgType::Tsvector),
        "vector" => Some(PgType::Vector(modifier.and_then(|m| m.parse::<u32>().ok()))),
        "varchar" | "character varying" => Some(PgType::Varchar(
            modifier.and_then(|m| m.parse::<u32>().ok()),
        )),
        "numeric" | "decimal" => {
            let ps = modifier.and_then(|m| {
                let mut it = m.split(',');
                let p = it.next()?.trim().parse::<u32>().ok()?;
                let s = it.next()?.trim().parse::<u32>().ok()?;
                Some((p, s))
            });
            Some(PgType::Numeric(ps))
        }
        _ => None,
    };

    if let Some(ty) = native {
        return ty;
    }
    if enum_names.contains(base) {
        return PgType::Enum(base.to_string());
    }
    // Domain / other extension type: verbatim passthrough.
    PgType::Enum(t.to_string())
}

/// Introspect every table and enum in `schema` from the live database, returning
/// kit specs that carry the **actual** live object names. Feeding the result into
/// [`crate::check_drift`] / [`crate::check_enum_drift`] against the same database
/// is, by construction, drift-clean — the cutover guarantee.
pub async fn introspect_schema(
    exec: &impl PgExecutor,
    schema: &str,
) -> Result<IntrospectedSchema, PgError> {
    let limits = SchemaLimits::default();
    validate_identifier(schema, "schema", &limits).map_err(|e| PgError::Backend(e.to_string()))?;
    let sch = escape_literal(schema);

    // ── Enum types + values (one row per label, grouped in declaration order) ──
    let enum_rows = exec
        .fetch_rows(&format!(
            "SELECT n.nspname, t.typname, e.enumlabel \
             FROM pg_type t \
             JOIN pg_namespace n ON n.oid = t.typnamespace \
             JOIN pg_enum e ON e.enumtypid = t.oid \
             WHERE t.typtype = 'e' AND n.nspname = '{sch}' \
             ORDER BY t.typname, e.enumsortorder"
        ))
        .await?;

    let mut enum_names: HashSet<String> = HashSet::new();
    let mut enums: Vec<EnumTypeSpec> = Vec::new();
    for row in &enum_rows {
        let es = req(row, 0)?;
        let en = req(row, 1)?;
        let label = req(row, 2)?;
        if let Some(last) = enums.last_mut() {
            if last.schema == es && last.name == en {
                last.values.push(label);
                continue;
            }
        }
        enum_names.insert(en.clone());
        enums.push(EnumTypeSpec::new(en, vec![label]).in_schema(es));
    }

    // ── Tables (ordinary + partitioned) ───────────────────────────────────────
    let table_rows = exec
        .fetch_rows(&format!(
            "SELECT c.relname \
             FROM pg_class c \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = '{sch}' AND c.relkind IN ('r', 'p') \
             ORDER BY c.relname"
        ))
        .await?;

    let mut tables = Vec::new();
    for trow in &table_rows {
        let table_name = req(trow, 0)?;
        validate_identifier(&table_name, "table", &limits)
            .map_err(|e| PgError::Backend(e.to_string()))?;
        // A validated `schema`.`table` rendered as a `::regclass` literal.
        let regclass = escape_literal(&format!("\"{schema}\".\"{table_name}\""));
        let tbl = escape_literal(&table_name);

        // Columns: type, nullability, default, generated kind.
        let col_rows = exec
            .fetch_rows(&format!(
                "SELECT a.attname, \
                        format_type(a.atttypid, a.atttypmod), \
                        (NOT a.attnotnull)::text, \
                        pg_get_expr(ad.adbin, ad.adrelid), \
                        a.attgenerated::text \
                 FROM pg_attribute a \
                 LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum \
                 WHERE a.attrelid = '{regclass}'::regclass AND a.attnum > 0 AND NOT a.attisdropped \
                 ORDER BY a.attnum"
            ))
            .await?;

        let mut columns = Vec::new();
        for crow in &col_rows {
            let cname = req(crow, 0)?;
            let data_type = req(crow, 1)?;
            let is_nullable = req(crow, 2)? == "true";
            let default_expr = opt(crow, 3);
            let generated = opt(crow, 4).unwrap_or_default();

            let mut col = ColumnSpec::new(cname, map_type(&data_type, &enum_names));
            if is_nullable {
                col = col.nullable();
            }
            // attgenerated 's' == STORED generated column; its expression lives in
            // pg_attrdef too. Otherwise the expression (if any) is the default.
            if generated == "s" {
                if let Some(expr) = default_expr {
                    col = col.generated_stored(expr);
                }
            } else if let Some(def) = default_expr {
                col = col.default_expr(def);
            }
            columns.push(col);
        }

        let mut spec = PgTableSpec::new(table_name.clone(), columns).in_schema(schema);

        // Primary key (ordinality order; compared as a set downstream).
        let pk_rows = exec
            .fetch_rows(&format!(
                "SELECT a.attname \
                 FROM pg_constraint c \
                 JOIN unnest(c.conkey) WITH ORDINALITY AS k(attnum, ord) ON true \
                 JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = k.attnum \
                 WHERE c.contype = 'p' AND c.conrelid = '{regclass}'::regclass \
                 ORDER BY k.ord"
            ))
            .await?;
        let pk: Vec<String> = pk_rows
            .iter()
            .map(|r| req(r, 0))
            .collect::<Result<_, _>>()?;
        if !pk.is_empty() {
            spec = spec.primary_key(pk);
        }

        // Foreign keys (from/to columns in conkey/confkey ordinality order).
        let fk_rows = exec
            .fetch_rows(&format!(
                "SELECT c.conname, \
                   (SELECT string_agg(a.attname, ',' ORDER BY k.ord) \
                      FROM unnest(c.conkey) WITH ORDINALITY AS k(attnum, ord) \
                      JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = k.attnum), \
                   rn.nspname, rt.relname, \
                   (SELECT string_agg(a.attname, ',' ORDER BY k.ord) \
                      FROM unnest(c.confkey) WITH ORDINALITY AS k(attnum, ord) \
                      JOIN pg_attribute a ON a.attrelid = c.confrelid AND a.attnum = k.attnum), \
                   c.confdeltype::text, c.confupdtype::text \
                 FROM pg_constraint c \
                 JOIN pg_class rt ON rt.oid = c.confrelid \
                 JOIN pg_namespace rn ON rn.oid = rt.relnamespace \
                 WHERE c.contype = 'f' AND c.conrelid = '{regclass}'::regclass \
                 ORDER BY c.conname"
            ))
            .await?;
        for frow in &fk_rows {
            let name = req(frow, 0)?;
            let from_cols = split_csv(&req(frow, 1)?);
            let ref_schema = req(frow, 2)?;
            let ref_table = req(frow, 3)?;
            let to_cols = split_csv(&req(frow, 4)?);
            let del = req(frow, 5)?;
            let upd = req(frow, 6)?;
            let table_to = format!("{ref_schema}.{ref_table}");
            spec = spec.foreign_key(
                ForeignKeySpec::new(name, from_cols, table_to, to_cols)
                    .on_delete(referential_action_from_char(&del))
                    .on_update(referential_action_from_char(&upd)),
            );
        }

        // Unique constraints (contype 'u' — folds in column-level UNIQUE).
        let uc_rows = exec
            .fetch_rows(&format!(
                "SELECT c.conname, \
                   (SELECT string_agg(a.attname, ',' ORDER BY k.ord) \
                      FROM unnest(c.conkey) WITH ORDINALITY AS k(attnum, ord) \
                      JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = k.attnum), \
                   coalesce(i.indnullsnotdistinct, false)::text \
                 FROM pg_constraint c \
                 LEFT JOIN pg_index i ON i.indexrelid = c.conindid \
                 WHERE c.contype = 'u' AND c.conrelid = '{regclass}'::regclass \
                 ORDER BY c.conname"
            ))
            .await?;
        for urow in &uc_rows {
            let name = req(urow, 0)?;
            let cols = split_csv(&req(urow, 1)?);
            let nnd = req(urow, 2)? == "true";
            let mut uc = UniqueConstraintSpec::new(name, cols);
            if nnd {
                uc = uc.nulls_not_distinct();
            }
            spec = spec.unique_constraint(uc);
        }

        // Check constraints (matched downstream by normalized expression).
        let cc_rows = exec
            .fetch_rows(&format!(
                "SELECT c.conname, pg_get_expr(c.conbin, c.conrelid) \
                 FROM pg_constraint c \
                 WHERE c.contype = 'c' AND c.conrelid = '{regclass}'::regclass \
                 ORDER BY c.conname"
            ))
            .await?;
        for crow in &cc_rows {
            let name = req(crow, 0)?;
            let expr = req(crow, 1)?;
            spec = spec.check(CheckConstraintSpec::new(name, expr));
        }

        // Indexes (non-primary). One row per key column so an expression member
        // carrying a comma keeps its boundary; grouped back per index in order.
        let idx_rows = exec
            .fetch_rows(&format!(
                "SELECT ic.relname, i.indisunique::text, am.amname, \
                        coalesce(pg_get_expr(i.indpred, i.indrelid), ''), \
                        k.n::text, pg_get_indexdef(i.indexrelid, k.n, true) \
                 FROM pg_index i \
                 JOIN pg_class ic ON ic.oid = i.indexrelid \
                 JOIN pg_class tc ON tc.oid = i.indrelid \
                 JOIN pg_am am ON am.oid = ic.relam \
                 JOIN generate_series(1, i.indnkeyatts) AS k(n) ON true \
                 WHERE tc.oid = '{regclass}'::regclass AND NOT i.indisprimary \
                 ORDER BY ic.relname, k.n"
            ))
            .await?;
        // (name, unique, method, predicate, key-defs in order).
        let mut idx_acc: Vec<(String, bool, String, String, Vec<String>)> = Vec::new();
        for irow in &idx_rows {
            let name = req(irow, 0)?;
            let is_unique = req(irow, 1)? == "true";
            let method = req(irow, 2)?;
            let predicate = req(irow, 3)?;
            let keydef = req(irow, 5)?;
            if let Some(last) = idx_acc.last_mut() {
                if last.0 == name {
                    last.4.push(keydef);
                    continue;
                }
            }
            idx_acc.push((name, is_unique, method, predicate, vec![keydef]));
        }
        for (name, is_unique, method, predicate, keys) in idx_acc {
            let cols: Vec<IndexColumn> = keys.into_iter().map(IndexColumn::expr).collect();
            let mut index = IndexSpec::new(name, cols).method(method);
            if is_unique {
                index = index.unique();
            }
            if !predicate.is_empty() {
                index = index.where_clause(predicate);
            }
            spec = spec.index(index);
        }

        // RLS policies.
        let pol_rows = exec
            .fetch_rows(&format!(
                "SELECT policyname, cmd, \
                        array_to_string(ARRAY(SELECT unnest(roles) ORDER BY 1), ','), \
                        qual, with_check \
                 FROM pg_policies \
                 WHERE schemaname = '{sch}' AND tablename = '{tbl}' \
                 ORDER BY policyname"
            ))
            .await?;
        for prow in &pol_rows {
            let name = req(prow, 0)?;
            let cmd = req(prow, 1)?;
            let roles = split_csv(&opt(prow, 2).unwrap_or_default());
            let qual = opt(prow, 3);
            let with_check = opt(prow, 4);
            let pf = match cmd.to_ascii_uppercase().as_str() {
                "SELECT" => PolicyFor::Select,
                "INSERT" => PolicyFor::Insert,
                "UPDATE" => PolicyFor::Update,
                "DELETE" => PolicyFor::Delete,
                _ => PolicyFor::All,
            };
            let mut policy = PolicySpec::new(name).for_command(pf).to_roles(roles);
            if let Some(q) = qual {
                policy = policy.using(q);
            }
            if let Some(w) = with_check {
                policy = policy.with_check(w);
            }
            spec = spec.policy(policy);
        }

        // RLS-enabled flag (fidelity; not part of the drift signature).
        let rls_rows = exec
            .fetch_rows(&format!(
                "SELECT c.relrowsecurity::text FROM pg_class c WHERE c.oid = '{regclass}'::regclass"
            ))
            .await?;
        let rls_enabled = rls_rows
            .first()
            .and_then(|r| r.first().cloned())
            .flatten()
            .map(|s| s == "true")
            .unwrap_or(false);
        if rls_enabled {
            spec = spec.enable_rls();
        }

        tables.push(spec);
    }

    tables.sort_by_key(|a| a.qualified_name());
    enums.sort_by_key(|a| a.qualified_name());

    Ok(IntrospectedSchema { tables, enums })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::client::LiveColumn;

    fn s(x: &str) -> Option<String> {
        Some(x.to_string())
    }

    /// A canned executor: `fetch_rows` routes by a token unique to each catalog
    /// query `introspect_schema` emits, returning pre-built text rows.
    #[derive(Default)]
    struct FakeExec {
        rows: HashMap<&'static str, Vec<Vec<Option<String>>>>,
    }

    impl FakeExec {
        fn route(&self, sql: &str) -> Vec<Vec<Option<String>>> {
            let key = if sql.contains("e.enumlabel") {
                "enums"
            } else if sql.contains("c.relkind") {
                "tables"
            } else if sql.contains("format_type") {
                "columns"
            } else if sql.contains("pg_get_indexdef") {
                "indexes"
            } else if sql.contains("pg_policies") {
                "policies"
            } else if sql.contains("relrowsecurity") {
                "rls"
            } else if sql.contains("contype = 'p'") {
                "pk"
            } else if sql.contains("contype = 'f'") {
                "fks"
            } else if sql.contains("contype = 'u'") {
                "uniques"
            } else if sql.contains("contype = 'c'") {
                "checks"
            } else {
                ""
            };
            self.rows.get(key).cloned().unwrap_or_default()
        }
    }

    impl PgExecutor for FakeExec {
        async fn command(&self, _sql: &str) -> Result<(), PgError> {
            Ok(())
        }
        async fn fetch_strings(&self, _sql: &str) -> Result<Vec<String>, PgError> {
            Ok(vec![])
        }
        async fn fetch_rows(&self, sql: &str) -> Result<Vec<Vec<Option<String>>>, PgError> {
            Ok(self.route(sql))
        }
        async fn fetch_columns(&self, _table: &str) -> Result<Vec<LiveColumn>, PgError> {
            Ok(vec![])
        }
    }

    fn widgets_fake() -> FakeExec {
        let mut rows: HashMap<&'static str, Vec<Vec<Option<String>>>> = HashMap::new();
        rows.insert(
            "enums",
            vec![
                vec![s("public"), s("status"), s("active")],
                vec![s("public"), s("status"), s("inactive")],
            ],
        );
        rows.insert("tables", vec![vec![s("widgets")]]);
        rows.insert(
            "columns",
            vec![
                // name, data_type, is_nullable, default, generated
                vec![
                    s("id"),
                    s("uuid"),
                    s("false"),
                    s("gen_random_uuid()"),
                    s(""),
                ],
                vec![s("org_id"), s("uuid"), s("false"), None, s("")],
                vec![s("name"), s("text"), s("true"), None, s("")],
                vec![
                    s("status"),
                    s("status"),
                    s("false"),
                    s("'active'::status"),
                    s(""),
                ],
                vec![s("body"), s("tsvector"), s("true"), None, s("")],
                vec![s("embedding"), s("vector(3)"), s("true"), None, s("")],
                vec![
                    s("created"),
                    s("timestamp with time zone"),
                    s("false"),
                    s("now()"),
                    s(""),
                ],
                vec![
                    s("search"),
                    s("tsvector"),
                    s("false"),
                    s("to_tsvector('english'::regconfig, name)"),
                    s("s"),
                ],
            ],
        );
        rows.insert("pk", vec![vec![s("id")]]);
        rows.insert(
            "fks",
            vec![vec![
                s("widgets_org_id_fkey"),
                s("org_id"),
                s("public"),
                s("orgs"),
                s("id"),
                s("c"),
                s("a"),
            ]],
        );
        rows.insert(
            "uniques",
            vec![vec![s("widgets_name_key"), s("name"), s("false")]],
        );
        rows.insert(
            "checks",
            vec![vec![s("widgets_name_check"), s("(char_length(name) > 0)")]],
        );
        rows.insert(
            "indexes",
            vec![vec![
                s("widgets_org_id_idx"),
                s("false"),
                s("btree"),
                s(""),
                s("1"),
                s("org_id"),
            ]],
        );
        rows.insert(
            "policies",
            vec![vec![
                s("org_isolation"),
                s("ALL"),
                s("public"),
                s("(org_id = current_org())"),
                None,
            ]],
        );
        rows.insert("rls", vec![vec![s("true")]]);
        FakeExec { rows }
    }

    #[tokio::test]
    async fn introspects_enum_types() {
        let schema = introspect_schema(&widgets_fake(), "public").await.unwrap();
        assert_eq!(schema.enums.len(), 1);
        let e = &schema.enums[0];
        assert_eq!(e.name, "status");
        assert_eq!(e.schema, "public");
        // Value *order* is preserved from enumsortorder.
        assert_eq!(e.values, vec!["active".to_string(), "inactive".to_string()]);
    }

    #[tokio::test]
    async fn introspects_columns_with_types_defaults_and_generated() {
        let schema = introspect_schema(&widgets_fake(), "public").await.unwrap();
        assert_eq!(schema.tables.len(), 1);
        let t = &schema.tables[0];
        assert_eq!(t.qualified_name(), "public.widgets");

        let by_name = |n: &str| t.columns.iter().find(|c| c.name == n).unwrap();

        // Enum column resolves to the named enum (not a passthrough).
        assert_eq!(by_name("status").ty, PgType::Enum("status".into()));
        // tsvector / pgvector are first-class.
        assert_eq!(by_name("body").ty, PgType::Tsvector);
        assert_eq!(by_name("embedding").ty, PgType::Vector(Some(3)));
        // Nullability.
        assert!(by_name("name").nullable);
        assert!(!by_name("id").nullable);
        // Default.
        assert_eq!(by_name("created").default.as_deref(), Some("now()"));
        // STORED generated column → expression captured, not a plain default.
        let search = by_name("search");
        assert!(search.default.is_none());
        assert_eq!(
            search.generated.as_ref().unwrap().expression,
            "to_tsvector('english'::regconfig, name)"
        );
    }

    #[tokio::test]
    async fn introspects_keys_constraints_indexes_and_policy() {
        let schema = introspect_schema(&widgets_fake(), "public").await.unwrap();
        let t = &schema.tables[0];

        assert_eq!(t.primary_key, vec!["id".to_string()]);

        assert_eq!(t.foreign_keys.len(), 1);
        let fk = &t.foreign_keys[0];
        assert_eq!(fk.columns_from, vec!["org_id".to_string()]);
        assert_eq!(fk.table_to, "public.orgs");
        assert_eq!(fk.columns_to, vec!["id".to_string()]);
        assert_eq!(fk.on_delete, Some(ReferentialAction::Cascade));
        assert_eq!(fk.on_update, Some(ReferentialAction::NoAction));

        assert_eq!(t.unique_constraints.len(), 1);
        assert_eq!(t.unique_constraints[0].columns, vec!["name".to_string()]);
        assert!(!t.unique_constraints[0].nulls_not_distinct);

        assert_eq!(t.check_constraints.len(), 1);
        assert_eq!(t.check_constraints[0].value, "(char_length(name) > 0)");

        assert_eq!(t.indexes.len(), 1);
        assert_eq!(t.indexes[0].columns[0].expression, "org_id");
        assert!(!t.indexes[0].unique);
        assert_eq!(t.indexes[0].method, "btree");

        assert_eq!(t.policies.len(), 1);
        let p = &t.policies[0];
        assert_eq!(p.for_, Some(PolicyFor::All));
        assert_eq!(p.to, vec!["public".to_string()]);
        assert_eq!(p.using.as_deref(), Some("(org_id = current_org())"));
        assert!(p.with_check.is_none());

        assert!(t.rls_enabled);
    }

    #[tokio::test]
    async fn unknown_type_routes_through_enum_passthrough() {
        let names: HashSet<String> = HashSet::new();
        // A domain / extension type the closed set doesn't know.
        assert_eq!(map_type("citext", &names), PgType::Enum("citext".into()));
        // Arrays recurse.
        assert_eq!(
            map_type("text[]", &names),
            PgType::Array(Box::new(PgType::Text))
        );
        // Modifiers parse.
        assert_eq!(
            map_type("numeric(10, 2)", &names),
            PgType::Numeric(Some((10, 2)))
        );
        assert_eq!(map_type("varchar(255)", &names), PgType::Varchar(Some(255)));
    }
}
