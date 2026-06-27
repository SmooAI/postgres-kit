//! Bring-your-own-client seam. The kit generates SQL and introspects shapes; it
//! never owns a connection or decodes rows itself. You implement [`PgExecutor`]
//! over your Postgres client (e.g. `sqlx`) — kept driver-agnostic so the shipped
//! crate stays tiny (serde + thiserror only).
//!
//! Methods return `impl Future + Send` (not `async fn`) so the futures are
//! spawn-friendly across runtimes.

use std::future::Future;

use thiserror::Error;

/// Error surface for executor I/O.
#[derive(Debug, Error)]
pub enum PgError {
    #[error("postgres backend error: {0}")]
    Backend(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// A live column as introspected from `information_schema` / `pg_catalog`.
/// Consumed by the (forthcoming) drift and migration paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveColumn {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
}

/// The minimal seam the kit needs against a live database.
pub trait PgExecutor {
    /// Execute a statement that returns no rows (DDL, migrations).
    fn command(&self, sql: &str) -> impl Future<Output = Result<(), PgError>> + Send;

    /// Run a query expected to yield a single text column per row.
    fn fetch_strings(&self, sql: &str)
        -> impl Future<Output = Result<Vec<String>, PgError>> + Send;

    /// Introspect the live columns of `table` (empty ⇒ table absent).
    fn fetch_columns(
        &self,
        table: &str,
    ) -> impl Future<Output = Result<Vec<LiveColumn>, PgError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    // A trivial in-memory double proves the trait is object-safe-enough to
    // implement and that the future bounds hold under the multi-thread runtime.
    struct FakeExec {
        columns: Vec<LiveColumn>,
    }

    impl PgExecutor for FakeExec {
        async fn command(&self, _sql: &str) -> Result<(), PgError> {
            Ok(())
        }
        async fn fetch_strings(&self, _sql: &str) -> Result<Vec<String>, PgError> {
            Ok(vec![])
        }
        async fn fetch_columns(&self, _table: &str) -> Result<Vec<LiveColumn>, PgError> {
            Ok(self.columns.clone())
        }
    }

    #[tokio::test]
    async fn executor_double_roundtrips() {
        let exec = FakeExec {
            columns: vec![LiveColumn {
                name: "id".into(),
                data_type: "uuid".into(),
                is_nullable: false,
            }],
        };
        exec.command("CREATE TABLE t ()").await.unwrap();
        assert!(exec.fetch_strings("SELECT 1").await.unwrap().is_empty());
        assert_eq!(exec.fetch_columns("t").await.unwrap().len(), 1);
    }
}
