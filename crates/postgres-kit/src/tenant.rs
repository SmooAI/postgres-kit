//! Tenant layer (feature `tenant`): a safe-by-construction, tenant-scoped typed
//! query layer over `sqlx`.
//!
//! This generalizes the api-prime `OrgScopedTable` spike. The point is NOT to be
//! an ORM — there is no query builder, no relations, no magic. It turns the
//! repeated, security-critical pattern
//!
//! ```text
//! SELECT {COLUMNS} FROM <t> WHERE id = $1 AND organization_id = $2
//! ```
//!
//! into trait code where the tenant filter is *structurally unskippable*. The
//! "always scope by tenant" rule (the class of cross-tenant IDOR leaks) is
//! enforced not by code review but by there being no hand-written `WHERE` to get
//! wrong: every statement the trait emits binds the tenant column itself.
//!
//! # Why `sqlx`, not the BYO [`PgExecutor`](crate::PgExecutor)
//!
//! The kit's default seam is string-only (`command` / `fetch_strings` /
//! `fetch_columns`) — it can neither decode an arbitrary typed `Row` nor bind
//! typed parameters. A typed, parameter-bound query layer genuinely needs a real
//! driver, so this module binds `sqlx::PgPool` behind the `sqlx` feature
//! (`tenant` enables `sqlx`). Everything else in the crate stays driver-agnostic.
//!
//! # Layering
//!
//! - **Pure SQL builders** ([`TenantScopedTable::select_by_tenant_sql`] and
//!   friends) construct validated, deterministic SQL with the tenant column
//!   baked in. They never touch a database and are the unit-tested core.
//! - **Execution helpers** (`list_by_tenant` / `find_by_id` / `delete_by_id` /
//!   `insert` / `update`) run those builders against a `&PgPool`. They are thin
//!   wrappers, exercised against a live database in integration tests.

use sqlx::postgres::{PgArguments, PgRow};
use sqlx::query::QueryAs;
use sqlx::{Encode, FromRow, PgPool, Postgres, Type};
use std::future::Future;
use thiserror::Error;

use crate::safety::{quote_identifier, validate_identifier, SchemaError, SchemaLimits};

