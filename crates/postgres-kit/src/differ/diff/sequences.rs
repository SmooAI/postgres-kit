//! Sequence pass. Renames/moves first (untagged arity-2 hints; may change
//! schema). Matched sequences whose options differ are re-emitted with `ALTER
//! SEQUENCE`. New/removed sequences are created/dropped.

use std::collections::BTreeSet;

use crate::differ::ir::{SchemaSnapshot, SnapSequence};
use crate::differ::renames::RenameHints;
use crate::differ::statement::DdlStatement;

use super::Plan;

pub fn diff(plan: &mut Plan, from: &SchemaSnapshot, to: &SchemaSnapshot, hints: &RenameHints) {
    let mut consumed_from: BTreeSet<String> = BTreeSet::new();

    for (to_key, to_s) in &to.sequences {
        if let Some(from_s) = from.sequences.get(to_key) {
            if options_differ(from_s, to_s) {
                plan.alter_sequences
                    .push(DdlStatement::AlterSequence(to_s.clone()));
            }
            consumed_from.insert(to_key.clone());
            continue;
        }
        if let Some(r) = super::rename_by_target(hints, &to_s.schema, &to_s.name) {
            let from_key = format!("{}.{}", r.from_schema, r.from);
            if let Some(from_s) = from.sequences.get(&from_key) {
                if r.from_schema != r.to_schema {
                    plan.seq_set_schema
                        .push(DdlStatement::AlterSequenceSetSchema {
                            name: r.from.clone(),
                            from_schema: r.from_schema.clone(),
                            to_schema: r.to_schema.clone(),
                        });
                }
                if r.from != r.to {
                    plan.rename_sequences.push(DdlStatement::RenameSequence {
                        schema: r.to_schema.clone(),
                        from: r.from.clone(),
                        to: r.to.clone(),
                    });
                }
                if options_differ(from_s, to_s) {
                    plan.alter_sequences
                        .push(DdlStatement::AlterSequence(to_s.clone()));
                }
                consumed_from.insert(from_key);
                continue;
            }
        }
        plan.create_sequences
            .push(DdlStatement::CreateSequence(to_s.clone()));
    }

    for (from_key, from_s) in &from.sequences {
        if consumed_from.contains(from_key) || to.sequences.contains_key(from_key) {
            continue;
        }
        plan.drop_sequences.push(DdlStatement::DropSequence {
            schema: from_s.schema.clone(),
            name: from_s.name.clone(),
        });
    }
}

fn options_differ(a: &SnapSequence, b: &SnapSequence) -> bool {
    a.increment != b.increment
        || a.min_value != b.min_value
        || a.max_value != b.max_value
        || a.start_with != b.start_with
        || a.cache != b.cache
        || a.cycle != b.cycle
}
