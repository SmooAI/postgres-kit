//! Migration scaffolding (feature `migrate`): a forward-only `*.sql` file runner
//! with a bookkeeping table, mirroring the clickhouse-kit migrator. The real
//! runner lands in a later phase; this stub reserves the module, the public
//! split helper, and proves the feature compiles.

/// Split a migration file into individual statements: strip `--` line comments,
/// split on `;`, and drop empty fragments. (Stable utility; the file runner that
/// consumes it is implemented in a later phase.)
pub fn split_sql_statements(sql: &str) -> Vec<String> {
    let stripped: String = sql
        .lines()
        .map(|line| match line.find("--") {
            Some(idx) => &line[..idx],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n");

    stripped
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_and_strips_comments() {
        let sql = "-- create\nCREATE TABLE x (a int); -- trailing\nINSERT INTO x VALUES (1);\n";
        let stmts = split_sql_statements(sql);
        assert_eq!(stmts.len(), 2);
        assert_eq!(stmts[0], "CREATE TABLE x (a int)");
        assert_eq!(stmts[1], "INSERT INTO x VALUES (1)");
    }
}
