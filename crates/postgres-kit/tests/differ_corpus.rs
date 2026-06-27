//! Harness for the differ conformance corpus (see `tests/corpus/mod.rs`).
//!
//! For each `Supported` case it runs the differ and asserts the rendered SQL
//! equals the expected SQL under normalized comparison — internal whitespace is
//! collapsed and ends are trimmed, but statement order and boundaries are kept.
//! `Skip` cases are counted and printed. With zero registered cases this passes.

mod corpus;

use corpus::{all_cases, Status};
use postgres_kit::differ::{diff, RenameHints};

/// Collapse all runs of whitespace (including newlines) to single spaces and
/// trim — so formatting differences don't cause false failures.
fn normalize(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn corpus_matches_expected_sql() {
    let cases = all_cases();
    let mut supported = 0usize;
    let mut skipped = 0usize;

    for case in &cases {
        match case.status {
            Status::Skip(reason) => {
                skipped += 1;
                println!("SKIP  {}: {}", case.name, reason);
            }
            Status::Supported => {
                supported += 1;
                let hints = RenameHints::parse(case.renames)
                    .unwrap_or_else(|e| panic!("case {}: invalid rename hints: {e}", case.name));
                let actual: Vec<String> = diff(&case.from, &case.to, &hints)
                    .iter()
                    .map(|s| normalize(&s.to_sql()))
                    .collect();
                let expected: Vec<String> =
                    case.expected_sql.iter().map(|s| normalize(s)).collect();
                assert_eq!(
                    actual, expected,
                    "differ output mismatch for case {}",
                    case.name
                );
            }
        }
    }

    println!(
        "corpus: {supported} supported, {skipped} skipped, {} total",
        cases.len()
    );
}
