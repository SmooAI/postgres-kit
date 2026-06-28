//! Corpus category: `checks` — CHECK constraint diffing.
//!
//! A conformance corpus of CHECK-constraint schema-diff scenarios. Each
//! [`DiffCase`] is one scenario: `from`/`to` are the two schemas translated into
//! [`SchemaSnapshot`] literals, `renames` lists the rename hints, and
//! `expected_sql` is the asserted statement output.

use super::{DiffCase, Status};
use postgres_kit::differ::ir::*;
use postgres_kit::differ::SchemaSnapshot;

/// `users` table with `id serial PRIMARY KEY` + `age integer`.
fn users_id_age() -> SnapTable {
    SnapTable::new("public.users")
        .col(SnapColumn::new("id", "serial").primary_key().not_null())
        .col(SnapColumn::new("age", "integer"))
}

/// `users` table with `id serial PRIMARY KEY` + `age integer` + `name varchar`.
fn users_id_age_name() -> SnapTable {
    SnapTable::new("public.users")
        .col(SnapColumn::new("id", "serial").primary_key().not_null())
        .col(SnapColumn::new("age", "integer"))
        .col(SnapColumn::new("name", "varchar"))
}

pub fn cases() -> Vec<DiffCase> {
    vec![
        // test('create table with check')
        DiffCase {
            name: "create table with check",
            from: SchemaSnapshot::builder().build(),
            to: SchemaSnapshot::builder()
                .table(
                    users_id_age()
                        .check(SnapCheck::new("some_check_name", "\"users\".\"age\" > 21")),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "CREATE TABLE \"users\" (\n\t\"id\" serial PRIMARY KEY NOT NULL,\n\t\"age\" integer,\n\tCONSTRAINT \"some_check_name\" CHECK (\"users\".\"age\" > 21)\n);\n",
            ],
            status: Status::Supported,
        },
        // test('add check contraint to existing table')
        DiffCase {
            name: "add check contraint to existing table",
            from: SchemaSnapshot::builder().table(users_id_age()).build(),
            to: SchemaSnapshot::builder()
                .table(
                    users_id_age()
                        .check(SnapCheck::new("some_check_name", "\"users\".\"age\" > 21")),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"users\" ADD CONSTRAINT \"some_check_name\" CHECK (\"users\".\"age\" > 21);",
            ],
            status: Status::Supported,
        },
        // test('drop check contraint in existing table')
        DiffCase {
            name: "drop check contraint in existing table",
            from: SchemaSnapshot::builder()
                .table(
                    users_id_age()
                        .check(SnapCheck::new("some_check_name", "\"users\".\"age\" > 21")),
                )
                .build(),
            to: SchemaSnapshot::builder().table(users_id_age()).build(),
            renames: &[],
            expected_sql: &["ALTER TABLE \"users\" DROP CONSTRAINT \"some_check_name\";"],
            status: Status::Supported,
        },
        // test('rename check constraint')
        DiffCase {
            name: "rename check constraint",
            from: SchemaSnapshot::builder()
                .table(
                    users_id_age()
                        .check(SnapCheck::new("some_check_name", "\"users\".\"age\" > 21")),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    users_id_age()
                        .check(SnapCheck::new("new_check_name", "\"users\".\"age\" > 21")),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"users\" DROP CONSTRAINT \"some_check_name\";",
                "ALTER TABLE \"users\" ADD CONSTRAINT \"new_check_name\" CHECK (\"users\".\"age\" > 21);",
            ],
            status: Status::Supported,
        },
        // test('alter check constraint')
        DiffCase {
            name: "alter check constraint",
            from: SchemaSnapshot::builder()
                .table(
                    users_id_age()
                        .check(SnapCheck::new("some_check_name", "\"users\".\"age\" > 21")),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    users_id_age()
                        .check(SnapCheck::new("new_check_name", "\"users\".\"age\" > 10")),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"users\" DROP CONSTRAINT \"some_check_name\";",
                "ALTER TABLE \"users\" ADD CONSTRAINT \"new_check_name\" CHECK (\"users\".\"age\" > 10);",
            ],
            status: Status::Supported,
        },
        // test('alter multiple check constraints')
        DiffCase {
            name: "alter multiple check constraints",
            from: SchemaSnapshot::builder()
                .table(
                    users_id_age_name()
                        .check(SnapCheck::new("some_check_name_1", "\"users\".\"age\" > 21"))
                        .check(SnapCheck::new(
                            "some_check_name_2",
                            "\"users\".\"name\" != 'Alex'",
                        )),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    users_id_age_name()
                        .check(SnapCheck::new("some_check_name_3", "\"users\".\"age\" > 21"))
                        .check(SnapCheck::new(
                            "some_check_name_4",
                            "\"users\".\"name\" != 'Alex'",
                        )),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"users\" DROP CONSTRAINT \"some_check_name_1\";",
                "ALTER TABLE \"users\" DROP CONSTRAINT \"some_check_name_2\";",
                "ALTER TABLE \"users\" ADD CONSTRAINT \"some_check_name_3\" CHECK (\"users\".\"age\" > 21);",
                "ALTER TABLE \"users\" ADD CONSTRAINT \"some_check_name_4\" CHECK (\"users\".\"name\" != 'Alex');",
            ],
            status: Status::Supported,
        },
        // test('create checks with same names') — asserts the diff REJECTS (throws);
        // there is no sqlStatements contract to encode.
        DiffCase {
            name: "create checks with same names",
            from: SchemaSnapshot::builder().build(),
            to: SchemaSnapshot::builder()
                .table(
                    users_id_age_name()
                        .check(SnapCheck::new("some_check_name", "\"users\".\"age\" > 21"))
                        .check(SnapCheck::new(
                            "some_check_name",
                            "\"users\".\"name\" != 'Alex'",
                        )),
                )
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("error case: duplicate constraint names rejected, no sqlStatements asserted"),
        },
    ]
}
