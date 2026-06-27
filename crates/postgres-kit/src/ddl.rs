//! `CREATE TABLE` generation from a [`PgTableSpec`].

use std::collections::BTreeSet;

use crate::safety::{quote_identifier, validate_identifier, SchemaError, SchemaLimits};
use crate::spec::PgTableSpec;

/// Render an idempotent `CREATE TABLE IF NOT EXISTS` for `table`.
///
/// Validates the table name, every column name (uniqueness + grammar), the
/// column count, and that each primary-key member refers to a declared column.
/// Identifiers are quoted; types and default expressions are emitted verbatim
/// (trusted, developer-authored).
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
        validate_identifier(&col.name, "column", limits)?;
        if !seen.insert(col.name.as_str()) {
            return Err(SchemaError::DuplicateColumn {
                table: table.name.clone(),
                name: col.name.clone(),
            });
        }
        let mut def = format!("{} {}", quote_identifier(&col.name), col.ty.to_sql_type());
        if !col.nullable {
            def.push_str(" NOT NULL");
        }
        if let Some(expr) = &col.default {
            def.push_str(" DEFAULT ");
            def.push_str(expr);
        }
        col_defs.push(def);
    }

    for pk in &table.primary_key {
        if !seen.contains(pk.as_str()) {
            return Err(SchemaError::UnknownPrimaryKeyColumn {
                table: table.name.clone(),
                name: pk.clone(),
            });
        }
    }

    let mut body = col_defs.join(",\n    ");
    if !table.primary_key.is_empty() {
        let pk_cols = table
            .primary_key
            .iter()
            .map(|c| quote_identifier(c))
            .collect::<Vec<_>>()
            .join(", ");
        body.push_str(&format!(",\n    PRIMARY KEY ({pk_cols})"));
    }

    Ok(format!(
        "CREATE TABLE IF NOT EXISTS {} (\n    {}\n);",
        quote_identifier(&table.name),
        body
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{ColumnSpec, PgTableSpec, PgType};

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
