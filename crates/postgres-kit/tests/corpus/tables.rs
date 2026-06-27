//! `tables` differ corpus — ported from drizzle-kit's `tests/pg-tables.test.ts`.
//!
//! Each [`DiffCase`] mirrors one `test(...)` in that file: `schema1` -> `from`,
//! `schema2` -> `to`, the rename hints copied verbatim into `renames`, and the
//! asserted `sqlStatements` copied verbatim into `expected_sql`.
//!
//! Tests that assert ONLY the `statements` IR (no `sqlStatements`) carry no SQL
//! contract, so they are `Skip("statements-only encoding")` — their `from`/`to`
//! snapshots are still authored faithfully so the differ agent can promote them.

use postgres_kit::differ::ir::*;

use super::{DiffCase, Status};

/// Empty schema (`{}` in the drizzle tests).
fn empty() -> SchemaSnapshot {
    SchemaSnapshot::default()
}

pub fn cases() -> Vec<DiffCase> {
    vec![
        // ---- add table #1 ----
        DiffCase {
            name: "add table #1",
            from: empty(),
            to: SchemaSnapshot::builder()
                .table(SnapTable::new("users"))
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- add table #2 ----
        DiffCase {
            name: "add table #2",
            from: empty(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id", "serial").not_null().primary_key()),
                )
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- add table #3 ----
        DiffCase {
            name: "add table #3",
            from: empty(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id", "serial").not_null())
                        .composite_pk(SnapCompositePk::new("users_pk", ["id"])),
                )
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- add table #4 ----
        DiffCase {
            name: "add table #4",
            from: empty(),
            to: SchemaSnapshot::builder()
                .table(SnapTable::new("users"))
                .table(SnapTable::new("posts"))
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- add table #5 ---- (folder schema; empty schema has no snapshot form)
        DiffCase {
            name: "add table #5",
            from: empty(),
            to: SchemaSnapshot::builder()
                .table(SnapTable::new("folder.users"))
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- add table #6 ----
        DiffCase {
            name: "add table #6",
            from: SchemaSnapshot::builder()
                .table(SnapTable::new("users1"))
                .build(),
            to: SchemaSnapshot::builder()
                .table(SnapTable::new("users2"))
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- add table #7 ----
        DiffCase {
            name: "add table #7",
            from: SchemaSnapshot::builder()
                .table(SnapTable::new("users1"))
                .build(),
            to: SchemaSnapshot::builder()
                .table(SnapTable::new("users"))
                .table(SnapTable::new("users2"))
                .build(),
            renames: &["public.users1->public.users2"],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- add table #8: geometry types ----
        DiffCase {
            name: "add table #8: geometry types",
            from: empty(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("geom", "geometry(point)").not_null())
                        .col(SnapColumn::new("geom1", "geometry(point)").not_null()),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "CREATE TABLE \"users\" (\n\t\"geom\" geometry(point) NOT NULL,\n\t\"geom1\" geometry(point) NOT NULL\n);\n",
            ],
            status: Status::Supported,
        },
        // ---- multiproject schema add table #1 ----
        DiffCase {
            name: "multiproject schema add table #1",
            from: empty(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("prefix_users")
                        .col(SnapColumn::new("id", "serial").not_null().primary_key()),
                )
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- multiproject schema drop table #1 ----
        DiffCase {
            name: "multiproject schema drop table #1",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("prefix_users")
                        .col(SnapColumn::new("id", "serial").not_null().primary_key()),
                )
                .build(),
            to: empty(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- multiproject schema alter table name #1 ----
        DiffCase {
            name: "multiproject schema alter table name #1",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("prefix_users")
                        .col(SnapColumn::new("id", "serial").not_null().primary_key()),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("prefix_users1")
                        .col(SnapColumn::new("id", "serial").not_null().primary_key()),
                )
                .build(),
            renames: &["public.prefix_users->public.prefix_users1"],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- add table #8: column with pgvector ----
        DiffCase {
            name: "add table #8: column with pgvector",
            from: empty(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users2")
                        .col(SnapColumn::new("id", "serial").not_null().primary_key())
                        .col(SnapColumn::new("name", "vector(3)")),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "CREATE TABLE \"users2\" (\n\t\"id\" serial PRIMARY KEY NOT NULL,\n\t\"name\" vector(3)\n);\n",
            ],
            status: Status::Supported,
        },
        // ---- add schema + table #1 ----
        DiffCase {
            name: "add schema + table #1",
            from: empty(),
            to: SchemaSnapshot::builder()
                .table(SnapTable::new("folder.users"))
                .build(),
            renames: &[],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- change schema with tables #1 ----
        DiffCase {
            name: "change schema with tables #1",
            from: SchemaSnapshot::builder()
                .table(SnapTable::new("folder.users"))
                .build(),
            to: SchemaSnapshot::builder()
                .table(SnapTable::new("folder2.users"))
                .build(),
            renames: &["folder->folder2"],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- change table schema #1 ----
        DiffCase {
            name: "change table schema #1",
            from: SchemaSnapshot::builder()
                .table(SnapTable::new("public.users"))
                .build(),
            to: SchemaSnapshot::builder()
                .table(SnapTable::new("folder.users"))
                .build(),
            renames: &["public.users->folder.users"],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- change table schema #2 ----
        DiffCase {
            name: "change table schema #2",
            from: SchemaSnapshot::builder()
                .table(SnapTable::new("folder.users"))
                .build(),
            to: SchemaSnapshot::builder()
                .table(SnapTable::new("public.users"))
                .build(),
            renames: &["folder.users->public.users"],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- change table schema #3 ----
        DiffCase {
            name: "change table schema #3",
            from: SchemaSnapshot::builder()
                .table(SnapTable::new("folder1.users"))
                .build(),
            to: SchemaSnapshot::builder()
                .table(SnapTable::new("folder2.users"))
                .build(),
            renames: &["folder1.users->folder2.users"],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- change table schema #4 ----
        DiffCase {
            name: "change table schema #4",
            from: SchemaSnapshot::builder()
                .table(SnapTable::new("folder1.users"))
                .build(),
            to: SchemaSnapshot::builder()
                .table(SnapTable::new("folder2.users"))
                .build(),
            renames: &["folder1.users->folder2.users"],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- change table schema #5 (move across schemas, drop old) ----
        DiffCase {
            name: "change table schema #5",
            from: SchemaSnapshot::builder()
                .table(SnapTable::new("folder1.users"))
                .build(),
            to: SchemaSnapshot::builder()
                .table(SnapTable::new("folder2.users"))
                .build(),
            renames: &["folder1.users->folder2.users"],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- change table schema #5 (rename and move table) ----
        DiffCase {
            name: "change table schema #5 (rename and move)",
            from: SchemaSnapshot::builder()
                .table(SnapTable::new("folder1.users"))
                .build(),
            to: SchemaSnapshot::builder()
                .table(SnapTable::new("folder2.users2"))
                .build(),
            renames: &["folder1.users->folder2.users2"],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- change table schema #6 ----
        DiffCase {
            name: "change table schema #6",
            from: SchemaSnapshot::builder()
                .table(SnapTable::new("folder1.users"))
                .build(),
            to: SchemaSnapshot::builder()
                .table(SnapTable::new("folder2.users2"))
                .build(),
            renames: &["folder1->folder2", "folder2.users->folder2.users2"],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- drop table + rename schema #1 ----
        DiffCase {
            name: "drop table + rename schema #1",
            from: SchemaSnapshot::builder()
                .table(SnapTable::new("folder1.users"))
                .build(),
            to: empty(),
            renames: &["folder1->folder2"],
            expected_sql: &[],
            status: Status::Skip("statements-only encoding"),
        },
        // ---- create table with tsvector ----
        DiffCase {
            name: "create table with tsvector",
            from: empty(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("posts")
                        .col(SnapColumn::new("id", "serial").not_null().primary_key())
                        .col(SnapColumn::new("title", "text").not_null())
                        .col(SnapColumn::new("description", "text").not_null())
                        .index(
                            SnapIndex::new(
                                "title_search_index",
                                [SnapIndexColumn::expr("to_tsvector('english', \"title\")")],
                            )
                            .method("gin"),
                        ),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "CREATE TABLE \"posts\" (\n\t\"id\" serial PRIMARY KEY NOT NULL,\n\t\"title\" text NOT NULL,\n\t\"description\" text NOT NULL\n);\n",
                "CREATE INDEX \"title_search_index\" ON \"posts\" USING gin (to_tsvector('english', \"title\"));",
            ],
            status: Status::Supported,
        },
        // ---- composite primary key ----
        DiffCase {
            name: "composite primary key",
            from: empty(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("works_to_creators")
                        .col(SnapColumn::new("work_id", "integer").not_null())
                        .col(SnapColumn::new("creator_id", "integer").not_null())
                        .col(SnapColumn::new("classification", "text").not_null())
                        .composite_pk(SnapCompositePk::new(
                            "works_to_creators_work_id_creator_id_classification_pk",
                            ["work_id", "creator_id", "classification"],
                        )),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "CREATE TABLE \"works_to_creators\" (\n\t\"work_id\" integer NOT NULL,\n\t\"creator_id\" integer NOT NULL,\n\t\"classification\" text NOT NULL,\n\tCONSTRAINT \"works_to_creators_work_id_creator_id_classification_pk\" PRIMARY KEY(\"work_id\",\"creator_id\",\"classification\")\n);\n",
            ],
            status: Status::Supported,
        },
        // ---- add column before creating unique constraint ----
        DiffCase {
            name: "add column before creating unique constraint",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("table")
                        .col(SnapColumn::new("id", "serial").not_null().primary_key()),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("table")
                        .col(SnapColumn::new("id", "serial").not_null().primary_key())
                        .col(SnapColumn::new("name", "text").not_null())
                        .unique(SnapUnique::new("uq", ["name"])),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" ADD COLUMN \"name\" text NOT NULL;",
                "ALTER TABLE \"table\" ADD CONSTRAINT \"uq\" UNIQUE(\"name\");",
            ],
            status: Status::Supported,
        },
        // ---- alter composite primary key ----
        DiffCase {
            name: "alter composite primary key",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("table")
                        .col(SnapColumn::new("col1", "integer").not_null())
                        .col(SnapColumn::new("col2", "integer").not_null())
                        .col(SnapColumn::new("col3", "text").not_null())
                        .composite_pk(SnapCompositePk::new("table_pk", ["col1", "col2"])),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("table")
                        .col(SnapColumn::new("col1", "integer").not_null())
                        .col(SnapColumn::new("col2", "integer").not_null())
                        .col(SnapColumn::new("col3", "text").not_null())
                        .composite_pk(SnapCompositePk::new("table_pk", ["col2", "col3"])),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"table\" DROP CONSTRAINT \"table_pk\";\n--> statement-breakpoint\nALTER TABLE \"table\" ADD CONSTRAINT \"table_pk\" PRIMARY KEY(\"col2\",\"col3\");",
            ],
            status: Status::Skip(
                "drizzle joins DROP+ADD PK into one breakpoint-delimited string; differ emits separate statements",
            ),
        },
        // ---- add index with op ----
        DiffCase {
            name: "add index with op",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id", "serial").not_null().primary_key())
                        .col(SnapColumn::new("name", "text").not_null()),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id", "serial").not_null().primary_key())
                        .col(SnapColumn::new("name", "text").not_null())
                        .index(
                            SnapIndex::new(
                                "users_name_index",
                                [SnapIndexColumn::column("name").opclass("gin_trgm_ops")],
                            )
                            .method("gin"),
                        ),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "CREATE INDEX \"users_name_index\" ON \"users\" USING gin (\"name\" gin_trgm_ops);",
            ],
            status: Status::Supported,
        },
        // ---- optional db aliases (snake case) ----
        DiffCase {
            name: "optional db aliases (snake case)",
            from: empty(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("t1")
                        .col(SnapColumn::new("t1_id1", "integer").not_null().primary_key())
                        .col(SnapColumn::new("t1_col2", "integer").not_null())
                        .col(SnapColumn::new("t1_col3", "integer").not_null())
                        .col(SnapColumn::new("t2_ref", "integer").not_null())
                        .col(SnapColumn::new("t1_uni", "integer").not_null())
                        .col(SnapColumn::new("t1_uni_idx", "integer").not_null())
                        .col(SnapColumn::new("t1_idx", "integer").not_null())
                        .unique(SnapUnique::new("t1_uni", ["t1_uni"]))
                        .foreign_key(SnapForeignKey::new(
                            "t1_t2_ref_t2_t2_id_fk",
                            ["t2_ref"],
                            "public.t2",
                            ["t2_id"],
                        ))
                        .foreign_key(SnapForeignKey::new(
                            "t1_t1_col2_t1_col3_t3_t3_id1_t3_id2_fk",
                            ["t1_col2", "t1_col3"],
                            "public.t3",
                            ["t3_id1", "t3_id2"],
                        ))
                        .index(
                            SnapIndex::new(
                                "t1_uni_idx",
                                [SnapIndexColumn::column("t1_uni_idx")],
                            )
                            .unique(),
                        )
                        .index(
                            SnapIndex::new("t1_idx", [SnapIndexColumn::column("t1_idx")])
                                .where_clause("\"t1\".\"t1_idx\" > 0"),
                        ),
                )
                .table(
                    SnapTable::new("t2")
                        .col(SnapColumn::new("t2_id", "serial").not_null().primary_key()),
                )
                .table(
                    SnapTable::new("t3")
                        .col(SnapColumn::new("t3_id1", "integer"))
                        .col(SnapColumn::new("t3_id2", "integer"))
                        .composite_pk(SnapCompositePk::new(
                            "t3_t3_id1_t3_id2_pk",
                            ["t3_id1", "t3_id2"],
                        )),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "CREATE TABLE \"t1\" (\n\t\"t1_id1\" integer PRIMARY KEY NOT NULL,\n\t\"t1_col2\" integer NOT NULL,\n\t\"t1_col3\" integer NOT NULL,\n\t\"t2_ref\" integer NOT NULL,\n\t\"t1_uni\" integer NOT NULL,\n\t\"t1_uni_idx\" integer NOT NULL,\n\t\"t1_idx\" integer NOT NULL,\n\tCONSTRAINT \"t1_uni\" UNIQUE(\"t1_uni\")\n);\n",
                "CREATE TABLE \"t2\" (\n\t\"t2_id\" serial PRIMARY KEY NOT NULL\n);\n",
                "CREATE TABLE \"t3\" (\n\t\"t3_id1\" integer,\n\t\"t3_id2\" integer,\n\tCONSTRAINT \"t3_t3_id1_t3_id2_pk\" PRIMARY KEY(\"t3_id1\",\"t3_id2\")\n);\n",
                "ALTER TABLE \"t1\" ADD CONSTRAINT \"t1_t2_ref_t2_t2_id_fk\" FOREIGN KEY (\"t2_ref\") REFERENCES \"public\".\"t2\"(\"t2_id\") ON DELETE no action ON UPDATE no action;",
                "ALTER TABLE \"t1\" ADD CONSTRAINT \"t1_t1_col2_t1_col3_t3_t3_id1_t3_id2_fk\" FOREIGN KEY (\"t1_col2\",\"t1_col3\") REFERENCES \"public\".\"t3\"(\"t3_id1\",\"t3_id2\") ON DELETE no action ON UPDATE no action;",
                "CREATE UNIQUE INDEX \"t1_uni_idx\" ON \"t1\" USING btree (\"t1_uni_idx\");",
                "CREATE INDEX \"t1_idx\" ON \"t1\" USING btree (\"t1_idx\") WHERE \"t1\".\"t1_idx\" > 0;",
            ],
            status: Status::Skip(
                "multi-table create: FKs/indexes emitted after all tables; FK ordering is declaration-order vs BTreeMap-sorted — defer to differ promotion",
            ),
        },
        // ---- optional db aliases (camel case) ----
        DiffCase {
            name: "optional db aliases (camel case)",
            from: empty(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("t1")
                        .col(SnapColumn::new("t1Id1", "integer").not_null().primary_key())
                        .col(SnapColumn::new("t1Col2", "integer").not_null())
                        .col(SnapColumn::new("t1Col3", "integer").not_null())
                        .col(SnapColumn::new("t2Ref", "integer").not_null())
                        .col(SnapColumn::new("t1Uni", "integer").not_null())
                        .col(SnapColumn::new("t1UniIdx", "integer").not_null())
                        .col(SnapColumn::new("t1Idx", "integer").not_null())
                        .unique(SnapUnique::new("t1Uni", ["t1Uni"]))
                        .foreign_key(SnapForeignKey::new(
                            "t1_t2Ref_t2_t2Id_fk",
                            ["t2Ref"],
                            "public.t2",
                            ["t2Id"],
                        ))
                        .foreign_key(SnapForeignKey::new(
                            "t1_t1Col2_t1Col3_t3_t3Id1_t3Id2_fk",
                            ["t1Col2", "t1Col3"],
                            "public.t3",
                            ["t3Id1", "t3Id2"],
                        ))
                        .index(
                            SnapIndex::new("t1UniIdx", [SnapIndexColumn::column("t1UniIdx")])
                                .unique(),
                        )
                        .index(
                            SnapIndex::new("t1Idx", [SnapIndexColumn::column("t1Idx")])
                                .where_clause("\"t1\".\"t1Idx\" > 0"),
                        ),
                )
                .table(
                    SnapTable::new("t2")
                        .col(SnapColumn::new("t2Id", "serial").not_null().primary_key()),
                )
                .table(
                    SnapTable::new("t3")
                        .col(SnapColumn::new("t3Id1", "integer"))
                        .col(SnapColumn::new("t3Id2", "integer"))
                        .composite_pk(SnapCompositePk::new(
                            "t3_t3Id1_t3Id2_pk",
                            ["t3Id1", "t3Id2"],
                        )),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "CREATE TABLE \"t1\" (\n\t\"t1Id1\" integer PRIMARY KEY NOT NULL,\n\t\"t1Col2\" integer NOT NULL,\n\t\"t1Col3\" integer NOT NULL,\n\t\"t2Ref\" integer NOT NULL,\n\t\"t1Uni\" integer NOT NULL,\n\t\"t1UniIdx\" integer NOT NULL,\n\t\"t1Idx\" integer NOT NULL,\n\tCONSTRAINT \"t1Uni\" UNIQUE(\"t1Uni\")\n);\n",
                "CREATE TABLE \"t2\" (\n\t\"t2Id\" serial PRIMARY KEY NOT NULL\n);\n",
                "CREATE TABLE \"t3\" (\n\t\"t3Id1\" integer,\n\t\"t3Id2\" integer,\n\tCONSTRAINT \"t3_t3Id1_t3Id2_pk\" PRIMARY KEY(\"t3Id1\",\"t3Id2\")\n);\n",
                "ALTER TABLE \"t1\" ADD CONSTRAINT \"t1_t2Ref_t2_t2Id_fk\" FOREIGN KEY (\"t2Ref\") REFERENCES \"public\".\"t2\"(\"t2Id\") ON DELETE no action ON UPDATE no action;",
                "ALTER TABLE \"t1\" ADD CONSTRAINT \"t1_t1Col2_t1Col3_t3_t3Id1_t3Id2_fk\" FOREIGN KEY (\"t1Col2\",\"t1Col3\") REFERENCES \"public\".\"t3\"(\"t3Id1\",\"t3Id2\") ON DELETE no action ON UPDATE no action;",
                "CREATE UNIQUE INDEX \"t1UniIdx\" ON \"t1\" USING btree (\"t1UniIdx\");",
                "CREATE INDEX \"t1Idx\" ON \"t1\" USING btree (\"t1Idx\") WHERE \"t1\".\"t1Idx\" > 0;",
            ],
            status: Status::Skip(
                "multi-table create: FKs/indexes emitted after all tables; FK ordering is declaration-order vs BTreeMap-sorted — defer to differ promotion",
            ),
        },
        // ---- alter foreign key: add ON DELETE cascade ----
        // Postgres has no in-place FK alter, so a changed referential action is a
        // drop+recreate, emitted as drizzle's single `alter_reference` statement.
        DiffCase {
            name: "alter foreign key add on delete cascade",
            from: fk_pair(SnapForeignKey::new(
                "posts_owner_id_users_id_fk",
                ["owner_id"],
                "public.users",
                ["id"],
            )),
            to: fk_pair(
                SnapForeignKey::new(
                    "posts_owner_id_users_id_fk",
                    ["owner_id"],
                    "public.users",
                    ["id"],
                )
                .on_delete("cascade"),
            ),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"posts\" DROP CONSTRAINT \"posts_owner_id_users_id_fk\";\nALTER TABLE \"posts\" ADD CONSTRAINT \"posts_owner_id_users_id_fk\" FOREIGN KEY (\"owner_id\") REFERENCES \"public\".\"users\"(\"id\") ON DELETE cascade ON UPDATE no action;",
            ],
            status: Status::Supported,
        },
        // ---- alter foreign key: add ON UPDATE cascade ----
        DiffCase {
            name: "alter foreign key add on update cascade",
            from: fk_pair(SnapForeignKey::new(
                "posts_owner_id_users_id_fk",
                ["owner_id"],
                "public.users",
                ["id"],
            )),
            to: fk_pair(
                SnapForeignKey::new(
                    "posts_owner_id_users_id_fk",
                    ["owner_id"],
                    "public.users",
                    ["id"],
                )
                .on_update("cascade"),
            ),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"posts\" DROP CONSTRAINT \"posts_owner_id_users_id_fk\";\nALTER TABLE \"posts\" ADD CONSTRAINT \"posts_owner_id_users_id_fk\" FOREIGN KEY (\"owner_id\") REFERENCES \"public\".\"users\"(\"id\") ON DELETE no action ON UPDATE cascade;",
            ],
            status: Status::Supported,
        },
        // ---- alter foreign key: change ON DELETE action (cascade -> set null) ----
        DiffCase {
            name: "alter foreign key change on delete action",
            from: fk_pair(
                SnapForeignKey::new(
                    "posts_owner_id_users_id_fk",
                    ["owner_id"],
                    "public.users",
                    ["id"],
                )
                .on_delete("cascade"),
            ),
            to: fk_pair(
                SnapForeignKey::new(
                    "posts_owner_id_users_id_fk",
                    ["owner_id"],
                    "public.users",
                    ["id"],
                )
                .on_delete("set null"),
            ),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"posts\" DROP CONSTRAINT \"posts_owner_id_users_id_fk\";\nALTER TABLE \"posts\" ADD CONSTRAINT \"posts_owner_id_users_id_fk\" FOREIGN KEY (\"owner_id\") REFERENCES \"public\".\"users\"(\"id\") ON DELETE set null ON UPDATE no action;",
            ],
            status: Status::Supported,
        },
        // ---- alter foreign key: remove ON DELETE action (cascade -> default) ----
        DiffCase {
            name: "alter foreign key remove on delete action",
            from: fk_pair(
                SnapForeignKey::new(
                    "posts_owner_id_users_id_fk",
                    ["owner_id"],
                    "public.users",
                    ["id"],
                )
                .on_delete("cascade"),
            ),
            to: fk_pair(SnapForeignKey::new(
                "posts_owner_id_users_id_fk",
                ["owner_id"],
                "public.users",
                ["id"],
            )),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"posts\" DROP CONSTRAINT \"posts_owner_id_users_id_fk\";\nALTER TABLE \"posts\" ADD CONSTRAINT \"posts_owner_id_users_id_fk\" FOREIGN KEY (\"owner_id\") REFERENCES \"public\".\"users\"(\"id\") ON DELETE no action ON UPDATE no action;",
            ],
            status: Status::Supported,
        },
        // ---- alter foreign key: add both ON DELETE and ON UPDATE ----
        DiffCase {
            name: "alter foreign key add on delete and on update",
            from: fk_pair(SnapForeignKey::new(
                "posts_owner_id_users_id_fk",
                ["owner_id"],
                "public.users",
                ["id"],
            )),
            to: fk_pair(
                SnapForeignKey::new(
                    "posts_owner_id_users_id_fk",
                    ["owner_id"],
                    "public.users",
                    ["id"],
                )
                .on_delete("cascade")
                .on_update("cascade"),
            ),
            renames: &[],
            expected_sql: &[
                "ALTER TABLE \"posts\" DROP CONSTRAINT \"posts_owner_id_users_id_fk\";\nALTER TABLE \"posts\" ADD CONSTRAINT \"posts_owner_id_users_id_fk\" FOREIGN KEY (\"owner_id\") REFERENCES \"public\".\"users\"(\"id\") ON DELETE cascade ON UPDATE cascade;",
            ],
            status: Status::Supported,
        },
        // ---- alter index: add WHERE predicate (drop + recreate) ----
        DiffCase {
            name: "alter index add where predicate",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id", "integer").not_null())
                        .index(SnapIndex::new(
                            "users_id_index",
                            [SnapIndexColumn::column("id")],
                        )),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id", "integer").not_null())
                        .index(
                            SnapIndex::new("users_id_index", [SnapIndexColumn::column("id")])
                                .where_clause("\"id\" > 0"),
                        ),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "DROP INDEX \"users_id_index\";",
                "CREATE INDEX \"users_id_index\" ON \"users\" USING btree (\"id\") WHERE \"id\" > 0;",
            ],
            status: Status::Supported,
        },
        // ---- alter index: change access method (btree -> hash) ----
        DiffCase {
            name: "alter index change method",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id", "integer").not_null())
                        .index(SnapIndex::new(
                            "users_id_index",
                            [SnapIndexColumn::column("id")],
                        )),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id", "integer").not_null())
                        .index(
                            SnapIndex::new("users_id_index", [SnapIndexColumn::column("id")])
                                .method("hash"),
                        ),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "DROP INDEX \"users_id_index\";",
                "CREATE INDEX \"users_id_index\" ON \"users\" USING hash (\"id\");",
            ],
            status: Status::Supported,
        },
        // ---- alter index: add a per-column opclass ----
        DiffCase {
            name: "alter index add opclass",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("name", "text").not_null())
                        .index(
                            SnapIndex::new("users_name_index", [SnapIndexColumn::column("name")])
                                .method("gin"),
                        ),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("name", "text").not_null())
                        .index(
                            SnapIndex::new(
                                "users_name_index",
                                [SnapIndexColumn::column("name").opclass("gin_trgm_ops")],
                            )
                            .method("gin"),
                        ),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "DROP INDEX \"users_name_index\";",
                "CREATE INDEX \"users_name_index\" ON \"users\" USING gin (\"name\" gin_trgm_ops);",
            ],
            status: Status::Supported,
        },
        // ---- alter index: change the indexed columns ----
        DiffCase {
            name: "alter index change columns",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id", "integer").not_null())
                        .col(SnapColumn::new("email", "text").not_null())
                        .index(SnapIndex::new("users_idx", [SnapIndexColumn::column("id")])),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id", "integer").not_null())
                        .col(SnapColumn::new("email", "text").not_null())
                        .index(SnapIndex::new("users_idx", [SnapIndexColumn::column("email")])),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "DROP INDEX \"users_idx\";",
                "CREATE INDEX \"users_idx\" ON \"users\" USING btree (\"email\");",
            ],
            status: Status::Supported,
        },
        // ---- alter index: non-unique -> unique ----
        DiffCase {
            name: "alter index set unique",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("email", "text").not_null())
                        .index(SnapIndex::new(
                            "users_email_index",
                            [SnapIndexColumn::column("email")],
                        )),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("email", "text").not_null())
                        .index(
                            SnapIndex::new(
                                "users_email_index",
                                [SnapIndexColumn::column("email")],
                            )
                            .unique(),
                        ),
                )
                .build(),
            renames: &[],
            expected_sql: &[
                "DROP INDEX \"users_email_index\";",
                "CREATE UNIQUE INDEX \"users_email_index\" ON \"users\" USING btree (\"email\");",
            ],
            status: Status::Supported,
        },
        // ---- drop index (no recreate) — keeps the plain-drop path working ----
        DiffCase {
            name: "drop index",
            from: SchemaSnapshot::builder()
                .table(
                    SnapTable::new("users")
                        .col(SnapColumn::new("id", "integer").not_null())
                        .index(SnapIndex::new(
                            "users_id_index",
                            [SnapIndexColumn::column("id")],
                        )),
                )
                .build(),
            to: SchemaSnapshot::builder()
                .table(SnapTable::new("users").col(SnapColumn::new("id", "integer").not_null()))
                .build(),
            renames: &[],
            expected_sql: &["DROP INDEX \"users_id_index\";"],
            status: Status::Supported,
        },
    ]
}

/// A `users(id)` parent plus a `posts(id, owner_id)` child carrying the given FK.
/// Both tables are otherwise identical across a from/to pair, so diffing two
/// `fk_pair`s with different FK definitions isolates the foreign-key change (no
/// table/column DDL is emitted).
fn fk_pair(fk: SnapForeignKey) -> SchemaSnapshot {
    SchemaSnapshot::builder()
        .table(SnapTable::new("users").col(SnapColumn::new("id", "uuid").primary_key()))
        .table(
            SnapTable::new("posts")
                .col(SnapColumn::new("id", "uuid").primary_key())
                .col(SnapColumn::new("owner_id", "uuid"))
                .foreign_key(fk),
        )
        .build()
}
