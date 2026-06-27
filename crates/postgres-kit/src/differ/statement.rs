//! The lean [`DdlStatement`] IR — one variant per SQL change the differ emits,
//! each rendering to a single SQL string via [`DdlStatement::to_sql`].
//!
//! Rendering matches drizzle-kit's `sqlStatements` output token-for-token (the
//! differ conformance corpus compares against it under whitespace-normalized
//! equality). Notable drizzle conventions reproduced here:
//!
//! - **`public` schema is implicit for relations** (tables, indexes, policies):
//!   `CREATE TABLE "users"`, not `"public"."users"`. Types, views, sequences and
//!   foreign-key *targets* are always schema-qualified.
//! - Constraint column lists are comma-joined with **no spaces** and no space
//!   before the paren (`PRIMARY KEY("a","b")`); `FOREIGN KEY (...)` /
//!   `CHECK (...)` keep their space.
//! - `DROP TABLE`/`DROP POLICY` carry `CASCADE`; FKs always render
//!   `ON DELETE ... ON UPDATE ...` (defaulting to `no action`).

use crate::differ::ir::{
    SnapColumn, SnapCompositePk, SnapEnum, SnapForeignKey, SnapIdentity, SnapIndex, SnapPolicy,
    SnapRole, SnapSequence, SnapTable, SnapUnique, SnapView,
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

/// Qualify a relation (table/index/policy target) the way drizzle does: the
/// `public` schema is left implicit, every other schema is rendered.
fn qualify_table(schema: &str, name: &str) -> String {
    if schema == "public" {
        quote_identifier(name)
    } else {
        format!("{}.{}", quote_identifier(schema), quote_identifier(name))
    }
}

/// Join and quote a list of bare column names with no spaces (constraint lists).
fn quote_cols_tight(cols: &[String]) -> String {
    cols.iter()
        .map(|c| quote_identifier(c))
        .collect::<Vec<_>>()
        .join(",")
}

/// Render a policy `TO` role list. Postgres role keywords stay unquoted; named
/// roles are quoted. An empty list defaults to `public`.
fn render_roles(roles: &[String]) -> String {
    if roles.is_empty() {
        return "public".to_string();
    }
    roles
        .iter()
        .map(|r| {
            if matches!(
                r.as_str(),
                "public" | "current_role" | "current_user" | "session_user"
            ) {
                r.clone()
            } else {
                quote_identifier(r)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// One DDL change. Each variant renders to exactly one SQL string.
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
    /// Lowercase `drop column` form drizzle emits when *recreating* a column
    /// (e.g. a changed generated expression), as a drop+add pair.
    DropColumnForRecreate {
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
    /// `ALTER COLUMN ... SET DATA TYPE <ty> USING <expr>` — the form used by the
    /// enum recreate cascade to repoint a column at the rebuilt type.
    SetColumnTypeUsing {
        schema: String,
        table: String,
        column: String,
        ty: String,
        using: String,
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
    /// `ALTER COLUMN ... SET GENERATED { ALWAYS | BY DEFAULT }`.
    SetColumnIdentityGenerated {
        schema: String,
        table: String,
        column: String,
        kind: IdentityKind,
    },
    /// `ALTER COLUMN ... SET <clause>` for a single identity option, where
    /// `clause` is e.g. `START WITH 100` or `CACHE 10`.
    SetColumnIdentityOption {
        schema: String,
        table: String,
        column: String,
        clause: String,
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
    AlterEnumSetSchema {
        name: String,
        from_schema: String,
        to_schema: String,
    },
    AddEnumValue {
        schema: String,
        name: String,
        value: String,
        /// Insert before this existing value (else appended).
        before: Option<String>,
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
    RenameView {
        schema: String,
        from: String,
        to: String,
        materialized: bool,
    },

    // ---- sequences ----
    CreateSequence(SnapSequence),
    DropSequence {
        schema: String,
        name: String,
    },
    AlterSequence(SnapSequence),
    RenameSequence {
        schema: String,
        from: String,
        to: String,
    },
    AlterSequenceSetSchema {
        name: String,
        from_schema: String,
        to_schema: String,
    },

    // ---- roles ----
    CreateRole(SnapRole),
    DropRole {
        name: String,
    },
    AlterRole(SnapRole),
    RenameRole {
        from: String,
        to: String,
    },
}

impl DdlStatement {
    /// Render this statement to SQL.
    pub fn to_sql(&self) -> String {
        match self {
            DdlStatement::CreateTable(t) => render_create_table(t),
            DdlStatement::DropTable { schema, name } => {
                format!("DROP TABLE {} CASCADE;", qualify_table(schema, name))
            }
            DdlStatement::RenameTable { schema, from, to } => format!(
                "ALTER TABLE {} RENAME TO {};",
                qualify_table(schema, from),
                quote_identifier(to)
            ),
            DdlStatement::AlterTableSetSchema {
                name,
                from_schema,
                to_schema,
            } => format!(
                "ALTER TABLE {} SET SCHEMA {};",
                qualify_table(from_schema, name),
                quote_identifier(to_schema)
            ),

            DdlStatement::AddColumn {
                schema,
                table,
                column,
            } => format!(
                "ALTER TABLE {} ADD COLUMN {};",
                qualify_table(schema, table),
                render_column_def(table, column)
            ),
            DdlStatement::DropColumn {
                schema,
                table,
                column,
            } => format!(
                "ALTER TABLE {} DROP COLUMN {};",
                qualify_table(schema, table),
                quote_identifier(column)
            ),
            DdlStatement::DropColumnForRecreate {
                schema,
                table,
                column,
            } => format!(
                "ALTER TABLE {} drop column {};",
                qualify_table(schema, table),
                quote_identifier(column)
            ),
            DdlStatement::RenameColumn {
                schema,
                table,
                from,
                to,
            } => format!(
                "ALTER TABLE {} RENAME COLUMN {} TO {};",
                qualify_table(schema, table),
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
                qualify_table(schema, table),
                quote_identifier(column),
                ty
            ),
            DdlStatement::SetColumnTypeUsing {
                schema,
                table,
                column,
                ty,
                using,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} SET DATA TYPE {ty} USING {using};",
                qualify_table(schema, table),
                quote_identifier(column)
            ),
            DdlStatement::SetColumnDefault {
                schema,
                table,
                column,
                default,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} SET DEFAULT {};",
                qualify_table(schema, table),
                quote_identifier(column),
                default
            ),
            DdlStatement::DropColumnDefault {
                schema,
                table,
                column,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} DROP DEFAULT;",
                qualify_table(schema, table),
                quote_identifier(column)
            ),
            DdlStatement::SetColumnNotNull {
                schema,
                table,
                column,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} SET NOT NULL;",
                qualify_table(schema, table),
                quote_identifier(column)
            ),
            DdlStatement::DropColumnNotNull {
                schema,
                table,
                column,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} DROP NOT NULL;",
                qualify_table(schema, table),
                quote_identifier(column)
            ),
            DdlStatement::SetColumnPrimaryKey {
                schema,
                table,
                column,
            } => format!(
                "ALTER TABLE {} ADD PRIMARY KEY ({});",
                qualify_table(schema, table),
                quote_identifier(column)
            ),
            DdlStatement::DropColumnPrimaryKey {
                schema,
                table,
                constraint,
            } => format!(
                "ALTER TABLE {} DROP CONSTRAINT {};",
                qualify_table(schema, table),
                quote_identifier(constraint)
            ),
            DdlStatement::SetColumnGenerated {
                schema,
                table,
                column,
                expression,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} ADD GENERATED ALWAYS AS ({expression}) STORED;",
                qualify_table(schema, table),
                quote_identifier(column)
            ),
            DdlStatement::DropColumnGenerated {
                schema,
                table,
                column,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} DROP EXPRESSION;",
                qualify_table(schema, table),
                quote_identifier(column)
            ),
            DdlStatement::SetColumnIdentity {
                schema,
                table,
                column,
                identity,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} ADD{};",
                qualify_table(schema, table),
                quote_identifier(column),
                render_identity_inline(table, column, identity)
            ),
            DdlStatement::DropColumnIdentity {
                schema,
                table,
                column,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} DROP IDENTITY;",
                qualify_table(schema, table),
                quote_identifier(column)
            ),
            DdlStatement::SetColumnIdentityGenerated {
                schema,
                table,
                column,
                kind,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} SET GENERATED {};",
                qualify_table(schema, table),
                quote_identifier(column),
                kind.to_sql()
            ),
            DdlStatement::SetColumnIdentityOption {
                schema,
                table,
                column,
                clause,
            } => format!(
                "ALTER TABLE {} ALTER COLUMN {} SET {clause};",
                qualify_table(schema, table),
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
            DdlStatement::AlterEnumSetSchema {
                name,
                from_schema,
                to_schema,
            } => format!(
                "ALTER TYPE {} SET SCHEMA {};",
                qualify(&format!("{from_schema}.{name}")),
                quote_identifier(to_schema)
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

            DdlStatement::CreateCheck {
                schema,
                table,
                name,
                value,
            } => format!(
                "ALTER TABLE {} ADD CONSTRAINT {} CHECK ({value});",
                qualify_table(schema, table),
                quote_identifier(name)
            ),
            DdlStatement::DropCheck {
                schema,
                table,
                name,
            } => format!(
                "ALTER TABLE {} DROP CONSTRAINT {};",
                qualify_table(schema, table),
                quote_identifier(name)
            ),
            DdlStatement::CreateUnique {
                schema,
                table,
                unique,
            } => format!(
                "ALTER TABLE {} ADD CONSTRAINT {} UNIQUE{}({});",
                qualify_table(schema, table),
                quote_identifier(&unique.name),
                if unique.nulls_not_distinct {
                    " NULLS NOT DISTINCT"
                } else {
                    ""
                },
                quote_cols_tight(&unique.columns)
            ),
            DdlStatement::DropUnique {
                schema,
                table,
                name,
            } => format!(
                "ALTER TABLE {} DROP CONSTRAINT {};",
                qualify_table(schema, table),
                quote_identifier(name)
            ),
            DdlStatement::CreateCompositePk { schema, table, pk } => format!(
                "ALTER TABLE {} ADD CONSTRAINT {} PRIMARY KEY({});",
                qualify_table(schema, table),
                quote_identifier(&pk.name),
                quote_cols_tight(&pk.columns)
            ),
            DdlStatement::DropCompositePk {
                schema,
                table,
                name,
            } => format!(
                "ALTER TABLE {} DROP CONSTRAINT {};",
                qualify_table(schema, table),
                quote_identifier(name)
            ),

            DdlStatement::CreateForeignKey { schema, table, fk } => format!(
                "ALTER TABLE {} ADD CONSTRAINT {} {};",
                qualify_table(schema, table),
                quote_identifier(&fk.name),
                render_fk_clause(fk)
            ),
            DdlStatement::DropForeignKey {
                schema,
                table,
                name,
            } => format!(
                "ALTER TABLE {} DROP CONSTRAINT {};",
                qualify_table(schema, table),
                quote_identifier(name)
            ),
            DdlStatement::AlterForeignKey { schema, table, fk } => {
                let t = qualify_table(schema, table);
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
                qualify_table(schema, table)
            ),
            DdlStatement::DisableRls { schema, table } => format!(
                "ALTER TABLE {} DISABLE ROW LEVEL SECURITY;",
                qualify_table(schema, table)
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
                "DROP POLICY {} ON {} CASCADE;",
                quote_identifier(name),
                qualify_table(schema, table)
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
                qualify_table(schema, table),
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
            DdlStatement::RenameView {
                schema,
                from,
                to,
                materialized,
            } => format!(
                "ALTER {}VIEW {} RENAME TO {};",
                if *materialized { "MATERIALIZED " } else { "" },
                qualify(&format!("{schema}.{from}")),
                quote_identifier(to)
            ),

            DdlStatement::CreateSequence(s) => render_sequence("CREATE SEQUENCE", s),
            DdlStatement::DropSequence { schema, name } => {
                format!("DROP SEQUENCE {};", qualify(&format!("{schema}.{name}")))
            }
            DdlStatement::AlterSequence(s) => render_sequence("ALTER SEQUENCE", s),
            DdlStatement::RenameSequence { schema, from, to } => format!(
                "ALTER SEQUENCE {} RENAME TO {};",
                qualify(&format!("{schema}.{from}")),
                quote_identifier(to)
            ),
            DdlStatement::AlterSequenceSetSchema {
                name,
                from_schema,
                to_schema,
            } => format!(
                "ALTER SEQUENCE {} SET SCHEMA {};",
                qualify(&format!("{from_schema}.{name}")),
                quote_identifier(to_schema)
            ),

            DdlStatement::CreateRole(r) => render_create_role(r),
            DdlStatement::DropRole { name } => format!("DROP ROLE {};", quote_identifier(name)),
            DdlStatement::AlterRole(r) => render_alter_role(r),
            DdlStatement::RenameRole { from, to } => format!(
                "ALTER ROLE {} RENAME TO {};",
                quote_identifier(from),
                quote_identifier(to)
            ),
        }
    }
}

fn escape_literal(s: &str) -> String {
    s.replace('\'', "''")
}

/// Render a full column definition (`"name" type [modifiers]`), shared by
/// `CREATE TABLE` and `ADD COLUMN`. `table` names the owning table (needed to
/// derive an identity column's implicit sequence name).
pub fn render_column_def(table: &str, col: &SnapColumn) -> String {
    let mut def = format!("{} {}", quote_identifier(&col.name), col.ty);
    if let Some(expr) = &col.generated {
        def.push_str(&format!(" GENERATED ALWAYS AS ({expr}) STORED"));
    }
    if let Some(id) = &col.identity {
        def.push_str(&render_identity_inline(table, &col.name, id));
    }
    if col.primary_key {
        def.push_str(" PRIMARY KEY");
    }
    // An identity column is implicitly NOT NULL — drizzle never renders the
    // keyword for it.
    if col.not_null && col.identity.is_none() {
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

/// Render the ` GENERATED <kind> AS IDENTITY (sequence name ... )` clause, filling
/// the Postgres `integer` identity defaults drizzle bakes in.
fn render_identity_inline(table: &str, column: &str, id: &SnapIdentity) -> String {
    let seq_name = format!("{table}_{column}_seq");
    let inc = id.increment.as_deref().unwrap_or("1");
    let min = id.min_value.as_deref().unwrap_or("1");
    let max = id.max_value.as_deref().unwrap_or("2147483647");
    let start = id.start_with.as_deref().unwrap_or("1");
    let cache = id.cache.as_deref().unwrap_or("1");
    let mut s = format!(
        " GENERATED {} AS IDENTITY (sequence name {} INCREMENT BY {inc} MINVALUE {min} MAXVALUE {max} START WITH {start} CACHE {cache}",
        id.kind.to_sql(),
        quote_identifier(&seq_name)
    );
    if id.cycle {
        s.push_str(" CYCLE");
    }
    s.push(')');
    s
}

fn render_fk_clause(fk: &SnapForeignKey) -> String {
    let mut clause = format!(
        "FOREIGN KEY ({}) REFERENCES {}({})",
        quote_cols_tight(&fk.columns_from),
        qualify(&fk.table_to),
        quote_cols_tight(&fk.columns_to)
    );
    clause.push_str(&format!(
        " ON DELETE {}",
        fk.on_delete.as_deref().unwrap_or("no action")
    ));
    clause.push_str(&format!(
        " ON UPDATE {}",
        fk.on_update.as_deref().unwrap_or("no action")
    ));
    clause
}

fn render_create_table(t: &SnapTable) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut cols: Vec<&SnapColumn> = t.columns.values().collect();
    cols.sort_by_key(|c| c.position);
    for col in cols {
        parts.push(render_column_def(&t.name, col));
    }
    for pk in t.composite_primary_keys.values() {
        parts.push(format!(
            "CONSTRAINT {} PRIMARY KEY({})",
            quote_identifier(&pk.name),
            quote_cols_tight(&pk.columns)
        ));
    }
    for uc in t.unique_constraints.values() {
        parts.push(format!(
            "CONSTRAINT {} UNIQUE{}({})",
            quote_identifier(&uc.name),
            if uc.nulls_not_distinct {
                " NULLS NOT DISTINCT"
            } else {
                ""
            },
            quote_cols_tight(&uc.columns)
        ));
    }
    for cc in t.check_constraints.values() {
        parts.push(format!(
            "CONSTRAINT {} CHECK ({})",
            quote_identifier(&cc.name),
            cc.value
        ));
    }
    format!(
        "CREATE TABLE {} (\n\t{}\n);\n",
        qualify_table(&t.schema, &t.name),
        parts.join(",\n\t")
    )
}

fn render_create_enum(e: &SnapEnum) -> String {
    let values = e
        .values
        .iter()
        .map(|v| format!("'{}'", escape_literal(v)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("CREATE TYPE {} AS ENUM({});", qualify(&e.key()), values)
}

fn render_create_index(schema: &str, table: &str, index: &SnapIndex) -> String {
    let cols = index
        .columns
        .iter()
        .map(|c| {
            let mut s = if c.is_expression {
                c.expression.clone()
            } else {
                quote_identifier(&c.expression)
            };
            if let Some(op) = &c.opclass {
                s.push(' ');
                s.push_str(op);
            }
            if !c.asc {
                s.push_str(" DESC");
            }
            if let Some(n) = &c.nulls {
                s.push_str(&format!(" NULLS {n}"));
            }
            s
        })
        .collect::<Vec<_>>()
        .join(",");
    let mut sql = format!(
        "CREATE {}INDEX {} ON {} USING {} ({})",
        if index.unique { "UNIQUE " } else { "" },
        quote_identifier(&index.name),
        qualify_table(schema, table),
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
    let as_ = policy.as_.map(|a| a.to_sql()).unwrap_or("PERMISSIVE");
    let for_ = policy.for_.map(|f| f.to_sql()).unwrap_or("ALL");
    let mut sql = format!(
        "CREATE POLICY {} ON {} AS {as_} FOR {for_} TO {}",
        quote_identifier(&policy.name),
        qualify_table(schema, table),
        render_roles(&policy.to)
    );
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
        "ALTER POLICY {} ON {} TO {}",
        quote_identifier(&policy.name),
        qualify_table(schema, table),
        render_roles(&policy.to)
    );
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
        "CREATE {}VIEW {} AS ({definition});",
        if v.materialized { "MATERIALIZED " } else { "" },
        qualify(&v.key())
    )
}

fn render_sequence(verb: &str, s: &SnapSequence) -> String {
    let inc = s.increment.as_deref().unwrap_or("1");
    let min = s.min_value.as_deref().unwrap_or("1");
    let max = s.max_value.as_deref().unwrap_or("9223372036854775807");
    let cache = s.cache.as_deref().unwrap_or("1");
    let mut sql = format!(
        "{verb} {} INCREMENT BY {inc} MINVALUE {min} MAXVALUE {max}",
        qualify(&s.key())
    );
    if let Some(start) = &s.start_with {
        sql.push_str(&format!(" START WITH {start}"));
    }
    sql.push_str(&format!(" CACHE {cache}"));
    if s.cycle {
        sql.push_str(" CYCLE");
    }
    sql.push(';');
    sql
}

fn render_create_role(r: &SnapRole) -> String {
    let mut opts: Vec<&str> = Vec::new();
    if r.create_db {
        opts.push("CREATEDB");
    }
    if r.create_role {
        opts.push("CREATEROLE");
    }
    if !r.inherit {
        opts.push("NOINHERIT");
    }
    if opts.is_empty() {
        format!("CREATE ROLE {};", quote_identifier(&r.name))
    } else {
        format!(
            "CREATE ROLE {} WITH {};",
            quote_identifier(&r.name),
            opts.join(" ")
        )
    }
}

fn render_alter_role(r: &SnapRole) -> String {
    let opts = [
        if r.create_db {
            "CREATEDB"
        } else {
            "NOCREATEDB"
        },
        if r.create_role {
            "CREATEROLE"
        } else {
            "NOCREATEROLE"
        },
        if r.inherit { "INHERIT" } else { "NOINHERIT" },
    ];
    format!(
        "ALTER ROLE {} WITH {};",
        quote_identifier(&r.name),
        opts.join(" ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::differ::ir::{SnapColumn, SnapIndexColumn, SnapRole};

    #[test]
    fn drop_table_cascades_and_omits_public() {
        let s = DdlStatement::DropTable {
            schema: "public".into(),
            name: "users".into(),
        };
        assert_eq!(s.to_sql(), r#"DROP TABLE "users" CASCADE;"#);
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
            r#"ALTER TABLE "users" ADD COLUMN "email" text NOT NULL DEFAULT '';"#
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
            r#"CREATE UNIQUE INDEX "idx_email" ON "users" USING btree ("email") WHERE deleted_at IS NULL;"#
        );
    }

    #[test]
    fn create_policy_renders_defaults() {
        let s = DdlStatement::CreatePolicy {
            schema: "public".into(),
            table: "docs".into(),
            policy: SnapPolicy::new("p").using("org_id = current_org()"),
        };
        assert_eq!(
            s.to_sql(),
            r#"CREATE POLICY "p" ON "docs" AS PERMISSIVE FOR ALL TO public USING (org_id = current_org());"#
        );
    }

    #[test]
    fn create_role_omits_with_when_default() {
        let s = DdlStatement::CreateRole(SnapRole::new("app"));
        assert_eq!(s.to_sql(), r#"CREATE ROLE "app";"#);
    }

    #[test]
    fn create_role_lists_nondefault_options() {
        let s = DdlStatement::CreateRole(SnapRole::new("app").create_db(true));
        assert_eq!(s.to_sql(), r#"CREATE ROLE "app" WITH CREATEDB;"#);
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
