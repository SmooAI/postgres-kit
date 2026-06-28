//! Corpus category: `columns`.
//!
//! A conformance corpus of column schema-diff scenarios. Each scenario becomes one
//! [`DiffCase`]: the two schemas map to [`DiffCase::from`] / [`DiffCase::to`], the
//! rename hints into [`DiffCase::renames`], and the asserted statement output into
//! [`DiffCase::expected_sql`].
//!
//! Some scenarios in this file assert only a structured statement encoding (never
//! the rendered SQL), so they are recorded as [`Status::Skip`] with reason
//! `"statements-only encoding"` — the schemas are still translated faithfully so
//! the differ agent can promote them later by supplying the expected SQL.

use postgres_kit::differ::ir::{
    SchemaSnapshot, SnapColumn, SnapCompositePk, SnapForeignKey, SnapTable,
};

use super::{DiffCase, Status};

pub fn cases() -> Vec<DiffCase> {
    vec![
        // ---- add columns #1 ----
        // schema1: users { id serial primaryKey }
        // schema2: users { id serial primaryKey, name text }
        DiffCase {
            name: "add columns #1",
            from: SchemaSnapshot::builder()
                .table(SnapTable::new("users").col(SnapColumn::new("id", "serial").primary_key()))
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id", "serial").primary_key())
                        .col(SnapColumn::new("name", "text")),
                )
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- add columns #2 ----
        // schema1: users { id serial primaryKey }
        // schema2: users { id serial primaryKey, name text, email text }
        DiffCase {
            name: "add columns #2",
            from: SchemaSnapshot::builder()
                .table(SnapTable::new("users").col(SnapColumn::new("id", "serial").primary_key()))
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id", "serial").primary_key())
                        .col(SnapColumn::new("name", "text"))
                        .col(SnapColumn::new("email", "text")),
                )
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- alter column change name #1 ----
        // schema1: users { id serial primaryKey, name text('name') }
        // schema2: users { id serial primaryKey, name text('name1') }
        DiffCase {
            name: "alter column change name #1",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id", "serial").primary_key())
                        .col(SnapColumn::new("name", "text")),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id", "serial").primary_key())
                        .col(SnapColumn::new("name1", "text")),
                )
                .build(),
            renames: &["public.users.name->public.users.name1"],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- alter column change name #2 ----
        // schema1: users { id serial primaryKey, name text('name') }
        // schema2: users { id serial primaryKey, name text('name1'), email text }
        DiffCase {
            name: "alter column change name #2",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id", "serial").primary_key())
                        .col(SnapColumn::new("name", "text")),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id", "serial").primary_key())
                        .col(SnapColumn::new("name1", "text"))
                        .col(SnapColumn::new("email", "text")),
                )
                .build(),
            renames: &["public.users.name->public.users.name1"],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- alter table add composite pk ----
        // schema1: table { id1 integer, id2 integer }
        // schema2: table { id1 integer, id2 integer, primaryKey([id1, id2]) }
        DiffCase {
            name: "alter table add composite pk",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("table")
                        .col(SnapColumn::new("id1", "integer"))
                        .col(SnapColumn::new("id2", "integer")),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("table")
                        .col(SnapColumn::new("id1", "integer"))
                        .col(SnapColumn::new("id2", "integer"))
                        .composite_pk(SnapCompositePk::new("table_id1_id2_pk", ["id1", "id2"])),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ADD CONSTRAINT \"table_id1_id2_pk\" PRIMARY KEY(\"id1\",\"id2\");",
            ],
            status: Status::Supported,
        },
        // ---- rename table rename column #1 ----
        // schema1: users('users')   { id integer('id') }
        // schema2: users('users1')  { id integer('id1') }
        DiffCase {
            name: "rename table rename column #1",
            from: SchemaSnapshot::builder()
                .table(SnapTable::new("users").col(SnapColumn::new("id", "integer")))
                .build(),
            to: SchemaSnapshot::builder()
                .table(SnapTable::new("users1").col(SnapColumn::new("id1", "integer")))
                .build(),
            renames: &[
                "public.users->public.users1",
                "public.users1.id->public.users1.id1",
            ],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- with composite pks #1 ----
        // schema1: users { id1, id2, pk(id1,id2 name=compositePK) }
        // schema2: users { id1, id2, text, pk(id1,id2 name=compositePK) }
        DiffCase {
            name: "with composite pks #1",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id1", "integer"))
                        .col(SnapColumn::new("id2", "integer"))
                        .composite_pk(SnapCompositePk::new("compositePK", ["id1", "id2"])),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id1", "integer"))
                        .col(SnapColumn::new("id2", "integer"))
                        .col(SnapColumn::new("text", "text"))
                        .composite_pk(SnapCompositePk::new("compositePK", ["id1", "id2"])),
                )
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- with composite pks #2 ----
        // schema1: users { id1, id2 }
        // schema2: users { id1, id2, pk(id1,id2 name=compositePK) }
        DiffCase {
            name: "with composite pks #2",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id1", "integer"))
                        .col(SnapColumn::new("id2", "integer")),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id1", "integer"))
                        .col(SnapColumn::new("id2", "integer"))
                        .composite_pk(SnapCompositePk::new("compositePK", ["id1", "id2"])),
                )
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- with composite pks #3 ----
        // schema1: users { id1, id2, pk(id1,id2 name=compositePK) }
        // schema2: users { id1, id3, pk(id1,id3 name=compositePK) }
        DiffCase {
            name: "with composite pks #3",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id1", "integer"))
                        .col(SnapColumn::new("id2", "integer"))
                        .composite_pk(SnapCompositePk::new("compositePK", ["id1", "id2"])),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id1", "integer"))
                        .col(SnapColumn::new("id3", "integer"))
                        .composite_pk(SnapCompositePk::new("compositePK", ["id1", "id3"])),
                )
                .build(),
            renames: &["public.users.id2->public.users.id3"],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- add multiple constraints #1 ----
        // Adds onDelete actions to three FKs across three referenced tables.
        DiffCase {
            name: "add multiple constraints #1",
            from: SchemaSnapshot::builder()
                .table(SnapTable::new("t1").col(SnapColumn::new("id", "uuid").primary_key().default("gen_random_uuid()")))
                .table(SnapTable::new("t2").col(SnapColumn::new("id", "uuid").primary_key().default("gen_random_uuid()")))
                .table(SnapTable::new("t3").col(SnapColumn::new("id", "uuid").primary_key().default("gen_random_uuid()")))
                .table(
                    SnapTable::new("ref1")
                        .col(SnapColumn::new("id1", "uuid"))
                        .col(SnapColumn::new("id2", "uuid"))
                        .col(SnapColumn::new("id3", "uuid"))
                        .foreign_key(SnapForeignKey::new("ref1_id1_t1_id_fk", ["id1"], "public.t1", ["id"]))
                        .foreign_key(SnapForeignKey::new("ref1_id2_t2_id_fk", ["id2"], "public.t2", ["id"]))
                        .foreign_key(SnapForeignKey::new("ref1_id3_t3_id_fk", ["id3"], "public.t3", ["id"])),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(SnapTable::new("t1").col(SnapColumn::new("id", "uuid").primary_key().default("gen_random_uuid()")))
                .table(SnapTable::new("t2").col(SnapColumn::new("id", "uuid").primary_key().default("gen_random_uuid()")))
                .table(SnapTable::new("t3").col(SnapColumn::new("id", "uuid").primary_key().default("gen_random_uuid()")))
                .table(
                    SnapTable::new("ref1")
                        .col(SnapColumn::new("id1", "uuid"))
                        .col(SnapColumn::new("id2", "uuid"))
                        .col(SnapColumn::new("id3", "uuid"))
                        .foreign_key(SnapForeignKey::new("ref1_id1_t1_id_fk", ["id1"], "public.t1", ["id"]).on_delete("cascade"))
                        .foreign_key(SnapForeignKey::new("ref1_id2_t2_id_fk", ["id2"], "public.t2", ["id"]).on_delete("set null"))
                        .foreign_key(SnapForeignKey::new("ref1_id3_t3_id_fk", ["id3"], "public.t3", ["id"]).on_delete("cascade")),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"ref1\" DROP CONSTRAINT \"ref1_id1_t1_id_fk\";\nALTER TABLE \"ref1\" ADD CONSTRAINT \"ref1_id1_t1_id_fk\" FOREIGN KEY (\"id1\") REFERENCES \"public\".\"t1\"(\"id\") ON DELETE cascade ON UPDATE no action;",
                "ALTER TABLE \"ref1\" DROP CONSTRAINT \"ref1_id2_t2_id_fk\";\nALTER TABLE \"ref1\" ADD CONSTRAINT \"ref1_id2_t2_id_fk\" FOREIGN KEY (\"id2\") REFERENCES \"public\".\"t2\"(\"id\") ON DELETE set null ON UPDATE no action;",
                "ALTER TABLE \"ref1\" DROP CONSTRAINT \"ref1_id3_t3_id_fk\";\nALTER TABLE \"ref1\" ADD CONSTRAINT \"ref1_id3_t3_id_fk\" FOREIGN KEY (\"id3\") REFERENCES \"public\".\"t3\"(\"id\") ON DELETE cascade ON UPDATE no action;",
            ],
            status: Status::Supported,
        },
        // ---- add multiple constraints #2 ----
        // Single referenced table t1 with three PK columns; three FKs gain onDelete.
        DiffCase {
            name: "add multiple constraints #2",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("t1")
                        .col(SnapColumn::new("id1", "uuid").primary_key().default("gen_random_uuid()"))
                        .col(SnapColumn::new("id2", "uuid").primary_key().default("gen_random_uuid()"))
                        .col(SnapColumn::new("id3", "uuid").primary_key().default("gen_random_uuid()")),
                )
                .table(
                    SnapTable::new("ref1")
                        .col(SnapColumn::new("id1", "uuid"))
                        .col(SnapColumn::new("id2", "uuid"))
                        .col(SnapColumn::new("id3", "uuid"))
                        .foreign_key(SnapForeignKey::new("ref1_id1_t1_id1_fk", ["id1"], "public.t1", ["id1"]))
                        .foreign_key(SnapForeignKey::new("ref1_id2_t1_id2_fk", ["id2"], "public.t1", ["id2"]))
                        .foreign_key(SnapForeignKey::new("ref1_id3_t1_id3_fk", ["id3"], "public.t1", ["id3"])),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("t1")
                        .col(SnapColumn::new("id1", "uuid").primary_key().default("gen_random_uuid()"))
                        .col(SnapColumn::new("id2", "uuid").primary_key().default("gen_random_uuid()"))
                        .col(SnapColumn::new("id3", "uuid").primary_key().default("gen_random_uuid()")),
                )
                .table(
                    SnapTable::new("ref1")
                        .col(SnapColumn::new("id1", "uuid"))
                        .col(SnapColumn::new("id2", "uuid"))
                        .col(SnapColumn::new("id3", "uuid"))
                        .foreign_key(SnapForeignKey::new("ref1_id1_t1_id1_fk", ["id1"], "public.t1", ["id1"]).on_delete("cascade"))
                        .foreign_key(SnapForeignKey::new("ref1_id2_t1_id2_fk", ["id2"], "public.t1", ["id2"]).on_delete("set null"))
                        .foreign_key(SnapForeignKey::new("ref1_id3_t1_id3_fk", ["id3"], "public.t1", ["id3"]).on_delete("cascade")),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"ref1\" DROP CONSTRAINT \"ref1_id1_t1_id1_fk\";\nALTER TABLE \"ref1\" ADD CONSTRAINT \"ref1_id1_t1_id1_fk\" FOREIGN KEY (\"id1\") REFERENCES \"public\".\"t1\"(\"id1\") ON DELETE cascade ON UPDATE no action;",
                "ALTER TABLE \"ref1\" DROP CONSTRAINT \"ref1_id2_t1_id2_fk\";\nALTER TABLE \"ref1\" ADD CONSTRAINT \"ref1_id2_t1_id2_fk\" FOREIGN KEY (\"id2\") REFERENCES \"public\".\"t1\"(\"id2\") ON DELETE set null ON UPDATE no action;",
                "ALTER TABLE \"ref1\" DROP CONSTRAINT \"ref1_id3_t1_id3_fk\";\nALTER TABLE \"ref1\" ADD CONSTRAINT \"ref1_id3_t1_id3_fk\" FOREIGN KEY (\"id3\") REFERENCES \"public\".\"t1\"(\"id3\") ON DELETE cascade ON UPDATE no action;",
            ],
            status: Status::Supported,
        },
        // ---- add multiple constraints #3 ----
        // Three referencing tables (ref1/ref2/ref3) each gain an onDelete action.
        DiffCase {
            name: "add multiple constraints #3",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("t1")
                        .col(SnapColumn::new("id1", "uuid").primary_key().default("gen_random_uuid()"))
                        .col(SnapColumn::new("id2", "uuid").primary_key().default("gen_random_uuid()"))
                        .col(SnapColumn::new("id3", "uuid").primary_key().default("gen_random_uuid()")),
                )
                .table(
                    SnapTable::new("ref1")
                        .col(SnapColumn::new("id", "uuid"))
                        .foreign_key(SnapForeignKey::new("ref1_id_t1_id1_fk", ["id"], "public.t1", ["id1"])),
                )
                .table(
                    SnapTable::new("ref2")
                        .col(SnapColumn::new("id", "uuid"))
                        .foreign_key(SnapForeignKey::new("ref2_id_t1_id2_fk", ["id"], "public.t1", ["id2"])),
                )
                .table(
                    SnapTable::new("ref3")
                        .col(SnapColumn::new("id", "uuid"))
                        .foreign_key(SnapForeignKey::new("ref3_id_t1_id3_fk", ["id"], "public.t1", ["id3"])),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("t1")
                        .col(SnapColumn::new("id1", "uuid").primary_key().default("gen_random_uuid()"))
                        .col(SnapColumn::new("id2", "uuid").primary_key().default("gen_random_uuid()"))
                        .col(SnapColumn::new("id3", "uuid").primary_key().default("gen_random_uuid()")),
                )
                .table(
                    SnapTable::new("ref1")
                        .col(SnapColumn::new("id", "uuid"))
                        .foreign_key(SnapForeignKey::new("ref1_id_t1_id1_fk", ["id"], "public.t1", ["id1"]).on_delete("cascade")),
                )
                .table(
                    SnapTable::new("ref2")
                        .col(SnapColumn::new("id", "uuid"))
                        .foreign_key(SnapForeignKey::new("ref2_id_t1_id2_fk", ["id"], "public.t1", ["id2"]).on_delete("set null")),
                )
                .table(
                    SnapTable::new("ref3")
                        .col(SnapColumn::new("id", "uuid"))
                        .foreign_key(SnapForeignKey::new("ref3_id_t1_id3_fk", ["id"], "public.t1", ["id3"]).on_delete("cascade")),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"ref1\" DROP CONSTRAINT \"ref1_id_t1_id1_fk\";\nALTER TABLE \"ref1\" ADD CONSTRAINT \"ref1_id_t1_id1_fk\" FOREIGN KEY (\"id\") REFERENCES \"public\".\"t1\"(\"id1\") ON DELETE cascade ON UPDATE no action;",
                "ALTER TABLE \"ref2\" DROP CONSTRAINT \"ref2_id_t1_id2_fk\";\nALTER TABLE \"ref2\" ADD CONSTRAINT \"ref2_id_t1_id2_fk\" FOREIGN KEY (\"id\") REFERENCES \"public\".\"t1\"(\"id2\") ON DELETE set null ON UPDATE no action;",
                "ALTER TABLE \"ref3\" DROP CONSTRAINT \"ref3_id_t1_id3_fk\";\nALTER TABLE \"ref3\" ADD CONSTRAINT \"ref3_id_t1_id3_fk\" FOREIGN KEY (\"id\") REFERENCES \"public\".\"t1\"(\"id3\") ON DELETE cascade ON UPDATE no action;",
            ],
            status: Status::Supported,
        },
        // ---- varchar and text default values escape single quotes ----
        // schema1: table { id serial primaryKey }
        // schema2: table { id serial primaryKey,
        //                  text text default "escape's quotes",
        //                  varchar varchar default "escape's quotes" }
        // The snapshot stores the already-rendered SQL literal (single quotes
        // doubled) as the default expression, matching the verbatim-default model
        // used elsewhere (e.g. `now()`).
        DiffCase {
            name: "varchar and text default values escape single quotes",
            from: SchemaSnapshot::builder()
                .table(SnapTable::new("table").col(SnapColumn::new("id", "serial").primary_key()))
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("table")
                        .col(SnapColumn::new("id", "serial").primary_key())
                        .col(SnapColumn::new("text", "text").default("'escape''s quotes'"))
                        .col(SnapColumn::new("varchar", "varchar").default("'escape''s quotes'")),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ADD COLUMN \"text\" text DEFAULT 'escape''s quotes';",
                "ALTER TABLE \"table\" ADD COLUMN \"varchar\" varchar DEFAULT 'escape''s quotes';",
            ],
            status: Status::Supported,
        },
    ]
}
