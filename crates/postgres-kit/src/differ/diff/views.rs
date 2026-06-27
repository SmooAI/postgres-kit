//! View pass. Renames/moves first (untagged arity-2 hints, which may change
//! schema). For a matched view:
//!
//! - A changed `.as` definition (or a change of materialized-ness) is a
//!   **drop + recreate** — the rebuilt `CREATE [MATERIALIZED] VIEW` carries every
//!   `WITH (...)` option, `USING`, `TABLESPACE` and `WITH NO DATA` clause, so no
//!   separate option alter is emitted.
//! - Otherwise the view stays in place and its `WITH (...)` options
//!   (`ALTER VIEW ... SET/RESET (...)`), `TABLESPACE`
//!   (`ALTER MATERIALIZED VIEW ... SET TABLESPACE ...`, dropping to `pg_default`)
//!   and access method (`SET ACCESS METHOD ...`, dropping to `heap`) are diffed
//!   in place, after any `SET SCHEMA` / `RENAME TO`.
//!
//! The drizzle `.existing()` flag (a reference to a view the kit does not manage)
//! is honored: a view that is `existing` in `to` never produces any view DDL
//! (creation/rename/move/option alters are all suppressed) — though a schema it
//! moves into is still created by the [`super::schemas`] pass. An `existing` view
//! that disappears is not dropped (it was never managed); an `existing -> real`
//! transition is a drop + recreate.

use std::collections::BTreeSet;

use crate::differ::ir::SchemaSnapshot;
use crate::differ::ir::SnapView;
use crate::differ::renames::RenameHints;
use crate::differ::statement::DdlStatement;

use super::Plan;

pub fn diff(plan: &mut Plan, from: &SchemaSnapshot, to: &SchemaSnapshot, hints: &RenameHints) {
    let mut consumed_from: BTreeSet<String> = BTreeSet::new();

    for (to_key, to_v) in &to.views {
        // A view that is `existing` in `to` is never managed: emit no view DDL,
        // but still consume any matching `from` view so it is not dropped.
        if to_v.existing {
            consume_match(from, hints, to_key, to_v, &mut consumed_from);
            continue;
        }

        // Matched by key (no rename).
        if let Some(from_v) = from.views.get(to_key) {
            diff_matched(plan, from_v, to_v);
            consumed_from.insert(to_key.clone());
            continue;
        }

        // Matched by rename/move hint.
        if let Some(r) = super::rename_by_target(hints, &to_v.schema, &to_v.name) {
            let from_key = format!("{}.{}", r.from_schema, r.from);
            if let Some(from_v) = from.views.get(&from_key) {
                diff_renamed(plan, from_v, to_v, r);
                consumed_from.insert(from_key);
                continue;
            }
        }

        // Brand-new view.
        plan.create_views
            .push(DdlStatement::CreateView(to_v.clone()));
    }

    for (from_key, from_v) in &from.views {
        if consumed_from.contains(from_key) || to.views.contains_key(from_key) {
            continue;
        }
        // An `existing` view was never managed by the kit, so it is never dropped.
        if from_v.existing {
            continue;
        }
        plan.drop_views.push(DdlStatement::DropView {
            schema: from_v.schema.clone(),
            name: from_v.name.clone(),
            materialized: from_v.materialized,
        });
    }
}

/// For an `existing` target view, mark its matching `from` view (by key, else by
/// rename hint) consumed so the drop pass leaves it alone — no DDL is emitted.
fn consume_match(
    from: &SchemaSnapshot,
    hints: &RenameHints,
    to_key: &str,
    to_v: &SnapView,
    consumed_from: &mut BTreeSet<String>,
) {
    if from.views.contains_key(to_key) {
        consumed_from.insert(to_key.to_string());
    } else if let Some(r) = super::rename_by_target(hints, &to_v.schema, &to_v.name) {
        let from_key = format!("{}.{}", r.from_schema, r.from);
        if from.views.contains_key(&from_key) {
            consumed_from.insert(from_key);
        }
    }
}

