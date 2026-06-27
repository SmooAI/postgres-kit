//! Column pass: renames first, then adds, drops, and per-column alterations
//! (type / default / not-null / generated / identity). Generated-expression
//! changes drop-and-readd the column (drizzle's lowercase `drop column` + `ADD
//! COLUMN` pair); dropping a generated expression is done in place.

use std::collections::{BTreeMap, BTreeSet};

use crate::differ::ir::{SnapColumn, SnapTable};
use crate::differ::renames::RenameHints;
use crate::differ::statement::DdlStatement;

use super::{identity, Plan};

#[allow(clippy::too_many_arguments)]
pub fn diff(
    plan: &mut Plan,
    from_t: &SnapTable,
    to_t: &SnapTable,
    hints: &RenameHints,
    recreating_enums: &BTreeSet<String>,
    renamed_enums: &BTreeMap<String, String>,
) {
    let schema = &to_t.schema;
    let table = &to_t.name;

    let mut from_consumed: BTreeSet<String> = BTreeSet::new();
    let mut to_consumed: BTreeSet<String> = BTreeSet::new();
    let mut pairs: Vec<(&SnapColumn, &SnapColumn)> = Vec::new();

    // Renames first.
    for (name, from_col) in &from_t.columns {
        if let Some(r) = hints.find_column_rename(schema, table, name) {
            if let Some(to_col) = to_t.columns.get(&r.to) {
                plan.rename_columns.push(DdlStatement::RenameColumn {
                    schema: schema.clone(),
                    table: table.clone(),
                    from: name.clone(),
                    to: r.to.clone(),
                });
                from_consumed.insert(name.clone());
                to_consumed.insert(r.to.clone());
                pairs.push((from_col, to_col));
            }
        }
    }

    // Adds + matched-by-name pairs.
    for (name, to_col) in &to_t.columns {
        if to_consumed.contains(name) {
            continue;
        }
        match from_t.columns.get(name) {
            Some(from_col) => pairs.push((from_col, to_col)),
            None => plan.add_columns.push(DdlStatement::AddColumn {
                schema: schema.clone(),
                table: table.clone(),
                column: to_col.clone(),
            }),
        }
    }

    // Drops.
    for name in from_t.columns.keys() {
        if from_consumed.contains(name) {
            continue;
        }
        if !to_t.columns.contains_key(name) {
            plan.drop_columns.push(DdlStatement::DropColumn {
                schema: schema.clone(),
                table: table.clone(),
                column: name.clone(),
            });
        }
    }

    for (from_col, to_col) in pairs {
        alter_column(
            plan,
            schema,
            table,
            from_col,
            to_col,
            recreating_enums,
            renamed_enums,
        );
    }
}

/// The base (non-array) portion of a column type, e.g. `enum[3][]` → `enum`.
fn base_type(ty: &str) -> &str {
    match ty.find('[') {
        Some(i) => &ty[..i],
        None => ty,
    }
}

#[allow(clippy::too_many_arguments)]
fn alter_column(
    plan: &mut Plan,
    schema: &str,
    table: &str,
    from_col: &SnapColumn,
    to_col: &SnapColumn,
    recreating_enums: &BTreeSet<String>,
    renamed_enums: &BTreeMap<String, String>,
) {
    // A column typed on an enum that is being rebuilt is fully rewritten (type
    // and default) by the enum recreate cascade — don't double-emit here.
    if recreating_enums.contains(base_type(&to_col.ty)) {
        return;
    }

    // A column typed on a renamed enum follows the rename automatically; its
    // type "change" (old base -> new base, same array dims) is not real DDL.
    let from_base = base_type(&from_col.ty);
    let to_base = base_type(&to_col.ty);
    let type_change_is_enum_rename = renamed_enums.get(from_base).map(String::as_str)
        == Some(to_base)
        && from_col.ty[from_base.len()..] == to_col.ty[to_base.len()..];
    // Generated-expression transitions.
    match (&from_col.generated, &to_col.generated) {
        (None, None) => {}
        (Some(_), None) => {
            plan.alter_columns.push(DdlStatement::DropColumnGenerated {
                schema: schema.to_string(),
                table: table.to_string(),
                column: to_col.name.clone(),
            });
        }
        (from_g, to_g) if from_g != to_g => {
            // Added a generated expression, or changed it: recreate the column.
            plan.recreate_columns
                .push(DdlStatement::DropColumnForRecreate {
                    schema: schema.to_string(),
                    table: table.to_string(),
                    column: to_col.name.clone(),
                });
            plan.recreate_columns.push(DdlStatement::AddColumn {
                schema: schema.to_string(),
                table: table.to_string(),
                column: to_col.clone(),
            });
            return;
        }
        _ => {}
    }

    if from_col.ty != to_col.ty && !type_change_is_enum_rename {
        plan.alter_columns.push(DdlStatement::SetColumnType {
            schema: schema.to_string(),
            table: table.to_string(),
            column: to_col.name.clone(),
            ty: to_col.ty.clone(),
        });
    }

    if from_col.default != to_col.default {
        match &to_col.default {
            Some(d) => plan.alter_columns.push(DdlStatement::SetColumnDefault {
                schema: schema.to_string(),
                table: table.to_string(),
                column: to_col.name.clone(),
                default: d.clone(),
            }),
            None => plan.alter_columns.push(DdlStatement::DropColumnDefault {
                schema: schema.to_string(),
                table: table.to_string(),
                column: to_col.name.clone(),
            }),
        }
    }

    // Not-null changes are governed by identity (identity columns are implicitly
    // NOT NULL); only diff the flag when neither side carries an identity.
    if from_col.identity.is_none()
        && to_col.identity.is_none()
        && from_col.not_null != to_col.not_null
    {
        if to_col.not_null {
            plan.alter_columns.push(DdlStatement::SetColumnNotNull {
                schema: schema.to_string(),
                table: table.to_string(),
                column: to_col.name.clone(),
            });
        } else {
            plan.alter_columns.push(DdlStatement::DropColumnNotNull {
                schema: schema.to_string(),
                table: table.to_string(),
                column: to_col.name.clone(),
            });
        }
    }

    identity::alter(plan, schema, table, from_col, to_col);
}
