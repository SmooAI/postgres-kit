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
- Unit test suite covering DDL rendering, injection rejection, and the executor seam.
