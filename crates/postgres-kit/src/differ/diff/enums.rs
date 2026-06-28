//! Enum-type pass. Renames/moves first (untagged arity-2 hints, which may change
//! schema). For matched types, value changes are either *additive* (`ALTER TYPE
//! ADD VALUE`, positioned `BEFORE` the following surviving value) or, when a value
//! is removed or reordered, a **recreate**: `DROP TYPE` + `CREATE TYPE`, plus a
//! cascade over every dependent column (Postgres can't drop/reorder enum values in
//! place). The cascade detours each dependent column through `text` and back:
//!
//! 1. `SET DATA TYPE text` (and, if defaulted, `SET DEFAULT '..'::text`)
//! 2. `DROP TYPE` then `CREATE TYPE` with the new value list
//! 3. (if defaulted) `SET DEFAULT '..'::"schema"."enum"[dims]`
//! 4. `SET DATA TYPE "schema"."enum"[dims] USING col::"schema"."enum"[dims]`

use std::collections::{BTreeMap, BTreeSet};

use crate::differ::ir::{SchemaSnapshot, SnapEnum};
use crate::differ::renames::RenameHints;
use crate::differ::statement::DdlStatement;
use crate::safety::quote_identifier;

use super::Plan;

pub fn diff(plan: &mut Plan, from: &SchemaSnapshot, to: &SchemaSnapshot, hints: &RenameHints) {
    let mut consumed_from: BTreeSet<String> = BTreeSet::new();

    for (to_key, to_e) in &to.enums {
        if let Some(from_e) = from.enums.get(to_key) {
            values_diff(plan, to, from_e, to_e);
            consumed_from.insert(to_key.clone());
            continue;
        }
        if let Some(r) = super::rename_by_target(hints, &to_e.schema, &to_e.name) {
            let from_key = format!("{}.{}", r.from_schema, r.from);
            if let Some(from_e) = from.enums.get(&from_key) {
                if r.from_schema != r.to_schema {
                    plan.enum_set_schema.push(DdlStatement::AlterEnumSetSchema {
                        name: r.from.clone(),
                        from_schema: r.from_schema.clone(),
                        to_schema: r.to_schema.clone(),
                    });
                }
                if r.from != r.to {
                    plan.rename_enums.push(DdlStatement::RenameEnum {
                        schema: r.to_schema.clone(),
                        from: r.from.clone(),
                        to: r.to.clone(),
                    });
                }
                values_diff(plan, to, from_e, to_e);
                consumed_from.insert(from_key);
                continue;
            }
        }
        plan.create_enums
            .push(DdlStatement::CreateEnum(to_e.clone()));
    }

    for (from_key, from_e) in &from.enums {
        if consumed_from.contains(from_key) || to.enums.contains_key(from_key) {
            continue;
        }
        plan.drop_enums.push(DdlStatement::DropEnum {
            schema: from_e.schema.clone(),
            name: from_e.name.clone(),
        });
    }
}

/// Bare names of every enum that will be *recreated* (a value removed or
/// reordered) in this diff. The [`super::columns`] pass uses this to defer a
/// dependent column's type/default change to the enum recreate cascade, which
/// already rewrites that column end to end.
pub(super) fn recreating_enums(
    from: &SchemaSnapshot,
    to: &SchemaSnapshot,
    hints: &RenameHints,
) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for (to_key, to_e) in &to.enums {
        let from_e = from.enums.get(to_key).or_else(|| {
            super::rename_by_target(hints, &to_e.schema, &to_e.name)
                .and_then(|r| from.enums.get(&format!("{}.{}", r.from_schema, r.from)))
        });
        if let Some(from_e) = from_e {
            if from_e.values != to_e.values && !is_additive(&from_e.values, &to_e.values) {
                set.insert(to_e.name.clone());
            }
        }
    }
    set
}

/// Map of `old bare name -> new bare name` for every enum that is *renamed* in
/// this diff. A column typed on a renamed enum follows the rename automatically in
/// Postgres, so the [`super::columns`] pass must not emit a spurious type change.
pub(super) fn renamed_enums(
    from: &SchemaSnapshot,
    to: &SchemaSnapshot,
    hints: &RenameHints,
) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for to_e in to.enums.values() {
        if from.enums.contains_key(&to_e.key()) {
            continue;
        }
        if let Some(r) = super::rename_by_target(hints, &to_e.schema, &to_e.name) {
            let from_key = format!("{}.{}", r.from_schema, r.from);
            if from.enums.contains_key(&from_key) && r.from != r.to {
                map.insert(r.from.clone(), r.to.clone());
            }
        }
    }
    map
}

/// A column that uses an enum type, with its array dimensions (e.g. `[3][2]`) and
/// optional default, captured for the recreate cascade.
struct DepColumn {
    table_schema: String,
    table_name: String,
    column: String,
    dims: String,
    default: Option<String>,
}