/// Error surface for the tenant layer: a spec/identifier problem caught while
/// building SQL, or a database error from `sqlx`.
#[derive(Debug, Error)]
pub enum TenantError {
    /// An identifier or column set failed validation while building the SQL.
    #[error(transparent)]
    Schema(#[from] SchemaError),
    /// The underlying `sqlx` driver returned an error.
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

/// A tenant-scoped table: a generated `Row`, its `COLUMNS` select list, the table
/// name, and the tenant + primary-key column names. Every default method binds
/// the tenant column itself, so a by-id read, update, or delete can never escape
/// its tenant.
///
/// `COLUMNS` is a verbatim select fragment (it may carry `::text` casts, commas,
/// etc.) and is treated as trusted, developer-authored input — never build it
/// from untrusted data. `NAME`, `TENANT_COLUMN`, `ID_COLUMN`, and any column
/// passed to `insert`/`update` are validated and double-quoted before use.
pub trait TenantScopedTable {
    /// The decoded row type. Generated alongside `COLUMNS` so the select list and
    /// the struct stay in lockstep.
    type Row: for<'r> FromRow<'r, PgRow> + Send + Unpin;

    /// The physical table name.
    const NAME: &'static str;

    /// The verbatim `SELECT` list (trusted fragment — see the trait docs).
    const COLUMNS: &'static str;

    /// The tenant-scoping column. Defaults to the SmooAI convention
    /// `organization_id`; override for tables that scope by a different column.
    const TENANT_COLUMN: &'static str = "organization_id";

    /// The primary-key column used by the by-id helpers. Defaults to `id`.
    const ID_COLUMN: &'static str = "id";

    // ---- Pure SQL builders (no I/O; the unit-tested, security-critical core) ----

    /// `SELECT {COLUMNS} FROM {table} WHERE {tenant} = $1`.
    fn select_by_tenant_sql() -> Result<String, SchemaError> {
        let limits = SchemaLimits::default();
        let table = qualified_ident(Self::NAME, "table", &limits)?;
        let tenant = qualified_ident(Self::TENANT_COLUMN, "tenant column", &limits)?;
        Ok(format!(
            "SELECT {} FROM {} WHERE {} = $1",
            Self::COLUMNS,
            table,
            tenant
        ))
    }

    /// `SELECT {COLUMNS} FROM {table} WHERE {id} = $1 AND {tenant} = $2`.
    fn select_by_id_sql() -> Result<String, SchemaError> {
        let limits = SchemaLimits::default();
        let table = qualified_ident(Self::NAME, "table", &limits)?;
        let id = qualified_ident(Self::ID_COLUMN, "id column", &limits)?;
        let tenant = qualified_ident(Self::TENANT_COLUMN, "tenant column", &limits)?;
        Ok(format!(
            "SELECT {} FROM {} WHERE {} = $1 AND {} = $2",
            Self::COLUMNS,
            table,
            id,
            tenant
        ))
    }

    /// `DELETE FROM {table} WHERE {id} = $1 AND {tenant} = $2`.
    fn delete_by_id_sql() -> Result<String, SchemaError> {
        let limits = SchemaLimits::default();
        let table = qualified_ident(Self::NAME, "table", &limits)?;
        let id = qualified_ident(Self::ID_COLUMN, "id column", &limits)?;
        let tenant = qualified_ident(Self::TENANT_COLUMN, "tenant column", &limits)?;
        Ok(format!(
            "DELETE FROM {} WHERE {} = $1 AND {} = $2",
            table, id, tenant
        ))
    }

    /// `INSERT INTO {table} ({tenant}, {cols...}) VALUES ($1, $2, ...) RETURNING {COLUMNS}`.
    ///
    /// The tenant column is always the first inserted column (bound as `$1`), so
    /// an insert can never omit it. `columns` is the caller-supplied,
    /// non-tenant column list (each validated + quoted); the tenant column may
    /// not appear in it.
    fn insert_sql(columns: &[&str]) -> Result<String, SchemaError> {
        let limits = SchemaLimits::default();
        let table = qualified_ident(Self::NAME, "table", &limits)?;
        let tenant = qualified_ident(Self::TENANT_COLUMN, "tenant column", &limits)?;
        let quoted = validate_mutable_columns(columns, Self::NAME, Self::TENANT_COLUMN, None)?;

        // Tenant is column 1; the rest follow.
        let mut col_list = String::new();
        col_list.push_str(&tenant);
        for c in &quoted {
            col_list.push_str(", ");
            col_list.push_str(c);
        }

        let placeholders = (1..=quoted.len() + 1)
            .map(|i| format!("${i}"))
            .collect::<Vec<_>>()
            .join(", ");

        Ok(format!(
            "INSERT INTO {} ({}) VALUES ({}) RETURNING {}",
            table,
            col_list,
            placeholders,
            Self::COLUMNS
        ))
    }

    /// `UPDATE {table} SET {c1} = $1, ... WHERE {id} = $N AND {tenant} = $N+1 RETURNING {COLUMNS}`.
    ///
    /// The `WHERE` clause always carries the tenant filter, so an update can
    /// never touch another tenant's row. `columns` is the non-empty set of
    /// columns to assign (each validated + quoted); the tenant and id columns may
    /// not appear in it (the scope is fixed and the PK is immutable).
    fn update_sql(columns: &[&str]) -> Result<String, SchemaError> {
        let limits = SchemaLimits::default();
        let table = qualified_ident(Self::NAME, "table", &limits)?;
        let id = qualified_ident(Self::ID_COLUMN, "id column", &limits)?;
        let tenant = qualified_ident(Self::TENANT_COLUMN, "tenant column", &limits)?;
        let quoted = validate_mutable_columns(
            columns,
            Self::NAME,
            Self::TENANT_COLUMN,
            Some(Self::ID_COLUMN),
        )?;
        if quoted.is_empty() {
            return Err(SchemaError::EmptyColumnSet {
                table: Self::NAME.to_string(),
            });
        }

        let assignments = quoted
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{} = ${}", c, i + 1))
            .collect::<Vec<_>>()
            .join(", ");

        // id and tenant are bound after the SET values.
        let id_placeholder = quoted.len() + 1;
        let tenant_placeholder = quoted.len() + 2;

        Ok(format!(
            "UPDATE {} SET {} WHERE {} = ${} AND {} = ${} RETURNING {}",
            table,
            assignments,
            id,
            id_placeholder,
            tenant,
            tenant_placeholder,
            Self::COLUMNS
        ))
    }

    // ---------------------------- Execution helpers ----------------------------

    /// All rows for `tenant` (no `ORDER BY` — callers that need ordering use raw
    /// SQL). The tenant filter is bound by the builder, never by the caller.
    fn list_by_tenant<T>(
        pool: &PgPool,
        tenant: T,
    ) -> impl Future<Output = Result<Vec<Self::Row>, TenantError>> + Send
    where
        T: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send,
    {
        async move {
            let sql = Self::select_by_tenant_sql()?;
            let rows = sqlx::query_as::<_, Self::Row>(&sql)
                .bind(tenant)
                .fetch_all(pool)
                .await?;
            Ok(rows)
        }
    }

    /// One row by id, scoped to `tenant`. `None` = not found OR belongs to another
    /// tenant — callers map both to 404 (never 403), so existence never leaks
    /// across tenants.
    fn find_by_id<T, I>(
        pool: &PgPool,
        tenant: T,
        id: I,
    ) -> impl Future<Output = Result<Option<Self::Row>, TenantError>> + Send
    where
        T: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send,
        I: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send,
    {
        async move {
            let sql = Self::select_by_id_sql()?;
            let row = sqlx::query_as::<_, Self::Row>(&sql)
                .bind(id)
                .bind(tenant)
                .fetch_optional(pool)
                .await?;
            Ok(row)
        }
    }

    /// Hard-delete by id, scoped to `tenant`. Returns rows affected (0 = not
    /// found / wrong tenant).
    fn delete_by_id<T, I>(
        pool: &PgPool,
        tenant: T,
        id: I,
    ) -> impl Future<Output = Result<u64, TenantError>> + Send
    where
        T: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send,
        I: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send,
    {
        async move {
            let sql = Self::delete_by_id_sql()?;
            let res = sqlx::query(&sql)
                .bind(id)
                .bind(tenant)
                .execute(pool)
                .await?;
            Ok(res.rows_affected())
        }
    }

    /// Insert a row for `tenant`, returning the inserted row. The tenant value is
    /// bound first by the helper; `bind_values` then binds the non-tenant
    /// `columns` in order. The tenant column is therefore always set and always
    /// the caller-provided `tenant` — it cannot be spoofed via `bind_values`.
    ///
    /// `bind_values` binds owned (`'static`) values such as `String`, `Uuid`, or
    /// `i64`; borrowed values must outlive the query.
    fn insert<'c, T, B>(
        pool: &'c PgPool,
        tenant: T,
        columns: &'c [&'c str],
        bind_values: B,
    ) -> impl Future<Output = Result<Self::Row, TenantError>> + Send + 'c
    where
        T: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send + 'c,
        B: for<'q> FnOnce(
                QueryAs<'q, Postgres, Self::Row, PgArguments>,
            ) -> QueryAs<'q, Postgres, Self::Row, PgArguments>
            + Send
            + 'c,
    {
        async move {
            let sql = Self::insert_sql(columns)?;
            let query = sqlx::query_as::<_, Self::Row>(&sql).bind(tenant);
            let query = bind_values(query);
            let row = query.fetch_one(pool).await?;
            Ok(row)
        }
    }

    /// Update a row by id, scoped to `tenant`, returning the updated row (`None`
    /// = not found / wrong tenant). `bind_values` binds the `columns` SET values
    /// in order; the helper then binds id and tenant for the `WHERE` clause, so
    /// the tenant scope is never under caller control.
    fn update<'c, T, I, B>(
        pool: &'c PgPool,
        tenant: T,
        id: I,
        columns: &'c [&'c str],
        bind_values: B,
    ) -> impl Future<Output = Result<Option<Self::Row>, TenantError>> + Send + 'c
    where
        T: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send + 'c,
        I: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send + 'c,
        B: for<'q> FnOnce(
                QueryAs<'q, Postgres, Self::Row, PgArguments>,
            ) -> QueryAs<'q, Postgres, Self::Row, PgArguments>
            + Send
            + 'c,
    {
        async move {
            let sql = Self::update_sql(columns)?;
            let query = sqlx::query_as::<_, Self::Row>(&sql);
            let query = bind_values(query);
            let row = query.bind(id).bind(tenant).fetch_optional(pool).await?;
            Ok(row)
        }
    }
}

