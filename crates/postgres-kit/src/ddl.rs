//! `CREATE TABLE` generation from a [`PgTableSpec`], plus standalone emitters for
//! enum types, indexes, and policies. All identifiers are validated and quoted;
//! type/default/expression fragments are emitted verbatim (trusted input).

use std::collections::BTreeSet;

use crate::safety::{quote_identifier, validate_identifier, SchemaError, SchemaLimits};
use crate::spec::{
    ColumnSpec, EnumTypeSpec, ForeignKeySpec, IdentitySpec, IndexSpec, PgTableSpec, PolicySpec,
    UniqueConstraintSpec,
};

/// Validate and quote a possibly schema-qualified name (`schema.table`), quoting
/// each segment.
fn quote_qualified(
    name: &str,
    kind: &'static str,
    limits: &SchemaLimits,
) -> Result<String, SchemaError> {
    let mut parts = Vec::new();
    for segment in name.split('.') {
        validate_identifier(segment, kind, limits)?;
        parts.push(quote_identifier(segment));
    }
    Ok(parts.join("."))
}

fn quote_cols(cols: &[String]) -> String {
    cols.iter()
        .map(|c| quote_identifier(c))
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_identity(identity: &IdentitySpec) -> String {
    let mut parts: Vec<String> = Vec::new();
    let s = &identity.sequence;
    if let Some(v) = &s.increment {
        parts.push(format!("INCREMENT BY {v}"));
    }
    if let Some(v) = &s.min_value {
        parts.push(format!("MINVALUE {v}"));
    }
    if let Some(v) = &s.max_value {
        parts.push(format!("MAXVALUE {v}"));
    }
    if let Some(v) = &s.start_with {
        parts.push(format!("START WITH {v}"));
    }
    if let Some(v) = &s.cache {
        parts.push(format!("CACHE {v}"));
    }
    if s.cycle {
        parts.push("CYCLE".to_string());
    }
    let opts = if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(" "))
    };
    format!(" GENERATED {} AS IDENTITY{}", identity.kind.to_sql(), opts)
}

/// Render a single column definition. Validates the column name.
fn render_column_def(col: &ColumnSpec, limits: &SchemaLimits) -> Result<String, SchemaError> {
    validate_identifier(&col.name, "column", limits)?;
    let mut def = format!("{} {}", quote_identifier(&col.name), col.ty.to_sql_type());
    if let Some(g) = &col.generated {
        def.push_str(&format!(" GENERATED ALWAYS AS ({}) STORED", g.expression));
    }
    if let Some(id) = &col.identity {
        def.push_str(&render_identity(id));
    }
    if !col.nullable {
        def.push_str(" NOT NULL");
    }
    if let Some(expr) = &col.default {
        def.push_str(" DEFAULT ");
        def.push_str(expr);
    }
    if let Some(u) = &col.unique {
        def.push_str(" UNIQUE");
        if u.nulls_not_distinct {
            def.push_str(" NULLS NOT DISTINCT");
        }
    }
    Ok(def)
}

fn render_fk_clause(fk: &ForeignKeySpec, limits: &SchemaLimits) -> Result<String, SchemaError> {
    for c in &fk.columns_from {
        validate_identifier(c, "column", limits)?;
    }
    for c in &fk.columns_to {
        validate_identifier(c, "column", limits)?;
    }
    let mut clause = format!(
        "FOREIGN KEY ({}) REFERENCES {} ({})",
        quote_cols(&fk.columns_from),
        quote_qualified(&fk.table_to, "table", limits)?,
        quote_cols(&fk.columns_to)
    );
    if let Some(a) = fk.on_delete {
        clause.push_str(&format!(" ON DELETE {}", a.to_sql()));
    }
    if let Some(a) = fk.on_update {
        clause.push_str(&format!(" ON UPDATE {}", a.to_sql()));
    }
    Ok(clause)
}

