# Roadmap

`smooai-postgres-kit` makes a Rust `PgTableSpec` the single source of truth for
the Postgres layer — owning declarative, diff-based schema-as-code migrations and
deriving rows, the typed sqlx layer, and TS/Zod types from one declaration. Tracked internally
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
- ✅ Conformance corpus of Postgres schema-diff scenarios
  (snapshot-in → expected-DDL-out): **258 cases, 249 asserted, 9
  tracked `Skip`** (remaining clusters below).
- ✅ **Phase-2 deferred-corpus promotion**: views (`WITH` options / `TABLESPACE`
  / `USING` / `SET SCHEMA` / `.existing()` reference / DROP-before-CREATE
  recreate), enum recreate cascade, sequences, identity (custom sequence name +
  `START WITH` → `MINVALUE` fallback), FK alter (DROP+ADD), index drop
  (`public`-implicit), and independent (schema-level) policies (`SnapIndPolicy`).
- ✅ **Phase-3 deferred-corpus promotion**: the **`tables`** and **`columns`**
  categories now assert rendered SQL end-to-end (26 cases `Skip` → `Supported`) —
  `CREATE`/`DROP TABLE`, `ALTER TABLE … RENAME` / `SET SCHEMA`, multi-table
  creates, composite-PK add/rename (breakpoint-delimited DROP+ADD), `ADD COLUMN`,
  `RENAME COLUMN`, and column-level composite-PK changes.
- ✅ **Non-`public` schema support**: `CREATE SCHEMA` / `DROP SCHEMA` generation
  (`SchemaSnapshot.schemas`, ordered first/last), the schema-qualify fix
  (`to_create_table_sql` + all emitters share `qualify_relation`: `public`
  implicit, non-`public` `"schema"."name"`), and a `DdlStatement::RawSql` escape
  hatch (`SECURITY DEFINER` functions / triggers / grants, ordered before
  policies) with `differ::assemble_create_migration`. See the runnable
  `examples/rpm_pizza_schema.rs`.

## Done — migrations, drift, RLS

- ✅ **Migrations** (`feature = "migrate"`) — forward-only `run_migrations` +
  `__pg_migrations` bookkeeping table (idempotent);
  `--> statement-breakpoint` splitting and `meta/_journal.json` read/write
  for the transition.
- ✅ **Drift** (`feature = "drift"`) — `check_drift` compares the spec set vs the
  live DB (missing/extra column, type & nullability mismatch, best-effort missing
  index / FK / policy); `is_clean()` gates CI. `canonical_pg_type` normalizes
  synonyms.
- ✅ **RLS policies** — declared in the spec, emitted by `create_policy_sql`, and
  diffed by the differ; the integration test proves a generated policy blocks a
  cross-tenant read. (`feature = "rls"` reserved for future policy-only gating.)
- ✅ **Introspection / cutover spec-generation** (`feature = "introspect"`) —
  `introspect_schema(exec, schema)` builds the `PgTableSpec`/`EnumTypeSpec` source
  of truth from a live DB (columns incl. `tsvector`/`vector`, defaults, generated
  columns, PKs, FKs, unique/check constraints, indexes incl. partial predicates,
  RLS policies + enabled flag, enum types) using the **actual** live names — via a
  new `PgExecutor::fetch_rows` (multi-column) seam. The cutover guarantee: the
  introspected specs are drift-clean against the same DB (`check_drift` /
  `check_enum_drift`), so the kit can take over schema source-of-truth as a no-op.
  An `#[ignore]` testcontainers test proves the introspect↔drift round-trip; PG15+.

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

## Deferred — the 9 remaining differ corpus `Skip`s

Three clusters in `tests/differ_corpus.rs` (3 cases each):

- **Schema-level renames** (`ALTER SCHEMA … RENAME TO`) — schemas are not modeled
  as renameable IR entities, so a schema rename degrades to a data-losing
  drop+create; not blessed as supported output. (`change schema with tables #1`,
  `change table schema #6`, `enums #5`.)
- **Enum quoting in column ops** — `ADD COLUMN` emits a bare enum type name
  (`my_enum` / `my_enum[]`) instead of `"my_enum"`, and `SET DEFAULT` emits a bare
  enum literal (`value3`) instead of `'value3'`; needs the lowering to resolve
  user-defined types/defaults against the enum registry. (`enums #20`, `enums #21`,
  `column is enum type … add default`.)
- **Error-case fixtures** — duplicate check-constraint / view / materialized-view
  names that Postgres rejects; the differ does not yet model rejection, so there is
  no SQL contract to assert.

## Non-goals (for now)

- A general ORM / relations / identity map — joins and aggregates stay raw `sqlx`.
- Dialects other than Postgres.
- Down-migrations / auto-rollback — forward-only by design.
