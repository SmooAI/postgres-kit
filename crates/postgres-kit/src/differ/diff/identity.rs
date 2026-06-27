//! Identity-column helper (driven by the [`super::columns`] pass). Diffs a
//! column's `GENERATED { ALWAYS | BY DEFAULT } AS IDENTITY` declaration: add,
//! drop, or per-option alteration (`SET GENERATED ...`, `SET START WITH`, etc).
//!
//! A custom identity *sequence name* (drizzle's `{ name: 'custom_seq' }`) rides
//! on [`SnapIdentity::name`]; the add/drop paths below carry the whole
//! `SnapIdentity` through to the statement layer, which renders the custom name
//! inline (falling back to the Postgres-implicit `{table}_{column}_seq` when it
//! is `None`). Postgres has no in-place "rename the identity sequence" DDL, so a
//! pure name change on an existing identity is intentionally a no-op here (it is
//! not in drizzle-kit's `sqlStatements` either).

use crate::differ::ir::SnapColumn;
use crate::differ::statement::DdlStatement;

use super::Plan;

pub fn alter(
    plan: &mut Plan,
    schema: &str,
    table: &str,
    from_col: &SnapColumn,
    to_col: &SnapColumn,
) {
    match (&from_col.identity, &to_col.identity) {
        (None, None) => {}
        (None, Some(id)) => plan.alter_columns.push(DdlStatement::SetColumnIdentity {
            schema: schema.to_string(),
            table: table.to_string(),
            column: to_col.name.clone(),
            identity: id.clone(),
        }),
        (Some(_), None) => plan.alter_columns.push(DdlStatement::DropColumnIdentity {
            schema: schema.to_string(),
            table: table.to_string(),
            column: to_col.name.clone(),
        }),
        (Some(f), Some(t)) => {
            if f.kind != t.kind {
                plan.alter_columns
                    .push(DdlStatement::SetColumnIdentityGenerated {
                        schema: schema.to_string(),
                        table: table.to_string(),
                        column: to_col.name.clone(),
                        kind: t.kind,
                    });
            }
            let col = &to_col.name;
            push_opt(
                plan,
                schema,
                table,
                col,
                &f.increment,
                &t.increment,
                "INCREMENT BY",
            );
            push_opt(
                plan,
                schema,
                table,
                col,
                &f.min_value,
                &t.min_value,
                "MINVALUE",
            );
            push_opt(
                plan,
                schema,
                table,
                col,
                &f.max_value,
                &t.max_value,
                "MAXVALUE",
            );
            push_opt(
                plan,
                schema,
                table,
                col,
                &f.start_with,
                &t.start_with,
                "START WITH",
            );
            push_opt(plan, schema, table, col, &f.cache, &t.cache, "CACHE");
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_opt(
    plan: &mut Plan,
    schema: &str,
    table: &str,
    column: &str,
    from: &Option<String>,
    to: &Option<String>,
    keyword: &str,
) {
    if from != to {
        if let Some(v) = to {
            plan.alter_columns
                .push(DdlStatement::SetColumnIdentityOption {
                    schema: schema.to_string(),
                    table: table.to_string(),
                    column: column.to_string(),
                    clause: format!("{keyword} {v}"),
                });
        }
    }
}