fn render_unique_clause(uc: &UniqueConstraintSpec) -> String {
    format!(
        "CONSTRAINT {} UNIQUE{} ({})",
        quote_identifier(&uc.name),
        if uc.nulls_not_distinct {
            " NULLS NOT DISTINCT"
        } else {
            ""
        },
        quote_cols(&uc.columns)
    )
}

/// Render an idempotent `CREATE TABLE IF NOT EXISTS` for `table`.
///
/// Validates the table name, every column name (uniqueness + grammar), the
/// column count, and that each primary-key member refers to a declared column.
/// Identifiers are quoted; types, defaults, generated expressions, and check
/// bodies are emitted verbatim (trusted, developer-authored). Inline foreign
/// keys, unique constraints, check constraints, generated and identity columns
/// are all rendered.
pub fn to_create_table_sql(
    table: &PgTableSpec,
    limits: &SchemaLimits,
) -> Result<String, SchemaError> {
    validate_identifier(&table.name, "table", limits)?;

    if table.columns.is_empty() {
        return Err(SchemaError::NoColumns {
            table: table.name.clone(),
        });
    }
    if table.columns.len() > limits.max_columns {
        return Err(SchemaError::TooManyColumns {
            table: table.name.clone(),
            count: table.columns.len(),
            max: limits.max_columns,
        });
    }

    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut col_defs = Vec::with_capacity(table.columns.len());
    for col in &table.columns {
        if !seen.insert(col.name.as_str()) {
            return Err(SchemaError::DuplicateColumn {
                table: table.name.clone(),
                name: col.name.clone(),
            });
        }
        col_defs.push(render_column_def(col, limits)?);
    }

    for pk in &table.primary_key {
        if !seen.contains(pk.as_str()) {
            return Err(SchemaError::UnknownPrimaryKeyColumn {
                table: table.name.clone(),
                name: pk.clone(),
            });
        }
    }

    let mut clauses = col_defs;
    if !table.primary_key.is_empty() {
        clauses.push(format!("PRIMARY KEY ({})", quote_cols(&table.primary_key)));
    }
    for uc in &table.unique_constraints {
        validate_identifier(&uc.name, "constraint", limits)?;
        clauses.push(render_unique_clause(uc));
    }
    for cc in &table.check_constraints {
        validate_identifier(&cc.name, "constraint", limits)?;
        clauses.push(format!(
            "CONSTRAINT {} CHECK ({})",
            quote_identifier(&cc.name),
            cc.value
        ));
    }
    for fk in &table.foreign_keys {
        validate_identifier(&fk.name, "constraint", limits)?;
        clauses.push(format!(
            "CONSTRAINT {} {}",
            quote_identifier(&fk.name),
            render_fk_clause(fk, limits)?
        ));
    }

    Ok(format!(
        "CREATE TABLE IF NOT EXISTS {} (\n    {}\n);",
        quote_identifier(&table.name),
        clauses.join(",\n    ")
    ))
}

/// Render `CREATE TYPE <name> AS ENUM (...)` for an enum-type declaration.
pub fn create_type_sql(e: &EnumTypeSpec, limits: &SchemaLimits) -> Result<String, SchemaError> {
    validate_identifier(&e.schema, "schema", limits)?;
    validate_identifier(&e.name, "type", limits)?;
    let values = e
        .values
        .iter()
        .map(|v| format!("'{}'", v.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "CREATE TYPE {} AS ENUM ({});",
        quote_qualified(&e.qualified_name(), "type", limits)?,
        values
    ))
}

