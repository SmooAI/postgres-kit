//! `smooai-postgres-kit` — a Rust-native, declarative Postgres schema toolkit.
//!
//! A [`PgTableSpec`] is the single source of truth. From one declaration you
//! derive `CREATE TABLE` DDL (today, [`to_create_table_sql`]) and — as the kit
//! grows — diff-based forward-only migrations, drift detection, serde/sqlx row
//! codegen, a tenant-scoped typed sqlx layer, and TS/Zod types. See
//! `ROADMAP.md` for the phase plan.
//!
//! The crate is published as `smooai-postgres-kit`; it imports as
//! `postgres_kit` — `use postgres_kit::...`.
//!
//! ## Safe by construction
//!
//! Identifiers are validated against Postgres' unquoted-identifier grammar and
//! length limit, then double-quoted, so a table/column name carrying SQL can't
//! escape its DDL position. Type/raw fragments that are necessarily verbatim
//! (defaults, generated expressions) are documented as trusted, developer-authored
//! input — never build them from untrusted data.

mod client;
mod ddl;
mod safety;
mod spec;

pub use client::{LiveColumn, PgError, PgExecutor};
pub use ddl::to_create_table_sql;
pub use safety::{quote_identifier, validate_identifier, SchemaError, SchemaLimits};
pub use spec::{ColumnSpec, PgTableSpec, PgType};
