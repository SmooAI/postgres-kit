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

/// The diff engine: snapshot IR, DSL lowering, DDL statements, and rename hints.
#[cfg(feature = "differ")]
pub mod differ;

/// serde/sqlx + TS/Zod row codegen (scaffolding).
#[cfg(feature = "codegen")]
pub mod codegen;

/// Tenant-scoped typed query layer over `sqlx`.
#[cfg(feature = "tenant")]
pub mod tenant;

/// Forward-only `*.sql` migration runner (scaffolding).
#[cfg(feature = "migrate")]
pub mod migrate;

/// Read-only drift gate against a live schema (scaffolding).
#[cfg(feature = "drift")]
pub mod drift;

pub use client::{LiveColumn, PgError, PgExecutor};
#[cfg(feature = "codegen")]
pub use codegen::{
    emit_insert_schema, emit_rust_module, emit_select_schema, emit_ts_module, insert_schema_name,
    row_type_name, select_schema_name, CodegenError, CodegenOptions,
};
pub use ddl::{create_index_sql, create_policy_sql, create_type_sql, to_create_table_sql};
#[cfg(feature = "drift")]
pub use drift::{canonical_pg_type, check_drift, Drift, DriftResult};
#[cfg(feature = "migrate")]
pub use migrate::{
    read_journal, run_migrations, split_sql_statements, MigrationJournal, MigrationJournalEntry,
    MigrationRunResult,
};
#[cfg(all(feature = "migrate", feature = "differ"))]
pub use migrate::{write_migration, WrittenMigration};
pub use safety::{
    qualify_relation, quote_identifier, validate_identifier, SchemaError, SchemaLimits,
};
pub use spec::{
    CheckConstraintSpec, ColumnSpec, ColumnUnique, EnumTypeSpec, ForeignKeySpec, GeneratedColumn,
    IdentityKind, IdentitySpec, IndexColumn, IndexSpec, PgTableSpec, PgType, PolicyAs, PolicyFor,
    PolicySpec, ReferentialAction, RoleSpec, SequenceOptions, SequenceSpec, UniqueConstraintSpec,
    ViewSpec,
};
#[cfg(feature = "tenant")]
pub use tenant::{TenantError, TenantScopedTable};
