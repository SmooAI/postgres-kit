//! Policy + RLS pass for tables present on both sides. Renames first; then
//! changes to a policy's `AS`/`FOR` force a drop+recreate (those can't be
//! `ALTER`ed), while role/`USING`/`WITH CHECK` changes use `ALTER POLICY`.
//! Row-level-security enable/disable is diffed from the table's `rls_enabled`.

use std::collections::BTreeSet;

use crate::differ::ir::SnapTable;
use crate::differ::renames::RenameHints;
use crate::differ::statement::DdlStatement;

use super::Plan;

pub fn diff(plan: &mut Plan, from_t: &SnapTable, to_t: &SnapTable, hints: &RenameHints) {
    let schema = &to_t.schema;
    let table = &to_t.name;

    let mut from_consumed: BTreeSet<String> = BTreeSet::new();
    let mut to_consumed: BTreeSet<String> = BTreeSet::new();

    // Renames first. A policy rename may be supplied either as a tagged
    // `policy:` hint or — since both are table-scoped, arity-3 names — as an
    // untagged hint that parses into the column bucket; consult both and accept
    // whichever resolves a `from`/`to` pair of policies on this table.
    for name in from_t.policies.keys() {
        let to_name = hints
            .find_policy_rename(schema, table, name)
            .map(|r| r.to.clone())
            .or_else(|| {
                hints
                    .find_column_rename(schema, table, name)
                    .map(|r| r.to.clone())
            });
        if let Some(to_name) = to_name {
            if to_t.policies.contains_key(&to_name) {
                plan.rename_policies.push(DdlStatement::RenamePolicy {
                    schema: schema.clone(),
                    table: table.clone(),
                    from: name.clone(),
                    to: to_name.clone(),
                });
                from_consumed.insert(name.clone());
                to_consumed.insert(to_name);
            }
        }
    }

    // Created / altered / recreated.
    for (name, to_p) in &to_t.policies {
        if to_consumed.contains(name) {
            continue;
        }
        match from_t.policies.get(name) {
            None => plan.create_policies.push(DdlStatement::CreatePolicy {
                schema: schema.clone(),
                table: table.clone(),
                policy: to_p.clone(),
            }),
            Some(from_p) => {
                if from_p.as_ != to_p.as_ || from_p.for_ != to_p.for_ {
                    plan.drop_policies.push(DdlStatement::DropPolicy {
                        schema: schema.clone(),
                        table: table.clone(),
                        name: name.clone(),
                    });
                    plan.create_policies.push(DdlStatement::CreatePolicy {
                        schema: schema.clone(),
                        table: table.clone(),
                        policy: to_p.clone(),
                    });
                } else if from_p.to != to_p.to
                    || from_p.using != to_p.using
                    || from_p.with_check != to_p.with_check
                {
                    plan.alter_policies.push(DdlStatement::AlterPolicy {
                        schema: schema.clone(),
                        table: table.clone(),
                        policy: to_p.clone(),
                    });
                }
            }
        }
    }

    // Dropped.
    for name in from_t.policies.keys() {
        if from_consumed.contains(name) {
            continue;
        }
        if !to_t.policies.contains_key(name) {
            plan.drop_policies.push(DdlStatement::DropPolicy {
                schema: schema.clone(),
                table: table.clone(),
                name: name.clone(),
            });
        }
    }

    // RLS toggle.
    if !from_t.rls_enabled && to_t.rls_enabled {
        plan.enable_rls.push(DdlStatement::EnableRls {
            schema: schema.clone(),
            table: table.clone(),
        });
    } else if from_t.rls_enabled && !to_t.rls_enabled {
        plan.disable_rls.push(DdlStatement::DisableRls {
            schema: schema.clone(),
            table: table.clone(),
        });
    }
}
