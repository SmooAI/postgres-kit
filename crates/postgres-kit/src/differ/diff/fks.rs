//! Foreign-key pass for tables present on both sides. Postgres has no in-place FK
//! alter, so a changed FK is dropped and re-added (drops in the teardown phase,
//! adds after columns exist).

use crate::differ::ir::SnapTable;
use crate::differ::statement::DdlStatement;

use super::Plan;

pub fn diff(plan: &mut Plan, from_t: &SnapTable, to_t: &SnapTable) {
    let schema = &to_t.schema;
    let table = &to_t.name;

    for (name, from_fk) in &from_t.foreign_keys {
        let dropped = match to_t.foreign_keys.get(name) {
            None => true,
            Some(to_fk) => to_fk != from_fk,
        };
        if dropped {
            plan.drop_foreign_keys.push(DdlStatement::DropForeignKey {
                schema: schema.clone(),
                table: table.clone(),
                name: name.clone(),
            });
        }
    }
    for (name, to_fk) in &to_t.foreign_keys {
        let added = match from_t.foreign_keys.get(name) {
            None => true,
            Some(from_fk) => from_fk != to_fk,
        };
        if added {
            plan.add_foreign_keys.push(DdlStatement::CreateForeignKey {
                schema: schema.clone(),
                table: table.clone(),
                fk: to_fk.clone(),
            });
        }
    }
}
