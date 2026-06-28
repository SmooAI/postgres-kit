//! Index pass for tables present on both sides.
//!
//! Postgres has no in-place `ALTER INDEX` for any property we model (indexed
//! columns/expressions, `USING <method>`, per-column opclass, the partial
//! `WHERE` predicate, or uniqueness), so a *changed* index is migrated by
//! dropping the old definition and creating the new one.
//!
//! To match the conformance corpus, an alter is emitted
//! as **two distinct statements**, not one combined string: a changed index is
//! represented as a separate `drop_index` and `create_index_pg` pair. The
//! `DROP INDEX` therefore lands in the teardown/drop phase (alongside the other
//! drops) and the `CREATE INDEX` in the later index-build phase — the
//! [`DdlStatement::AlterIndex`] combined form is intentionally *not* used here.

use crate::differ::ir::SnapTable;
use crate::differ::statement::DdlStatement;

use super::Plan;

pub fn diff(plan: &mut Plan, from_t: &SnapTable, to_t: &SnapTable) {
    let schema = &to_t.schema;
    let table = &to_t.name;

    // Drops, plus the drop half of every alter (an index whose definition
    // changed): both go in the drop phase.
    for (name, from_i) in &from_t.indexes {
        let removed_or_changed = match to_t.indexes.get(name) {
            // Gone in `to` — a straight drop.
            None => true,
            // Present on both sides but different — drop the old definition so
            // the recreate below can lay down the new one.
            Some(to_i) => to_i != from_i,
        };
        if removed_or_changed {
            plan.drop_indexes.push(DdlStatement::DropIndex {
                schema: schema.clone(),
                name: name.clone(),
            });
        }
    }

    // Creates, plus the create half of every alter: both go in the index-build
    // phase, which the plan assembles after the matching drops.
    for (name, to_i) in &to_t.indexes {
        let added_or_changed = match from_t.indexes.get(name) {
            // Brand new in `to`.
            None => true,
            // Changed — recreate with the new definition.
            Some(from_i) => from_i != to_i,
        };
        if added_or_changed {
            plan.create_indexes.push(DdlStatement::CreateIndex {
                schema: schema.clone(),
                table: table.clone(),
                index: to_i.clone(),
            });
        }
    }
}
