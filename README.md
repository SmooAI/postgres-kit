# smooai-postgres-kit

[![crates.io](https://img.shields.io/crates/v/smooai-postgres-kit.svg)](https://crates.io/crates/smooai-postgres-kit)
[![docs.rs](https://img.shields.io/docsrs/smooai-postgres-kit)](https://docs.rs/smooai-postgres-kit)
[![CI](https://github.com/SmooAI/postgres-kit/actions/workflows/rust.yml/badge.svg)](https://github.com/SmooAI/postgres-kit/actions/workflows/rust.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

**A Rust-native, declarative Postgres schema toolkit. Your `PgTableSpec` is the single source of truth.**

Rust has excellent SQL access (`sqlx`) and migration *runners* (`sqlx migrate`, `refinery`, `sea-orm-migration`) — but nothing that does **declarative, diff-based migrations** the way [Drizzle Kit](https://orm.drizzle.team/) or [Atlas](https://atlasgo.io/) do, in Rust, as a library. `smooai-postgres-kit` fills that gap: you declare a table once, in Rust, and the kit derives the rest.

```toml
[dependencies]
smooai-postgres-kit = "0.1"
```

> The crate is `smooai-postgres-kit`; it imports as **`postgres_kit`** — `use postgres_kit::...`.

## One declaration → everything downstream

```rust
use postgres_kit::{to_create_table_sql, ColumnSpec, PgTableSpec, PgType, SchemaLimits};

let managed_websites = PgTableSpec::new(
    "managed_websites",
    vec![
        ColumnSpec::new("id", PgType::Uuid).default_expr("gen_random_uuid()"),
        ColumnSpec::new("organization_id", PgType::Uuid),
        ColumnSpec::new("domain", PgType::Text),
        ColumnSpec::new("status", PgType::Enum("managed_website_status".into()))
            .default_expr("'development'"),
        ColumnSpec::new("tags", PgType::Array(Box::new(PgType::Text)))
            .default_expr("'{}'::text[]"),
        ColumnSpec::new("last_deployed_at", PgType::Timestamptz).nullable(),
    ],
)
.primary_key(["id"]);

let ddl = to_create_table_sql(&managed_websites, &SchemaLimits::default()).unwrap();
// CREATE TABLE IF NOT EXISTS "managed_websites" (
//     "id" uuid NOT NULL DEFAULT gen_random_uuid(),
//     "organization_id" uuid NOT NULL,
//     ...
//     PRIMARY KEY ("id")
// );
```

That same `PgTableSpec` is the input to the diff-based migrator, the drift checker, the serde/sqlx row codegen, the tenant-scoped sqlx layer, and the TS/Zod emitter (see the roadmap).

## Safe by construction

Identifiers are validated against Postgres' unquoted-identifier grammar and 63-byte limit, then double-quoted — a table or column name carrying SQL can't escape its DDL position. Type and default fragments that must be verbatim are documented as trusted, developer-authored input.

## Bring your own client

The kit never owns a connection or decodes rows. You implement one small trait over your client (e.g. `sqlx`):

```rust
use postgres_kit::{LiveColumn, PgError, PgExecutor};

impl PgExecutor for MyPool {
    async fn command(&self, sql: &str) -> Result<(), PgError> { /* ... */ }
    async fn fetch_strings(&self, sql: &str) -> Result<Vec<String>, PgError> { /* ... */ }
    async fn fetch_columns(&self, table: &str) -> Result<Vec<LiveColumn>, PgError> { /* ... */ }
}
```

so the shipped crate stays driver-agnostic and tiny (serde + thiserror).

## Status

Pre-1.0 but feature-complete on the core. Shipped and tested: the `PgType` type system, identifier safety, the `PgTableSpec` DSL, `CREATE TABLE` / index / enum / policy DDL, the BYO executor seam, the **diff engine** (`differ`, default — with a 247-case Drizzle-Kit conformance corpus, 125 asserted), **forward-only migrations** (`migrate`), the **drift gate** (`drift`), the **tenant-scoped query layer** (`tenant`, anti-IDOR), and **serde/sqlx + TS/Zod codegen** (`codegen`). A `#[ignore]`d testcontainers integration test exercises the engine against a real Postgres (CREATE + migrate + introspect round-trip + a generated RLS policy blocking a cross-tenant read). Deferred differ cases and follow-ups are tracked in [`ROADMAP.md`](./ROADMAP.md). API will still move before 1.0.

## License

MIT © Smoo AI
