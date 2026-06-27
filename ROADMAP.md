# Roadmap

`smooai-postgres-kit` makes a Rust `PgTableSpec` the single source of truth for
the Postgres layer — taking over migrations from Drizzle Kit and deriving rows,
the typed sqlx layer, and TS/Zod types from one declaration. Tracked internally
as SMOODEV-2119 (ADR-048); the Postgres counterpart to `smooai-clickhouse-kit`.

The generic engine is vendor-neutral and gated behind cargo features so the
public crate carries no SmooAI specifics.

## Done (v0.0.x) — the safe foundation

- ✅ `PgType` closed type system + `to_sql_type`
- ✅ Identifier safety (grammar + 63-byte limit + quoting) and schema bounds
- ✅ `PgTableSpec` / `ColumnSpec` DSL + `to_create_table_sql` (PK, NOT NULL, defaults)
- ✅ `PgExecutor` bring-your-own-client seam (driver-agnostic)
- ✅ Foreign keys, unique / partial / expression indexes, check constraints,
  generated & identity columns; standalone `create_type_sql` / `create_index_sql`
  / `create_policy_sql` emitters

## Done — the diff engine (the hard 90%)

- ✅ Snapshot IR + differ (`snapshot → diff → DDL`): table/column add/drop/
  type-change, **rename detection** (`RenameHints`, vs drop+add) for tables,
  columns, enums, policies, and roles; checks, generated & identity columns,
  enums, RLS policies, roles, sequences, and views. _Cargo feature: `differ`
  (default)._
- ✅ Conformance corpus ported from Drizzle Kit's permissively-licensed Postgres
  fixtures (snapshot-in → expected-DDL-out): **247 cases, 125 asserted, 122
  tracked `Skip`** (deferred categories below).
- ✅ **Non-`public` schema support**: `CREATE SCHEMA` / `DROP SCHEMA` generation
  (`SchemaSnapshot.schemas`, ordered first/last), the schema-qualify fix
  (`to_create_table_sql` + all emitters share `qualify_relation`: `public`
  implicit, non-`public` `"schema"."name"`), and a `DdlStatement::RawSql` escape
  hatch (`SECURITY DEFINER` functions / triggers / grants, ordered before
  policies) with `differ::assemble_create_migration`. See the runnable
  `examples/rpm_pizza_schema.rs`.

## Done — migrations, drift, RLS

- ✅ **Migrations** (`feature = "migrate"`) — forward-only `run_migrations` +
  `__pg_migrations` bookkeeping table (idempotent); drizzle
  `--> statement-breakpoint` splitting and `_journal.json`-compatible read/write
  for the transition.
- ✅ **Drift** (`feature = "drift"`) — `check_drift` compares the spec set vs the
  live DB (missing/extra column, type & nullability mismatch, best-effort missing
  index / FK / policy); `is_clean()` gates CI. `canonical_pg_type` normalizes
  synonyms.
- ✅ **RLS policies** — declared in the spec, emitted by `create_policy_sql`, and
  diffed by the differ; the integration test proves a generated policy blocks a
  cross-tenant read. (`feature = "rls"` reserved for future policy-only gating.)

## Done — the typed layers

- ✅ **`feature = "sqlx"`** — `sqlx` pulled in as an optional dep; backs the
  tenant query layer's `FromRow` rows + bound params.
- ✅ **`feature = "tenant"`** — `TenantScopedTable`: `list_by_tenant` /
  `find_by_id` / `delete_by_id` / `insert` / `update` bind the tenant filter
  themselves, making the scope invariant *structural* (anti-IDOR).
- ✅ **`feature = "codegen"`** — `PgTableSpec` → Rust serde/sqlx row module
  (`emit_rust_module`) + a `COLUMNS` const (enum `::text`, arrays, nullability),
  plus TS types + Zod (`emit_ts_module`, a `createSelectSchema` replacement) for
  polyglot consumers.

## Deferred — differ corpus `Skip`s (next promotion passes)

Tracked by the 122 `Skip` cases in `tests/differ_corpus.rs`:

- Cross-category enum↔column moves: enum value add/remove/reorder when dependent
  table **columns** change data type (the largest skip cluster).
- View / materialized-view `WITH` options, `TABLESPACE`, `USING` access method,
  `SET SCHEMA`, and the drizzle `.existing()` flag — not modeled in the IR. (The
  `CREATE SCHEMA` half of these skips is now done; what remains is the view
  `WITH`-options / `.existing()` modeling they are bundled with.)
- Policies linked to tables absent from the snapshot (drizzle
  `create_ind_policy` / `alter_ind_policy` on non-schema tables).
- Custom identity sequence names (`SnapIdentity` has no name field).
- Multi-table-create FK/index emission ordering (declaration order vs
  `BTreeMap`-sorted) and composite-PK DROP+ADD joined into one drizzle breakpoint.

## Non-goals (for now)

- A general ORM / relations / identity map — joins and aggregates stay raw `sqlx`.
- Dialects other than Postgres.
- Down-migrations / auto-rollback — forward-only by design.
