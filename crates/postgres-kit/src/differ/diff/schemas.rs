//! Schema pass: emit `CREATE SCHEMA` for every non-`public` schema present in
//! `to` but not `from`, and `DROP SCHEMA` for the reverse. The `public` schema is
//! implicit and never appears in `SchemaSnapshot::schemas`, so it is never
//! created or dropped here.
//!
//! Drops are intentionally conservative: a `DROP SCHEMA IF EXISTS` is emitted
//! (without `CASCADE`) only when a schema disappears entirely from the snapshot.
//! `CreateSchema` is ordered first of all (before any type/table); `DropSchema`
//! is ordered last of all (after every object that lived in it is gone).

use crate::differ::ir::SchemaSnapshot;
use crate::differ::statement::DdlStatement;

use super::Plan;

pub fn diff(plan: &mut Plan, from: &SchemaSnapshot, to: &SchemaSnapshot) {
    for name in &to.schemas {
        if !from.schemas.contains(name) {
            plan.create_schemas
                .push(DdlStatement::CreateSchema { name: name.clone() });
        }
    }
    for name in &from.schemas {
        if !to.schemas.contains(name) {
            plan.drop_schemas
                .push(DdlStatement::DropSchema { name: name.clone() });
        }
    }
}
