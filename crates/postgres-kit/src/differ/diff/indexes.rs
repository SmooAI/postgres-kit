//! Index pass for tables present on both sides. A changed index is dropped and
//! recreated (Postgres has no in-place alter for the properties we model).

use crate::differ::ir::SnapTable;
use crate::differ::statement::DdlStatement;

use super::Plan;

pub fn diff(plan: &mut Plan, from_t: &SnapTable, to_t: &SnapTable) {
    let schema = &to_t.schema;
    let table = &to_t.name;

    for (name, from_i) in &from_t.indexes {
        let dropped = match to_t.indexes.get(name) {
            None => true,
            Some(to_i) => to_i != from_i,
        };
        if dropped {
            plan.drop_indexes.push(DdlStatement::DropIndex {
                schema: schema.clone(),
                name: name.clone(),
            });
        }
    }
    for (name, to_i) in &to_t.indexes {
        let added = match from_t.indexes.get(name) {
            None => true,
            Some(from_i) => from_i != to_i,
        };
        if added {
            plan.create_indexes.push(DdlStatement::CreateIndex {
                schema: schema.clone(),
                table: table.clone(),
                index: to_i.clone(),
            });
        }
    }
}
