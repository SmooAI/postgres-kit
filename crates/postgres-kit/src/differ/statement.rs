//! The lean [`DdlStatement`] IR — one variant per SQL change the differ emits,
//! each rendering to a single SQL string via [`DdlStatement::to_sql`].
//!
//! These are *not* drizzle's squashed encodings; they are keyed to the SQL we
//! actually run. The rendering here is a best-effort baseline the differ agent
//! refines against the corpus.

use crate::differ::ir::{
    SnapColumn, SnapCompositePk, SnapEnum, SnapForeignKey, SnapIdentity, SnapIndex, SnapPolicy,
    SnapSequence, SnapTable, SnapUnique, SnapView,
};
use crate::safety::quote_identifier;
use crate::spec::IdentityKind;

/// Quote a possibly schema-qualified name, quoting each dotted segment so a name
/// carrying SQL can't escape its position. `public.users` → `"public"."users"`.
fn qualify(name: &str) -> String {
    name.split('.')
        .map(quote_identifier)
        .collect::<Vec<_>>()
        .join(".")
}

/// Join and quote a list of bare column names.
fn quote_cols(cols: &[String]) -> String {
    cols.iter()
        .map(|c| quote_identifier(c))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One DDL change. Each variant renders to exactly one SQL string (the enum-value
/// cascade variant renders several statements joined by `;\n`, since Postgres has
/// no single statement for it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DdlStatement {
    // ---- tables ----
    CreateTable(SnapTable),
    DropTable {
        schema: String,
        name: String,
    },
    RenameTable {
        schema: String,
        from: String,
        to: String,
    },
    AlterTableSetSchema {
        name: String,
        from_schema: String,
        to_schema: String,
    },

    // ---- columns ----
    AddColumn {
        schema: String,
        table: String,
        column: SnapColumn,
    },
    DropColumn {
        schema: String,
        table: String,
        column: String,
    },
    RenameColumn {
        schema: String,
        table: String,
        from: String,
        to: String,
    },
    SetColumnType {
        schema: String,
        table: String,
        column: String,
        ty: String,
    },
    SetColumnDefault {
        schema: String,
        table: String,
        column: String,
        default: String,
    },
    DropColumnDefault {
        schema: String,
        table: String,
        column: String,
    },
    SetColumnNotNull {
        schema: String,
        table: String,
        column: String,
    },
    DropColumnNotNull {
        schema: String,
        table: String,
        column: String,
    },
    SetColumnPrimaryKey {
        schema: String,
        table: String,
        column: String,
    },
    DropColumnPrimaryKey {
        schema: String,
        table: String,
        /// Constraint name to drop (typically `<table>_pkey`).
        constraint: String,
    },
    SetColumnGenerated {
        schema: String,
        table: String,
        column: String,
        expression: String,
    },
    DropColumnGenerated {
        schema: String,
        table: String,
        column: String,
    },
    SetColumnIdentity {
        schema: String,
        table: String,
        column: String,
        identity: SnapIdentity,
    },
    DropColumnIdentity {
        schema: String,
        table: String,
        column: String,
    },

    // ---- enums ----
    CreateEnum(SnapEnum),
    DropEnum {
        schema: String,
        name: String,
    },
    RenameEnum {
        schema: String,
        from: String,
        to: String,
    },
    AddEnumValue {
        schema: String,
        name: String,
        value: String,
        /// Insert before this existing value (else appended).
        before: Option<String>,
    },
    /// Postgres can't drop or reorder enum values in place; rebuild the type and
    /// repoint the columns using it. Renders a multi-statement cascade.
    RecreateEnumCascade {
        enum_: SnapEnum,
        /// `(schema, table, column)` triples that reference the enum.
        using_columns: Vec<(String, String, String)>,
    },

    // ---- table-level constraints ----
    CreateCheck {
        schema: String,
        table: String,
        name: String,
        value: String,
    },
    DropCheck {
        schema: String,
        table: String,
        name: String,
    },
    CreateUnique {
        schema: String,
        table: String,
        unique: SnapUnique,
    },
    DropUnique {
        schema: String,
        table: String,
        name: String,
    },
    CreateCompositePk {
        schema: String,
        table: String,
        pk: SnapCompositePk,
    },
    DropCompositePk {
        schema: String,
        table: String,
        name: String,
    },

    // ---- foreign keys ----
    CreateForeignKey {
        schema: String,
        table: String,
        fk: SnapForeignKey,
    },
    DropForeignKey {
        schema: String,
        table: String,
        name: String,
    },
    /// Drop then recreate (Postgres has no in-place FK alter).
    AlterForeignKey {
        schema: String,
        table: String,
        fk: SnapForeignKey,
    },

    // ---- indexes ----
    CreateIndex {
        schema: String,
        table: String,
        index: SnapIndex,
    },
    DropIndex {
        schema: String,
        name: String,
    },
    /// Drop then recreate (Postgres has no in-place index alter for these).
    AlterIndex {
        schema: String,
        table: String,
        index: SnapIndex,
    },

    // ---- RLS ----
    EnableRls {
        schema: String,
        table: String,
    },
    DisableRls {
        schema: String,
        table: String,
    },

    // ---- policies ----
    CreatePolicy {
        schema: String,
        table: String,
        policy: SnapPolicy,
    },
    DropPolicy {
        schema: String,
        table: String,
        name: String,
    },
    AlterPolicy {
        schema: String,
        table: String,
        policy: SnapPolicy,
    },
    RenamePolicy {
        schema: String,
        table: String,
        from: String,
        to: String,
    },

    // ---- views ----
    CreateView(SnapView),
    DropView {
        schema: String,
        name: String,
        materialized: bool,
    },

    // ---- sequences ----
    CreateSequence(SnapSequence),
    DropSequence {
        schema: String,
        name: String,
    },
    AlterSequence(SnapSequence),

    // ---- roles ----
    CreateRole(crate::differ::ir::SnapRole),
    DropRole {
        name: String,
    },
}

impl DdlStatement {
    /// Render this statement to SQL.
    pub fn to_sql(&self) -> String {
        match self {
            DdlStatement::CreateTable(t) => render_create_table(t),
            DdlStatement::DropTable { schema, name } => {
                format!("DROP TABLE {};", qualify(&format!("{schema}.{name}")))
            }
            DdlStatement::RenameTable { schema, from, to } => format!(
                "ALTER TABLE {} RENAME TO {};",
                qualify(&format!("{schema}.{from}")),
                quote_identifier(to)
            ),
            DdlStatement::AlterTableSetSchema {
                name,
                from_schema,
                to_schema,
            } => format!(
                "ALTER TABLE {} SET SCHEMA {};",
                qualify(&format!("{from_schema}.{name}")),
                quote_identifier(to_schema)
            ),

            DdlStatement::AddColumn {
                schema,
                table,
                column,
            } => format!(
                "ALTER TABLE {} ADD COLUMN {};",
                qualify(&format!("{schema}.{table}")),
                render_column_def(column)
            ),
            DdlStatement::DropColumn {
                schema,
                table,
                column,
            } => format!(
                "ALTER TABLE {} DROP COLUMN {};",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(column)
            ),
            DdlStatement::RenameColumn {
                schema,
                table,
                from,
                to,
            } => format!(
                "ALTER TABLE {} RENAME COLUMN {} TO {};",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(from),
                quote_identifier(to)
            ),
            DdlStatement::SetColumnType {
                schema,
                table,
                column,
                ty,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} SET DATA TYPE {};",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(column),
                ty
            ),
            DdlStatement::SetColumnDefault {
                schema,
                table,
                column,
                default,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} SET DEFAULT {};",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(column),
                default
            ),
            DdlStatement::DropColumnDefault {
                schema,
                table,
                column,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} DROP DEFAULT;",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(column)
            ),
            DdlStatement::SetColumnNotNull {
                schema,
                table,
                column,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} SET NOT NULL;",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(column)
            ),
            DdlStatement::DropColumnNotNull {
                schema,
                table,
                column,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} DROP NOT NULL;",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(column)
            ),
            DdlStatement::SetColumnPrimaryKey {
                schema,
                table,
                column,
            } => format!(
                "ALTER TABLE {} ADD PRIMARY KEY ({});",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(column)
            ),
            DdlStatement::DropColumnPrimaryKey {
                schema,
                table,
                constraint,
            } => format!(
                "ALTER TABLE {} DROP CONSTRAINT {};",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(constraint)
            ),
            DdlStatement::SetColumnGenerated {
                schema,
                table,
                column,
                expression,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} ADD GENERATED ALWAYS AS ({expression}) STORED;",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(column)
            ),
            DdlStatement::DropColumnGenerated {
                schema,
                table,
                column,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} DROP EXPRESSION;",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(column)
            ),
            DdlStatement::SetColumnIdentity {
                schema,
                table,
                column,
                identity,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} ADD GENERATED {} AS IDENTITY{};",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(column),
                identity_kind_sql(identity.kind),
                render_identity_options(identity)
            ),
            DdlStatement::DropColumnIdentity {
                schema,
                table,
                column,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} DROP IDENTITY;",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(column)
            ),

            DdlStatement::CreateEnum(e) => render_create_enum(e),
            DdlStatement::DropEnum { schema, name } => {
                format!("DROP TYPE {};", qualify(&format!("{schema}.{name}")))
            }
            DdlStatement::RenameEnum { schema, from, to } => format!(
                "ALTER TYPE {} RENAME TO {};",
                qualify(&format!("{schema}.{from}")),
                quote_identifier(to)
            ),
            DdlStatement::AddEnumValue {
                schema,
                name,
                value,
                before,
            } => {
                let pos = match before {
                    Some(b) => format!(" BEFORE '{}'", escape_literal(b)),
                    None => String::new(),
                };
                format!(
                    "ALTER TYPE {} ADD VALUE '{}'{};",
                    qualify(&format!("{schema}.{name}")),
                    escape_literal(value),
                    pos
                )
            }
            DdlStatement::RecreateEnumCascade {
                enum_,
                using_columns,
            } => render_recreate_enum_cascade(enum_, using_columns),

            DdlStatement::CreateCheck {
                schema,
                table,
                name,
                value,
            } => format!(
                "ALTER TABLE {} ADD CONSTRAINT {} CHECK ({value});",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(name)
            ),
            DdlStatement::DropCheck {
                schema,
                table,
                name,
            } => format!(
                "ALTER TABLE {} DROP CONSTRAINT {};",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(name)
            ),
            DdlStatement::CreateUnique {
                schema,
                table,
                unique,
            } => format!(
                "ALTER TABLE {} ADD CONSTRAINT {} UNIQUE{} ({});",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(&unique.name),
                if unique.nulls_not_distinct {
                    " NULLS NOT DISTINCT"
                } else {
                    ""
                },
                quote_cols(&unique.columns)
            ),
            DdlStatement::DropUnique {
                schema,
                table,
                name,
            } => format!(
                "ALTER TABLE {} DROP CONSTRAINT {};",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(name)
            ),
            DdlStatement::CreateCompositePk { schema, table, pk } => format!(
                "ALTER TABLE {} ADD CONSTRAINT {} PRIMARY KEY ({});",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(&pk.name),
                quote_cols(&pk.columns)
            ),
            DdlStatement::DropCompositePk {
                schema,
                table,
                name,
            } => format!(
                "ALTER TABLE {} DROP CONSTRAINT {};",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(name)
            ),

            DdlStatement::CreateForeignKey { schema, table, fk } => format!(
                "ALTER TABLE {} ADD CONSTRAINT {} {};",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(&fk.name),
                render_fk_clause(fk)
            ),
            DdlStatement::DropForeignKey {
                schema,
                table,
                name,
            } => format!(
                "ALTER TABLE {} DROP CONSTRAINT {};",
                qualify(&format!("{schema}.{table}")),
                quote_identifier(name)
            ),
            DdlStatement::AlterForeignKey { schema, table, fk } => {
                let t = qualify(&format!("{schema}.{table}"));
                format!(
                    "ALTER TABLE {t} DROP CONSTRAINT {};\nALTER TABLE {t} ADD CONSTRAINT {} {};",
                    quote_identifier(&fk.name),
                    quote_identifier(&fk.name),
                    render_fk_clause(fk)
                )
            }

            DdlStatement::CreateIndex {
                schema,
                table,
                index,
            } => render_create_index(schema, table, index),
            DdlStatement::DropIndex { schema, name } => {
                format!("DROP INDEX {};", qualify(&format!("{schema}.{name}")))
            }
            DdlStatement::AlterIndex {
                schema,
                table,
                index,
            } => format!(
                "DROP INDEX {};\n{}",
                qualify(&format!("{schema}.{}", index.name)),
                render_create_index(schema, table, index)
            ),

            DdlStatement::EnableRls { schema, table } => format!(
                "ALTER TABLE {} ENABLE ROW LEVEL SECURITY;",
                qualify(&format!("{schema}.{table}"))
            ),
            DdlStatement::DisableRls { schema, table } => format!(
                "ALTER TABLE {} DISABLE ROW LEVEL SECURITY;",
                qualify(&format!("{schema}.{table}"))
            ),

            DdlStatement::CreatePolicy {
                schema,
                table,
                policy,
            } => render_create_policy(schema, table, policy),
            DdlStatement::DropPolicy {
                schema,
                table,
                name,
            } => format!(
                "DROP POLICY {} ON {};",
                quote_identifier(name),
                qualify(&format!("{schema}.{table}"))
            ),
            DdlStatement::AlterPolicy {
                schema,
                table,
                policy,
            } => render_alter_policy(schema, table, policy),
            DdlStatement::RenamePolicy {
                schema,
                table,
                from,
                to,
            } => format!(
                "ALTER POLICY {} ON {} RENAME TO {};",
                quote_identifier(from),
                qualify(&format!("{schema}.{table}")),
                quote_identifier(to)
            ),

            DdlStatement::CreateView(v) => render_create_view(v),
            DdlStatement::DropView {
                schema,
                name,
                materialized,
            } => format!(
                "DROP {}VIEW {};",
                if *materialized { "MATERIALIZED " } else { "" },
                qualify(&format!("{schema}.{name}"))
            ),

            DdlStatement::CreateSequence(s) => render_create_sequence("CREATE SEQUENCE", s),
            DdlStatement::DropSequence { schema, name } => {
                format!("DROP SEQUENCE {};", qualify(&format!("{schema}.{name}")))
            }
            DdlStatement::AlterSequence(s) => render_create_sequence("ALTER SEQUENCE", s),

            DdlStatement::CreateRole(r) => render_create_role(r),
            DdlStatement::DropRole { name } => format!("DROP ROLE {};", quote_identifier(name)),
        }
    }
}

