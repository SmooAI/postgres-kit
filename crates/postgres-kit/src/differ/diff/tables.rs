//! Table-level pass: classify each table as created / dropped / renamed / matched,
//! emit the table statement, and drive the per-table sub-diffs (columns,
//! constraints, foreign keys, indexes, policies) for tables present on both sides.

use std::collections::{BTreeMap, BTreeSet};

use crate::differ::ir::{SchemaSnapshot, SnapTable};
use crate::differ::renames::RenameHints;
use crate::differ::statement::DdlStatement;

use super::{columns, constraints, enums, fks, indexes, policies, Plan};

pub fn diff(plan: &mut Plan, from: &SchemaSnapshot, to: &SchemaSnapshot, hints: &RenameHints) {
    let mut consumed_from: BTreeSet<String> = BTreeSet::new();
    // Cross-category context: enums being rebuilt own their dependent columns'
    // type/default rewrites; renamed enums are followed automatically by their
    // dependent columns. Both suppress otherwise-spurious column statements.
    let recreating = enums::recreating_enums(from, to, hints);
    let renamed = enums::renamed_enums(from, to, hints);
    // Bare enum names present on each side, plus the `to`-side schema for each so a
    // column whose type changes to/from an enum can emit a schema-qualified
    // `SET DATA TYPE ... USING` cast.
    let enums_from: BTreeSet<String> = from.enums.values().map(|e| e.name.clone()).collect();
    let enums_to: BTreeMap<String, String> = to
        .enums
        .values()
        .map(|e| (e.name.clone(), e.schema.clone()))
        .collect();

    for (to_key, to_t) in &to.tables {
        if let Some(from_t) = from.tables.get(to_key) {
            // Matched in place.
            alter_existing(
                plan,
                from_t,
                to_t,
                hints,
                &recreating,
                &renamed,
                &enums_from,
                &enums_to,
            );
            consumed_from.insert(to_key.clone());
            continue;
        }
        // Renamed? A relation hint whose target is this table, whose source is a
        // table present in `from`.
        if let Some(r) = super::rename_by_target(hints, &to_t.schema, &to_t.name) {
            let from_key = format!("{}.{}", r.from_schema, r.from);
            if let Some(from_t) = from.tables.get(&from_key) {
                if r.from_schema != r.to_schema {
                    plan.rename_tables.push(DdlStatement::AlterTableSetSchema {
                        name: r.from.clone(),
                        from_schema: r.from_schema.clone(),
                        to_schema: r.to_schema.clone(),
                    });
                }
                if r.from != r.to {
                    plan.rename_tables.push(DdlStatement::RenameTable {
                        schema: r.to_schema.clone(),
                        from: r.from.clone(),
                        to: r.to.clone(),
                    });
                }
                alter_existing(
                    plan,
                    from_t,
                    to_t,
                    hints,
                    &recreating,
                    &renamed,
                    &enums_from,
                    &enums_to,
                );
                consumed_from.insert(from_key);
                continue;
            }
        }
        // Created.
        create_table(plan, to_t);
    }

    for (from_key, from_t) in &from.tables {
        if consumed_from.contains(from_key) {
            continue;
        }
        if !to.tables.contains_key(from_key) {
            drop_table(plan, from_t);
        }
    }
}

/// Drive all sub-object diffs for a table present on both sides (post-rename the
/// effective name is `to_t`'s).
#[allow(clippy::too_many_arguments)]
fn alter_existing(
    plan: &mut Plan,
    from_t: &SnapTable,
    to_t: &SnapTable,
    hints: &RenameHints,
    recreating_enums: &BTreeSet<String>,
    renamed_enums: &BTreeMap<String, String>,
    enums_from: &BTreeSet<String>,
    enums_to: &BTreeMap<String, String>,
) {
    columns::diff(
        plan,
        from_t,
        to_t,
        hints,
        recreating_enums,
        renamed_enums,
        enums_from,
        enums_to,
    );
    constraints::diff(plan, from_t, to_t);
    fks::diff(plan, from_t, to_t);
    indexes::diff(plan, from_t, to_t);
    policies::diff(plan, from_t, to_t, hints);
}

/// Emit `CREATE TABLE` plus the out-of-line objects created separately
/// (RLS toggle, policies, foreign keys, indexes). Columns, composite PKs, unique
/// and check constraints are rendered inline by `CREATE TABLE`.
fn create_table(plan: &mut Plan, t: &SnapTable) {
    plan.create_tables
        .push(DdlStatement::CreateTable(t.clone()));

    if t.rls_enabled {
        plan.enable_rls.push(DdlStatement::EnableRls {
            schema: t.schema.clone(),
            table: t.name.clone(),
        });
    }
    for fk in t.foreign_keys_ordered() {
        plan.add_foreign_keys.push(DdlStatement::CreateForeignKey {
            schema: t.schema.clone(),
            table: t.name.clone(),
            fk: fk.clone(),
        });
    }
    for idx in t.indexes_ordered() {
        plan.create_indexes.push(DdlStatement::CreateIndex {
            schema: t.schema.clone(),
            table: t.name.clone(),
            index: idx.clone(),
        });
    }
    for policy in t.policies_ordered() {
        plan.create_policies.push(DdlStatement::CreatePolicy {
            schema: t.schema.clone(),
            table: t.name.clone(),
            policy: policy.clone(),
        });
    }
}

/// Drop a table: its policies first (CASCADE on the table handles the rest).
fn drop_table(plan: &mut Plan, t: &SnapTable) {
    for policy in t.policies_ordered() {
        plan.drop_policies.push(DdlStatement::DropPolicy {
            schema: t.schema.clone(),
            table: t.name.clone(),
            name: policy.name.clone(),
        });
    }
    plan.drop_tables.push(DdlStatement::DropTable {
        schema: t.schema.clone(),
        name: t.name.clone(),
    });
}