/// Diff a view matched by key (no schema/name change).
fn diff_matched(plan: &mut Plan, from_v: &SnapView, to_v: &SnapView) {
    // `existing -> real`, a changed definition, or a change of materialized-ness
    // all force a drop + recreate (the recreate carries every option clause).
    if from_v.existing
        || from_v.definition != to_v.definition
        || from_v.materialized != to_v.materialized
    {
        recreate(plan, from_v, to_v);
        return;
    }
    alter_in_place(
        plan,
        &to_v.schema,
        &to_v.name,
        to_v.materialized,
        from_v,
        to_v,
    );
}

/// Diff a view matched by a rename/move hint. The `WITH (...)` / tablespace /
/// access-method diff targets the *final* (post-move/rename) identity.
fn diff_renamed(
    plan: &mut Plan,
    from_v: &SnapView,
    to_v: &SnapView,
    r: &crate::differ::renames::TableRename,
) {
    // `existing -> real`, a changed definition, or a change of materialized-ness
    // collapse the move/rename into a drop + recreate.
    if from_v.existing
        || from_v.definition != to_v.definition
        || from_v.materialized != to_v.materialized
    {
        recreate(plan, from_v, to_v);
        return;
    }

    // SET SCHEMA (move) is ordered before RENAME, which is ordered before the
    // in-place option alters.
    if r.from_schema != r.to_schema {
        plan.view_set_schema.push(DdlStatement::AlterViewSetSchema {
            from_schema: r.from_schema.clone(),
            name: r.from.clone(),
            to_schema: r.to_schema.clone(),
            materialized: to_v.materialized,
        });
    }
    if r.from != r.to {
        plan.rename_views.push(DdlStatement::RenameView {
            schema: r.to_schema.clone(),
            from: r.from.clone(),
            to: r.to.clone(),
            materialized: to_v.materialized,
        });
    }
    alter_in_place(
        plan,
        &to_v.schema,
        &to_v.name,
        to_v.materialized,
        from_v,
        to_v,
    );
}

/// Drop the old view and create the new one (the create carries all options).
fn recreate(plan: &mut Plan, from_v: &SnapView, to_v: &SnapView) {
    plan.drop_views.push(DdlStatement::DropView {
        schema: from_v.schema.clone(),
        name: from_v.name.clone(),
        materialized: from_v.materialized,
    });
    plan.create_views
        .push(DdlStatement::CreateView(to_v.clone()));
}

/// Diff a kept view's `WITH (...)` options, tablespace and access method,
/// targeting `(schema, name)` (the final identity after any move/rename).
fn alter_in_place(
    plan: &mut Plan,
    schema: &str,
    name: &str,
    materialized: bool,
    from_v: &SnapView,
    to_v: &SnapView,
) {
    // Options: keys present-and-changed or newly added -> SET; keys removed ->
    // RESET. BTreeMap iteration keeps both lists deterministically sorted, which
    // matches drizzle's alphabetical option order.
    let mut set: Vec<(String, String)> = Vec::new();
    let mut reset: Vec<String> = Vec::new();
    for (k, v) in &to_v.with_options {
        if from_v.with_options.get(k) != Some(v) {
            set.push((k.clone(), v.clone()));
        }
    }
    for k in from_v.with_options.keys() {
        if !to_v.with_options.contains_key(k) {
            reset.push(k.clone());
        }
    }
    if !set.is_empty() {
        plan.alter_views.push(DdlStatement::AlterViewSetOptions {
            schema: schema.to_string(),
            name: name.to_string(),
            materialized,
            options: set,
        });
    }
    if !reset.is_empty() {
        plan.alter_views.push(DdlStatement::AlterViewResetOptions {
            schema: schema.to_string(),
            name: name.to_string(),
            materialized,
            keys: reset,
        });
    }

    // Tablespace: a removed tablespace resets to `pg_default` (materialized only).
    if from_v.tablespace != to_v.tablespace {
        plan.alter_views.push(DdlStatement::AlterViewSetTablespace {
            schema: schema.to_string(),
            name: name.to_string(),
            tablespace: to_v
                .tablespace
                .clone()
                .unwrap_or_else(|| "pg_default".to_string()),
        });
    }

    // Access method: a removed access method resets to `heap` (materialized only).
    if from_v.using != to_v.using {
        plan.alter_views
            .push(DdlStatement::AlterViewSetAccessMethod {
                schema: schema.to_string(),
                name: name.to_string(),
                using: to_v.using.clone().unwrap_or_else(|| "heap".to_string()),
            });
    }
}