fn identity_kind_sql(kind: IdentityKind) -> &'static str {
    kind.to_sql()
}

fn escape_literal(s: &str) -> String {
    s.replace('\'', "''")
}

/// Render a full column definition (`"name" type [modifiers]`), shared by
/// `CREATE TABLE` and `ADD COLUMN`.
pub fn render_column_def(col: &SnapColumn) -> String {
    let mut def = format!("{} {}", quote_identifier(&col.name), col.ty);
    if let Some(expr) = &col.generated {
        def.push_str(&format!(" GENERATED ALWAYS AS ({expr}) STORED"));
    }
    if let Some(id) = &col.identity {
        def.push_str(&format!(
            " GENERATED {} AS IDENTITY{}",
            identity_kind_sql(id.kind),
            render_identity_options(id)
        ));
    }
    if col.not_null {
        def.push_str(" NOT NULL");
    }
    if let Some(default) = &col.default {
        def.push_str(&format!(" DEFAULT {default}"));
    }
    if let Some(u) = &col.unique {
        def.push_str(" UNIQUE");
        if u.nulls_not_distinct {
            def.push_str(" NULLS NOT DISTINCT");
        }
    }
    def
}

fn render_identity_options(id: &SnapIdentity) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(v) = &id.increment {
        parts.push(format!("INCREMENT BY {v}"));
    }
    if let Some(v) = &id.min_value {
        parts.push(format!("MINVALUE {v}"));
    }
    if let Some(v) = &id.max_value {
        parts.push(format!("MAXVALUE {v}"));
    }
    if let Some(v) = &id.start_with {
        parts.push(format!("START WITH {v}"));
    }
    if let Some(v) = &id.cache {
        parts.push(format!("CACHE {v}"));
    }
    if id.cycle {
        parts.push("CYCLE".to_string());
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(" "))
    }
}