/// Emit the value changes between two matched enum types (`to_e` carries the final
/// schema/name post-move).
fn values_diff(plan: &mut Plan, to: &SchemaSnapshot, from_e: &SnapEnum, to_e: &SnapEnum) {
    if from_e.values == to_e.values {
        return;
    }
    if is_additive(&from_e.values, &to_e.values) {
        for (i, v) in to_e.values.iter().enumerate() {
            if from_e.values.contains(v) {
                continue;
            }
            let before = to_e.values[i + 1..]
                .iter()
                .find(|x| from_e.values.contains(*x))
                .cloned();
            plan.add_enum_values.push(DdlStatement::AddEnumValue {
                schema: to_e.schema.clone(),
                name: to_e.name.clone(),
                value: v.clone(),
                before,
            });
        }
    } else {
        recreate(plan, to, to_e);
    }
}

/// A value was removed or reordered: rebuild the type, cascading through every
/// dependent column.
fn recreate(plan: &mut Plan, to: &SchemaSnapshot, to_e: &SnapEnum) {
    let deps = collect_dep_columns(to, &to_e.name);
    let qenum = format!(
        "{}.{}",
        quote_identifier(&to_e.schema),
        quote_identifier(&to_e.name)
    );

    // 1. Detour each dependent column through `text` (default first cast to text).
    for d in &deps {
        plan.recreate_enums.push(DdlStatement::SetColumnType {
            schema: d.table_schema.clone(),
            table: d.table_name.clone(),
            column: d.column.clone(),
            ty: "text".to_string(),
        });
        if let Some(def) = &d.default {
            plan.recreate_enums.push(DdlStatement::SetColumnDefault {
                schema: d.table_schema.clone(),
                table: d.table_name.clone(),
                column: d.column.clone(),
                default: format!("{}::text", sql_literal(def)),
            });
        }
    }

    // 2. Rebuild the type.
    plan.recreate_enums.push(DdlStatement::DropEnum {
        schema: to_e.schema.clone(),
        name: to_e.name.clone(),
    });
    plan.recreate_enums
        .push(DdlStatement::CreateEnum(to_e.clone()));

    // 3/4. Repoint each dependent column back at the rebuilt type.
    for d in &deps {
        let qty = format!("{qenum}{}", d.dims);
        if let Some(def) = &d.default {
            plan.recreate_enums.push(DdlStatement::SetColumnDefault {
                schema: d.table_schema.clone(),
                table: d.table_name.clone(),
                column: d.column.clone(),
                default: format!("{}::{qty}", sql_literal(def)),
            });
        }
        plan.recreate_enums.push(DdlStatement::SetColumnTypeUsing {
            schema: d.table_schema.clone(),
            table: d.table_name.clone(),
            column: d.column.clone(),
            ty: qty.clone(),
            using: format!("{}::{qty}", quote_identifier(&d.column)),
        });
    }
}

/// Scan every table for columns whose (possibly array) type references `enum_name`.
///
/// The cascade walks dependent columns in schema-declaration order, which puts
/// the `public`-schema tables ahead of objects in named schemas. `to.tables` is a
/// [`BTreeMap`] keyed by `schema.name`, which would otherwise float a `new_schema`
/// table ahead of `public` (`'n' < 'p'`); a final stable sort restores the
/// `public`-first grouping while keeping each table's own columns in `position`
/// order.
fn collect_dep_columns(to: &SchemaSnapshot, enum_name: &str) -> Vec<DepColumn> {
    let mut deps: Vec<DepColumn> = Vec::new();
    for table in to.tables.values() {
        let mut cols: Vec<_> = table.columns.values().collect();
        cols.sort_by_key(|c| c.position);
        for col in cols {
            let (base, dims) = split_array_type(&col.ty);
            if base == enum_name {
                deps.push(DepColumn {
                    table_schema: table.schema.clone(),
                    table_name: table.name.clone(),
                    column: col.name.clone(),
                    dims: dims.to_string(),
                    default: col.default.clone(),
                });
            }
        }
    }
    deps.sort_by(|a, b| {
        (a.table_schema != "public", &a.table_schema, &a.table_name).cmp(&(
            b.table_schema != "public",
            &b.table_schema,
            &b.table_name,
        ))
    });
    deps
}

/// Split a column type into its base name and array-dimension suffix, e.g.
/// `enum[3][2]` → `("enum", "[3][2]")`, `enum` → `("enum", "")`.
fn split_array_type(ty: &str) -> (&str, &str) {
    match ty.find('[') {
        Some(i) => (&ty[..i], &ty[i..]),
        None => (ty, ""),
    }
}

/// Render a verbatim default value as a single-quoted SQL string literal.
fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// `to` is `from` plus appended/inserted values, with every existing value kept in
/// its original relative order (so `ADD VALUE` suffices, no recreate needed).
fn is_additive(from: &[String], to: &[String]) -> bool {
    let surviving: Vec<&String> = to.iter().filter(|v| from.contains(v)).collect();
    surviving.len() == from.len() && surviving.iter().zip(from).all(|(a, b)| *a == b)
}
