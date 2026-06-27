//! View pass. Renames first (untagged arity-2 hints). New/removed views are
//! created/dropped; a changed definition is a drop+recreate.

use std::collections::BTreeSet;

use crate::differ::ir::SchemaSnapshot;
use crate::differ::renames::RenameHints;
use crate::differ::statement::DdlStatement;

use super::Plan;

pub fn diff(plan: &mut Plan, from: &SchemaSnapshot, to: &SchemaSnapshot, hints: &RenameHints) {
    let mut consumed_from: BTreeSet<String> = BTreeSet::new();

    for (to_key, to_v) in &to.views {
        if let Some(from_v) = from.views.get(to_key) {
            if from_v.definition != to_v.definition || from_v.materialized != to_v.materialized {
                plan.drop_views.push(DdlStatement::DropView {
                    schema: from_v.schema.clone(),
                    name: from_v.name.clone(),
                    materialized: from_v.materialized,
                });
                plan.create_views
                    .push(DdlStatement::CreateView(to_v.clone()));
            }
            consumed_from.insert(to_key.clone());
            continue;
        }
        if let Some(r) = super::rename_by_target(hints, &to_v.schema, &to_v.name) {
            let from_key = format!("{}.{}", r.from_schema, r.from);
            if from.views.contains_key(&from_key) {
                if r.from != r.to {
                    plan.rename_views.push(DdlStatement::RenameView {
                        schema: r.to_schema.clone(),
                        from: r.from.clone(),
                        to: r.to.clone(),
                        materialized: to_v.materialized,
                    });
                }
                consumed_from.insert(from_key);
                continue;
            }
        }
        plan.create_views
            .push(DdlStatement::CreateView(to_v.clone()));
    }

    for (from_key, from_v) in &from.views {
        if consumed_from.contains(from_key) || to.views.contains_key(from_key) {
            continue;
        }
        plan.drop_views.push(DdlStatement::DropView {
            schema: from_v.schema.clone(),
            name: from_v.name.clone(),
            materialized: from_v.materialized,
        });
    }
}
