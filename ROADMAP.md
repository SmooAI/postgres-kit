# Roadmap

`smooai-postgres-kit` makes a Rust `PgTableSpec` the single source of truth for
the Postgres layer — taking over migrations from Drizzle Kit and deriving rows,
the typed sqlx layer, and TS/Zod types from one declaration. Tracked internally
as SMOODEV-2119 (ADR-048); the Postgres counterpart to `smooai-clickhouse-kit`.

The generic engine is vendor-neutral and gated behind cargo features so the
public crate carries no SmooAI specifics.

## Now (v0.0.x) — the safe foundation

- ✅ `PgType` closed type system + `to_sql_type`
- ✅ Identifier safety (grammar + 63-byte limit + quoting) and schema bounds
- ✅ `PgTableSpec` / `ColumnSpec` DSL + `to_create_table_sql` (PK, NOT NULL, defaults)
- ✅ `PgExecutor` bring-your-own-client seam (driver-agnostic)
- ⏳ Foreign keys, unique / partial / expression indexes, check constraints, generated columns

## Next — the diff engine (the hard 90%)

A snapshot IR + a differ (`snapshot → diff → DDL`): column add/drop/type-change,
**rename detection** (vs drop+add), FK add/drop/cascade change, index changes,
enum value add/reorder, default/nullable changes → ordered `ALTER` statements.
Conformance corpus ported from Drizzle Kit's permissively-licensed Postgres
fixtures (snapshot-in → expected-DDL-out). _Cargo feature: default._

## Then — migrations, drift, RLS

- **Migrations** — forward-only runner + bookkeeping table; emits Drizzle
  `_journal.json`-compatible files during transition. Live-DB introspection
  (`pg_catalog` / `information_schema`) → snapshot.
- **Drift** — `check_drift` compares the spec set vs the live DB; `is_clean()`
  gates CI.
- **RLS policy diffing** (`feature = "rls"`) — declare and diff row-level-security
  policies. Net-new; Postgres-generic.

## Then — the typed layers

- **`feature = "sqlx"`** — serde + `sqlx::FromRow` rows + a `COLUMNS` const
  (enum `::text`, arrays, nullability, keyword handling) and a light,
  safe-by-construction typed query layer. Not a query builder.
- **`feature = "tenant"`** — a generic tenant-scoped table trait whose
  `list_by_tenant` / `find_by_id` / `delete_by_id` bind the tenant filter
  themselves, making the scope invariant *structural* (anti-IDOR).
- **`feature = "codegen"`** — `PgTableSpec` → `*_row.rs`, plus TS types + Zod
  (a `createSelectSchema` replacement) for polyglot consumers.

## Non-goals (for now)

- A general ORM / relations / identity map — joins and aggregates stay raw `sqlx`.
- Dialects other than Postgres.
- Down-migrations / auto-rollback — forward-only by design.