/// Validate an identifier and return its double-quoted form.
fn qualified_ident(
    name: &str,
    kind: &'static str,
    limits: &SchemaLimits,
) -> Result<String, SchemaError> {
    validate_identifier(name, kind, limits)?;
    Ok(quote_identifier(name))
}

/// Validate a caller-supplied mutable column list: each must be a legal
/// identifier, none may duplicate, and none may equal the (always-reserved)
/// tenant column or an optionally-reserved column (the PK on update). Returns the
/// quoted identifiers in order.
fn validate_mutable_columns(
    columns: &[&str],
    table: &str,
    tenant_column: &str,
    also_reserved: Option<&str>,
) -> Result<Vec<String>, SchemaError> {
    let limits = SchemaLimits::default();
    let mut seen: Vec<&str> = Vec::with_capacity(columns.len());
    let mut quoted = Vec::with_capacity(columns.len());
    for &col in columns {
        validate_identifier(col, "column", &limits)?;
        if col == tenant_column || also_reserved == Some(col) {
            return Err(SchemaError::ReservedColumn {
                table: table.to_string(),
                column: col.to_string(),
            });
        }
        if seen.contains(&col) {
            return Err(SchemaError::DuplicateColumn {
                table: table.to_string(),
                name: col.to_string(),
            });
        }
        seen.push(col);
        quoted.push(quote_identifier(col));
    }
    Ok(quoted)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A typed row stand-in. We never decode it (the unit tests don't touch a
    // database), so a trivial manual `FromRow` keeps the test free of the sqlx
    // derive macro.
    #[derive(Debug)]
    struct WidgetRow;

    impl<'r> FromRow<'r, PgRow> for WidgetRow {
        fn from_row(_row: &'r PgRow) -> Result<Self, sqlx::Error> {
            Ok(WidgetRow)
        }
    }

    /// A table on the default `organization_id` tenant column.
    struct Widget;
    impl TenantScopedTable for Widget {
        type Row = WidgetRow;
        const NAME: &'static str = "widgets";
        const COLUMNS: &'static str = "id, name, organization_id";
    }

    /// A table that scopes by a non-default tenant column and a non-default PK.
    struct Account;
    impl TenantScopedTable for Account {
        type Row = WidgetRow;
        const NAME: &'static str = "accounts";
        const COLUMNS: &'static str = "account_id, tenant_id";
        const TENANT_COLUMN: &'static str = "tenant_id";
        const ID_COLUMN: &'static str = "account_id";
    }

    #[test]
    fn select_by_tenant_binds_default_tenant_column() {
        assert_eq!(
            Widget::select_by_tenant_sql().unwrap(),
            r#"SELECT id, name, organization_id FROM "widgets" WHERE "organization_id" = $1"#
        );
    }

    #[test]
    fn select_by_id_scopes_to_tenant() {
        assert_eq!(
            Widget::select_by_id_sql().unwrap(),
            r#"SELECT id, name, organization_id FROM "widgets" WHERE "id" = $1 AND "organization_id" = $2"#
        );
    }

    #[test]
    fn delete_by_id_scopes_to_tenant() {
        assert_eq!(
            Widget::delete_by_id_sql().unwrap(),
            r#"DELETE FROM "widgets" WHERE "id" = $1 AND "organization_id" = $2"#
        );
    }

    #[test]
    fn tenant_column_is_configurable() {
        // The override flows through every builder.
        assert_eq!(
            Account::select_by_id_sql().unwrap(),
            r#"SELECT account_id, tenant_id FROM "accounts" WHERE "account_id" = $1 AND "tenant_id" = $2"#
        );
        assert_eq!(
            Account::select_by_tenant_sql().unwrap(),
            r#"SELECT account_id, tenant_id FROM "accounts" WHERE "tenant_id" = $1"#
        );
    }

    #[test]
    fn insert_forces_tenant_as_first_column() {
        assert_eq!(
            Widget::insert_sql(&["name", "color"]).unwrap(),
            r#"INSERT INTO "widgets" ("organization_id", "name", "color") VALUES ($1, $2, $3) RETURNING id, name, organization_id"#
        );
    }

    #[test]
    fn insert_with_no_extra_columns_still_sets_tenant() {
        assert_eq!(
            Widget::insert_sql(&[]).unwrap(),
            r#"INSERT INTO "widgets" ("organization_id") VALUES ($1) RETURNING id, name, organization_id"#
        );
    }

    #[test]
    fn insert_rejects_tenant_column_in_list() {
        assert!(matches!(
            Widget::insert_sql(&["name", "organization_id"]),
            Err(SchemaError::ReservedColumn { .. })
        ));
    }

    #[test]
    fn update_sets_columns_and_scopes_where_by_tenant() {
        assert_eq!(
            Widget::update_sql(&["name", "color"]).unwrap(),
            r#"UPDATE "widgets" SET "name" = $1, "color" = $2 WHERE "id" = $3 AND "organization_id" = $4 RETURNING id, name, organization_id"#
        );
    }

    #[test]
    fn update_rejects_empty_column_set() {
        assert!(matches!(
            Widget::update_sql(&[]),
            Err(SchemaError::EmptyColumnSet { .. })
        ));
    }

    #[test]
    fn update_rejects_tenant_and_id_columns() {
        assert!(matches!(
            Widget::update_sql(&["organization_id"]),
            Err(SchemaError::ReservedColumn { .. })
        ));
        assert!(matches!(
            Widget::update_sql(&["id"]),
            Err(SchemaError::ReservedColumn { .. })
        ));
    }

    #[test]
    fn rejects_duplicate_columns() {
        assert!(matches!(
            Widget::insert_sql(&["name", "name"]),
            Err(SchemaError::DuplicateColumn { .. })
        ));
    }

    #[test]
    fn rejects_injection_in_column_name() {
        assert!(matches!(
            Widget::insert_sql(&["name\"; DROP TABLE widgets; --"]),
            Err(SchemaError::InvalidIdentifier { .. })
        ));
    }

    /// Compile-and-bind assertion: every execution helper type-checks, builds a
    /// `Send` future, and is awaitable end-to-end (including the heterogeneous
    /// `insert`/`update` bind closures). It is never *called*, so it touches no
    /// database — the security behavior is asserted by the SQL-builder tests
    /// above; live execution is covered by integration tests.
    #[allow(dead_code, clippy::let_underscore_future)]
    async fn execution_api_compiles(pool: &PgPool) -> Result<(), TenantError> {
        use uuid::Uuid;
        let tenant = Uuid::nil();
        let id = Uuid::nil();

        let _all = Widget::list_by_tenant(pool, tenant).await?;
        let _one = Widget::find_by_id(pool, tenant, id).await?;
        let _n = Widget::delete_by_id(pool, tenant, id).await?;
        let _ins = Widget::insert(pool, tenant, &["name"], |q| q.bind("hi".to_string())).await?;
        let _upd =
            Widget::update(pool, tenant, id, &["name"], |q| q.bind("hi".to_string())).await?;
        Ok(())
    }

    #[test]
    fn helpers_compile_for_custom_tenant_column() {
        // Touch the `Account` impl so its (overridden) builders are exercised at
        // compile time alongside the default-column `Widget`.
        assert!(Account::insert_sql(&["name"]).is_ok());
        assert!(Account::update_sql(&["name"]).is_ok());
    }
}
