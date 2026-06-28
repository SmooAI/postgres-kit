//! Forward-only migration runner (feature `migrate`). Applies `*.sql` files from
//! a directory in lexical order, recording each in a `__pg_migrations`
//! bookkeeping table so re-runs are idempotent. There is no auto-diff and no
//! down-migration — schema change is expressed as ordered, append-only SQL files.
//!
//! The on-disk layout follows a conventional migration-journal format: migration
//! files live at the directory root (`NNNN_<tag>.sql`, statements separated by
//! `--> statement-breakpoint`) and a `meta/_journal.json` records the ordered
//! tags. This lets `pnpm db:generate` / `pnpm db:migrate:local` keep working
//! against the same directory during a transition to this kit. Use
//! [`write_migration`] to emit a numbered file + journal entry from a
//! `Vec<DdlStatement>`, and [`run_migrations`] to apply them.

use crate::client::{PgError, PgExecutor};
use std::path::Path;

#[cfg(feature = "differ")]
use crate::differ::DdlStatement;
#[cfg(feature = "differ")]
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The bookkeeping table that records which migration files have been applied.
const MIGRATIONS_TABLE: &str = "__pg_migrations";

/// The statement separator written between rendered statements.
const STATEMENT_BREAKPOINT: &str = "--> statement-breakpoint";

/// The migration journal schema version this kit reads and writes.
const JOURNAL_VERSION: &str = "7";

/// The journal dialect tag for Postgres.
const JOURNAL_DIALECT: &str = "postgresql";

/// Outcome of a [`run_migrations`] pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationRunResult {
    /// Every `*.sql` file found in the directory, lexically sorted.
    pub discovered: Vec<String>,
    /// Files already recorded as applied (skipped this pass).
    pub skipped: Vec<String>,
    /// Files applied during this pass, in the order they ran.
    pub applied: Vec<String>,
}

/// Ensure the bookkeeping table exists, then apply every pending `*.sql` file in
/// `dir` (lexical order), recording each as it succeeds. Idempotent: files
/// already present in `__pg_migrations` are skipped.
///
/// Statements within a file are split on `--> statement-breakpoint` when present
/// (a convention robust to `;` inside function bodies), otherwise on
/// `;`. Each file's statements run, then the filename is recorded; a mid-file
/// failure leaves earlier statements applied but unrecorded, so a re-run replays
/// from the start of that file.
pub async fn run_migrations(
    exec: &impl PgExecutor,
    dir: &Path,
) -> Result<MigrationRunResult, PgError> {
    ensure_migrations_table(exec).await?;

    let discovered = discover_migration_files(dir)?;
    let already_applied = fetch_applied(exec).await?;

    let mut skipped = Vec::new();
    let mut applied = Vec::new();

    for filename in &discovered {
        if already_applied.contains(filename) {
            skipped.push(filename.clone());
            continue;
        }

        let path = dir.join(filename);
        let sql = std::fs::read_to_string(&path)?;
        for statement in split_sql_statements(&sql) {
            exec.command(&statement).await?;
        }
        exec.command(&record_statement(filename)).await?;
        applied.push(filename.clone());
    }

    Ok(MigrationRunResult {
        discovered,
        skipped,
        applied,
    })
}

/// Create the `__pg_migrations` table if it does not already exist.
async fn ensure_migrations_table(exec: &impl PgExecutor) -> Result<(), PgError> {
    let ddl = format!(
        "CREATE TABLE IF NOT EXISTS {MIGRATIONS_TABLE} (\n\
         \x20   filename text PRIMARY KEY,\n\
         \x20   applied_at timestamptz NOT NULL DEFAULT now()\n\
         )"
    );
    exec.command(&ddl).await
}

/// Read the already-applied migration filenames from the bookkeeping table.
async fn fetch_applied(exec: &impl PgExecutor) -> Result<Vec<String>, PgError> {
    exec.fetch_strings(&format!(
        "SELECT filename FROM {MIGRATIONS_TABLE} ORDER BY filename"
    ))
    .await
}

/// Record a single applied migration filename.
fn record_statement(filename: &str) -> String {
    // Filenames come from a trusted directory listing, but escape single quotes
    // defensively so an odd filename can't break the INSERT.
    let escaped = filename.replace('\'', "''");
    format!("INSERT INTO {MIGRATIONS_TABLE} (filename) VALUES ('{escaped}')")
}