fn render_fk_clause(fk: &SnapForeignKey) -> String {
    let mut clause = format!(
        "FOREIGN KEY ({}) REFERENCES {} ({})",
        quote_cols(&fk.columns_from),
        qualify(&fk.table_to),
        quote_cols(&fk.columns_to)
    );
    if let Some(a) = &fk.on_delete {
        clause.push_str(&format!(" ON DELETE {a}"));
    }
    if let Some(a) = &fk.on_update {
        clause.push_str(&format!(" ON UPDATE {a}"));
    }
    clause
}

fn render_create_table(t: &SnapTable) -> String {
    let mut parts: Vec<String> = Vec::new();
    for col in t.columns.values() {
        parts.push(render_column_def(col));
    }
    for pk in t.composite_primary_keys.values() {
        parts.push(format!(
            "CONSTRAINT {} PRIMARY KEY ({})",
            quote_identifier(&pk.name),
            quote_cols(&pk.columns)
        ));
    }
    for uc in t.unique_constraints.values() {
        parts.push(format!(
            "CONSTRAINT {} UNIQUE{} ({})",
            quote_identifier(&uc.name),
            if uc.nulls_not_distinct {
                " NULLS NOT DISTINCT"
            } else {
                ""
            },
            quote_cols(&uc.columns)
        ));
    }
    for cc in t.check_constraints.values() {
        parts.push(format!(
            "CONSTRAINT {} CHECK ({})",
            quote_identifier(&cc.name),
            cc.value
        ));
    }
    for fk in t.foreign_keys.values() {
        parts.push(format!(
            "CONSTRAINT {} {}",
            quote_identifier(&fk.name),
            render_fk_clause(fk)
        ));
    }
    format!(
        "CREATE TABLE {} (\n    {}\n);",
        qualify(&t.key()),
        parts.join(",\n    ")
    )
}

