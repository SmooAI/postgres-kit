//! The schema differ proper: compare a `from` and `to` [`SchemaSnapshot`] and
//! produce the ordered [`DdlStatement`]s that migrate one into the other.
//!
//! The work is split into per-object-kind passes ([`tables`], [`columns`],
//! [`constraints`], [`fks`], [`indexes`], [`policies`], [`enums`], [`sequences`],
//! [`views`], [`roles`], plus the [`identity`] helper). Each pass **resolves
//! renames first** (consulting [`RenameHints`]) — a hinted `from -> to` is treated
//! as a rename/move and removes *both* names from the add/drop pools, so a hinted
//! rename never degrades into a destructive drop+add. Every pass appends to the
//! phase buckets of a [`Plan`]; [`order`] concatenates them into the final,
//! drizzle-compatible statement order.

mod columns;
mod constraints;
mod enums;
mod fks;
mod identity;
mod indexes;
mod order;
mod policies;
mod roles;
mod sequences;
mod tables;
mod views;

pub use order::Plan;

use crate::differ::ir::SchemaSnapshot;
use crate::differ::renames::RenameHints;
use crate::differ::statement::DdlStatement;

/// Diff `from` into `to`, returning the ordered migration statements.
pub fn diff(from: &SchemaSnapshot, to: &SchemaSnapshot, hints: &RenameHints) -> Vec<DdlStatement> {
    let mut plan = Plan::default();

    enums::diff(&mut plan, from, to, hints);
    sequences::diff(&mut plan, from, to, hints);
    roles::diff(&mut plan, from, to, hints);
    tables::diff(&mut plan, from, to, hints);
    views::diff(&mut plan, from, to, hints);

    plan.assemble()
}

/// Resolve a relation rename/move hint by its *target* `(schema, name)` — the
/// untagged arity-2 hints are keyed by source, so enum/sequence/view/table passes
/// match against the `to` side here.
pub(crate) fn rename_by_target<'a>(
    hints: &'a RenameHints,
    to_schema: &str,
    to_name: &str,
) -> Option<&'a crate::differ::renames::TableRename> {
    hints
        .tables
        .iter()
        .find(|r| r.to_schema == to_schema && r.to == to_name)
}
