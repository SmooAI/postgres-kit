//! Differ corpus — category `views`.
//!
//! Ported faithfully from drizzle-kit's `tests/pg-views.test.ts`. Each `test(...)`
//! there calls `diffTestSchemas(schema1, schema2, renames)` and asserts a
//! `sqlStatements` array; we translate `schema1 -> from`, `schema2 -> to`, copy
//! `renames` verbatim, and copy the asserted `sqlStatements` into `expected_sql`.
//!
//! The current [`SnapView`] IR models only `definition` + `materialized`. Anything
//! that depends on view `WITH (...)` options (check_option / security_barrier /
//! security_invoker / autovacuum_* / fillfactor / ...), `TABLESPACE`, `USING`
//! (access method), `WITH NO DATA`, the drizzle `.existing()` flag, or `CREATE
//! SCHEMA` is not representable yet and is marked [`Status::Skip`] with a reason.
//! The error-case tests (duplicate view names that throw) are skipped too. The
//! differ/integrator agent will promote Skip -> Supported as the IR grows.

use super::{DiffCase, Status};
use postgres_kit::differ::ir::{SchemaSnapshot, SnapColumn, SnapTable, SnapView};

/// `pgTable('users', { id: integer('id').primaryKey().notNull() })`.
fn users_table() -> SnapTable {
    SnapTable::new("users").col(SnapColumn::new("id", "integer").primary_key().not_null())
}

/// The verbatim `CREATE TABLE "users"` statement drizzle emits for `users_table`.
const CREATE_USERS_TABLE: &str =
    "CREATE TABLE \"users\" (\n\t\"id\" integer PRIMARY KEY NOT NULL\n);\n";

fn empty() -> SchemaSnapshot {
    SchemaSnapshot::builder().build()
}

