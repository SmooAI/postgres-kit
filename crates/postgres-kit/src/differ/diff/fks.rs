//! Foreign-key pass for tables present on both sides. Postgres has no in-place FK
//! alter, so a changed FK (an `ON DELETE` / `ON UPDATE` action change, or a
//! repointed column/target) is dropped and re-added.
//!
//! - A FK present in `from` but gone in `to` is a **drop**, emitted in the
//!   teardown phase.
//! - A FK present in `to` but not `from` is an **add**, emitted in the build phase
//!   after columns exist.
//! - A FK present on both sides whose definition differs is an **alter**: a single
//!   [`DdlStatement::AlterForeignKey`] drop+recreate statement, also emitted in the
//!   build phase (it carries its own inline `DROP CONSTRAINT`, so it does not go
//!   through the teardown drop bucket — that would split the change across phases
//!   and re-order it relative to drizzle's `alter_reference` output).

use crate::differ::ir::SnapTable;
use crate::differ::statement::DdlStatement;

use super::Plan;

pub fn diff(plan: &mut Plan, from_t: &SnapTable, to_t: &SnapTable) {
    let schema = &to_t.schema;
    let table = &to_t.name;

    // Drops: present in `from`, absent in `to`.
    for name in from_t.foreign_keys.keys() {
        if !to_t.foreign_keys.contains_key(name) {
            plan.drop_foreign_keys.push(DdlStatement::DropForeignKey {
                schema: schema.clone(),
                table: table.clone(),
                name: name.clone(),
            });
        }
    }

    // Adds (new name) and alters (same name, changed definition). Both ride the
    // build-phase `add_foreign_keys` bucket so they land after columns exist; the
    // alter is a single combined drop+recreate statement.
    for (name, to_fk) in &to_t.foreign_keys {
        match from_t.foreign_keys.get(name) {
            None => plan.add_foreign_keys.push(DdlStatement::CreateForeignKey {
                schema: schema.clone(),
                table: table.clone(),
                fk: to_fk.clone(),
            }),
            Some(from_fk) if from_fk != to_fk => {
                plan.add_foreign_keys.push(DdlStatement::AlterForeignKey {
                    schema: schema.clone(),
                    table: table.clone(),
                    fk: to_fk.clone(),
                });
            }
            Some(_) => {}
        }
    }
}