/// Render `CREATE [UNIQUE] INDEX ... ON <schema>.<table> ...`.
pub fn create_index_sql(
    schema: &str,
    table: &str,
    index: &IndexSpec,
    limits: &SchemaLimits,
) -> Result<String, SchemaError> {
    validate_identifier(&index.name, "index", limits)?;
    let target = quote_qualified(&format!("{schema}.{table}"), "table", limits)?;
    let cols = index
        .columns
        .iter()
        .map(|c| {
            let mut s = if c.is_expression {
                format!("({})", c.expression)
            } else {
                validate_identifier(&c.expression, "column", limits)?;
                quote_identifier(&c.expression)
            };
            if let Some(op) = &c.opclass {
                s.push(' ');
                s.push_str(op);
            }
            s.push_str(if c.asc { " ASC" } else { " DESC" });
            if let Some(n) = &c.nulls {
                s.push_str(&format!(" NULLS {n}"));
            }
            Ok::<String, SchemaError>(s)
        })
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");

    let mut sql = format!(
        "CREATE {}INDEX {} ON {} USING {} ({})",
        if index.unique { "UNIQUE " } else { "" },
        quote_identifier(&index.name),
        target,
        index.method,
        cols
    );
    if let Some(w) = &index.where_clause {
        sql.push_str(&format!(" WHERE {w}"));
    }
    sql.push(';');
    Ok(sql)
}