pub fn cases() -> Vec<DiffCase> {
    vec![
        // ---- create view ----
        DiffCase {
            name: "create table and view #1",
            from: empty(),
            to: SchemaSnapshot::builder()
                .table(users_table())
                .view(SnapView::new(
                    "public.some_view",
                    r#"select "id" from "users""#,
                ))
                .build(),
            renames: &[],
            expected_sql: &[
                CREATE_USERS_TABLE,
                r#"CREATE VIEW "public"."some_view" AS (select "id" from "users");"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "create table and view #2",
            from: empty(),
            to: SchemaSnapshot::builder()
                .table(users_table())
                .view(SnapView::new(
                    "public.some_view",
                    r#"SELECT * FROM "users""#,
                ))
                .build(),
            renames: &[],
            expected_sql: &[
                CREATE_USERS_TABLE,
                r#"CREATE VIEW "public"."some_view" AS (SELECT * FROM "users");"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "create table and view #3",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("view WITH options (check_option/security_*) not in IR"),
        },
        DiffCase {
            name: "create table and view #4",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("CREATE SCHEMA + view WITH options not in IR"),
        },
        DiffCase {
            name: "create table and view #5",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("error case: duplicate view names (rejects)"),
        },
        DiffCase {
            name: "create table and view #6",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("view WITH options (check_option) not in IR"),
        },
        DiffCase {
            name: "create view with existing flag",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("drizzle .existing() flag not in IR"),
        },
        // ---- create materialized view ----
        DiffCase {
            name: "create table and materialized view #1",
            from: empty(),
            to: SchemaSnapshot::builder()
                .table(users_table())
                .view(
                    SnapView::new("public.some_view", r#"select "id" from "users""#).materialized(),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                CREATE_USERS_TABLE,
                r#"CREATE MATERIALIZED VIEW "public"."some_view" AS (select "id" from "users");"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "create table and materialized view #2",
            from: empty(),
            to: SchemaSnapshot::builder()
                .table(users_table())
                .view(SnapView::new("public.some_view", r#"SELECT * FROM "users""#).materialized())
                .build(),
            renames: &[],
            expected_sql: &[
                CREATE_USERS_TABLE,
                r#"CREATE MATERIALIZED VIEW "public"."some_view" AS (SELECT * FROM "users");"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "create table and materialized view #3",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("materialized view WITH options/TABLESPACE/USING not in IR"),
        },
        DiffCase {
            name: "create table and materialized view #4",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("error case: duplicate materialized view names (rejects)"),
        },
        DiffCase {
            name: "create table and materialized view #5",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("materialized view WITH options not in IR"),
        },
        DiffCase {
            name: "create materialized view with existing flag",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("drizzle .existing() flag not in IR"),
        },
        // ---- drop view ----
        DiffCase {
            name: "drop view #1",
            from: SchemaSnapshot::builder()
                .table(users_table())
                .view(SnapView::new(
                    "public.some_view",
                    r#"SELECT * FROM "users""#,
                ))
                .build(),
            to: SchemaSnapshot::builder().table(users_table()).build(),
            renames: &[],
            expected_sql: &[r#"DROP VIEW "public"."some_view";"#],
            status: Status::Supported,
        },
        DiffCase {
            name: "drop view with existing flag",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("drizzle .existing() flag not in IR"),
        },
        DiffCase {
            name: "drop materialized view #1",
            from: SchemaSnapshot::builder()
                .table(users_table())
                .view(SnapView::new("public.some_view", r#"SELECT * FROM "users""#).materialized())
                .build(),
            to: SchemaSnapshot::builder().table(users_table()).build(),
            renames: &[],
            expected_sql: &[r#"DROP MATERIALIZED VIEW "public"."some_view";"#],
            status: Status::Supported,
        },
        DiffCase {
            name: "drop materialized view with existing flag",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("drizzle .existing() flag not in IR"),
        },
        // ---- rename view ----
        DiffCase {
            name: "rename view #1",
            from: SchemaSnapshot::builder()
                .view(SnapView::new(
                    "public.some_view",
                    r#"SELECT * FROM "users""#,
                ))
                .build(),
            to: SchemaSnapshot::builder()
                .view(SnapView::new(
                    "public.new_some_view",
                    r#"SELECT * FROM "users""#,
                ))
                .build(),
            renames: &["public.some_view->public.new_some_view"],
            expected_sql: &[r#"ALTER VIEW "public"."some_view" RENAME TO "new_some_view";"#],
            status: Status::Supported,
        },
        DiffCase {
            name: "rename view with existing flag",
            from: empty(),
            to: empty(),
            renames: &["public.some_view->public.new_some_view"],
            expected_sql: &[],
            status: Status::Skip("drizzle .existing() flag not in IR"),
        },
        DiffCase {
            name: "rename materialized view #1",
            from: SchemaSnapshot::builder()
                .view(SnapView::new("public.some_view", r#"SELECT * FROM "users""#).materialized())
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.new_some_view", r#"SELECT * FROM "users""#)
                        .materialized(),
                )
                .build(),
            renames: &["public.some_view->public.new_some_view"],
            expected_sql: &[
                r#"ALTER MATERIALIZED VIEW "public"."some_view" RENAME TO "new_some_view";"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "rename materialized view with existing flag",
            from: empty(),
            to: empty(),
            renames: &["public.some_view->public.new_some_view"],
            expected_sql: &[],
            status: Status::Skip("drizzle .existing() flag not in IR"),
        },
        // ---- alter view schema ----
        DiffCase {
            name: "view alter schema",
            from: empty(),
            to: empty(),
            renames: &["public.some_view->new_schema.some_view"],
            expected_sql: &[],
            status: Status::Skip("CREATE SCHEMA + SET SCHEMA not in IR"),
        },
        DiffCase {
            name: "view alter schema with existing flag",
            from: empty(),
            to: empty(),
            renames: &["public.some_view->new_schema.some_view"],
            expected_sql: &[],
            status: Status::Skip("CREATE SCHEMA + drizzle .existing() flag not in IR"),
        },
        DiffCase {
            name: "view alter schema for materialized",
            from: empty(),
            to: empty(),
            renames: &["public.some_view->new_schema.some_view"],
            expected_sql: &[],
            status: Status::Skip("CREATE SCHEMA + SET SCHEMA not in IR"),
        },
        DiffCase {
            name: "view alter schema for materialized with existing flag",
            from: empty(),
            to: empty(),
            renames: &["public.some_view->new_schema.some_view"],
            expected_sql: &[],
            status: Status::Skip("CREATE SCHEMA + drizzle .existing() flag not in IR"),
        },
        // ---- add / drop / alter WITH options ----
        DiffCase {
            name: "add with option to view #1",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("view WITH options not in IR"),
        },
        DiffCase {
            name: "add with option to view with existing flag",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("view WITH options + .existing() flag not in IR"),
        },
        DiffCase {
            name: "add with option to materialized view #1",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("materialized view WITH options not in IR"),
        },
        DiffCase {
            name: "add with option to materialized view with existing flag",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("materialized view WITH options + .existing() flag not in IR"),
        },
        DiffCase {
            name: "drop with option from view #1",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("view WITH options not in IR"),
        },
        DiffCase {
            name: "drop with option from view with existing flag",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("view WITH options + .existing() flag not in IR"),
        },
        DiffCase {
            name: "drop with option from materialized view #1",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("materialized view WITH options not in IR"),
        },
        DiffCase {
            name: "drop with option from materialized view with existing flag",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("materialized view WITH options + .existing() flag not in IR"),
        },
        DiffCase {
            name: "alter with option in view #1",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("view WITH options not in IR"),
        },
        DiffCase {
            name: "alter with option in view with existing flag",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("view WITH options + .existing() flag not in IR"),
        },
        DiffCase {
            name: "alter with option in materialized view #1",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("materialized view WITH options not in IR"),
        },
        DiffCase {
            name: "alter with option in materialized view with existing flag",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("materialized view WITH options + .existing() flag not in IR"),
        },
        DiffCase {
            name: "alter with option in view #2",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("view WITH options not in IR"),
        },
        DiffCase {
            name: "alter with option in materialized view #2",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("materialized view WITH options not in IR"),
        },
        // ---- alter ".as" definition (all carry WITH options) ----
        DiffCase {
            name: "alter view \".as\" value",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("view WITH options not in IR (drop+recreate carries options)"),
        },
        DiffCase {
            name: "alter view \".as\" value with existing flag",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("view WITH options + .existing() flag not in IR"),
        },
        DiffCase {
            name: "alter materialized view \".as\" value",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip(
                "materialized view WITH options not in IR (drop+recreate carries options)",
            ),
        },
        DiffCase {
            name: "alter materialized view \".as\" value with existing flag",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("materialized view WITH options + .existing() flag not in IR"),
        },
        DiffCase {
            name: "drop existing flag",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("drizzle .existing() flag + WITH options not in IR"),
        },
        // ---- tablespace ----
        DiffCase {
            name: "alter tablespace - materialize",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("materialized view TABLESPACE not in IR"),
        },
        DiffCase {
            name: "set tablespace - materialize",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("materialized view TABLESPACE not in IR"),
        },
        DiffCase {
            name: "drop tablespace - materialize",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("materialized view TABLESPACE not in IR"),
        },
        DiffCase {
            name: "set existing - materialized",
            from: empty(),
            to: empty(),
            renames: &["public.some_view->public.new_some_view"],
            expected_sql: &[],
            status: Status::Skip("drizzle .existing() flag + TABLESPACE/WITH options not in IR"),
        },
        DiffCase {
            name: "drop existing - materialized",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("no concrete sqlStatements asserted (length-only) + .existing()"),
        },
        DiffCase {
            name: "set existing",
            from: empty(),
            to: empty(),
            renames: &["public.some_view->public.new_some_view"],
            expected_sql: &[],
            status: Status::Skip("drizzle .existing() flag + WITH options not in IR"),
        },
        // ---- USING (access method) ----
        DiffCase {
            name: "alter using - materialize",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("materialized view USING (access method) not in IR"),
        },
        DiffCase {
            name: "set using - materialize",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("materialized view USING (access method) not in IR"),
        },
        DiffCase {
            name: "drop using - materialize",
            from: empty(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("materialized view USING (access method) not in IR"),
        },
        // ---- combined rename + alter ----
        DiffCase {
            name: "rename view and alter view",
            from: empty(),
            to: empty(),
            renames: &["public.some_view->public.new_some_view"],
            expected_sql: &[],
            status: Status::Skip("view WITH options not in IR (rename + SET option)"),
        },
        DiffCase {
            name: "moved schema and alter view",
            from: empty(),
            to: empty(),
            renames: &["public.some_view->my_schema.some_view"],
            expected_sql: &[],
            status: Status::Skip("SET SCHEMA + view WITH options not in IR"),
        },
    ]
}
