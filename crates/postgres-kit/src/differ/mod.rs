//! The schema differ: compare two [`SchemaSnapshot`]s and emit the
//! [`DdlStatement`]s that migrate `from` into `to`.
//!
//! Authoring flow: write [`crate::spec::PgTableSpec`]s, [`lower`] them to a
//! [`SchemaSnapshot`], and [`diff`] the previous snapshot against the new one.
//!
//! [`diff`] is a stub for now (returns no statements); the differ agent fills it
//! in against the corpus harness in `tests/`.

pub mod diff;
pub mod ir;
pub mod lower;
pub mod renames;
pub mod statement;

pub use ir::{
    SchemaSnapshot, SchemaSnapshotBuilder, SnapCheck, SnapColumn, SnapColumnUnique,
    SnapCompositePk, SnapEnum, SnapForeignKey, SnapIdentity, SnapIndex, SnapIndexColumn,
    SnapPolicy, SnapRole, SnapSequence, SnapTable, SnapUnique, SnapView,
};
pub use lower::{lower, lower_table, lower_tables};
pub use renames::{ColumnRename, EnumRename, PolicyRename, RenameHints, RoleRename, TableRename};
pub use statement::DdlStatement;

/// Diff `from` into `to`, returning the ordered DDL statements that perform the
/// migration. `hints` resolves ambiguous drop+add pairs into renames/moves.
pub fn diff(from: &SchemaSnapshot, to: &SchemaSnapshot, hints: &RenameHints) -> Vec<DdlStatement> {
    diff::diff(from, to, hints)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_diff_is_noop() {
        let from = SchemaSnapshot::default();
        let to = SchemaSnapshot::default();
        let hints = RenameHints::default();
        assert!(diff(&from, &to, &hints).is_empty());
    }
}
