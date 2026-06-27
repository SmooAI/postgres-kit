//! The differ conformance corpus.
//!
//! Each case declares a `from` snapshot, a `to` snapshot, rename hints, and the
//! exact SQL the differ should emit. The harness in `tests/differ_corpus.rs`
//! runs [`postgres_kit::differ::diff`] over every `Supported` case and asserts the
//! rendered statements match `expected_sql` under normalized comparison; `Skip`
//! cases are counted and printed but not asserted.
//!
//! ## Adding cases
//!
//! Category modules live alongside this file as `tests/corpus/<category>.rs` and
//! each exposes `pub fn cases() -> Vec<DiffCase>`. To register one, add a
//! `pub mod <category>;` line below and extend [`all_cases`] with
//! `out.extend(<category>::cases());`. The differ/integrator agent owns those
//! category files; this scaffold compiles with zero categories.

#![allow(dead_code)]

use postgres_kit::differ::SchemaSnapshot;

/// Whether a case is asserted or merely tracked.
pub enum Status {
    /// The differ is expected to produce `expected_sql` exactly (normalized).
    Supported,
    /// Not yet covered; the harness counts and prints the reason but skips it.
    Skip(&'static str),
}

/// One differ conformance scenario.
pub struct DiffCase {
    pub name: &'static str,
    pub from: SchemaSnapshot,
    pub to: SchemaSnapshot,
    pub renames: &'static [&'static str],
    pub expected_sql: &'static [&'static str],
    pub status: Status,
}

// ---- category registry (differ/integrator agent expands this) ----
// pub mod tables;
// pub mod columns;
// pub mod enums;
// pub mod constraints;
// pub mod indexes;
// pub mod policies;
// pub mod views;
// pub mod sequences;
// pub mod roles;

/// Collect every registered case. Empty until categories are registered above.
pub fn all_cases() -> Vec<DiffCase> {
    #[allow(unused_mut)]
    let mut out: Vec<DiffCase> = Vec::new();
    // out.extend(tables::cases());
    // out.extend(columns::cases());
    // ...
    out
}
