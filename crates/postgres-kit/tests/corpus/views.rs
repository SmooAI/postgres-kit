//! Differ corpus — category `views`.
//!
//! A conformance corpus of view schema-diff scenarios. Each scenario diffs
//! `schema1 -> from`, `schema2 -> to` with rename hints and asserts a statement
//! array in `expected_sql`.
//!
//! [`SnapView`] now models `WITH (...)` options (snake-cased key -> rendered value,
//! kept alphabetical via the BTreeMap), `TABLESPACE`, `USING` (access method),
//! `WITH NO DATA`, materialized-ness, and the "existing" (unmanaged) view flag
//! (built via [`SnapView::reference`]). The only remaining [`Status::Skip`] cases
//! are the two error-path scenarios (duplicate view names that Postgres rejects),
//! which the differ does not model.

use super::{DiffCase, Status};
use postgres_kit::differ::ir::{SchemaSnapshot, SnapColumn, SnapTable, SnapView};

/// `pgTable('users', { id: integer('id').primaryKey().notNull() })`.
fn users_table() -> SnapTable {
    SnapTable::new("users").col(SnapColumn::new("id", "integer").primary_key().not_null())
}

/// `new_schema.users` — the same shape, qualified into a named schema.
fn new_schema_users_table() -> SnapTable {
    SnapTable::new("new_schema.users")
        .col(SnapColumn::new("id", "integer").primary_key().not_null())
}

/// The verbatim `CREATE TABLE "users"` statement emitted for `users_table`.
const CREATE_USERS_TABLE: &str =
    "CREATE TABLE \"users\" (\n\t\"id\" integer PRIMARY KEY NOT NULL\n);\n";

