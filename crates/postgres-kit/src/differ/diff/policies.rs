//! Policy + RLS pass for tables present on both sides. Renames first; then
//! changes to a policy's `AS`/`FOR` force a drop+recreate (those can't be
//! `ALTER`ed), while role/`USING`/`WITH CHECK` changes use `ALTER POLICY`.
//! Row-level-security enable/disable is diffed from the table's `rls_enabled`.
//!
//! [`diff_independent`] handles *schema-level* (independent) policies — those
//! linked to a table that is **absent** from the snapshot (drizzle's
//! `pgPolicy(...).link(<table not in the schema>)`, emitted as
//! `create_ind_policy` / `drop_ind_policy` / `alter_ind_policy` /
//! `rename_ind_policy`). They live in [`SchemaSnapshot::ind_policies`] rather than
//! inside a `SnapTable`, render with an *explicit* `"schema"."table"` target (even
//! for `public`), and never toggle the absent table's RLS.

use std::collections::BTreeSet;

use crate::differ::ir::{SchemaSnapshot, SnapTable};
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

/// Diff the schema-level (independent) policies — those keyed in
/// [`SchemaSnapshot::ind_policies`] because their target table is not present in
/// the snapshot. Mirrors the per-table [`diff`] (rename → create/alter/recreate →
/// drop) but emits the `*IndPolicy` statement variants (explicit `"schema"."table"`
/// target) and never touches RLS on the absent table.
///
/// Ordering is via dedicated `Plan` buckets that sit immediately after their
/// in-table counterparts: a mixed diff therefore emits table-policy creates before
/// independent-policy creates (matching drizzle), and an `AS`/`FOR` change still
/// drops (teardown phase) before it recreates (build phase).
pub fn diff_independent(
    plan: &mut Plan,
    from: &SchemaSnapshot,
    to: &SchemaSnapshot,
    hints: &RenameHints,
) {
    let mut from_consumed: BTreeSet<String> = BTreeSet::new();
    let mut to_consumed: BTreeSet<String> = BTreeSet::new();

    // Renames first. Independent-policy renames carry the `ind_policy:` tag so
    // they never collide with the table-scoped (untagged arity-3) column form.
    for (key, from_ip) in &from.ind_policies {
        let from_name = &from_ip.policy.name;
        if let Some(r) = hints.find_ind_policy_rename(&from_ip.schema, &from_ip.table, from_name) {
            let to_key = format!("{}.{}.{}", from_ip.schema, from_ip.table, r.to);
            if to.ind_policies.contains_key(&to_key) {
                plan.rename_ind_policies
                    .push(DdlStatement::RenameIndPolicy {
                        schema: from_ip.schema.clone(),
                        table: from_ip.table.clone(),
                        from: from_name.clone(),
                        to: r.to.clone(),
                    });
                from_consumed.insert(key.clone());
                to_consumed.insert(to_key);
            }
        }
    }

    // Created / altered / recreated.
    for (key, to_ip) in &to.ind_policies {
        if to_consumed.contains(key) {
            continue;
        }
        match from.ind_policies.get(key) {
            None => plan
                .create_ind_policies
                .push(DdlStatement::CreateIndPolicy {
                    schema: to_ip.schema.clone(),
                    table: to_ip.table.clone(),
                    policy: to_ip.policy.clone(),
                }),
            Some(from_ip) => {
                let fp = &from_ip.policy;
                let tp = &to_ip.policy;
                if fp.as_ != tp.as_ || fp.for_ != tp.for_ {
                    plan.drop_ind_policies.push(DdlStatement::DropIndPolicy {
                        schema: to_ip.schema.clone(),
                        table: to_ip.table.clone(),
                        name: tp.name.clone(),
                    });
                    plan.create_ind_policies
                        .push(DdlStatement::CreateIndPolicy {
                            schema: to_ip.schema.clone(),
                            table: to_ip.table.clone(),
                            policy: tp.clone(),
                        });
                } else if fp.to != tp.to || fp.using != tp.using || fp.with_check != tp.with_check {
                    plan.alter_ind_policies.push(DdlStatement::AlterIndPolicy {
                        schema: to_ip.schema.clone(),
                        table: to_ip.table.clone(),
                        policy: tp.clone(),
                    });
                }
            }
        }
    }

    // Dropped.
    for (key, from_ip) in &from.ind_policies {
        if from_consumed.contains(key) {
            continue;
        }
        if !to.ind_policies.contains_key(key) {
            plan.drop_ind_policies.push(DdlStatement::DropIndPolicy {
                schema: from_ip.schema.clone(),
                table: from_ip.table.clone(),
                name: from_ip.policy.name.clone(),
            });
        }
    }
}
