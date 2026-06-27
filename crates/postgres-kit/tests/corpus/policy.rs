//! Policy / RLS conformance cases, ported from drizzle-kit's
//! `drizzle-kit/tests/rls/pg-policy.test.ts`.
//!
//! Each drizzle `test(...)` calls `diffTestSchemas(schema1, schema2, renames)`
//! and asserts a `sqlStatements` array. We translate `schema1 -> from`,
//! `schema2 -> to` as [`SchemaSnapshot`] literals, copy `renames` verbatim, and
//! copy the asserted `sqlStatements` into `expected_sql`.
//!
//! Mechanical translation notes:
//! - `pgPolicy('n', { as: 'permissive' })` -> `SnapPolicy::new("n").as_permissiveness(PolicyAs::Permissive)`
//! - `for: 'delete'` -> `.for_command(PolicyFor::Delete)`; `to: 'current_role'` -> `.to_roles(["current_role"])`
//! - `using: sql`true`` -> `.using("true")`; `withCheck: sql`true`` -> `.with_check("true")`
//! - drizzle implicitly emits `ENABLE ROW LEVEL SECURITY` for any table that has
//!   a policy, so a table carrying >=1 policy is rendered with `.enable_rls()`.
//! - `pgRole('manager').existing()` is referenced by name only (`"manager"`); an
//!   `.existing()` role is never `CREATE`d, so it is not added to the snapshot.
//!
//! Independent (schema-level) policies: drizzle's `.link(<table not in the schema
//! object>)` produces *individual* policies (`create_ind_policy` /
//! `drop_ind_policy` / `alter_ind_policy` / `rename_ind_policy`) whose SQL targets
//! a fully-qualified `"schema"."table"` that is otherwise absent from the schema.
//! These live in [`SchemaSnapshot::ind_policies`] (built via `.ind_policy(...)`),
//! not inside a `SnapTable`, and render with an explicit schema (even `public`).
//!
//! Still skipped: the `BTreeMap` insertion-order case noted inline.

use postgres_kit::differ::ir::*;
use postgres_kit::{PolicyAs, PolicyFor};

use super::{DiffCase, Status};

/// A `users`-shaped table with the canonical `id integer PRIMARY KEY NOT NULL`.
fn tbl(name: &str) -> SnapTable {
    SnapTable::new(name).col(SnapColumn::new("id", "integer").primary_key().not_null())
}

/// Wrap a single table into a snapshot.
fn snap(table: SnapTable) -> SchemaSnapshot {
    SchemaSnapshot::builder().table(table).build()
}

/// An empty schema.
fn empty() -> SchemaSnapshot {
    SchemaSnapshot::builder().build()
}

/// Wrap a single independent (schema-level) policy on an absent `schema.table`.
fn snap_ind(schema: &str, table: &str, policy: SnapPolicy) -> SchemaSnapshot {
    SchemaSnapshot::builder()
        .ind_policy(SnapIndPolicy::new(schema, table, policy))
        .build()
}

