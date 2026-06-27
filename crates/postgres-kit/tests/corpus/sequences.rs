//! Corpus category: `sequences`.
//!
//! Ported verbatim from drizzle-kit's `tests/pg-sequences.test.ts`. Each case
//! translates `schema1` → [`DiffCase::from`] and `schema2` → [`DiffCase::to`] as
//! [`SchemaSnapshot`] literals, copies the `renames` hints verbatim, and copies
//! the asserted `sqlStatements` array into `expected_sql`.
//!
//! Sequence defaults (drizzle `pgSequence(name, opts)`): unspecified options fall
//! back to `increment: '1'`, `minValue: '1'`, `maxValue: '9223372036854775807'`,
//! `cache: '1'`, `cycle: false`. The snapshot below records only the options the
//! drizzle schema actually sets (leaving the rest `None`); the differ is
//! responsible for filling the defaults when it renders the SQL.

use postgres_kit::differ::ir::*;
use postgres_kit::differ::SchemaSnapshot;

use super::{DiffCase, Status};

/// Build a [`SnapSequence`] setting exactly the fields the drizzle schema does.
/// `SnapSequence` only ships builders for `increment`/`start_with`/`cycle`, so the
/// remaining (public) fields are assigned directly.
fn seq(
    qualified: &str,
    increment: Option<&str>,
    min_value: Option<&str>,
    max_value: Option<&str>,
    start_with: Option<&str>,
    cache: Option<&str>,
    cycle: bool,
) -> SnapSequence {
    let mut s = SnapSequence::new(qualified);
    s.increment = increment.map(Into::into);
    s.min_value = min_value.map(Into::into);
    s.max_value = max_value.map(Into::into);
    s.start_with = start_with.map(Into::into);
    s.cache = cache.map(Into::into);
    s.cycle = cycle;
    s
}

