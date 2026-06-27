//! Statement ordering. The differ fills a [`Plan`] of per-phase buckets; each
//! pass appends to the bucket for its statement kind, iterating the (already
//! deterministic, `BTreeMap`-keyed) snapshot so within-bucket order is stable.
//! [`Plan::assemble`] then concatenates the buckets in a single fixed phase order
//! that reproduces drizzle-kit's `sqlStatements` sequence:
//!
//! create types → enum moves/renames/recreate/add-values → sequences →
//! roles → disable RLS → drop policies → drop dependents (FK/index/constraints)
//! → drop tables → create tables → rename tables/columns → recreate/add/alter/
//! drop columns → add constraints (checks/uniques/PKs/FKs) → create indexes →
//! enable RLS → create/alter/rename policies → views → drop types.

use crate::differ::statement::DdlStatement;

/// Per-phase statement buckets. Passes append to the relevant field; the final
/// plan is [`Plan::assemble`]d into a single ordered statement list.
#[derive(Debug, Default)]
pub struct Plan {
    // enums (created first; dropped last)
    pub create_enums: Vec<DdlStatement>,
    pub enum_set_schema: Vec<DdlStatement>,
    pub rename_enums: Vec<DdlStatement>,
    pub recreate_enums: Vec<DdlStatement>,
    pub add_enum_values: Vec<DdlStatement>,

    // sequences
    pub create_sequences: Vec<DdlStatement>,
    pub seq_set_schema: Vec<DdlStatement>,
    pub rename_sequences: Vec<DdlStatement>,
    pub alter_sequences: Vec<DdlStatement>,
    pub drop_sequences: Vec<DdlStatement>,

    // roles
    pub rename_roles: Vec<DdlStatement>,
    pub drop_roles: Vec<DdlStatement>,
    pub create_roles: Vec<DdlStatement>,
    pub alter_roles: Vec<DdlStatement>,

    // teardown (drops) — RLS disabled before its policies are dropped, policies
    // before their table, dependents before tables
    pub disable_rls: Vec<DdlStatement>,
    pub drop_policies: Vec<DdlStatement>,
    pub drop_foreign_keys: Vec<DdlStatement>,
    pub drop_indexes: Vec<DdlStatement>,
    pub drop_checks: Vec<DdlStatement>,
    pub drop_uniques: Vec<DdlStatement>,
    pub drop_composite_pks: Vec<DdlStatement>,
    pub drop_tables: Vec<DdlStatement>,

    // build (creates / alters)
    pub create_tables: Vec<DdlStatement>,
    pub rename_tables: Vec<DdlStatement>,
    pub rename_columns: Vec<DdlStatement>,
    pub recreate_columns: Vec<DdlStatement>,
    pub add_columns: Vec<DdlStatement>,
    pub alter_columns: Vec<DdlStatement>,
    pub drop_columns: Vec<DdlStatement>,
    pub add_checks: Vec<DdlStatement>,
    pub add_uniques: Vec<DdlStatement>,
    pub add_composite_pks: Vec<DdlStatement>,
    pub add_foreign_keys: Vec<DdlStatement>,
    pub create_indexes: Vec<DdlStatement>,

    // RLS / policies (after tables exist)
    pub enable_rls: Vec<DdlStatement>,
    pub create_policies: Vec<DdlStatement>,
    pub alter_policies: Vec<DdlStatement>,
    pub rename_policies: Vec<DdlStatement>,

    // views
    pub create_views: Vec<DdlStatement>,
    pub drop_views: Vec<DdlStatement>,
    pub rename_views: Vec<DdlStatement>,

    // enums dropped last
    pub drop_enums: Vec<DdlStatement>,
}

impl Plan {
    /// Concatenate every bucket in the fixed phase order.
    pub fn assemble(self) -> Vec<DdlStatement> {
        let buckets: [Vec<DdlStatement>; 39] = [
            self.create_enums,
            self.enum_set_schema,
            self.rename_enums,
            self.recreate_enums,
            self.add_enum_values,
            self.create_sequences,
            self.seq_set_schema,
            self.rename_sequences,
            self.alter_sequences,
            self.drop_sequences,
            self.rename_roles,
            self.drop_roles,
            self.create_roles,
            self.alter_roles,
            self.disable_rls,
            self.drop_policies,
            self.drop_foreign_keys,
            self.drop_indexes,
            self.drop_checks,
            self.drop_uniques,
            self.drop_composite_pks,
            self.drop_tables,
            self.create_tables,
            self.rename_tables,
            self.rename_columns,
            self.recreate_columns,
            self.add_columns,
            self.alter_columns,
            self.drop_columns,
            self.add_checks,
            self.add_uniques,
            self.add_composite_pks,
            self.add_foreign_keys,
            self.create_indexes,
            self.enable_rls,
            self.create_policies,
            self.alter_policies,
            self.rename_policies,
            self.create_views,
        ];
        let mut out: Vec<DdlStatement> = Vec::new();
        for b in buckets {
            out.extend(b);
        }
        // drop_views / rename_views / drop_enums fold in after create_views.
        out.extend(self.drop_views);
        out.extend(self.rename_views);
        out.extend(self.drop_enums);
        out
    }
}
