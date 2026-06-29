# Changelog

All notable changes to `smooai-postgres-kit` are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/); the project adheres to
[Semantic Versioning](https://semver.org/) (pre-1.0: minor = breaking).

## [Unreleased]

### Added

- **Codegen header / doc-name overrides** (`feature = "codegen"`) —
  `CodegenOptions::header(...)` fully replaces the leading `@generated` comment
  block, and `CodegenOptions::source_name(...)` overrides the table name shown in
  the row struct's doc comment (defaulting to the DB name). Both exist so a
  consuming generator can emit bytes identical to an existing hand-rolled
  generator's whose header/doc carry source-schema metadata a `PgTableSpec` alone
  can't know (e.g. a Drizzle import path + camelCase export name). This makes the
  kit a drop-in, no-op-diff replacement for such a generator. Proven by a new
  golden test that reproduces a real committed downstream row module byte-for-byte
  (SMOODEV-2150).

- **First-class database introspection** (`feature = "introspect"`) — new
  `introspect_schema(exec, schema) -> IntrospectedSchema { tables, enums }` builds
  a `PgTableSpec`/`EnumTypeSpec` source of truth directly from a live database via
  `pg_catalog`, using the **actual** live object names. It captures columns (types
  incl. `tsvector`/`vector`, nullability, defaults, `STORED` generated columns),
  primary keys, foreign keys (real names + `ON DELETE`/`ON UPDATE`), unique
  constraints (incl. `NULLS NOT DISTINCT`), check constraints, indexes (incl.
  partial-index predicates + access method), RLS policies, the RLS-enabled flag,
  and user-defined enum types. This is the **cutover spec-generation path**: feed
  the introspected specs straight into `check_drift` / `check_enum_drift` against
  the same database and the result is, by construction, drift-clean — proving the
  kit can take over schema source-of-truth as a no-op. Requires Postgres 15+
  (reads `attgenerated`/`indnullsnotdistinct`); an `#[ignore]` testcontainers test
  proves the introspect↔drift round-trip is clean. Promotes the proven queries
  from the one-off cutover validator into a reusable kit feature.
- **`PgExecutor::fetch_rows`** — new trait method returning each row as
  text-rendered cells (`Vec<Vec<Option<String>>>`, SQL `NULL` ⇒ `None`), the
  multi-column shape introspection needs (`fetch_strings` only yields one column
  per row). Introspection queries cast every column to `::text`, so a driver impl
  reads each cell as an optional string. _Breaking_: existing `PgExecutor` impls
  must add `fetch_rows` (pre-1.0).
- **`tsvector` / pgvector `vector` column types** — `PgType::Tsvector` (→ `tsvector`)
  and `PgType::Vector(Option<u32>)` (→ `vector` / `vector(n)`) are now first-class:
  `to_sql_type` renders them, codegen decodes them as text (Rust `String` via a
  `::text` cast, TS/Zod `string`), and `canonical_pg_type` normalizes them in the
  drift gate. Previously these only worked smuggled through `PgType::Enum`.
- **Enum drift** — new `check_enum_drift(exec, &[EnumTypeSpec])` reports
  `Drift::MissingEnumType` / `Drift::EnumValuesMismatch` (name + value *set*,
  order-independent) by introspecting `pg_enum`.

### Changed

- **Name-agnostic drift** (`feature = "drift"`) — `check_drift` now matches foreign
  keys, indexes, and policies by **definition instead of name**, eliminating
  cosmetic-rename false positives (legacy constraint/index/policy names from
  non-cascading table renames, 63-byte identifier truncation, and Postgres'
  default `_fkey` naming):
    - FK by `(from-columns, referenced table, to-columns, on-delete, on-update)`
      via `pg_constraint` (referenced table defaults to the `public` schema so an
      unqualified spec target lines up with the catalog's `nspname.relname`).
    - Index by `(columns/expressions, unique, predicate, access method)` via
      `pg_index` + `pg_get_indexdef` (predicates/expressions compared with
      whitespace + parentheses stripped so the spec form matches the canonical
      form Postgres echoes back).
    - Policy by `(command, roles, using, with-check)` via `pg_policies` (an empty
      `TO` list defaults to `public`; roles sorted).
  The `Missing{ForeignKey,Index,Policy}` variants are unchanged on the wire — the
  `name`/`constraint`/`policy` field now carries the spec's declared name for
  reporting only.
- **Extended drift coverage** — `check_drift` additionally compares (all
  name-agnostic, best-effort): primary keys (`Drift::PrimaryKeyMismatch`, by
  column *set*), unique constraints including column-level `UNIQUE`
  (`Drift::MissingUniqueConstraint`, by column set + `NULLS NOT DISTINCT`), and
  check constraints (`Drift::MissingCheckConstraint`, by normalized expression)
  via `pg_constraint`.

### Added (prior)

- **Non-`public` schema support** — build an app whose tables live in a dedicated
  Postgres schema (e.g. `rpm_pizza`) cleanly, no hand-workarounds:
    - `CREATE SCHEMA` generation: `SchemaSnapshot` now carries a `schemas` set
      (non-`public` only); `lower` / `lower_tables` collect every schema referenced
      by a table/enum/view/sequence; the differ emits
      `CREATE SCHEMA IF NOT EXISTS "name"` first (and `DROP SCHEMA IF EXISTS` last)
      via new `DdlStatement::CreateSchema` / `DropSchema` variants.
    - **Raw-SQL escape hatch**: `DdlStatement::RawSql(String)` injects verbatim SQL
      the kit deliberately doesn't model (`CREATE FUNCTION … SECURITY DEFINER`,
      triggers, grants), ordered after table/type/FK/index creation but before
      `ENABLE ROW LEVEL SECURITY` / policy creation so a helper function exists
      before any policy that references it. New `differ::diff_with_raw_sql` and the
      one-call `differ::assemble_create_migration(tables, enums, extra_raw)`.
    - Runnable dogfood example `examples/rpm_pizza_schema.rs` (the consumer how-to)
      + an ordering-contract integration test `tests/rpm_pizza.rs`.
- **Schema-qualify fix** (bug): `to_create_table_sql` ignored `table.schema`, so a
  `.in_schema("rpm_pizza")` table was created in the wrong schema. All emitters now
  share one convention via `qualify_relation` — `public` implicit (bare),
  non-`public` rendered `"schema"."name"`: `to_create_table_sql`,
  `create_type_sql`, `create_index_sql`, `create_policy_sql`, and the differ's
  statement renderer agree (foreign-key *targets* stay fully qualified, as before).
- Initial crate scaffold (lib `postgres_kit`).
- `PgType` closed Postgres type system with `to_sql_type` rendering (incl. enums,
  arrays, `varchar(n)`, `numeric(p,s)`).
- `PgTableSpec` / `ColumnSpec` declarative DSL with builder helpers.
- `to_create_table_sql` — `CREATE TABLE IF NOT EXISTS` with primary keys,
  `NOT NULL`, and defaults; validates identifiers, column uniqueness, column
  count, and primary-key references.
- Identifier safety (`validate_identifier`, `quote_identifier`) and `SchemaLimits`
  (Postgres-correct 63-byte identifier / 1600-column defaults).
- `PgExecutor` bring-your-own-client trait + `PgError` + `LiveColumn`.
- Standalone DDL emitters: `create_type_sql` (enums), `create_index_sql` (unique /
  partial / expression / opclass), `create_policy_sql` (RLS policies). Inline
  foreign keys, unique / check constraints, generated columns, and identity
  columns in `to_create_table_sql`.
- **Diff engine** (`feature = "differ"`, default): a `SchemaSnapshot` IR, DSL
  lowering (`lower` / `lower_table` / `lower_tables`), `diff()` →
  `Vec<DdlStatement>`, and `RenameHints` (table / column / enum / policy / role
  rename detection vs. drop+add). Covers tables, columns, checks, enums, generated
  & identity columns, RLS policies, roles, sequences, and views.
- **Differ conformance corpus** (`tests/differ_corpus.rs`): a conformance corpus
  of Postgres schema-diff scenarios — 258 cases, 249 asserted
  (snapshot-in → expected-DDL-out under normalized comparison), 9 tracked as
  `Skip` for behaviour outside the snapshot IR (see ROADMAP follow-ups).
- **Deferred-corpus promotion** (phase 2): the previously-`Skip`'d differ
  categories are now asserted end-to-end —
    - **Views**: `WITH (...)` storage options, materialized-view `TABLESPACE`,
      `USING` access method, `SET SCHEMA`, in-place option `SET`/`RESET`, the
      "existing" (unmanaged) view reference flag (`SnapView::reference`), and
      DROP-before-CREATE recreate ordering. New `DdlStatement` variants
      `AlterViewSetSchema` / `AlterViewSetOptions` / `AlterViewResetOptions` /
      `AlterViewSetTablespace` / `AlterViewSetAccessMethod`; new `SnapView` fields
      (`existing`, `with_options`, `tablespace`, `using`, `with_no_data`).
    - **Enum recreate cascade** + **sequences** (create / alter / rename /
      `SET SCHEMA` / drop) + **identity** (custom sequence name via a new
      `SnapIdentity.name`; ascending-sequence `START WITH` falls back to
      `MINVALUE`).
    - **FK alter**: a changed `ON DELETE`/`ON UPDATE` renders as a single
      DROP-then-ADD `AlterForeignKey` statement (the "add multiple constraints"
      cases).
    - **Index alter / drop**: `DROP INDEX` omits the implicit `public` schema to
      match the expected rendering.
    - **Independent (schema-level) policies**: policies linked to a table absent
      from the snapshot, always schema-qualified — new `SnapIndPolicy` IR +
      `SchemaSnapshotBuilder::ind_policy`, `DdlStatement::{Create,Drop,Alter,Rename}IndPolicy`,
      an `ind_policy:` rename-hint tag, and dedicated `Plan` buckets ordered
      alongside their table-policy counterparts.
- **Deferred-corpus promotion** (phase 3): the `tables` and `columns` categories
  are now asserted end-to-end against rendered SQL (26 cases flipped
  `Skip` → `Supported`) — `CREATE`/`DROP TABLE`, `ALTER TABLE … RENAME`,
  `ALTER TABLE … SET SCHEMA`, multi-table creates, composite-PK add/rename
  (breakpoint-delimited DROP+ADD), `ADD COLUMN`, `RENAME COLUMN`, and column-level
  composite-PK changes. The corpus is now 249 asserted / 9 `Skip`.
- **Migrations** (`feature = "migrate"`): forward-only `run_migrations` over a
  `*.sql` directory with a `__pg_migrations` bookkeeping table (idempotent re-runs),
  `split_sql_statements` (`--> statement-breakpoint` aware), and
  migration-journal I/O (`read_journal`, `write_migration`).
- **Drift gate** (`feature = "drift"`): `check_drift` compares specs vs. the live
  schema (missing table/column, extra column, type & nullability mismatch,
  best-effort missing index / FK / policy); `DriftResult::is_clean()` gates CI.
  `canonical_pg_type` normalizes spelling synonyms.
- **Tenant layer** (`feature = "tenant"`, pulls in `sqlx`): `TenantScopedTable` —
  a safe-by-construction, tenant-scoped typed query layer where the tenant filter
  is structurally unskippable (anti-IDOR). Pure SQL builders + `sqlx`-backed
  execution helpers (`list_by_tenant` / `find_by_id` / `delete_by_id` / `insert` /
  `update`).
- **Codegen** (`feature = "codegen"`): `PgTableSpec` → Rust serde/sqlx row module
  (`emit_rust_module`), TS/Zod module (`emit_ts_module`), and select/insert schema
  emitters, with a `COLUMNS` const (enum `::text` casts, arrays, nullability).
- Live-DB integration test (`tests/integration.rs`, `#[ignore]` + testcontainers):
  applies a generated `CREATE TABLE` + a migration, asserts migration idempotency,
  round-trips introspection through `check_drift`, and proves a generated RLS
  policy blocks a cross-tenant read.
- Unit test suite covering DDL rendering, injection rejection, the executor seam,
  the diff engine per category, migrations, drift, the tenant builders, and codegen.

### Changed

- **Migration journal API renamed** (breaking) to neutral names independent of any
  external toolkit. The journal writer is now `write_migration` (was
  `write_…_migration`), the journal reader `read_journal` (was `read_…_journal`),
  and the journal types `MigrationJournal` / `MigrationJournalEntry`. The on-disk
  layout is unchanged (`meta/_journal.json`, `--> statement-breakpoint` markers).

### Follow-ups (the remaining 9 corpus `Skip`s, in 3 clusters)

- **Schema-level renames** (`ALTER SCHEMA … RENAME TO`) — 3 cases (`change schema
  with tables #1`, `change table schema #6`, `enums #5`). Schemas are not modeled
  as renameable IR entities, so a schema rename degrades to a data-losing
  drop+create; we deliberately do not bless that as supported output.
- **Enum quoting in column ops** — 3 cases (`enums #20`, `enums #21`, `column is
  enum type … add default`). `ADD COLUMN` emits a bare enum type name
  (`my_enum` / `my_enum[]`) instead of the quoted `"my_enum"`, and `SET DEFAULT`
  emits a bare enum literal (`value3`) instead of `'value3'`. Needs the lowering
  to resolve user-defined types/defaults against the enum registry.
- **Error-case fixtures** — 3 cases (`create checks with same names`, plus the
  duplicate view / materialized-view name cases). Postgres rejects these; the
  differ does not yet model rejection, so there is no SQL contract to assert.
