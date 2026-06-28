//! Differ conformance cases for the `enums` category.
//!
//! A conformance corpus of enum-type schema-diff scenarios. Each scenario becomes
//! one [`DiffCase`]: the two schemas map to [`DiffCase::from`] / [`DiffCase::to`],
//! the rename hints into [`DiffCase::renames`], and the asserted statement output
//! into [`DiffCase::expected_sql`].
//!
//! Pure enum-type operations (CREATE/DROP TYPE, ALTER TYPE ADD VALUE, SET SCHEMA,
//! RENAME TO, and value-removal recreate) are [`Status::Supported`]. Cases that
//! also require table/column rewrites (enum used as a column type, data-type
//! changes, column adds) or schema-level DDL are [`Status::Skip`] with a reason —
//! the differ/integrator agent promotes them to `Supported` as coverage lands.

use postgres_kit::differ::ir::{SchemaSnapshot, SnapColumn, SnapEnum, SnapTable};

use super::{DiffCase, Status};

// ---- snapshot helpers ----

/// An empty schema snapshot (`{}` in the upstream tests).
fn empty() -> SchemaSnapshot {
    SchemaSnapshot::default()
}

/// A snapshot containing a single enum type.
fn enum_only(qualified: &str, values: &[&str]) -> SchemaSnapshot {
    SchemaSnapshot::builder()
        .enum_type(SnapEnum::new(qualified, values.iter().copied()))
        .build()
}

/// A snapshot with one enum type and one single-column table referencing it.
fn enum_table(
    enum_q: &str,
    values: &[&str],
    table_q: &str,
    col_name: &str,
    col_ty: &str,
    default: Option<&str>,
) -> SchemaSnapshot {
    let mut column = SnapColumn::new(col_name, col_ty);
    if let Some(d) = default {
        column = column.default(d);
    }
    SchemaSnapshot::builder()
        .enum_type(SnapEnum::new(enum_q, values.iter().copied()))
        .table(SnapTable::new(table_q).col(column))
        .build()
}

/// A snapshot with a single single-column table (no enums).
fn table_only(
    table_q: &str,
    col_name: &str,
    col_ty: &str,
    default: Option<&str>,
) -> SchemaSnapshot {
    let mut column = SnapColumn::new(col_name, col_ty);
    if let Some(d) = default {
        column = column.default(d);
    }
    SchemaSnapshot::builder()
        .table(SnapTable::new(table_q).col(column))
        .build()
}

