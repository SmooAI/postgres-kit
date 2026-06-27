//! Role pass. Renames first (untagged arity-1 hints). Matched roles whose options
//! differ are re-emitted with `ALTER ROLE`; new/removed roles are created/dropped.

use std::collections::BTreeSet;

use crate::differ::ir::{SchemaSnapshot, SnapRole};
use crate::differ::renames::RenameHints;
use crate::differ::statement::DdlStatement;

use super::Plan;

pub fn diff(plan: &mut Plan, from: &SchemaSnapshot, to: &SchemaSnapshot, hints: &RenameHints) {
    let mut consumed_from: BTreeSet<String> = BTreeSet::new();

    for (name, to_r) in &to.roles {
        if let Some(from_r) = from.roles.get(name) {
            if options_differ(from_r, to_r) {
                plan.alter_roles.push(DdlStatement::AlterRole(to_r.clone()));
            }
            consumed_from.insert(name.clone());
            continue;
        }
        if let Some(rr) = hints.roles.iter().find(|rr| &rr.to == name) {
            if from.roles.contains_key(&rr.from) {
                plan.rename_roles.push(DdlStatement::RenameRole {
                    from: rr.from.clone(),
                    to: rr.to.clone(),
                });
                consumed_from.insert(rr.from.clone());
                continue;
            }
        }
        plan.create_roles
            .push(DdlStatement::CreateRole(to_r.clone()));
    }

    for name in from.roles.keys() {
        if consumed_from.contains(name) || to.roles.contains_key(name) {
            continue;
        }
        plan.drop_roles
            .push(DdlStatement::DropRole { name: name.clone() });
    }
}

fn options_differ(a: &SnapRole, b: &SnapRole) -> bool {
    a.create_db != b.create_db || a.create_role != b.create_role || a.inherit != b.inherit
}