fn render_create_enum(e: &SnapEnum) -> String {
    let values = e
        .values
        .iter()
        .map(|v| format!("'{}'", escape_literal(v)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("CREATE TYPE {} AS ENUM ({});", qualify(&e.key()), values)
}

fn render_recreate_enum_cascade(
    e: &SnapEnum,
    using_columns: &[(String, String, String)],
) -> String {
    let qname = qualify(&e.key());
    let tmp = qualify(&format!("{}.{}__new", e.schema, e.name));
    let mut stmts: Vec<String> = Vec::new();
    stmts.push(render_create_enum(&SnapEnum {
        schema: e.schema.clone(),
        name: format!("{}__new", e.name),
        values: e.values.clone(),
    }));
    for (schema, table, column) in using_columns {
        stmts.push(format!(
            "ALTER TABLE {} ALTER COLUMN {} TYPE {} USING {}::text::{};",
            qualify(&format!("{schema}.{table}")),
            quote_identifier(column),
            tmp,
            quote_identifier(column),
            tmp
        ));
    }
    stmts.push(format!("DROP TYPE {qname};"));
    stmts.push(format!(
        "ALTER TYPE {tmp} RENAME TO {};",
        quote_identifier(&e.name)
    ));
    stmts.join("\n")
}

fn render_create_index(schema: &str, table: &str, index: &SnapIndex) -> String {
    let cols = index
        .columns
        .iter()
        .map(|c| {
            let mut s = if c.is_expression {
                format!("({})", c.expression)
            } else {
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
            s
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut sql = format!(
        "CREATE {}INDEX {} ON {} USING {} ({})",
        if index.unique { "UNIQUE " } else { "" },
        quote_identifier(&index.name),
        qualify(&format!("{schema}.{table}")),
        index.method,
        cols
    );
    if let Some(w) = &index.where_clause {
        sql.push_str(&format!(" WHERE {w}"));
    }
    sql.push(';');
    sql
}

fn render_create_policy(schema: &str, table: &str, policy: &SnapPolicy) -> String {
    let mut sql = format!(
        "CREATE POLICY {} ON {}",
        quote_identifier(&policy.name),
        qualify(&format!("{schema}.{table}"))
    );
    if let Some(a) = policy.as_ {
        sql.push_str(&format!(" AS {}", a.to_sql()));
    }
    if let Some(f) = policy.for_ {
        sql.push_str(&format!(" FOR {}", f.to_sql()));
    }
    if !policy.to.is_empty() {
        sql.push_str(&format!(" TO {}", quote_cols(&policy.to)));
    }
    if let Some(u) = &policy.using {
        sql.push_str(&format!(" USING ({u})"));
    }
    if let Some(w) = &policy.with_check {
        sql.push_str(&format!(" WITH CHECK ({w})"));
    }
    sql.push(';');
    sql
}

fn render_alter_policy(schema: &str, table: &str, policy: &SnapPolicy) -> String {
    let mut sql = format!(
        "ALTER POLICY {} ON {}",
        quote_identifier(&policy.name),
        qualify(&format!("{schema}.{table}"))
    );
    if !policy.to.is_empty() {
        sql.push_str(&format!(" TO {}", quote_cols(&policy.to)));
    }
    if let Some(u) = &policy.using {
        sql.push_str(&format!(" USING ({u})"));
    }
    if let Some(w) = &policy.with_check {
        sql.push_str(&format!(" WITH CHECK ({w})"));
    }
    sql.push(';');
    sql
}

fn render_create_view(v: &SnapView) -> String {
    let definition = v.definition.clone().unwrap_or_default();
    format!(
        "CREATE {}VIEW {} AS {};",
        if v.materialized { "MATERIALIZED " } else { "" },
        qualify(&v.key()),
        definition
    )
}

fn render_create_sequence(verb: &str, s: &SnapSequence) -> String {
    let mut sql = format!("{verb} {}", qualify(&s.key()));
    if let Some(v) = &s.increment {
        sql.push_str(&format!(" INCREMENT BY {v}"));
    }
    if let Some(v) = &s.min_value {
        sql.push_str(&format!(" MINVALUE {v}"));
    }
    if let Some(v) = &s.max_value {
        sql.push_str(&format!(" MAXVALUE {v}"));
    }
    if let Some(v) = &s.start_with {
        sql.push_str(&format!(" START WITH {v}"));
    }
    if let Some(v) = &s.cache {
        sql.push_str(&format!(" CACHE {v}"));
    }
    if s.cycle {
        sql.push_str(" CYCLE");
    }
    sql.push(';');
    sql
}

fn render_create_role(r: &crate::differ::ir::SnapRole) -> String {
    let mut opts: Vec<String> = Vec::new();
    opts.push(
        if r.create_db {
            "CREATEDB"
        } else {
            "NOCREATEDB"
        }
        .to_string(),
    );
    opts.push(
        if r.create_role {
            "CREATEROLE"
        } else {
            "NOCREATEROLE"
        }
        .to_string(),
    );
    opts.push(if r.inherit { "INHERIT" } else { "NOINHERIT" }.to_string());
    format!(
        "CREATE ROLE {} WITH {};",
        quote_identifier(&r.name),
        opts.join(" ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::differ::ir::{SnapColumn, SnapIndexColumn, SnapRole};

    #[test]
    fn drop_table_quotes_qualified() {
        let s = DdlStatement::DropTable {
            schema: "public".into(),
            name: "users".into(),
        };
        assert_eq!(s.to_sql(), r#"DROP TABLE "public"."users";"#);
    }

    #[test]
    fn add_column_renders_modifiers() {
        let s = DdlStatement::AddColumn {
            schema: "public".into(),
            table: "users".into(),
            column: SnapColumn::new("email", "text").not_null().default("''"),
        };
        assert_eq!(
            s.to_sql(),
            r#"ALTER TABLE "public"."users" ADD COLUMN "email" text NOT NULL DEFAULT '';"#
        );
    }

    #[test]
    fn create_index_renders_method_and_where() {
        let s = DdlStatement::CreateIndex {
            schema: "public".into(),
            table: "users".into(),
            index: SnapIndex::new("idx_email", [SnapIndexColumn::column("email")])
                .unique()
                .where_clause("deleted_at IS NULL"),
        };
        assert_eq!(
            s.to_sql(),
            r#"CREATE UNIQUE INDEX "idx_email" ON "public"."users" USING btree ("email" ASC) WHERE deleted_at IS NULL;"#
        );
    }

    #[test]
    fn create_policy_renders_clauses() {
        let s = DdlStatement::CreatePolicy {
            schema: "public".into(),
            table: "docs".into(),
            policy: SnapPolicy::new("p").using("org_id = current_org()"),
        };
        assert_eq!(
            s.to_sql(),
            r#"CREATE POLICY "p" ON "public"."docs" USING (org_id = current_org());"#
        );
    }

    #[test]
    fn create_role_renders_options() {
        let s = DdlStatement::CreateRole(SnapRole::new("app").create_db(true));
        assert_eq!(
            s.to_sql(),
            r#"CREATE ROLE "app" WITH CREATEDB NOCREATEROLE INHERIT;"#
        );
    }

    #[test]
    fn add_enum_value_escapes_and_positions() {
        let s = DdlStatement::AddEnumValue {
            schema: "public".into(),
            name: "role".into(),
            value: "super".into(),
            before: Some("admin".into()),
        };
        assert_eq!(
            s.to_sql(),
            r#"ALTER TYPE "public"."role" ADD VALUE 'super' BEFORE 'admin';"#
        );
    }
}