/// Render `CREATE POLICY <name> ON <schema>.<table> ...`.
pub fn create_policy_sql(
    schema: &str,
    table: &str,
    policy: &PolicySpec,
    limits: &SchemaLimits,
) -> Result<String, SchemaError> {
    validate_identifier(&policy.name, "policy", limits)?;
    let target = quote_qualified(&format!("{schema}.{table}"), "table", limits)?;
    let mut sql = format!(
        "CREATE POLICY {} ON {}",
        quote_identifier(&policy.name),
        target
    );
    if let Some(a) = policy.as_ {
        sql.push_str(&format!(" AS {}", a.to_sql()));
    }
    if let Some(f) = policy.for_ {
        sql.push_str(&format!(" FOR {}", f.to_sql()));
    }
    if !policy.to.is_empty() {
        for role in &policy.to {
            validate_identifier(role, "role", limits)?;
        }
        sql.push_str(&format!(" TO {}", quote_cols(&policy.to)));
    }
    if let Some(u) = &policy.using {
        sql.push_str(&format!(" USING ({u})"));
    }
    if let Some(w) = &policy.with_check {
        sql.push_str(&format!(" WITH CHECK ({w})"));
    }
    sql.push(';');
    Ok(sql)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{
        CheckConstraintSpec, ColumnSpec, ForeignKeySpec, IndexColumn, PgTableSpec, PgType,
        PolicyFor, ReferentialAction, UniqueConstraintSpec,
    };

    fn limits() -> SchemaLimits {
        SchemaLimits::default()
    }

    #[test]
    fn renders_create_table_with_pk_and_constraints() {
        let table = PgTableSpec::new(
            "managed_websites",
            vec![
                ColumnSpec::new("id", PgType::Uuid).default_expr("gen_random_uuid()"),
                ColumnSpec::new("organization_id", PgType::Uuid),
                ColumnSpec::new("domain", PgType::Text),
                ColumnSpec::new("status", PgType::Enum("managed_website_status".into()))
                    .default_expr("'development'"),
                ColumnSpec::new("tags", PgType::Array(Box::new(PgType::Text)))
                    .default_expr("'{}'::text[]"),
                ColumnSpec::new("last_deployed_at", PgType::Timestamptz).nullable(),
            ],
        )
        .primary_key(["id"]);

        let sql = to_create_table_sql(&table, &limits()).unwrap();
        let expected = "CREATE TABLE IF NOT EXISTS \"managed_websites\" (\n    \
\"id\" uuid NOT NULL DEFAULT gen_random_uuid(),\n    \
\"organization_id\" uuid NOT NULL,\n    \
\"domain\" text NOT NULL,\n    \
\"status\" managed_website_status NOT NULL DEFAULT 'development',\n    \
\"tags\" text[] NOT NULL DEFAULT '{}'::text[],\n    \
\"last_deployed_at\" timestamptz,\n    \
PRIMARY KEY (\"id\")\n);";
        assert_eq!(sql, expected);
    }

    #[test]
    fn renders_inline_fk_unique_check_generated_identity() {
        let table = PgTableSpec::new(
            "items",
            vec![
                ColumnSpec::new("id", PgType::Int8).identity_always(),
                ColumnSpec::new("org_id", PgType::Uuid),
                ColumnSpec::new("price", PgType::Int4),
                ColumnSpec::new("total", PgType::Int4).generated_stored("price * 2"),
            ],
        )
        .primary_key(["id"])
        .foreign_key(
            ForeignKeySpec::new("fk_org", ["org_id"], "public.orgs", ["id"])
                .on_delete(ReferentialAction::Cascade),
        )
        .unique_constraint(UniqueConstraintSpec::new(
            "u_org_price",
            ["org_id", "price"],
        ))
        .check(CheckConstraintSpec::new("c_price_pos", "price > 0"));

        let sql = to_create_table_sql(&table, &limits()).unwrap();
        assert!(sql.contains("\"id\" bigint GENERATED ALWAYS AS IDENTITY NOT NULL"));
        assert!(sql.contains("\"total\" integer GENERATED ALWAYS AS (price * 2) STORED NOT NULL"));
        assert!(sql.contains(
            "CONSTRAINT \"fk_org\" FOREIGN KEY (\"org_id\") REFERENCES \"public\".\"orgs\" (\"id\") ON DELETE CASCADE"
        ));
        assert!(sql.contains("CONSTRAINT \"u_org_price\" UNIQUE (\"org_id\", \"price\")"));
        assert!(sql.contains("CONSTRAINT \"c_price_pos\" CHECK (price > 0)"));
    }

    #[test]
    fn renders_enum_index_policy() {
        let e = EnumTypeSpec::new("role", ["admin", "member"]);
        assert_eq!(
            create_type_sql(&e, &limits()).unwrap(),
            "CREATE TYPE \"public\".\"role\" AS ENUM ('admin', 'member');"
        );

        let idx = IndexSpec::new("idx_lower_email", [IndexColumn::expr("lower(email)")]).unique();
        assert_eq!(
            create_index_sql("public", "users", &idx, &limits()).unwrap(),
            "CREATE UNIQUE INDEX \"idx_lower_email\" ON \"public\".\"users\" USING btree ((lower(email)) ASC);"
        );

        let p = PolicySpec::new("p_select")
            .for_command(PolicyFor::Select)
            .using("org_id = current_org()");
        assert_eq!(
            create_policy_sql("public", "docs", &p, &limits()).unwrap(),
            "CREATE POLICY \"p_select\" ON \"public\".\"docs\" FOR SELECT USING (org_id = current_org());"
        );
    }

    #[test]
    fn rejects_empty_table() {
        let table = PgTableSpec::new("t", vec![]);
        assert!(matches!(
            to_create_table_sql(&table, &limits()),
            Err(SchemaError::NoColumns { .. })
        ));
    }

    #[test]
    fn rejects_duplicate_columns() {
        let table = PgTableSpec::new(
            "t",
            vec![
                ColumnSpec::new("a", PgType::Int4),
                ColumnSpec::new("a", PgType::Text),
            ],
        );
        assert!(matches!(
            to_create_table_sql(&table, &limits()),
            Err(SchemaError::DuplicateColumn { .. })
        ));
    }

    #[test]
    fn rejects_unknown_primary_key_column() {
        let table =
            PgTableSpec::new("t", vec![ColumnSpec::new("a", PgType::Int4)]).primary_key(["b"]);
        assert!(matches!(
            to_create_table_sql(&table, &limits()),
            Err(SchemaError::UnknownPrimaryKeyColumn { .. })
        ));
    }

    #[test]
    fn rejects_injection_in_identifiers() {
        let table = PgTableSpec::new(
            "t",
            vec![ColumnSpec::new("a\"; DROP TABLE x;--", PgType::Int4)],
        );
        assert!(matches!(
            to_create_table_sql(&table, &limits()),
            Err(SchemaError::InvalidIdentifier { .. })
        ));
    }
}