/// The verbatim `CREATE TABLE "new_schema"."users"` statement.
const CREATE_NEW_SCHEMA_USERS_TABLE: &str =
    "CREATE TABLE \"new_schema\".\"users\" (\n\t\"id\" integer PRIMARY KEY NOT NULL\n);\n";

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
            to: SchemaSnapshot::builder()
                .table(users_table())
                .view(
                    SnapView::new("public.some_view1", r#"SELECT * FROM "users""#)
                        .with_option("check_option", "local")
                        .with_option("security_barrier", "false")
                        .with_option("security_invoker", "true"),
                )
                .view(
                    SnapView::new("public.some_view2", r#"select "id" from "users""#)
                        .with_option("check_option", "cascaded")
                        .with_option("security_barrier", "true")
                        .with_option("security_invoker", "false"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                CREATE_USERS_TABLE,
                r#"CREATE VIEW "public"."some_view1" WITH (check_option = local, security_barrier = false, security_invoker = true) AS (SELECT * FROM "users");"#,
                r#"CREATE VIEW "public"."some_view2" WITH (check_option = cascaded, security_barrier = true, security_invoker = false) AS (select "id" from "users");"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "create table and view #4",
            from: empty(),
            to: SchemaSnapshot::builder()
                .schema("new_schema")
                .table(new_schema_users_table())
                .view(
                    SnapView::new(
                        "new_schema.some_view1",
                        r#"SELECT * FROM "new_schema"."users""#,
                    )
                    .with_option("check_option", "local")
                    .with_option("security_barrier", "false")
                    .with_option("security_invoker", "true"),
                )
                .view(
                    SnapView::new(
                        "new_schema.some_view2",
                        r#"select "id" from "new_schema"."users""#,
                    )
                    .with_option("check_option", "cascaded")
                    .with_option("security_barrier", "true")
                    .with_option("security_invoker", "false"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                r#"CREATE SCHEMA IF NOT EXISTS "new_schema";"#,
                CREATE_NEW_SCHEMA_USERS_TABLE,
                r#"CREATE VIEW "new_schema"."some_view1" WITH (check_option = local, security_barrier = false, security_invoker = true) AS (SELECT * FROM "new_schema"."users");"#,
                r#"CREATE VIEW "new_schema"."some_view2" WITH (check_option = cascaded, security_barrier = true, security_invoker = false) AS (select "id" from "new_schema"."users");"#,
            ],
            status: Status::Supported,
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
            to: SchemaSnapshot::builder()
                .table(users_table())
                .view(
                    SnapView::new("public.some_view", r#"SELECT * FROM "users""#)
                        .with_option("check_option", "cascaded"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                CREATE_USERS_TABLE,
                r#"CREATE VIEW "public"."some_view" WITH (check_option = cascaded) AS (SELECT * FROM "users");"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "create view with existing flag",
            from: empty(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::reference("public.some_view").with_option("check_option", "cascaded"),
                )
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Supported,
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
            to: SchemaSnapshot::builder()
                .table(users_table())
                .view(SnapView::new("public.some_view1", r#"SELECT * FROM "users""#).materialized())
                .view(
                    SnapView::new("public.some_view2", r#"select "id" from "users""#)
                        .materialized()
                        .using("heap")
                        .tablespace("some_tablespace")
                        .with_no_data()
                        .with_option("autovacuum_enabled", "true")
                        .with_option("autovacuum_freeze_max_age", "1")
                        .with_option("autovacuum_freeze_min_age", "1")
                        .with_option("autovacuum_freeze_table_age", "1")
                        .with_option("autovacuum_multixact_freeze_max_age", "1")
                        .with_option("autovacuum_multixact_freeze_min_age", "1")
                        .with_option("autovacuum_multixact_freeze_table_age", "1")
                        .with_option("autovacuum_vacuum_cost_delay", "1")
                        .with_option("autovacuum_vacuum_cost_limit", "1")
                        .with_option("autovacuum_vacuum_scale_factor", "1")
                        .with_option("autovacuum_vacuum_threshold", "1")
                        .with_option("fillfactor", "1")
                        .with_option("log_autovacuum_min_duration", "1")
                        .with_option("parallel_workers", "1")
                        .with_option("toast_tuple_target", "1")
                        .with_option("user_catalog_table", "true")
                        .with_option("vacuum_index_cleanup", "off")
                        .with_option("vacuum_truncate", "false"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                CREATE_USERS_TABLE,
                r#"CREATE MATERIALIZED VIEW "public"."some_view1" AS (SELECT * FROM "users");"#,
                r#"CREATE MATERIALIZED VIEW "public"."some_view2" USING "heap" WITH (autovacuum_enabled = true, autovacuum_freeze_max_age = 1, autovacuum_freeze_min_age = 1, autovacuum_freeze_table_age = 1, autovacuum_multixact_freeze_max_age = 1, autovacuum_multixact_freeze_min_age = 1, autovacuum_multixact_freeze_table_age = 1, autovacuum_vacuum_cost_delay = 1, autovacuum_vacuum_cost_limit = 1, autovacuum_vacuum_scale_factor = 1, autovacuum_vacuum_threshold = 1, fillfactor = 1, log_autovacuum_min_duration = 1, parallel_workers = 1, toast_tuple_target = 1, user_catalog_table = true, vacuum_index_cleanup = off, vacuum_truncate = false) TABLESPACE some_tablespace AS (select "id" from "users") WITH NO DATA;"#,
            ],
            status: Status::Supported,
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
            to: SchemaSnapshot::builder()
                .table(users_table())
                .view(
                    SnapView::new("public.some_view", r#"SELECT * FROM "users""#)
                        .materialized()
                        .with_option("autovacuum_freeze_min_age", "14"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                CREATE_USERS_TABLE,
                r#"CREATE MATERIALIZED VIEW "public"."some_view" WITH (autovacuum_freeze_min_age = 14) AS (SELECT * FROM "users");"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "create materialized view with existing flag",
            from: empty(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::reference("public.some_view")
                        .materialized()
                        .with_option("autovacuum_enabled", "true"),
                )
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Supported,
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
            from: SchemaSnapshot::builder()
                .view(SnapView::reference("public.some_view"))
                .build(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Supported,
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
            from: SchemaSnapshot::builder()
                .view(SnapView::reference("public.some_view").materialized())
                .build(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Supported,
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
            from: SchemaSnapshot::builder()
                .view(SnapView::reference("public.some_view"))
                .build(),
            to: SchemaSnapshot::builder()
                .view(SnapView::reference("public.new_some_view"))
                .build(),
            renames: &["public.some_view->public.new_some_view"],
            expected_sql: &[],
            status: Status::Supported,
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
            from: SchemaSnapshot::builder()
                .view(SnapView::reference("public.some_view").materialized())
                .build(),
            to: SchemaSnapshot::builder()
                .view(SnapView::reference("public.new_some_view").materialized())
                .build(),
            renames: &["public.some_view->public.new_some_view"],
            expected_sql: &[],
            status: Status::Supported,
        },
        // ---- alter view schema ----
        DiffCase {
            name: "view alter schema",
            from: SchemaSnapshot::builder()
                .view(SnapView::new(
                    "public.some_view",
                    r#"SELECT * FROM "users""#,
                ))
                .build(),
            to: SchemaSnapshot::builder()
                .schema("new_schema")
                .view(SnapView::new(
                    "new_schema.some_view",
                    r#"SELECT * FROM "users""#,
                ))
                .build(),
            renames: &["public.some_view->new_schema.some_view"],
            expected_sql: &[
                r#"CREATE SCHEMA IF NOT EXISTS "new_schema";"#,
                r#"ALTER VIEW "public"."some_view" SET SCHEMA "new_schema";"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "view alter schema with existing flag",
            from: SchemaSnapshot::builder()
                .view(SnapView::reference("public.some_view"))
                .build(),
            to: SchemaSnapshot::builder()
                .schema("new_schema")
                .view(SnapView::reference("new_schema.some_view"))
                .build(),
            renames: &["public.some_view->new_schema.some_view"],
            expected_sql: &[r#"CREATE SCHEMA IF NOT EXISTS "new_schema";"#],
            status: Status::Supported,
        },
        DiffCase {
            name: "view alter schema for materialized",
            from: SchemaSnapshot::builder()
                .view(SnapView::new("public.some_view", r#"SELECT * FROM "users""#).materialized())
                .build(),
            to: SchemaSnapshot::builder()
                .schema("new_schema")
                .view(
                    SnapView::new("new_schema.some_view", r#"SELECT * FROM "users""#)
                        .materialized(),
                )
                .build(),
            renames: &["public.some_view->new_schema.some_view"],
            expected_sql: &[
                r#"CREATE SCHEMA IF NOT EXISTS "new_schema";"#,
                r#"ALTER MATERIALIZED VIEW "public"."some_view" SET SCHEMA "new_schema";"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "view alter schema for materialized with existing flag",
            from: SchemaSnapshot::builder()
                .view(SnapView::reference("public.some_view").materialized())
                .build(),
            to: SchemaSnapshot::builder()
                .schema("new_schema")
                .view(SnapView::reference("new_schema.some_view").materialized())
                .build(),
            renames: &["public.some_view->new_schema.some_view"],
            expected_sql: &[r#"CREATE SCHEMA IF NOT EXISTS "new_schema";"#],
            status: Status::Supported,
        },
        // ---- add / drop / alter WITH options ----
        DiffCase {
            name: "add with option to view #1",
            from: SchemaSnapshot::builder()
                .view(SnapView::new(
                    "public.some_view",
                    r#"select "id" from "users""#,
                ))
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", r#"select "id" from "users""#)
                        .with_option("check_option", "cascaded")
                        .with_option("security_barrier", "true"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                r#"ALTER VIEW "public"."some_view" SET (check_option = cascaded, security_barrier = true);"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "add with option to view with existing flag",
            from: SchemaSnapshot::builder()
                .view(SnapView::reference("public.some_view"))
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::reference("public.some_view")
                        .with_option("check_option", "cascaded")
                        .with_option("security_barrier", "true"),
                )
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Supported,
        },
        DiffCase {
            name: "add with option to materialized view #1",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", r#"select "id" from "users""#).materialized(),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", r#"select "id" from "users""#)
                        .materialized()
                        .with_option("autovacuum_multixact_freeze_max_age", "3"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                r#"ALTER MATERIALIZED VIEW "public"."some_view" SET (autovacuum_multixact_freeze_max_age = 3);"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "add with option to materialized view with existing flag",
            from: SchemaSnapshot::builder()
                .view(SnapView::reference("public.some_view").materialized())
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::reference("public.some_view")
                        .materialized()
                        .with_option("autovacuum_multixact_freeze_max_age", "3"),
                )
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Supported,
        },
        DiffCase {
            name: "drop with option from view #1",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", r#"select "id" from "users""#)
                        .with_option("check_option", "cascaded")
                        .with_option("security_barrier", "true")
                        .with_option("security_invoker", "true"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(SnapView::new(
                    "public.some_view",
                    r#"select "id" from "users""#,
                ))
                .build(),
            renames: &[],
            expected_sql: &[
                r#"ALTER VIEW "public"."some_view" RESET (check_option, security_barrier, security_invoker);"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "drop with option from view with existing flag",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::reference("public.some_view")
                        .with_option("check_option", "cascaded")
                        .with_option("security_barrier", "true")
                        .with_option("security_invoker", "true"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(SnapView::reference("public.some_view"))
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Supported,
        },
        DiffCase {
            name: "drop with option from materialized view #1",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", r#"select "id" from "users""#)
                        .materialized()
                        .with_option("autovacuum_enabled", "true")
                        .with_option("autovacuum_freeze_max_age", "10"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", r#"select "id" from "users""#).materialized(),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                r#"ALTER MATERIALIZED VIEW "public"."some_view" RESET (autovacuum_enabled, autovacuum_freeze_max_age);"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "drop with option from materialized view with existing flag",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::reference("public.some_view")
                        .materialized()
                        .with_option("autovacuum_enabled", "true")
                        .with_option("autovacuum_freeze_max_age", "10"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(SnapView::reference("public.some_view").materialized())
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter with option in view #1",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", r#"select "id" from "users""#)
                        .with_option("security_barrier", "true")
                        .with_option("security_invoker", "true"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", r#"select "id" from "users""#)
                        .with_option("security_barrier", "true"),
                )
                .build(),
            renames: &[],
            expected_sql: &[r#"ALTER VIEW "public"."some_view" RESET (security_invoker);"#],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter with option in view with existing flag",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::reference("public.some_view")
                        .with_option("security_barrier", "true")
                        .with_option("security_invoker", "true"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::reference("public.some_view").with_option("security_barrier", "true"),
                )
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter with option in materialized view #1",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", r#"select "id" from "users""#)
                        .materialized()
                        .with_option("autovacuum_enabled", "true")
                        .with_option("autovacuum_vacuum_scale_factor", "1"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", r#"select "id" from "users""#)
                        .materialized()
                        .with_option("autovacuum_enabled", "true"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                r#"ALTER MATERIALIZED VIEW "public"."some_view" RESET (autovacuum_vacuum_scale_factor);"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter with option in materialized view with existing flag",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::reference("public.some_view")
                        .materialized()
                        .with_option("autovacuum_enabled", "true")
                        .with_option("autovacuum_vacuum_scale_factor", "1"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::reference("public.some_view")
                        .materialized()
                        .with_option("autovacuum_enabled", "true"),
                )
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter with option in view #2",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", r#"select distinct "id" from "users""#)
                        .with_option("check_option", "local")
                        .with_option("security_barrier", "true")
                        .with_option("security_invoker", "true"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", r#"select distinct "id" from "users""#)
                        .with_option("check_option", "cascaded")
                        .with_option("security_barrier", "true")
                        .with_option("security_invoker", "true"),
                )
                .build(),
            renames: &[],
            expected_sql: &[r#"ALTER VIEW "public"."some_view" SET (check_option = cascaded);"#],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter with option in materialized view #2",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", r#"select "id" from "users""#)
                        .materialized()
                        .with_option("autovacuum_enabled", "true")
                        .with_option("fillfactor", "1"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", r#"select "id" from "users""#)
                        .materialized()
                        .with_option("autovacuum_enabled", "false")
                        .with_option("fillfactor", "1"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                r#"ALTER MATERIALIZED VIEW "public"."some_view" SET (autovacuum_enabled = false);"#,
            ],
            status: Status::Supported,
        },
        // ---- alter ".as" definition (drop + recreate, carrying WITH options) ----
        DiffCase {
            name: "alter view \".as\" value",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT '123'")
                        .with_option("check_option", "local")
                        .with_option("security_barrier", "true")
                        .with_option("security_invoker", "true"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT '1234'")
                        .with_option("check_option", "local")
                        .with_option("security_barrier", "true")
                        .with_option("security_invoker", "true"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                r#"DROP VIEW "public"."some_view";"#,
                r#"CREATE VIEW "public"."some_view" WITH (check_option = local, security_barrier = true, security_invoker = true) AS (SELECT '1234');"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter view \".as\" value with existing flag",
            from: SchemaSnapshot::builder()
                .view(SnapView::reference("public.some_view"))
                .build(),
            to: SchemaSnapshot::builder()
                .view(SnapView::reference("public.some_view"))
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter materialized view \".as\" value",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT '123'")
                        .materialized()
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT '1234'")
                        .materialized()
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                r#"DROP MATERIALIZED VIEW "public"."some_view";"#,
                r#"CREATE MATERIALIZED VIEW "public"."some_view" WITH (autovacuum_vacuum_cost_limit = 1) AS (SELECT '1234');"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter materialized view \".as\" value with existing flag",
            from: SchemaSnapshot::builder()
                .view(SnapView::reference("public.some_view").materialized())
                .build(),
            to: SchemaSnapshot::builder()
                .view(SnapView::reference("public.some_view").materialized())
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Supported,
        },
        DiffCase {
            name: "drop existing flag",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::reference("public.some_view")
                        .materialized()
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT 'asd'")
                        .materialized()
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                r#"DROP MATERIALIZED VIEW "public"."some_view";"#,
                r#"CREATE MATERIALIZED VIEW "public"."some_view" WITH (autovacuum_vacuum_cost_limit = 1) AS (SELECT 'asd');"#,
            ],
            status: Status::Supported,
        },
        // ---- tablespace (materialized) ----
        DiffCase {
            name: "alter tablespace - materialize",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT 'asd'")
                        .materialized()
                        .tablespace("some_tablespace")
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT 'asd'")
                        .materialized()
                        .tablespace("new_tablespace")
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                r#"ALTER MATERIALIZED VIEW "public"."some_view" SET TABLESPACE new_tablespace;"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "set tablespace - materialize",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT 'asd'")
                        .materialized()
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT 'asd'")
                        .materialized()
                        .tablespace("new_tablespace")
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                r#"ALTER MATERIALIZED VIEW "public"."some_view" SET TABLESPACE new_tablespace;"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "drop tablespace - materialize",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT 'asd'")
                        .materialized()
                        .tablespace("new_tablespace")
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT 'asd'")
                        .materialized()
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                r#"ALTER MATERIALIZED VIEW "public"."some_view" SET TABLESPACE pg_default;"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "set existing - materialized",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT 'asd'")
                        .materialized()
                        .tablespace("new_tablespace")
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::reference("public.new_some_view")
                        .materialized()
                        .with_no_data()
                        .with_option("autovacuum_freeze_min_age", "1")
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            renames: &["public.some_view->public.new_some_view"],
            expected_sql: &[],
            status: Status::Supported,
        },
        DiffCase {
            name: "drop existing - materialized",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::reference("public.some_view")
                        .materialized()
                        .tablespace("new_tablespace")
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT 'asd'")
                        .materialized()
                        .with_no_data()
                        .with_option("autovacuum_freeze_min_age", "1")
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                r#"DROP MATERIALIZED VIEW "public"."some_view";"#,
                r#"CREATE MATERIALIZED VIEW "public"."some_view" WITH (autovacuum_freeze_min_age = 1, autovacuum_vacuum_cost_limit = 1) AS (SELECT 'asd') WITH NO DATA;"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "set existing",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT 'asd'")
                        .with_option("check_option", "cascaded"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::reference("public.new_some_view")
                        .with_option("check_option", "cascaded")
                        .with_option("security_barrier", "true"),
                )
                .build(),
            renames: &["public.some_view->public.new_some_view"],
            expected_sql: &[],
            status: Status::Supported,
        },
        // ---- USING (access method, materialized) ----
        DiffCase {
            name: "alter using - materialize",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT 'asd'")
                        .materialized()
                        .tablespace("some_tablespace")
                        .using("some_using")
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT 'asd'")
                        .materialized()
                        .tablespace("some_tablespace")
                        .using("new_using")
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                r#"ALTER MATERIALIZED VIEW "public"."some_view" SET ACCESS METHOD "new_using";"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "set using - materialize",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT 'asd'")
                        .materialized()
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT 'asd'")
                        .materialized()
                        .using("new_using")
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                r#"ALTER MATERIALIZED VIEW "public"."some_view" SET ACCESS METHOD "new_using";"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "drop using - materialize",
            from: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT 'asd'")
                        .materialized()
                        .using("new_using")
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.some_view", "SELECT 'asd'")
                        .materialized()
                        .with_option("autovacuum_vacuum_cost_limit", "1"),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                r#"ALTER MATERIALIZED VIEW "public"."some_view" SET ACCESS METHOD "heap";"#,
            ],
            status: Status::Supported,
        },
        // ---- combined rename + alter ----
        DiffCase {
            name: "rename view and alter view",
            from: SchemaSnapshot::builder()
                .view(SnapView::new(
                    "public.some_view",
                    r#"SELECT * FROM "users""#,
                ))
                .build(),
            to: SchemaSnapshot::builder()
                .view(
                    SnapView::new("public.new_some_view", r#"SELECT * FROM "users""#)
                        .with_option("check_option", "cascaded"),
                )
                .build(),
            renames: &["public.some_view->public.new_some_view"],
            expected_sql: &[
                r#"ALTER VIEW "public"."some_view" RENAME TO "new_some_view";"#,
                r#"ALTER VIEW "public"."new_some_view" SET (check_option = cascaded);"#,
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "moved schema and alter view",
            from: SchemaSnapshot::builder()
                .schema("my_schema")
                .view(SnapView::new(
                    "public.some_view",
                    r#"SELECT * FROM "users""#,
                ))
                .build(),
            to: SchemaSnapshot::builder()
                .schema("my_schema")
                .view(
                    SnapView::new("my_schema.some_view", r#"SELECT * FROM "users""#)
                        .with_option("check_option", "cascaded"),
                )
                .build(),
            renames: &["public.some_view->my_schema.some_view"],
            expected_sql: &[
                r#"ALTER VIEW "public"."some_view" SET SCHEMA "my_schema";"#,
                r#"ALTER VIEW "my_schema"."some_view" SET (check_option = cascaded);"#,
            ],
            status: Status::Supported,
        },
    ]
}
