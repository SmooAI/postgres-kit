# Changelog

All notable changes to `smooai-postgres-kit` are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/); the project adheres to
[Semantic Versioning](https://semver.org/) (pre-1.0: minor = breaking).

## [Unreleased]

### Added

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
  Kit's permissively-licensed Postgres fixtures): 247 cases — 125 asserted
  (snapshot-in → expected-DDL-out under normalized comparison), 122 tracked as
  `Skip` for features outside the snapshot IR (see ROADMAP follow-ups).
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

- Cross-category differ promotion: enum value add/remove/reorder when dependent
  table **columns** change type (the enum↔column data-type-change cases).
- View / materialized-view `WITH` options, `TABLESPACE`, `USING` access method,
  `SET SCHEMA`, and the drizzle `.existing()` flag (not modeled in the IR).
- Policies linked to tables absent from the snapshot (drizzle's
  `create_ind_policy` / `alter_ind_policy` on non-schema tables).
- Custom identity sequence names (`SnapIdentity` has no name field).
- FK/index emission ordering for multi-table creates (declaration order vs.
  `BTreeMap`-sorted) and composite-PK DROP+ADD joined into one drizzle breakpoint.
