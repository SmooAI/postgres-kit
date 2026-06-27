# Changelog

All notable changes to `smooai-postgres-kit` are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/); the project adheres to
[Semantic Versioning](https://semver.org/) (pre-1.0: minor = breaking).

## [Unreleased]

### Added

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
- **Differ conformance corpus** (`tests/differ_corpus.rs`, ported from Drizzle
  Kit's permissively-licensed Postgres fixtures): 258 cases — 198 asserted
  (snapshot-in → expected-DDL-out under normalized comparison), 60 tracked as
  `Skip` for features outside the snapshot IR (see ROADMAP follow-ups).
- **Deferred-corpus promotion** (phase 2): the previously-`Skip`'d differ
  categories are now asserted end-to-end —
    - **Views**: `WITH (...)` storage options, materialized-view `TABLESPACE`,
      `USING` access method, `SET SCHEMA`, in-place option `SET`/`RESET`, the
      drizzle `.existing()` reference flag (`SnapView::reference`), and
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
      match drizzle's convertor.
    - **Independent (schema-level) policies**: policies linked to a table absent
      from the snapshot, always schema-qualified — new `SnapIndPolicy` IR +
      `SchemaSnapshotBuilder::ind_policy`, `DdlStatement::{Create,Drop,Alter,Rename}IndPolicy`,
      an `ind_policy:` rename-hint tag, and dedicated `Plan` buckets ordered
      alongside their table-policy counterparts.
- **Migrations** (`feature = "migrate"`): forward-only `run_migrations` over a
  `*.sql` directory with a `__pg_migrations` bookkeeping table (idempotent re-runs),
  `split_sql_statements` (drizzle `--> statement-breakpoint` aware), and
  drizzle-compatible journal I/O (`read_drizzle_journal`, `write_drizzle_migration`).
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

### Follow-ups (tracked as corpus `Skip`s, deferred to later phases)

- **Columns category** (the bulk of the remaining `Skip`s): column add / default
  add / data-type change cases — including every enum↔standard and enum↔enum
  data-type-change variant — are deferred to a dedicated columns-promotion pass.
- Multi-construct **ordering** mismatches vs. drizzle's insertion order:
  FK/index emission for multi-table creates (declaration order vs.
  `BTreeMap`-sorted), composite-PK DROP+ADD joined into one drizzle breakpoint,
  and multi-policy creation order (BTreeMap name-sorted vs. insertion order).
- **Tables/schema** "statements-only encoding" goldens (add/drop/move table,
  multiproject schema) whose drizzle fixtures assert statements only, not
  `sqlStatements`.
- Enum **schema rename** (`ALTER SCHEMA`) — schemas are not modeled as renameable
  IR entities.
- Error-case fixtures (duplicate view / constraint names that drizzle rejects).
