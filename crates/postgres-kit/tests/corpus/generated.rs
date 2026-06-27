//! Corpus category: `generated` (STORED generated columns).
//!
//! Ported verbatim from drizzle-kit's `tests/pg-generated.test.ts`. Every test in
//! that file exercises `text(...).generatedAlwaysAs(...)` in three flavors
//! (callback / sql / string), but all three produce the *same* normalized
//! `generated` expression and therefore the same emitted SQL. We mirror each test
//! faithfully: `schema1 -> from`, `schema2 -> to`, `renames` verbatim, and the
//! asserted `sqlStatements` copied into `expected_sql`.
//!
//! Column name note: drizzle's field `generatedName: text('gen_name')` has DB
//! column name `gen_name` (the snapshot keys by DB column name), so renaming the
//! *field* (e.g. `generatedName -> generatedName1`) with the same `text('gen_name')`
//! is a no-op rename here — both `from` and `to` key the column as `gen_name`.

use super::{DiffCase, Status};
use postgres_kit::differ::ir::*;

/// The three columns shared by every `users` table in this file.
fn users_base() -> SnapTable {
    SnapTable::new("public.users")
        .col(SnapColumn::new("id", "integer"))
        .col(SnapColumn::new("id2", "integer"))
        .col(SnapColumn::new("name", "text"))
}

fn schema(table: SnapTable) -> SchemaSnapshot {
    SchemaSnapshot::builder().table(table).build()
}

/// `add column with generated constraint`: `gen_name` does not exist in `from`,
/// and is added in `to` as a STORED generated column.
fn add_column_with_generated(name: &'static str) -> DiffCase {
    DiffCase {
        name,
        from: schema(users_base()),
        to: schema(
            users_base()
                .col(SnapColumn::new("gen_name", "text").generated(r#""users"."name" || 'hello'"#)),
        ),
        renames: &[],
        expected_sql: &[
            r#"ALTER TABLE "users" ADD COLUMN "gen_name" text GENERATED ALWAYS AS ("users"."name" || 'hello') STORED;"#,
        ],
        status: Status::Supported,
    }
}

/// `add generated constraint to an existing column`: `gen_name` exists in `from`
/// as a plain NOT NULL column, and gains a STORED generated expression in `to`.
/// drizzle drops and re-adds the column.
fn add_generated_to_existing(name: &'static str) -> DiffCase {
    DiffCase {
        name,
        from: schema(users_base().col(SnapColumn::new("gen_name", "text").not_null())),
        to: schema(
            users_base().col(
                SnapColumn::new("gen_name", "text")
                    .not_null()
                    .generated(r#""users"."name" || 'to add'"#),
            ),
        ),
        renames: &[],
        expected_sql: &[
            r#"ALTER TABLE "users" drop column "gen_name";"#,
            r#"ALTER TABLE "users" ADD COLUMN "gen_name" text GENERATED ALWAYS AS ("users"."name" || 'to add') STORED NOT NULL;"#,
        ],
        status: Status::Supported,
    }
}

/// `drop generated constraint`: `gen_name` is a STORED generated column in `from`
/// and a plain column in `to`. drizzle drops the expression in place.
fn drop_generated(name: &'static str) -> DiffCase {
    DiffCase {
        name,
        from: schema(users_base().col(
            SnapColumn::new("gen_name", "text").generated(r#""users"."name" || 'to delete'"#),
        )),
        to: schema(users_base().col(SnapColumn::new("gen_name", "text"))),
        renames: &[],
        expected_sql: &[r#"ALTER TABLE "users" ALTER COLUMN "gen_name" DROP EXPRESSION;"#],
        status: Status::Supported,
    }
}

/// `change generated constraint`: `gen_name`'s STORED expression changes. drizzle
/// drops and re-adds the column with the new expression.
fn change_generated(name: &'static str) -> DiffCase {
    DiffCase {
        name,
        from: schema(
            users_base().col(SnapColumn::new("gen_name", "text").generated(r#""users"."name""#)),
        ),
        to: schema(
            users_base()
                .col(SnapColumn::new("gen_name", "text").generated(r#""users"."name" || 'hello'"#)),
        ),
        renames: &[],
        expected_sql: &[
            r#"ALTER TABLE "users" drop column "gen_name";"#,
            r#"ALTER TABLE "users" ADD COLUMN "gen_name" text GENERATED ALWAYS AS ("users"."name" || 'hello') STORED;"#,
        ],
        status: Status::Supported,
    }
}

pub fn cases() -> Vec<DiffCase> {
    vec![
        // --- generated as callback ---
        add_column_with_generated("generated as callback: add column with generated constraint"),
        add_generated_to_existing(
            "generated as callback: add generated constraint to an exisiting column",
        ),
        drop_generated("generated as callback: drop generated constraint"),
        change_generated("generated as callback: change generated constraint"),
        // --- generated as sql ---
        add_column_with_generated("generated as sql: add column with generated constraint"),
        add_generated_to_existing(
            "generated as sql: add generated constraint to an exisiting column",
        ),
        drop_generated("generated as sql: drop generated constraint"),
        change_generated("generated as sql: change generated constraint"),
        // --- generated as string ---
        add_column_with_generated("generated as string: add column with generated constraint"),
        add_generated_to_existing(
            "generated as string: add generated constraint to an exisiting column",
        ),
        drop_generated("generated as string: drop generated constraint"),
        change_generated("generated as string: change generated constraint"),
    ]
}