pub fn cases() -> Vec<DiffCase> {
    vec![
        // ---- add / drop policy with implicit rls toggle ----
        DiffCase {
            name: "add policy + enable rls",
            from: snap(tbl("users")),
            to: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"users\" ENABLE ROW LEVEL SECURITY;",
                "CREATE POLICY \"test\" ON \"users\" AS PERMISSIVE FOR ALL TO public;",
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "drop policy + disable rls",
            from: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            to: snap(tbl("users")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"users\" DISABLE ROW LEVEL SECURITY;",
                "DROP POLICY \"test\" ON \"users\" CASCADE;",
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "add policy without enable rls",
            from: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            to: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .policy(SnapPolicy::new("newRls"))
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &["CREATE POLICY \"newRls\" ON \"users\" AS PERMISSIVE FOR ALL TO public;"],
            status: Status::Supported,
        },
        DiffCase {
            name: "drop policy without disable rls",
            from: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .policy(SnapPolicy::new("oldRls"))
                    .enable_rls(),
            ),
            to: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &["DROP POLICY \"oldRls\" ON \"users\" CASCADE;"],
            status: Status::Supported,
        },
        // ---- alter policy without recreation ----
        DiffCase {
            name: "alter policy without recreation: changing roles",
            from: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            to: snap(
                tbl("users")
                    .policy(
                        SnapPolicy::new("test")
                            .as_permissiveness(PolicyAs::Permissive)
                            .to_roles(["current_role"]),
                    )
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &["ALTER POLICY \"test\" ON \"users\" TO current_role;"],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter policy without recreation: changing using",
            from: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            to: snap(
                tbl("users")
                    .policy(
                        SnapPolicy::new("test")
                            .as_permissiveness(PolicyAs::Permissive)
                            .using("true"),
                    )
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &["ALTER POLICY \"test\" ON \"users\" TO public USING (true);"],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter policy without recreation: changing with check",
            from: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            to: snap(
                tbl("users")
                    .policy(
                        SnapPolicy::new("test")
                            .as_permissiveness(PolicyAs::Permissive)
                            .with_check("true"),
                    )
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &["ALTER POLICY \"test\" ON \"users\" TO public WITH CHECK (true);"],
            status: Status::Supported,
        },
        // ---- alter policy with recreation ----
        DiffCase {
            name: "alter policy with recreation: changing as",
            from: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            to: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Restrictive))
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &[
                "DROP POLICY \"test\" ON \"users\" CASCADE;",
                "CREATE POLICY \"test\" ON \"users\" AS RESTRICTIVE FOR ALL TO public;",
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter policy with recreation: changing for",
            from: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            to: snap(
                tbl("users")
                    .policy(
                        SnapPolicy::new("test")
                            .as_permissiveness(PolicyAs::Permissive)
                            .for_command(PolicyFor::Delete),
                    )
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &[
                "DROP POLICY \"test\" ON \"users\" CASCADE;",
                "CREATE POLICY \"test\" ON \"users\" AS PERMISSIVE FOR DELETE TO public;",
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter policy with recreation: changing both as and for",
            from: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            to: snap(
                tbl("users")
                    .policy(
                        SnapPolicy::new("test")
                            .as_permissiveness(PolicyAs::Restrictive)
                            .for_command(PolicyFor::Insert),
                    )
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &[
                "DROP POLICY \"test\" ON \"users\" CASCADE;",
                "CREATE POLICY \"test\" ON \"users\" AS RESTRICTIVE FOR INSERT TO public;",
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter policy with recreation: changing all fields",
            from: snap(
                tbl("users")
                    .policy(
                        SnapPolicy::new("test")
                            .as_permissiveness(PolicyAs::Permissive)
                            .for_command(PolicyFor::Select)
                            .using("true"),
                    )
                    .enable_rls(),
            ),
            to: snap(
                tbl("users")
                    .policy(
                        SnapPolicy::new("test")
                            .as_permissiveness(PolicyAs::Restrictive)
                            .to_roles(["current_role"])
                            .with_check("true"),
                    )
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &[
                "DROP POLICY \"test\" ON \"users\" CASCADE;",
                "CREATE POLICY \"test\" ON \"users\" AS RESTRICTIVE FOR ALL TO current_role WITH CHECK (true);",
            ],
            status: Status::Supported,
        },
        // ---- rename ----
        DiffCase {
            name: "rename policy",
            from: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            to: snap(
                tbl("users")
                    .policy(SnapPolicy::new("newName").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            renames: &["public.users.test->public.users.newName"],
            expected_sql: &["ALTER POLICY \"test\" ON \"users\" RENAME TO \"newName\";"],
            status: Status::Supported,
        },
        DiffCase {
            name: "rename policy in renamed table",
            from: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            to: snap(
                tbl("users2")
                    .policy(SnapPolicy::new("newName").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            renames: &[
                "public.users->public.users2",
                "public.users2.test->public.users2.newName",
            ],
            expected_sql: &[
                "ALTER TABLE \"users\" RENAME TO \"users2\";",
                "ALTER POLICY \"test\" ON \"users2\" RENAME TO \"newName\";",
            ],
            status: Status::Supported,
        },
        // ---- create / drop table carrying a policy ----
        DiffCase {
            name: "create table with a policy",
            from: empty(),
            to: snap(
                tbl("users2")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &[
                "CREATE TABLE \"users2\" (\n\t\"id\" integer PRIMARY KEY NOT NULL\n);\n",
                "ALTER TABLE \"users2\" ENABLE ROW LEVEL SECURITY;",
                "CREATE POLICY \"test\" ON \"users2\" AS PERMISSIVE FOR ALL TO public;",
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "drop table with a policy",
            from: snap(
                tbl("users2")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            to: empty(),
            renames: &[],
            expected_sql: &[
                "DROP POLICY \"test\" ON \"users2\" CASCADE;",
                "DROP TABLE \"users2\" CASCADE;",
            ],
            status: Status::Supported,
        },
        // ---- multiple `to` roles ----
        DiffCase {
            name: "add policy with multiple to roles",
            from: snap(tbl("users")),
            to: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").to_roles(["current_role", "manager"]))
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"users\" ENABLE ROW LEVEL SECURITY;",
                "CREATE POLICY \"test\" ON \"users\" AS PERMISSIVE FOR ALL TO current_role, \"manager\";",
            ],
            status: Status::Supported,
        },
        // ---- forced rls toggles (no policy) ----
        DiffCase {
            name: "create table with rls enabled",
            from: empty(),
            to: snap(tbl("users").enable_rls()),
            renames: &[],
            expected_sql: &[
                "CREATE TABLE \"users\" (\n\t\"id\" integer PRIMARY KEY NOT NULL\n);\n",
                "ALTER TABLE \"users\" ENABLE ROW LEVEL SECURITY;",
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "enable rls force",
            from: snap(tbl("users")),
            to: snap(tbl("users").enable_rls()),
            renames: &[],
            expected_sql: &["ALTER TABLE \"users\" ENABLE ROW LEVEL SECURITY;"],
            status: Status::Supported,
        },
        DiffCase {
            name: "disable rls force",
            from: snap(tbl("users").enable_rls()),
            to: snap(tbl("users")),
            renames: &[],
            expected_sql: &["ALTER TABLE \"users\" DISABLE ROW LEVEL SECURITY;"],
            status: Status::Supported,
        },
        DiffCase {
            name: "drop policy with enabled rls",
            from: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").to_roles(["current_role", "manager"]))
                    .enable_rls(),
            ),
            to: snap(tbl("users").enable_rls()),
            renames: &[],
            expected_sql: &["DROP POLICY \"test\" ON \"users\" CASCADE;"],
            status: Status::Supported,
        },
        DiffCase {
            name: "add policy with enabled rls",
            from: snap(tbl("users").enable_rls()),
            to: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").to_roles(["current_role", "manager"]))
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &["CREATE POLICY \"test\" ON \"users\" AS PERMISSIVE FOR ALL TO current_role, \"manager\";"],
            status: Status::Supported,
        },
        // ---- link / unlink to a table that IS in the schema (attaches normally) ----
        DiffCase {
            name: "add policy + link table",
            from: snap(tbl("users")),
            to: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"users\" ENABLE ROW LEVEL SECURITY;",
                "CREATE POLICY \"test\" ON \"users\" AS PERMISSIVE FOR ALL TO public;",
            ],
            status: Status::Supported,
        },
        DiffCase {
            // schema1's unlinked top-level policy is attached to no table => a
            // no-op; only the linked schema2 policy materializes.
            name: "link table",
            from: snap(tbl("users")),
            to: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"users\" ENABLE ROW LEVEL SECURITY;",
                "CREATE POLICY \"test\" ON \"users\" AS PERMISSIVE FOR ALL TO public;",
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "unlink table",
            from: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            to: snap(tbl("users")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"users\" DISABLE ROW LEVEL SECURITY;",
                "DROP POLICY \"test\" ON \"users\" CASCADE;",
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "drop policy with link",
            from: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            to: snap(tbl("users")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"users\" DISABLE ROW LEVEL SECURITY;",
                "DROP POLICY \"test\" ON \"users\" CASCADE;",
            ],
            status: Status::Supported,
        },
        DiffCase {
            // Two new policies created in one diff; drizzle emits them in
            // insertion order (test1, then the linked test). The snapshot keys
            // policies in a BTreeMap, so they iterate name-sorted (test, test1),
            // which inverts the expected statement order.
            name: "add policy in table and with link table",
            from: snap(tbl("users")),
            to: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test1").to_roles(["current_user"]))
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"users\" ENABLE ROW LEVEL SECURITY;",
                "CREATE POLICY \"test1\" ON \"users\" AS PERMISSIVE FOR ALL TO current_user;",
                "CREATE POLICY \"test\" ON \"users\" AS PERMISSIVE FOR ALL TO public;",
            ],
            status: Status::Skip(
                "multi-policy creation emits in insertion order (test1, test); BTreeMap iterates name-sorted (test, test1)",
            ),
        },
        // ---- linked policies on a NON-schema table (ind_policy) ----
        DiffCase {
            name: "link non-schema table",
            from: empty(),
            to: snap_ind(
                "public",
                "users",
                SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive),
            ),
            renames: &[],
            expected_sql: &["CREATE POLICY \"test\" ON \"public\".\"users\" AS PERMISSIVE FOR ALL TO public;"],
            status: Status::Supported,
        },
        DiffCase {
            name: "unlink non-schema table",
            from: snap_ind(
                "public",
                "users",
                SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive),
            ),
            to: empty(),
            renames: &[],
            expected_sql: &["DROP POLICY \"test\" ON \"public\".\"users\" CASCADE;"],
            status: Status::Supported,
        },
        DiffCase {
            name: "add policy + link non-schema table",
            from: snap(tbl("users")),
            to: SchemaSnapshot::builder()
                .table(
                    tbl("users")
                        .policy(SnapPolicy::new("test2").as_permissiveness(PolicyAs::Permissive))
                        .enable_rls(),
                )
                .ind_policy(SnapIndPolicy::new(
                    "public",
                    "cities",
                    SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive),
                ))
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"users\" ENABLE ROW LEVEL SECURITY;",
                "CREATE POLICY \"test2\" ON \"users\" AS PERMISSIVE FOR ALL TO public;",
                "CREATE POLICY \"test\" ON \"public\".\"cities\" AS PERMISSIVE FOR ALL TO public;",
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "add policy + link non-schema table from auth schema",
            from: snap(tbl("users")),
            to: SchemaSnapshot::builder()
                .table(
                    tbl("users")
                        .policy(SnapPolicy::new("test2").as_permissiveness(PolicyAs::Permissive))
                        .enable_rls(),
                )
                .ind_policy(SnapIndPolicy::new(
                    "auth",
                    "cities",
                    SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive),
                ))
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"users\" ENABLE ROW LEVEL SECURITY;",
                "CREATE POLICY \"test2\" ON \"users\" AS PERMISSIVE FOR ALL TO public;",
                "CREATE POLICY \"test\" ON \"auth\".\"cities\" AS PERMISSIVE FOR ALL TO public;",
            ],
            status: Status::Supported,
        },
        DiffCase {
            name: "rename policy that is linked",
            from: snap_ind(
                "public",
                "users",
                SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive),
            ),
            to: snap_ind(
                "public",
                "users",
                SnapPolicy::new("newName").as_permissiveness(PolicyAs::Permissive),
            ),
            renames: &["ind_policy:public.users.test->public.users.newName"],
            expected_sql: &["ALTER POLICY \"test\" ON \"public\".\"users\" RENAME TO \"newName\";"],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter policy that is linked: changing roles",
            from: snap_ind(
                "public",
                "users",
                SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive),
            ),
            to: snap_ind(
                "public",
                "users",
                SnapPolicy::new("test")
                    .as_permissiveness(PolicyAs::Permissive)
                    .to_roles(["current_role"]),
            ),
            renames: &[],
            expected_sql: &["ALTER POLICY \"test\" ON \"public\".\"users\" TO current_role;"],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter policy that is linked: with check",
            from: snap_ind(
                "public",
                "users",
                SnapPolicy::new("test")
                    .as_permissiveness(PolicyAs::Permissive)
                    .with_check("true"),
            ),
            to: snap_ind(
                "public",
                "users",
                SnapPolicy::new("test")
                    .as_permissiveness(PolicyAs::Permissive)
                    .with_check("false"),
            ),
            renames: &[],
            expected_sql: &["ALTER POLICY \"test\" ON \"public\".\"users\" TO public WITH CHECK (false);"],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter policy that is linked: using",
            from: snap_ind(
                "public",
                "users",
                SnapPolicy::new("test")
                    .as_permissiveness(PolicyAs::Permissive)
                    .using("true"),
            ),
            to: snap_ind(
                "public",
                "users",
                SnapPolicy::new("test")
                    .as_permissiveness(PolicyAs::Permissive)
                    .using("false"),
            ),
            renames: &[],
            expected_sql: &["ALTER POLICY \"test\" ON \"public\".\"users\" TO public USING (false);"],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter policy that is linked: for recreation",
            from: snap_ind(
                "public",
                "users",
                SnapPolicy::new("test")
                    .as_permissiveness(PolicyAs::Permissive)
                    .for_command(PolicyFor::Insert),
            ),
            to: snap_ind(
                "public",
                "users",
                SnapPolicy::new("test")
                    .as_permissiveness(PolicyAs::Permissive)
                    .for_command(PolicyFor::Delete),
            ),
            renames: &[],
            expected_sql: &[
                "DROP POLICY \"test\" ON \"public\".\"users\" CASCADE;",
                "CREATE POLICY \"test\" ON \"public\".\"users\" AS PERMISSIVE FOR DELETE TO public;",
            ],
            status: Status::Supported,
        },
        // ---- alter policy declared in the table (array form) ----
        DiffCase {
            name: "alter policy in the table: changing roles",
            from: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").as_permissiveness(PolicyAs::Permissive))
                    .enable_rls(),
            ),
            to: snap(
                tbl("users")
                    .policy(
                        SnapPolicy::new("test")
                            .as_permissiveness(PolicyAs::Permissive)
                            .to_roles(["current_role"]),
                    )
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &["ALTER POLICY \"test\" ON \"users\" TO current_role;"],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter policy in the table: with check",
            from: snap(
                tbl("users")
                    .policy(
                        SnapPolicy::new("test")
                            .as_permissiveness(PolicyAs::Permissive)
                            .with_check("true"),
                    )
                    .enable_rls(),
            ),
            to: snap(
                tbl("users")
                    .policy(
                        SnapPolicy::new("test")
                            .as_permissiveness(PolicyAs::Permissive)
                            .with_check("false"),
                    )
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &["ALTER POLICY \"test\" ON \"users\" TO public WITH CHECK (false);"],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter policy in the table: using",
            from: snap(
                tbl("users")
                    .policy(
                        SnapPolicy::new("test")
                            .as_permissiveness(PolicyAs::Permissive)
                            .using("true"),
                    )
                    .enable_rls(),
            ),
            to: snap(
                tbl("users")
                    .policy(
                        SnapPolicy::new("test")
                            .as_permissiveness(PolicyAs::Permissive)
                            .using("false"),
                    )
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &["ALTER POLICY \"test\" ON \"users\" TO public USING (false);"],
            status: Status::Supported,
        },
        DiffCase {
            name: "alter policy in the table: for recreation",
            from: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").for_command(PolicyFor::Insert))
                    .enable_rls(),
            ),
            to: snap(
                tbl("users")
                    .policy(SnapPolicy::new("test").for_command(PolicyFor::Delete))
                    .enable_rls(),
            ),
            renames: &[],
            expected_sql: &[
                "DROP POLICY \"test\" ON \"users\" CASCADE;",
                "CREATE POLICY \"test\" ON \"users\" AS PERMISSIVE FOR DELETE TO public;",
            ],
            status: Status::Supported,
        },
    ]
}
