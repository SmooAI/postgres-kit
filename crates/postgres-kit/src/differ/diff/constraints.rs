//! Table-level constraint pass for tables present on both sides: CHECK, UNIQUE,
//! and composite PRIMARY KEY constraints. There is no in-place alter for these,
//! so a changed constraint is dropped and re-added (all drops precede all adds via
//! the [`super::order`] phases). Renames are surfaced as drop+add too,
//! so there is no rename branch here.

use crate::differ::ir::SnapTable;
use crate::differ::statement::DdlStatement;

use super::Plan;

pub fn diff(plan: &mut Plan, from_t: &SnapTable, to_t: &SnapTable) {
    let schema = &to_t.schema;
    let table = &to_t.name;

    // ---- checks ----
    for (name, from_c) in &from_t.check_constraints {
        let dropped = match to_t.check_constraints.get(name) {
            None => true,
            Some(to_c) => to_c.value != from_c.value,
        };
        if dropped {
            plan.drop_checks.push(DdlStatement::DropCheck {
                schema: schema.clone(),
                table: table.clone(),
                name: name.clone(),
            });
        }
    }
    for (name, to_c) in &to_t.check_constraints {
        let added = match from_t.check_constraints.get(name) {
            None => true,
            Some(from_c) => from_c.value != to_c.value,
        };
        if added {
            plan.add_checks.push(DdlStatement::CreateCheck {
                schema: schema.clone(),
                table: table.clone(),
                name: name.clone(),
                value: to_c.value.clone(),
            });
        }
    }

    // ---- unique constraints ----
    for (name, from_u) in &from_t.unique_constraints {
        let dropped = match to_t.unique_constraints.get(name) {
            None => true,
            Some(to_u) => to_u != from_u,
        };
        if dropped {
            plan.drop_uniques.push(DdlStatement::DropUnique {
                schema: schema.clone(),
                table: table.clone(),
                name: name.clone(),
            });
        }
    }
    for (name, to_u) in &to_t.unique_constraints {
        let added = match from_t.unique_constraints.get(name) {
            None => true,
            Some(from_u) => from_u != to_u,
        };
        if added {
            plan.add_uniques.push(DdlStatement::CreateUnique {
                schema: schema.clone(),
                table: table.clone(),
                unique: to_u.clone(),
            });
        }
    }

    // ---- composite primary keys ----
    // A composite PK that vanished is a straight drop. A *changed* PK that keeps
    // its name is a single combined drop+recreate (`AlterCompositePk`) — Postgres
    // has no in-place PK alter, and the corpus expects the two halves joined into
    // one breakpoint-delimited statement rather than split across phases.
    for name in from_t.composite_primary_keys.keys() {
        if !to_t.composite_primary_keys.contains_key(name) {
            plan.drop_composite_pks.push(DdlStatement::DropCompositePk {
                schema: schema.clone(),
                table: table.clone(),
                name: name.clone(),
            });
        }
    }
    for (name, to_pk) in &to_t.composite_primary_keys {
        match from_t.composite_primary_keys.get(name) {
            None => plan
                .add_composite_pks
                .push(DdlStatement::CreateCompositePk {
                    schema: schema.clone(),
                    table: table.clone(),
                    pk: to_pk.clone(),
                }),
            Some(from_pk) if from_pk != to_pk => {
                plan.alter_composite_pks
                    .push(DdlStatement::AlterCompositePk {
                        schema: schema.clone(),
                        table: table.clone(),
                        pk: to_pk.clone(),
                    });
            }
            Some(_) => {}
        }
    }
}