/// List `*.sql` files in `dir`, returning their filenames in lexical order.
/// The `meta/` subdirectory (journal) is ignored — only top-level files are read.
fn discover_migration_files(dir: &Path) -> Result<Vec<String>, PgError> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("sql") {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                files.push(name.to_string());
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Split a migration file into individual statements. When the
/// `--> statement-breakpoint` separator is present, split on it (robust to `;`
/// inside `$$`-quoted function bodies); otherwise strip `--` line comments and
/// split on `;`. Empty fragments are dropped either way.
pub fn split_sql_statements(sql: &str) -> Vec<String> {
    if sql.contains(STATEMENT_BREAKPOINT) {
        return sql
            .split(STATEMENT_BREAKPOINT)
            .map(strip_line_comments)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }

    strip_line_comments(sql)
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Strip `--` line comments from every line, preserving newlines between lines.
fn strip_line_comments(sql: &str) -> String {
    sql.lines()
        .map(|line| match line.find("--") {
            Some(idx) => &line[..idx],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Migration journal
// ---------------------------------------------------------------------------

/// A `meta/_journal.json` document: the ordered list of migration tags the kit
/// maintains so `db:migrate` knows what to apply and in what order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationJournal {
    /// Journal schema version (currently `"7"`).
    pub version: String,
    /// SQL dialect (`"postgresql"`).
    pub dialect: String,
    /// Ordered migration entries, one per `NNNN_<tag>.sql` file.
    pub entries: Vec<MigrationJournalEntry>,
}

impl Default for MigrationJournal {
    fn default() -> Self {
        Self {
            version: JOURNAL_VERSION.to_string(),
            dialect: JOURNAL_DIALECT.to_string(),
            entries: Vec::new(),
        }
    }
}

/// A single entry in the migration journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationJournalEntry {
    /// Zero-based ordinal, matching the `NNNN` filename prefix.
    pub idx: u32,
    /// Per-entry schema version (mirrors [`MigrationJournal::version`]).
    pub version: String,
    /// Creation time in Unix epoch milliseconds.
    pub when: u64,
    /// The migration tag: `NNNN_<name>` (also the `.sql` filename stem).
    pub tag: String,
    /// Whether `--> statement-breakpoint` markers are written (always `true`).
    pub breakpoints: bool,
}

/// Path to the journal within a migrations directory (`<dir>/meta/_journal.json`).
fn journal_path(dir: &Path) -> std::path::PathBuf {
    dir.join("meta").join("_journal.json")
}

/// Read the migration journal from `<dir>/meta/_journal.json`. Returns a default
/// (empty) journal when the file does not exist.
pub fn read_journal(dir: &Path) -> Result<MigrationJournal, PgError> {
    let path = journal_path(dir);
    match std::fs::read_to_string(&path) {
        Ok(contents) => Ok(serde_json::from_str(&contents)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(MigrationJournal::default()),
        Err(e) => Err(PgError::Io(e)),
    }
}

/// The result of [`write_migration`]: the new file's tag and SQL path.
#[cfg(feature = "differ")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenMigration {
    /// The migration tag (`NNNN_<name>`), also the journal `tag`.
    pub tag: String,
    /// Absolute-or-relative path (under `dir`) to the written `.sql` file.
    pub path: std::path::PathBuf,
}

/// Write a numbered migration to `dir` from a list of
/// [`DdlStatement`]s, and append its entry to `<dir>/meta/_journal.json`.
///
/// The next ordinal is `max(existing idx) + 1` (or `0` for a fresh directory),
/// rendered as a 4-digit prefix; the file is `<dir>/<NNNN>_<name>.sql` with
/// statements joined by `--> statement-breakpoint`. `name` must be a short,
/// filename-safe slug (`[A-Za-z0-9_-]+`); it is rejected otherwise so it can
/// never escape `dir`.
#[cfg(feature = "differ")]
pub fn write_migration(
    dir: &Path,
    name: &str,
    statements: &[DdlStatement],
) -> Result<WrittenMigration, PgError> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(PgError::Backend(format!(
            "invalid migration name {name:?}: expected a non-empty [A-Za-z0-9_-]+ slug"
        )));
    }

    let mut journal = read_journal(dir)?;
    let idx = journal
        .entries
        .iter()
        .map(|e| e.idx)
        .max()
        .map_or(0, |m| m + 1);
    let tag = format!("{idx:04}_{name}");

    let body = render_migration_body(statements);
    let sql_path = dir.join(format!("{tag}.sql"));
    std::fs::create_dir_all(dir)?;
    std::fs::write(&sql_path, body)?;

    let when = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    journal.entries.push(MigrationJournalEntry {
        idx,
        version: JOURNAL_VERSION.to_string(),
        when,
        tag: tag.clone(),
        breakpoints: true,
    });

    let journal_file = journal_path(dir);
    if let Some(parent) = journal_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(&journal)?;
    std::fs::write(&journal_file, format!("{serialized}\n"))?;

    Ok(WrittenMigration {
        tag,
        path: sql_path,
    })
}

/// Render statements into a migration body: each statement's SQL
/// separated by a `--> statement-breakpoint` marker line.
#[cfg(feature = "differ")]
fn render_migration_body(statements: &[DdlStatement]) -> String {
    let sep = format!("\n{STATEMENT_BREAKPOINT}\n");
    let body = statements
        .iter()
        .map(DdlStatement::to_sql)
        .collect::<Vec<_>>()
        .join(&sep);
    format!("{body}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::LiveColumn;
    use std::sync::Mutex;

    /// A fake executor that simulates the `__pg_migrations` table in memory.
    #[derive(Default)]
    struct FakeExec {
        applied: Mutex<Vec<String>>,
        commands: Mutex<Vec<String>>,
    }

    impl PgExecutor for FakeExec {
        async fn command(&self, sql: &str) -> Result<(), PgError> {
            self.commands.lock().unwrap().push(sql.to_string());
            if let Some(rest) = sql.strip_prefix(&format!("INSERT INTO {MIGRATIONS_TABLE} ")) {
                // Parse the single-quoted filename out of `VALUES ('name')`.
                if let (Some(start), Some(end)) = (rest.find('\''), rest.rfind('\'')) {
                    if start < end {
                        let name = rest[start + 1..end].replace("''", "'");
                        self.applied.lock().unwrap().push(name);
                    }
                }
            }
            Ok(())
        }

        async fn fetch_strings(&self, _sql: &str) -> Result<Vec<String>, PgError> {
            let mut applied = self.applied.lock().unwrap().clone();
            applied.sort();
            Ok(applied)
        }

        async fn fetch_columns(&self, _table: &str) -> Result<Vec<LiveColumn>, PgError> {
            Ok(vec![])
        }
    }

    /// Create a unique scratch directory for a test, cleaned up by the caller.
    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pg_kit_migrate_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn splits_and_strips_comments() {
        let sql = "-- create\nCREATE TABLE x (a int); -- trailing\nINSERT INTO x VALUES (1);\n";
        let stmts = split_sql_statements(sql);
        assert_eq!(stmts.len(), 2);
        assert_eq!(stmts[0], "CREATE TABLE x (a int)");
        assert_eq!(stmts[1], "INSERT INTO x VALUES (1)");
    }

    #[test]
    fn empty_input_yields_no_statements() {
        assert!(split_sql_statements("").is_empty());
        assert!(split_sql_statements("   \n  ;; \n -- only a comment").is_empty());
    }

    #[test]
    fn splits_on_statement_breakpoints() {
        // A `;` inside a function body must NOT split a statement when the
        // statement-breakpoint marker is present.
        let sql =
            "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END; $$ LANGUAGE plpgsql;\n\
                   --> statement-breakpoint\n\
                   CREATE TABLE x (a int);\n";
        let stmts = split_sql_statements(sql);
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].starts_with("CREATE FUNCTION f()"));
        assert!(stmts[0].contains("RETURN 1; END;"));
        assert_eq!(stmts[1], "CREATE TABLE x (a int);");
    }

    #[test]
    fn record_statement_escapes_quotes() {
        assert_eq!(
            record_statement("0000_init.sql"),
            "INSERT INTO __pg_migrations (filename) VALUES ('0000_init.sql')"
        );
        assert_eq!(
            record_statement("o'brien.sql"),
            "INSERT INTO __pg_migrations (filename) VALUES ('o''brien.sql')"
        );
    }

    #[tokio::test]
    async fn run_migrations_applies_then_is_idempotent() {
        let dir = temp_dir();
        std::fs::write(dir.join("0000_a.sql"), "CREATE TABLE a (id int);").unwrap();
        std::fs::write(
            dir.join("0001_b.sql"),
            "CREATE TABLE b (id int);\n--> statement-breakpoint\nCREATE INDEX bi ON b (id);",
        )
        .unwrap();

        let exec = FakeExec::default();

        let first = run_migrations(&exec, &dir).await.unwrap();
        assert_eq!(first.discovered, vec!["0000_a.sql", "0001_b.sql"]);
        assert_eq!(first.applied, vec!["0000_a.sql", "0001_b.sql"]);
        assert!(first.skipped.is_empty());

        // Re-run: everything is already recorded, nothing re-applies.
        let second = run_migrations(&exec, &dir).await.unwrap();
        assert!(second.applied.is_empty());
        assert_eq!(second.skipped, vec!["0000_a.sql", "0001_b.sql"]);

        // The CREATE INDEX (after a breakpoint) ran as its own command.
        let cmds = exec.commands.lock().unwrap().clone();
        assert!(cmds.iter().any(|c| c == "CREATE INDEX bi ON b (id);"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(feature = "differ")]
    #[tokio::test]
    async fn write_migration_creates_file_and_journal() {
        use crate::differ::DdlStatement;

        let dir = temp_dir();

        let first = write_migration(
            &dir,
            "init",
            &[DdlStatement::DropTable {
                schema: "public".to_string(),
                name: "legacy".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(first.tag, "0000_init");
        assert!(first.path.exists());

        let second = write_migration(
            &dir,
            "more",
            &[DdlStatement::DropTable {
                schema: "public".to_string(),
                name: "other".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(second.tag, "0001_more");

        // Journal records both entries, in order, with the expected shape.
        let journal = read_journal(&dir).unwrap();
        assert_eq!(journal.version, "7");
        assert_eq!(journal.dialect, "postgresql");
        assert_eq!(journal.entries.len(), 2);
        assert_eq!(journal.entries[0].idx, 0);
        assert_eq!(journal.entries[0].tag, "0000_init");
        assert!(journal.entries[0].breakpoints);
        assert_eq!(journal.entries[1].idx, 1);
        assert_eq!(journal.entries[1].tag, "0001_more");

        // The written file is splittable back into statements.
        let body = std::fs::read_to_string(&second.path).unwrap();
        let stmts = split_sql_statements(&body);
        assert_eq!(stmts.len(), 1);
        assert!(stmts[0].contains("DROP TABLE"));

        // And the runner picks up exactly what we wrote.
        let exec = FakeExec::default();
        let run = run_migrations(&exec, &dir).await.unwrap();
        assert_eq!(run.applied, vec!["0000_init.sql", "0001_more.sql"]);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(feature = "differ")]
    #[test]
    fn write_migration_preserves_raw_sql_before_policy() {
        use crate::differ::ir::SnapPolicy;
        use crate::differ::DdlStatement;

        let dir = temp_dir();
        let func = "CREATE FUNCTION rpm_pizza.is_store_manager(store uuid) RETURNS boolean AS $$ SELECT true $$ LANGUAGE sql;";
        let written = write_migration(
            &dir,
            "rpm",
            &[
                DdlStatement::RawSql(func.to_string()),
                DdlStatement::CreatePolicy {
                    schema: "rpm_pizza".to_string(),
                    table: "task_instances".to_string(),
                    policy: SnapPolicy::new("gm_select")
                        .using("rpm_pizza.is_store_manager(store_id)"),
                },
            ],
        )
        .unwrap();

        let body = std::fs::read_to_string(&written.path).unwrap();
        let func_at = body.find("CREATE FUNCTION").expect("function present");
        let policy_at = body.find("CREATE POLICY").expect("policy present");
        assert!(func_at < policy_at, "raw function must precede the policy");

        // The raw SQL round-trips back out as its own statement, verbatim.
        let stmts = split_sql_statements(&body);
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].contains("CREATE FUNCTION rpm_pizza.is_store_manager"));
        assert!(stmts[1].contains("CREATE POLICY"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(feature = "differ")]
    #[test]
    fn write_migration_rejects_unsafe_names() {
        let dir = temp_dir();
        let err = write_migration(&dir, "../escape", &[]).unwrap_err();
        assert!(matches!(err, PgError::Backend(_)));
        assert!(write_migration(&dir, "", &[]).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