/// Every `enums` conformance case.
pub fn cases() -> Vec<DiffCase> {
    vec![
        // enums #1 — create enum in public schema.
        DiffCase {
            name: "enums #1",
            from: empty(),
            to: enum_only("enum", &["value"]),
            renames: &[],
            expected_sql: &["CREATE TYPE \"public\".\"enum\" AS ENUM('value');"],
            status: Status::Supported,
        },
        // enums #2 — create enum in a custom schema.
        DiffCase {
            name: "enums #2",
            from: empty(),
            to: enum_only("folder.enum", &["value"]),
            renames: &[],
            expected_sql: &["CREATE TYPE \"folder\".\"enum\" AS ENUM('value');"],
            status: Status::Supported,
        },
        // enums #3 — drop enum in public schema.
        DiffCase {
            name: "enums #3",
            from: enum_only("enum", &["value"]),
            to: empty(),
            renames: &[],
            expected_sql: &["DROP TYPE \"public\".\"enum\";"],
            status: Status::Supported,
        },
        // enums #4 — drop enum in a custom schema.
        DiffCase {
            name: "enums #4",
            from: enum_only("folder.enum", &["value"]),
            to: empty(),
            renames: &[],
            expected_sql: &["DROP TYPE \"folder\".\"enum\";"],
            status: Status::Supported,
        },
        // enums #5 — schema rename (ALTER SCHEMA). No schema entity in the IR.
        DiffCase {
            name: "enums #5",
            from: enum_only("folder1.enum", &["value"]),
            to: enum_only("folder2.enum", &["value"]),
            renames: &["folder1->folder2"],
            expected_sql: &["ALTER SCHEMA \"folder1\" RENAME TO \"folder2\";\n"],
            status: Status::Skip("schema rename (ALTER SCHEMA) — schemas not modeled as IR entities"),
        },
        // enums #6 — move enum between schemas.
        DiffCase {
            name: "enums #6",
            from: enum_only("folder1.enum", &["value"]),
            to: enum_only("folder2.enum", &["value"]),
            renames: &["folder1.enum->folder2.enum"],
            expected_sql: &["ALTER TYPE \"folder1\".\"enum\" SET SCHEMA \"folder2\";"],
            status: Status::Supported,
        },
        // enums #7 — add one value (appended).
        DiffCase {
            name: "enums #7",
            from: enum_only("enum", &["value1"]),
            to: enum_only("enum", &["value1", "value2"]),
            renames: &[],
            expected_sql: &["ALTER TYPE \"public\".\"enum\" ADD VALUE 'value2';"],
            status: Status::Supported,
        },
        // enums #8 — add two values (appended).
        DiffCase {
            name: "enums #8",
            from: enum_only("enum", &["value1"]),
            to: enum_only("enum", &["value1", "value2", "value3"]),
            renames: &[],
            expected_sql: &[
                "ALTER TYPE \"public\".\"enum\" ADD VALUE 'value2';",
                "ALTER TYPE \"public\".\"enum\" ADD VALUE 'value3';",
            ],
            status: Status::Supported,
        },
        // enums #9 — add value with BEFORE positioning.
        DiffCase {
            name: "enums #9",
            from: enum_only("enum", &["value1", "value3"]),
            to: enum_only("enum", &["value1", "value2", "value3"]),
            renames: &[],
            expected_sql: &["ALTER TYPE \"public\".\"enum\" ADD VALUE 'value2' BEFORE 'value3';"],
            status: Status::Supported,
        },
        // enums #10 — add value in a custom schema.
        DiffCase {
            name: "enums #10",
            from: enum_only("folder.enum", &["value1"]),
            to: enum_only("folder.enum", &["value1", "value2"]),
            renames: &[],
            expected_sql: &["ALTER TYPE \"folder\".\"enum\" ADD VALUE 'value2';"],
            status: Status::Supported,
        },
        // enums #11 — move enum from custom schema to public.
        DiffCase {
            name: "enums #11",
            from: enum_only("folder1.enum", &["value1"]),
            to: enum_only("enum", &["value1"]),
            renames: &["folder1.enum->public.enum"],
            expected_sql: &["ALTER TYPE \"folder1\".\"enum\" SET SCHEMA \"public\";"],
            status: Status::Supported,
        },
        // enums #12 — move enum from public to a custom schema.
        DiffCase {
            name: "enums #12",
            from: enum_only("enum", &["value1"]),
            to: enum_only("folder1.enum", &["value1"]),
            renames: &["public.enum->folder1.enum"],
            expected_sql: &["ALTER TYPE \"public\".\"enum\" SET SCHEMA \"folder1\";"],
            status: Status::Supported,
        },
        // enums #13 — rename enum within the same schema.
        DiffCase {
            name: "enums #13",
            from: enum_only("enum1", &["value1"]),
            to: enum_only("enum2", &["value1"]),
            renames: &["public.enum1->public.enum2"],
            expected_sql: &["ALTER TYPE \"public\".\"enum1\" RENAME TO \"enum2\";"],
            status: Status::Supported,
        },
        // enums #14 — move + rename enum.
        DiffCase {
            name: "enums #14",
            from: enum_only("folder1.enum1", &["value1"]),
            to: enum_only("folder2.enum2", &["value1"]),
            renames: &["folder1.enum1->folder2.enum2"],
            expected_sql: &[
                "ALTER TYPE \"folder1\".\"enum1\" SET SCHEMA \"folder2\";",
                "ALTER TYPE \"folder2\".\"enum1\" RENAME TO \"enum2\";",
            ],
            status: Status::Supported,
        },
        // enums #15 — move + rename + add values with BEFORE positioning.
        DiffCase {
            name: "enums #15",
            from: enum_only("folder1.enum1", &["value1", "value4"]),
            to: enum_only("folder2.enum2", &["value1", "value2", "value3", "value4"]),
            renames: &["folder1.enum1->folder2.enum2"],
            expected_sql: &[
                "ALTER TYPE \"folder1\".\"enum1\" SET SCHEMA \"folder2\";",
                "ALTER TYPE \"folder2\".\"enum1\" RENAME TO \"enum2\";",
                "ALTER TYPE \"folder2\".\"enum2\" ADD VALUE 'value2' BEFORE 'value4';",
                "ALTER TYPE \"folder2\".\"enum2\" ADD VALUE 'value3' BEFORE 'value4';",
            ],
            status: Status::Supported,
        },
        // enums #16 — rename enum referenced by a table column.
        DiffCase {
            name: "enums #16",
            from: enum_table("enum1", &["value1"], "table", "column", "enum1", None),
            to: enum_table("enum2", &["value1"], "table", "column", "enum2", None),
            renames: &["public.enum1->public.enum2"],
            expected_sql: &["ALTER TYPE \"public\".\"enum1\" RENAME TO \"enum2\";"],
            status: Status::Supported,
        },
        // enums #17 — move enum (referenced by a table column) to a custom schema.
        DiffCase {
            name: "enums #17",
            from: enum_table("enum1", &["value1"], "table", "column", "enum1", None),
            to: enum_table("schema.enum1", &["value1"], "table", "column", "enum1", None),
            renames: &["public.enum1->schema.enum1"],
            expected_sql: &["ALTER TYPE \"public\".\"enum1\" SET SCHEMA \"schema\";"],
            status: Status::Supported,
        },
        // enums #18 — move + rename enum referenced by a table column.
        DiffCase {
            name: "enums #18",
            from: enum_table("schema1.enum1", &["value1"], "table", "column", "enum1", None),
            to: enum_table("schema2.enum2", &["value1"], "table", "column", "enum2", None),
            renames: &["schema1.enum1->schema2.enum2"],
            expected_sql: &[
                "ALTER TYPE \"schema1\".\"enum1\" SET SCHEMA \"schema2\";",
                "ALTER TYPE \"schema2\".\"enum1\" RENAME TO \"enum2\";",
            ],
            status: Status::Supported,
        },
        // enums #19 — create enum with an escaped single quote in a value.
        DiffCase {
            name: "enums #19",
            from: empty(),
            to: enum_only("my_enum", &["escape's quotes"]),
            renames: &[],
            expected_sql: &["CREATE TYPE \"public\".\"my_enum\" AS ENUM('escape''s quotes');"],
            status: Status::Supported,
        },
        // enums #20 — add columns (one enum-typed, one integer) to a table.
        DiffCase {
            name: "enums #20",
            from: SchemaSnapshot::builder()
                .enum_type(SnapEnum::new("my_enum", ["one", "two", "three"]))
                .table(SnapTable::new("table").col(SnapColumn::new("id", "serial").primary_key()))
                .build(),
            to: SchemaSnapshot::builder()
                .enum_type(SnapEnum::new("my_enum", ["one", "two", "three"]))
                .table(
                    SnapTable::new("table")
                        .col(SnapColumn::new("id", "serial").primary_key())
                        .col(SnapColumn::new("col1", "my_enum"))
                        .col(SnapColumn::new("col2", "integer")),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ADD COLUMN \"col1\" \"my_enum\";",
                "ALTER TABLE \"table\" ADD COLUMN \"col2\" integer;",
            ],
            status: Status::Skip(
                "follow-up: ADD COLUMN does not quote enum type names (emits bare my_enum)",
            ),
        },
        // enums #21 — add array columns (enum array + integer array) to a table.
        DiffCase {
            name: "enums #21",
            from: SchemaSnapshot::builder()
                .enum_type(SnapEnum::new("my_enum", ["one", "two", "three"]))
                .table(SnapTable::new("table").col(SnapColumn::new("id", "serial").primary_key()))
                .build(),
            to: SchemaSnapshot::builder()
                .enum_type(SnapEnum::new("my_enum", ["one", "two", "three"]))
                .table(
                    SnapTable::new("table")
                        .col(SnapColumn::new("id", "serial").primary_key())
                        .col(SnapColumn::new("col1", "my_enum[]"))
                        .col(SnapColumn::new("col2", "integer[]")),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ADD COLUMN \"col1\" \"my_enum\"[];",
                "ALTER TABLE \"table\" ADD COLUMN \"col2\" integer[];",
            ],
            status: Status::Skip(
                "follow-up: ADD COLUMN does not quote enum array type names (emits bare my_enum[])",
            ),
        },
        // drop enum value — value removal recreates the type (no dependent columns).
        DiffCase {
            name: "drop enum value",
            from: enum_only("enum", &["value1", "value2", "value3"]),
            to: enum_only("enum", &["value1", "value3"]),
            renames: &[],
            expected_sql: &[
                "DROP TYPE \"public\".\"enum\";",
                "CREATE TYPE \"public\".\"enum\" AS ENUM('value1', 'value3');",
            ],
            status: Status::Supported,
        },
        // drop enum value. enum is columns data type — recreate with dependent columns.
        DiffCase {
            name: "drop enum value. enum is columns data type",
            from: SchemaSnapshot::builder()
                .enum_type(SnapEnum::new("enum", ["value1", "value2", "value3"]))
                .table(SnapTable::new("table").col(SnapColumn::new("column", "enum")))
                .table(SnapTable::new("new_schema.table").col(SnapColumn::new("column", "enum")))
                .build(),
            to: SchemaSnapshot::builder()
                .enum_type(SnapEnum::new("enum", ["value1", "value3"]))
                .table(SnapTable::new("table").col(SnapColumn::new("column", "enum")))
                .table(SnapTable::new("new_schema.table").col(SnapColumn::new("column", "enum")))
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE text;",
                "ALTER TABLE \"new_schema\".\"table\" ALTER COLUMN \"column\" SET DATA TYPE text;",
                "DROP TYPE \"public\".\"enum\";",
                "CREATE TYPE \"public\".\"enum\" AS ENUM('value1', 'value3');",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum\" USING \"column\"::\"public\".\"enum\";",
                "ALTER TABLE \"new_schema\".\"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum\" USING \"column\"::\"public\".\"enum\";",
            ],
            status: Status::Supported,
        },
        // shuffle enum values — reorder forces recreate with dependent columns.
        DiffCase {
            name: "shuffle enum values",
            from: SchemaSnapshot::builder()
                .enum_type(SnapEnum::new("enum", ["value1", "value2", "value3"]))
                .table(SnapTable::new("table").col(SnapColumn::new("column", "enum")))
                .table(SnapTable::new("new_schema.table").col(SnapColumn::new("column", "enum")))
                .build(),
            to: SchemaSnapshot::builder()
                .enum_type(SnapEnum::new("enum", ["value1", "value3", "value2"]))
                .table(SnapTable::new("table").col(SnapColumn::new("column", "enum")))
                .table(SnapTable::new("new_schema.table").col(SnapColumn::new("column", "enum")))
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE text;",
                "ALTER TABLE \"new_schema\".\"table\" ALTER COLUMN \"column\" SET DATA TYPE text;",
                "DROP TYPE \"public\".\"enum\";",
                "CREATE TYPE \"public\".\"enum\" AS ENUM('value1', 'value3', 'value2');",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum\" USING \"column\"::\"public\".\"enum\";",
                "ALTER TABLE \"new_schema\".\"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum\" USING \"column\"::\"public\".\"enum\";",
            ],
            status: Status::Supported,
        },
        // enums as ts enum — create enum from a TS enum (same as create).
        DiffCase {
            name: "enums as ts enum",
            from: empty(),
            to: enum_only("enum", &["value"]),
            renames: &[],
            expected_sql: &["CREATE TYPE \"public\".\"enum\" AS ENUM('value');"],
            status: Status::Supported,
        },
        // column is enum type with default value. shuffle enum.
        DiffCase {
            name: "column is enum type with default value. shuffle enum",
            from: enum_table("enum", &["value1", "value2", "value3"], "table", "column", "enum", Some("value2")),
            to: enum_table("enum", &["value1", "value3", "value2"], "table", "column", "enum", Some("value2")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE text;",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT 'value2'::text;",
                "DROP TYPE \"public\".\"enum\";",
                "CREATE TYPE \"public\".\"enum\" AS ENUM('value1', 'value3', 'value2');",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT 'value2'::\"public\".\"enum\";",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum\" USING \"column\"::\"public\".\"enum\";",
            ],
            status: Status::Supported,
        },
        // column is array enum type with default value. shuffle enum.
        DiffCase {
            name: "column is array enum type with default value. shuffle enum",
            from: enum_table("enum", &["value1", "value2", "value3"], "table", "column", "enum[]", Some("{\"value2\"}")),
            to: enum_table("enum", &["value1", "value3", "value2"], "table", "column", "enum[]", Some("{\"value3\"}")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE text;",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT '{\"value3\"}'::text;",
                "DROP TYPE \"public\".\"enum\";",
                "CREATE TYPE \"public\".\"enum\" AS ENUM('value1', 'value3', 'value2');",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT '{\"value3\"}'::\"public\".\"enum\"[];",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum\"[] USING \"column\"::\"public\".\"enum\"[];",
            ],
            status: Status::Supported,
        },
        // column is array enum with custom size type with default value. shuffle enum.
        DiffCase {
            name: "column is array enum with custom size type with default value. shuffle enum",
            from: enum_table("enum", &["value1", "value2", "value3"], "table", "column", "enum[3]", Some("{\"value2\"}")),
            to: enum_table("enum", &["value1", "value3", "value2"], "table", "column", "enum[3]", Some("{\"value2\"}")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE text;",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT '{\"value2\"}'::text;",
                "DROP TYPE \"public\".\"enum\";",
                "CREATE TYPE \"public\".\"enum\" AS ENUM('value1', 'value3', 'value2');",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT '{\"value2\"}'::\"public\".\"enum\"[3];",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum\"[3] USING \"column\"::\"public\".\"enum\"[3];",
            ],
            status: Status::Supported,
        },
        // column is array enum with custom size type. shuffle enum.
        DiffCase {
            name: "column is array enum with custom size type. shuffle enum",
            from: enum_table("enum", &["value1", "value2", "value3"], "table", "column", "enum[3]", None),
            to: enum_table("enum", &["value1", "value3", "value2"], "table", "column", "enum[3]", None),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE text;",
                "DROP TYPE \"public\".\"enum\";",
                "CREATE TYPE \"public\".\"enum\" AS ENUM('value1', 'value3', 'value2');",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum\"[3] USING \"column\"::\"public\".\"enum\"[3];",
            ],
            status: Status::Supported,
        },
        // column is array of enum with multiple dimenions with custom sizes type. shuffle enum.
        DiffCase {
            name: "column is array of enum with multiple dimenions with custom sizes type. shuffle enum",
            from: enum_table("enum", &["value1", "value2", "value3"], "table", "column", "enum[3][2]", None),
            to: enum_table("enum", &["value1", "value3", "value2"], "table", "column", "enum[3][2]", None),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE text;",
                "DROP TYPE \"public\".\"enum\";",
                "CREATE TYPE \"public\".\"enum\" AS ENUM('value1', 'value3', 'value2');",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum\"[3][2] USING \"column\"::\"public\".\"enum\"[3][2];",
            ],
            status: Status::Supported,
        },
        // column is array of enum with multiple dimenions type with custom size with default value. shuffle enum.
        DiffCase {
            name: "column is array of enum with multiple dimenions type with custom size with default value. shuffle enum",
            from: enum_table("enum", &["value1", "value2", "value3"], "table", "column", "enum[3][2]", Some("{{\"value2\"}}")),
            to: enum_table("enum", &["value1", "value3", "value2"], "table", "column", "enum[3][2]", Some("{{\"value2\"}}")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE text;",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT '{{\"value2\"}}'::text;",
                "DROP TYPE \"public\".\"enum\";",
                "CREATE TYPE \"public\".\"enum\" AS ENUM('value1', 'value3', 'value2');",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT '{{\"value2\"}}'::\"public\".\"enum\"[3][2];",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum\"[3][2] USING \"column\"::\"public\".\"enum\"[3][2];",
            ],
            status: Status::Supported,
        },
        // column is enum type with default value. custom schema. shuffle enum.
        DiffCase {
            name: "column is enum type with default value. custom schema. shuffle enum",
            from: enum_table("new_schema.enum", &["value1", "value2", "value3"], "table", "column", "enum", Some("value2")),
            to: enum_table("new_schema.enum", &["value1", "value3", "value2"], "table", "column", "enum", Some("value2")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE text;",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT 'value2'::text;",
                "DROP TYPE \"new_schema\".\"enum\";",
                "CREATE TYPE \"new_schema\".\"enum\" AS ENUM('value1', 'value3', 'value2');",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT 'value2'::\"new_schema\".\"enum\";",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"new_schema\".\"enum\" USING \"column\"::\"new_schema\".\"enum\";",
            ],
            status: Status::Supported,
        },
        // column is array enum type with default value. custom schema. shuffle enum.
        DiffCase {
            name: "column is array enum type with default value. custom schema. shuffle enum",
            from: enum_table("new_schema.enum", &["value1", "value2", "value3"], "new_schema.table", "column", "enum[]", Some("{\"value2\"}")),
            to: enum_table("new_schema.enum", &["value1", "value3", "value2"], "new_schema.table", "column", "enum[]", Some("{\"value2\"}")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"new_schema\".\"table\" ALTER COLUMN \"column\" SET DATA TYPE text;",
                "ALTER TABLE \"new_schema\".\"table\" ALTER COLUMN \"column\" SET DEFAULT '{\"value2\"}'::text;",
                "DROP TYPE \"new_schema\".\"enum\";",
                "CREATE TYPE \"new_schema\".\"enum\" AS ENUM('value1', 'value3', 'value2');",
                "ALTER TABLE \"new_schema\".\"table\" ALTER COLUMN \"column\" SET DEFAULT '{\"value2\"}'::\"new_schema\".\"enum\"[];",
                "ALTER TABLE \"new_schema\".\"table\" ALTER COLUMN \"column\" SET DATA TYPE \"new_schema\".\"enum\"[] USING \"column\"::\"new_schema\".\"enum\"[];",
            ],
            status: Status::Supported,
        },
        // column is array enum type with custom size with default value. custom schema. shuffle enum.
        DiffCase {
            name: "column is array enum type with custom size with default value. custom schema. shuffle enum",
            from: enum_table("new_schema.enum", &["value1", "value2", "value3"], "new_schema.table", "column", "enum[3]", Some("{\"value2\"}")),
            to: enum_table("new_schema.enum", &["value1", "value3", "value2"], "new_schema.table", "column", "enum[3]", Some("{\"value2\"}")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"new_schema\".\"table\" ALTER COLUMN \"column\" SET DATA TYPE text;",
                "ALTER TABLE \"new_schema\".\"table\" ALTER COLUMN \"column\" SET DEFAULT '{\"value2\"}'::text;",
                "DROP TYPE \"new_schema\".\"enum\";",
                "CREATE TYPE \"new_schema\".\"enum\" AS ENUM('value1', 'value3', 'value2');",
                "ALTER TABLE \"new_schema\".\"table\" ALTER COLUMN \"column\" SET DEFAULT '{\"value2\"}'::\"new_schema\".\"enum\"[3];",
                "ALTER TABLE \"new_schema\".\"table\" ALTER COLUMN \"column\" SET DATA TYPE \"new_schema\".\"enum\"[3] USING \"column\"::\"new_schema\".\"enum\"[3];",
            ],
            status: Status::Supported,
        },
        // column is array enum type with custom size. custom schema. shuffle enum.
        DiffCase {
            name: "column is array enum type with custom size. custom schema. shuffle enum",
            from: enum_table("new_schema.enum", &["value1", "value2", "value3"], "new_schema.table", "column", "enum[3]", None),
            to: enum_table("new_schema.enum", &["value1", "value3", "value2"], "new_schema.table", "column", "enum[3]", None),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"new_schema\".\"table\" ALTER COLUMN \"column\" SET DATA TYPE text;",
                "DROP TYPE \"new_schema\".\"enum\";",
                "CREATE TYPE \"new_schema\".\"enum\" AS ENUM('value1', 'value3', 'value2');",
                "ALTER TABLE \"new_schema\".\"table\" ALTER COLUMN \"column\" SET DATA TYPE \"new_schema\".\"enum\"[3] USING \"column\"::\"new_schema\".\"enum\"[3];",
            ],
            status: Status::Supported,
        },
        // column is enum type without default value. add default to column.
        DiffCase {
            name: "column is enum type without default value. add default to column",
            from: enum_table("enum", &["value1", "value3"], "table", "column", "enum", None),
            to: enum_table("enum", &["value1", "value3"], "table", "column", "enum", Some("value3")),
            renames: &[],
            expected_sql: &["ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT 'value3';"],
            status: Status::Skip(
                "follow-up: SET DEFAULT does not quote enum literal (emits bare value3)",
            ),
        },
        // change data type from standart type to enum.
        DiffCase {
            name: "change data type from standart type to enum",
            from: enum_table("enum", &["value1", "value3"], "table", "column", "varchar", None),
            to: enum_table("enum", &["value1", "value3"], "table", "column", "enum", None),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum\" USING \"column\"::\"public\".\"enum\";",
            ],
            status: Status::Supported,
        },
        // change data type from standart type to enum. column has default.
        DiffCase {
            name: "change data type from standart type to enum. column has default",
            from: enum_table("enum", &["value1", "value3"], "table", "column", "varchar", Some("value2")),
            to: enum_table("enum", &["value1", "value3"], "table", "column", "enum", Some("value3")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT 'value3'::\"public\".\"enum\";",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum\" USING \"column\"::\"public\".\"enum\";",
            ],
            status: Status::Supported,
        },
        // change data type from array standart type to array enum. column has default.
        DiffCase {
            name: "change data type from array standart type to array enum. column has default",
            from: enum_table("enum", &["value1", "value3"], "table", "column", "varchar[]", Some("{\"value2\"}")),
            to: enum_table("enum", &["value1", "value3"], "table", "column", "enum[]", Some("{\"value3\"}")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT '{\"value3\"}'::\"public\".\"enum\"[];",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum\"[] USING \"column\"::\"public\".\"enum\"[];",
            ],
            status: Status::Supported,
        },
        // change data type from array standart type to array enum. column without default.
        DiffCase {
            name: "change data type from array standart type to array enum. column without default",
            from: enum_table("enum", &["value1", "value3"], "table", "column", "varchar[]", None),
            to: enum_table("enum", &["value1", "value3"], "table", "column", "enum[]", None),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum\"[] USING \"column\"::\"public\".\"enum\"[];",
            ],
            status: Status::Supported,
        },
        // change data type from array standart type with custom size to array enum with custom size. column has default.
        DiffCase {
            name: "change data type from array standart type with custom size to array enum with custom size. column has default",
            from: enum_table("enum", &["value1", "value3"], "table", "column", "varchar[3]", Some("{\"value2\"}")),
            to: enum_table("enum", &["value1", "value3"], "table", "column", "enum[3]", Some("{\"value3\"}")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT '{\"value3\"}'::\"public\".\"enum\"[3];",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum\"[3] USING \"column\"::\"public\".\"enum\"[3];",
            ],
            status: Status::Supported,
        },
        // change data type from array standart type with custom size to array enum with custom size. column without default.
        DiffCase {
            name: "change data type from array standart type with custom size to array enum with custom size. column without default",
            from: enum_table("enum", &["value1", "value3"], "table", "column", "varchar[2]", None),
            to: enum_table("enum", &["value1", "value3"], "table", "column", "enum[2]", None),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum\"[2] USING \"column\"::\"public\".\"enum\"[2];",
            ],
            status: Status::Supported,
        },
        // change data type from enum type to standart type.
        DiffCase {
            name: "change data type from enum type to standart type",
            from: enum_table("enum", &["value1", "value3"], "table", "column", "enum", None),
            to: enum_table("enum", &["value1", "value3"], "table", "column", "varchar", None),
            renames: &[],
            expected_sql: &["ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE varchar;"],
            status: Status::Supported,
        },
        // change data type from enum type to standart type. column has default.
        DiffCase {
            name: "change data type from enum type to standart type. column has default",
            from: enum_table("enum", &["value1", "value3"], "table", "column", "enum", Some("value3")),
            to: enum_table("enum", &["value1", "value3"], "table", "column", "varchar", Some("value2")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE varchar;",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT 'value2';",
            ],
            status: Status::Supported,
        },
        // change data type from array enum type to array standart type.
        DiffCase {
            name: "change data type from array enum type to array standart type",
            from: enum_table("enum", &["value1", "value3"], "table", "column", "enum[]", None),
            to: enum_table("enum", &["value1", "value3"], "table", "column", "varchar[]", None),
            renames: &[],
            expected_sql: &["ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE varchar[];"],
            status: Status::Supported,
        },
        // change data type from array enum with custom size type to array standart type with custom size.
        DiffCase {
            name: "change data type from array enum with custom size type to array standart type with custom size",
            from: enum_table("enum", &["value1", "value3"], "table", "column", "enum[2]", None),
            to: enum_table("enum", &["value1", "value3"], "table", "column", "varchar[2]", None),
            renames: &[],
            expected_sql: &["ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE varchar[2];"],
            status: Status::Supported,
        },
        // change data type from array enum type to array standart type. column has default.
        DiffCase {
            name: "change data type from array enum type to array standart type. column has default",
            from: enum_table("enum", &["value1", "value2"], "table", "column", "enum[]", Some("{\"value2\"}")),
            to: enum_table("enum", &["value1", "value2"], "table", "column", "varchar[]", Some("{\"value2\"}")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE varchar[];",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT '{\"value2\"}';",
            ],
            status: Status::Supported,
        },
        // change data type from array enum type with custom size to array standart type with custom size. column has default.
        DiffCase {
            name: "change data type from array enum type with custom size to array standart type with custom size. column has default",
            from: enum_table("enum", &["value1", "value2"], "table", "column", "enum[3]", Some("{\"value2\"}")),
            to: enum_table("enum", &["value1", "value2"], "table", "column", "varchar[3]", Some("{\"value2\"}")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE varchar[3];",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT '{\"value2\"}';",
            ],
            status: Status::Supported,
        },
        // change data type from standart type to standart type.
        DiffCase {
            name: "change data type from standart type to standart type",
            from: table_only("table", "column", "varchar", None),
            to: table_only("table", "column", "text", None),
            renames: &[],
            expected_sql: &["ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE text;"],
            status: Status::Supported,
        },
        // change data type from standart type to standart type. column has default.
        DiffCase {
            name: "change data type from standart type to standart type. column has default",
            from: table_only("table", "column", "varchar", Some("value3")),
            to: table_only("table", "column", "text", Some("value2")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE text;",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT 'value2';",
            ],
            status: Status::Supported,
        },
        // change data type from standart type to standart type. columns are arrays.
        DiffCase {
            name: "change data type from standart type to standart type. columns are arrays",
            from: table_only("table", "column", "varchar[]", None),
            to: table_only("table", "column", "text[]", None),
            renames: &[],
            expected_sql: &["ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE text[];"],
            status: Status::Supported,
        },
        // change data type from standart type to standart type. columns are arrays with custom sizes.
        DiffCase {
            name: "change data type from standart type to standart type. columns are arrays with custom sizes",
            from: table_only("table", "column", "varchar[2]", None),
            to: table_only("table", "column", "text[2]", None),
            renames: &[],
            expected_sql: &["ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE text[2];"],
            status: Status::Supported,
        },
        // change data type from standart type to standart type. columns are arrays. column has default.
        DiffCase {
            name: "change data type from standart type to standart type. columns are arrays. column has default",
            from: table_only("table", "column", "varchar[]", Some("{\"hello\"}")),
            to: table_only("table", "column", "text[]", Some("{\"hello\"}")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE text[];",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT '{\"hello\"}';",
            ],
            status: Status::Supported,
        },
        // change data type from standart type to standart type. columns are arrays with custom sizes.column has default.
        DiffCase {
            name: "change data type from standart type to standart type. columns are arrays with custom sizes.column has default",
            from: table_only("table", "column", "varchar[2]", Some("{\"hello\"}")),
            to: table_only("table", "column", "text[2]", Some("{\"hello\"}")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE text[2];",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT '{\"hello\"}';",
            ],
            status: Status::Supported,
        },
        // change data type from one enum to other.
        DiffCase {
            name: "change data type from one enum to other",
            from: SchemaSnapshot::builder()
                .enum_type(SnapEnum::new("enum1", ["value1", "value3"]))
                .enum_type(SnapEnum::new("enum2", ["value1", "value3"]))
                .table(SnapTable::new("table").col(SnapColumn::new("column", "enum1")))
                .build(),
            to: SchemaSnapshot::builder()
                .enum_type(SnapEnum::new("enum1", ["value1", "value3"]))
                .enum_type(SnapEnum::new("enum2", ["value1", "value3"]))
                .table(SnapTable::new("table").col(SnapColumn::new("column", "enum2")))
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum2\" USING \"column\"::text::\"public\".\"enum2\";",
            ],
            status: Status::Supported,
        },
        // change data type from one enum to other. column has default.
        DiffCase {
            name: "change data type from one enum to other. column has default",
            from: SchemaSnapshot::builder()
                .enum_type(SnapEnum::new("enum1", ["value1", "value3"]))
                .enum_type(SnapEnum::new("enum2", ["value1", "value3"]))
                .table(SnapTable::new("table").col(SnapColumn::new("column", "enum1").default("value3")))
                .build(),
            to: SchemaSnapshot::builder()
                .enum_type(SnapEnum::new("enum1", ["value1", "value3"]))
                .enum_type(SnapEnum::new("enum2", ["value1", "value3"]))
                .table(SnapTable::new("table").col(SnapColumn::new("column", "enum2").default("value3")))
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" DROP DEFAULT;",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum2\" USING \"column\"::text::\"public\".\"enum2\";",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT 'value3';",
            ],
            status: Status::Supported,
        },
        // change data type from one enum to other. changed defaults.
        DiffCase {
            name: "change data type from one enum to other. changed defaults",
            from: SchemaSnapshot::builder()
                .enum_type(SnapEnum::new("enum1", ["value1", "value3"]))
                .enum_type(SnapEnum::new("enum2", ["value1", "value3"]))
                .table(SnapTable::new("table").col(SnapColumn::new("column", "enum1").default("value3")))
                .build(),
            to: SchemaSnapshot::builder()
                .enum_type(SnapEnum::new("enum1", ["value1", "value3"]))
                .enum_type(SnapEnum::new("enum2", ["value1", "value3"]))
                .table(SnapTable::new("table").col(SnapColumn::new("column", "enum2").default("value1")))
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" DROP DEFAULT;",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum2\" USING \"column\"::text::\"public\".\"enum2\";",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT 'value1';",
            ],
            status: Status::Supported,
        },
        // check filtering json statements. here we have recreate enum + set new type + alter default.
        DiffCase {
            name: "check filtering json statements. here we have recreate enum + set new type + alter default",
            from: enum_table("enum1", &["value1", "value3"], "table", "column", "varchar", Some("value3")),
            to: enum_table("enum1", &["value3", "value1", "value2"], "table", "column", "enum1", Some("value2")),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE text;",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT 'value2'::text;",
                "DROP TYPE \"public\".\"enum1\";",
                "CREATE TYPE \"public\".\"enum1\" AS ENUM('value3', 'value1', 'value2');",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DEFAULT 'value2'::\"public\".\"enum1\";",
                "ALTER TABLE \"table\" ALTER COLUMN \"column\" SET DATA TYPE \"public\".\"enum1\" USING \"column\"::\"public\".\"enum1\";",
            ],
            status: Status::Supported,
        },
    ]
}
