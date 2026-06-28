//! Corpus category: `roles`.
//!
//! A conformance corpus of role schema-diff scenarios. Each case
//! maps `from`/`to` schemas as [`SchemaSnapshot`] literals, the `renames` hints,
//! and the asserted statement array into `expected_sql`.
//!
//! Role defaults (a bare role): `createDb: false`, `createRole: false`,
//! `inherit: true` — which match [`SnapRole`]'s defaults.

use postgres_kit::differ::ir::*;
use postgres_kit::differ::SchemaSnapshot;

use super::{DiffCase, Status};

pub fn cases() -> Vec<DiffCase> {
    vec![
        // test('create role')
        DiffCase {
            name: "create role",
            from: SchemaSnapshot::builder().build(),
            to: SchemaSnapshot::builder()
                .role(SnapRole::new("manager"))
                .build(),
            renames: &[],
            expected_sql: &[r#"CREATE ROLE "manager";"#],
            status: Status::Supported,
        },
        // test('create role with properties')
        DiffCase {
            name: "create role with properties",
            from: SchemaSnapshot::builder().build(),
            to: SchemaSnapshot::builder()
                .role(
                    SnapRole::new("manager")
                        .create_db(true)
                        .create_role(true)
                        .inherit(false),
                )
                .build(),
            renames: &[],
            expected_sql: &[r#"CREATE ROLE "manager" WITH CREATEDB CREATEROLE NOINHERIT;"#],
            status: Status::Supported,
        },
        // test('create role with some properties')
        DiffCase {
            name: "create role with some properties",
            from: SchemaSnapshot::builder().build(),
            to: SchemaSnapshot::builder()
                .role(SnapRole::new("manager").create_db(true).inherit(false))
                .build(),
            renames: &[],
            expected_sql: &[r#"CREATE ROLE "manager" WITH CREATEDB NOINHERIT;"#],
            status: Status::Supported,
        },
        // test('drop role')
        DiffCase {
            name: "drop role",
            from: SchemaSnapshot::builder()
                .role(SnapRole::new("manager"))
                .build(),
            to: SchemaSnapshot::builder().build(),
            renames: &[],
            expected_sql: &[r#"DROP ROLE "manager";"#],
            status: Status::Supported,
        },
        // test('create and drop role')
        DiffCase {
            name: "create and drop role",
            from: SchemaSnapshot::builder()
                .role(SnapRole::new("manager"))
                .build(),
            to: SchemaSnapshot::builder()
                .role(SnapRole::new("admin"))
                .build(),
            renames: &[],
            expected_sql: &[r#"DROP ROLE "manager";"#, r#"CREATE ROLE "admin";"#],
            status: Status::Supported,
        },
        // test('rename role')
        DiffCase {
            name: "rename role",
            from: SchemaSnapshot::builder()
                .role(SnapRole::new("manager"))
                .build(),
            to: SchemaSnapshot::builder()
                .role(SnapRole::new("admin"))
                .build(),
            renames: &["manager->admin"],
            expected_sql: &[r#"ALTER ROLE "manager" RENAME TO "admin";"#],
            status: Status::Supported,
        },
        // test('alter all role field')
        DiffCase {
            name: "alter all role field",
            from: SchemaSnapshot::builder()
                .role(SnapRole::new("manager"))
                .build(),
            to: SchemaSnapshot::builder()
                .role(
                    SnapRole::new("manager")
                        .create_db(true)
                        .create_role(true)
                        .inherit(false),
                )
                .build(),
            renames: &[],
            expected_sql: &[r#"ALTER ROLE "manager" WITH CREATEDB CREATEROLE NOINHERIT;"#],
            status: Status::Supported,
        },
        // test('alter createdb in role')
        DiffCase {
            name: "alter createdb in role",
            from: SchemaSnapshot::builder()
                .role(SnapRole::new("manager"))
                .build(),
            to: SchemaSnapshot::builder()
                .role(SnapRole::new("manager").create_db(true))
                .build(),
            renames: &[],
            expected_sql: &[r#"ALTER ROLE "manager" WITH CREATEDB NOCREATEROLE INHERIT;"#],
            status: Status::Supported,
        },
        // test('alter createrole in role')
        DiffCase {
            name: "alter createrole in role",
            from: SchemaSnapshot::builder()
                .role(SnapRole::new("manager"))
                .build(),
            to: SchemaSnapshot::builder()
                .role(SnapRole::new("manager").create_role(true))
                .build(),
            renames: &[],
            expected_sql: &[r#"ALTER ROLE "manager" WITH NOCREATEDB CREATEROLE INHERIT;"#],
            status: Status::Supported,
        },
        // test('alter inherit in role')
        DiffCase {
            name: "alter inherit in role",
            from: SchemaSnapshot::builder()
                .role(SnapRole::new("manager"))
                .build(),
            to: SchemaSnapshot::builder()
                .role(SnapRole::new("manager").inherit(false))
                .build(),
            renames: &[],
            expected_sql: &[r#"ALTER ROLE "manager" WITH NOCREATEDB NOCREATEROLE NOINHERIT;"#],
            status: Status::Supported,
        },
    ]
}