pub fn cases() -> Vec<DiffCase> {
    vec![
        // test('create sequence')
        DiffCase {
            name: "create sequence",
            from: SchemaSnapshot::builder().build(),
            to: SchemaSnapshot::builder()
                .sequence(seq(
                    "public.name",
                    None,
                    None,
                    None,
                    Some("100"),
                    None,
                    false,
                ))
                .build(),
            renames: &[],
            expected_sql: &[
                r#"CREATE SEQUENCE "public"."name" INCREMENT BY 1 MINVALUE 1 MAXVALUE 9223372036854775807 START WITH 100 CACHE 1;"#,
            ],
            status: Status::Supported,
        },
        // test('create sequence: all fields')
        DiffCase {
            name: "create sequence: all fields",
            from: SchemaSnapshot::builder().build(),
            to: SchemaSnapshot::builder()
                .sequence(seq(
                    "public.name",
                    Some("2"),
                    Some("100"),
                    Some("10000"),
                    Some("100"),
                    Some("10"),
                    true,
                ))
                .build(),
            renames: &[],
            expected_sql: &[
                r#"CREATE SEQUENCE "public"."name" INCREMENT BY 2 MINVALUE 100 MAXVALUE 10000 START WITH 100 CACHE 10 CYCLE;"#,
            ],
            status: Status::Supported,
        },
        // test('create sequence: custom schema')
        DiffCase {
            name: "create sequence: custom schema",
            from: SchemaSnapshot::builder().build(),
            to: SchemaSnapshot::builder()
                .sequence(seq(
                    "custom.name",
                    None,
                    None,
                    None,
                    Some("100"),
                    None,
                    false,
                ))
                .build(),
            renames: &[],
            expected_sql: &[
                r#"CREATE SEQUENCE "custom"."name" INCREMENT BY 1 MINVALUE 1 MAXVALUE 9223372036854775807 START WITH 100 CACHE 1;"#,
            ],
            status: Status::Supported,
        },
        // test('create sequence: custom schema + all fields')
        DiffCase {
            name: "create sequence: custom schema + all fields",
            from: SchemaSnapshot::builder().build(),
            to: SchemaSnapshot::builder()
                .sequence(seq(
                    "custom.name",
                    Some("2"),
                    Some("100"),
                    Some("10000"),
                    Some("100"),
                    Some("10"),
                    true,
                ))
                .build(),
            renames: &[],
            expected_sql: &[
                r#"CREATE SEQUENCE "custom"."name" INCREMENT BY 2 MINVALUE 100 MAXVALUE 10000 START WITH 100 CACHE 10 CYCLE;"#,
            ],
            status: Status::Supported,
        },
        // test('drop sequence')
        DiffCase {
            name: "drop sequence",
            from: SchemaSnapshot::builder()
                .sequence(seq(
                    "public.name",
                    None,
                    None,
                    None,
                    Some("100"),
                    None,
                    false,
                ))
                .build(),
            to: SchemaSnapshot::builder().build(),
            renames: &[],
            expected_sql: &[r#"DROP SEQUENCE "public"."name";"#],
            status: Status::Supported,
        },
        // test('drop sequence: custom schema')
        DiffCase {
            name: "drop sequence: custom schema",
            from: SchemaSnapshot::builder()
                .sequence(seq(
                    "custom.name",
                    None,
                    None,
                    None,
                    Some("100"),
                    None,
                    false,
                ))
                .build(),
            to: SchemaSnapshot::builder().build(),
            renames: &[],
            expected_sql: &[r#"DROP SEQUENCE "custom"."name";"#],
            status: Status::Supported,
        },
        // test('rename sequence')
        DiffCase {
            name: "rename sequence",
            from: SchemaSnapshot::builder()
                .sequence(seq(
                    "public.name",
                    None,
                    None,
                    None,
                    Some("100"),
                    None,
                    false,
                ))
                .build(),
            to: SchemaSnapshot::builder()
                .sequence(seq(
                    "public.name_new",
                    None,
                    None,
                    None,
                    Some("100"),
                    None,
                    false,
                ))
                .build(),
            renames: &["public.name->public.name_new"],
            expected_sql: &[r#"ALTER SEQUENCE "public"."name" RENAME TO "name_new";"#],
            status: Status::Supported,
        },
        // test('rename sequence in custom schema')
        DiffCase {
            name: "rename sequence in custom schema",
            from: SchemaSnapshot::builder()
                .sequence(seq(
                    "custom.name",
                    None,
                    None,
                    None,
                    Some("100"),
                    None,
                    false,
                ))
                .build(),
            to: SchemaSnapshot::builder()
                .sequence(seq(
                    "custom.name_new",
                    None,
                    None,
                    None,
                    Some("100"),
                    None,
                    false,
                ))
                .build(),
            renames: &["custom.name->custom.name_new"],
            expected_sql: &[r#"ALTER SEQUENCE "custom"."name" RENAME TO "name_new";"#],
            status: Status::Supported,
        },
        // test('move sequence between schemas #1')
        DiffCase {
            name: "move sequence between schemas #1",
            from: SchemaSnapshot::builder()
                .sequence(seq(
                    "public.name",
                    None,
                    None,
                    None,
                    Some("100"),
                    None,
                    false,
                ))
                .build(),
            to: SchemaSnapshot::builder()
                .sequence(seq(
                    "custom.name",
                    None,
                    None,
                    None,
                    Some("100"),
                    None,
                    false,
                ))
                .build(),
            renames: &["public.name->custom.name"],
            expected_sql: &[r#"ALTER SEQUENCE "public"."name" SET SCHEMA "custom";"#],
            status: Status::Supported,
        },
        // test('move sequence between schemas #2')
        DiffCase {
            name: "move sequence between schemas #2",
            from: SchemaSnapshot::builder()
                .sequence(seq(
                    "custom.name",
                    None,
                    None,
                    None,
                    Some("100"),
                    None,
                    false,
                ))
                .build(),
            to: SchemaSnapshot::builder()
                .sequence(seq(
                    "public.name",
                    None,
                    None,
                    None,
                    Some("100"),
                    None,
                    false,
                ))
                .build(),
            renames: &["custom.name->public.name"],
            expected_sql: &[r#"ALTER SEQUENCE "custom"."name" SET SCHEMA "public";"#],
            status: Status::Supported,
        },
        // test('alter sequence')
        DiffCase {
            name: "alter sequence",
            from: SchemaSnapshot::builder()
                .sequence(seq(
                    "public.name",
                    None,
                    None,
                    None,
                    Some("100"),
                    None,
                    false,
                ))
                .build(),
            to: SchemaSnapshot::builder()
                .sequence(seq(
                    "public.name",
                    None,
                    None,
                    None,
                    Some("105"),
                    None,
                    false,
                ))
                .build(),
            renames: &[],
            expected_sql: &[
                r#"ALTER SEQUENCE "public"."name" INCREMENT BY 1 MINVALUE 1 MAXVALUE 9223372036854775807 START WITH 105 CACHE 1;"#,
            ],
            status: Status::Supported,
        },
    ]
}
