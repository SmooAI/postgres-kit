//! The schema differ: compare two [`SchemaSnapshot`]s and emit the
//! [`DdlStatement`]s that migrate `from` into `to`.
//!
//! Authoring flow: write [`crate::spec::PgTableSpec`]s, [`lower`] them to a
//! [`SchemaSnapshot`], and [`diff`] the previous snapshot against the new one.
//!
//! For a from-scratch `CREATE` migration (incl. non-`public` schemas, with
//! optional raw-SQL injections), [`assemble_create_migration`] is the one-call
//! path; [`diff_with_raw_sql`] is the lower-level seam.

pub mod diff;
pub mod ir;
pub mod lower;
pub mod renames;
pub mod statement;

pub use ir::{
    SchemaSnapshot, SchemaSnapshotBuilder, SnapCheck, SnapColumn, SnapColumnUnique,
    SnapCompositePk, SnapEnum, SnapForeignKey, SnapIdentity, SnapIndex, SnapIndexColumn,
    SnapPolicy, SnapRole, SnapSequence, SnapTable, SnapUnique, SnapView,
};
pub use lower::{lower, lower_table, lower_tables};
pub use renames::{ColumnRename, EnumRename, PolicyRename, RenameHints, RoleRename, TableRename};
pub use statement::DdlStatement;

use crate::spec::{EnumTypeSpec, PgTableSpec};

/// Diff `from` into `to`, returning the ordered DDL statements that perform the
/// migration. `hints` resolves ambiguous drop+add pairs into renames/moves.
pub fn diff(from: &SchemaSnapshot, to: &SchemaSnapshot, hints: &RenameHints) -> Vec<DdlStatement> {
    diff::diff(from, to, hints)
}

/// Diff `from` into `to`, additionally splicing caller-supplied raw SQL into the
/// migration at the correct phase (after table/type/FK/index creation, before
/// RLS-enable and policy creation). See [`DdlStatement::RawSql`].
pub fn diff_with_raw_sql(
    from: &SchemaSnapshot,
    to: &SchemaSnapshot,
    hints: &RenameHints,
    raw_sql: &[String],
) -> Vec<DdlStatement> {
    diff::diff_with_raw_sql(from, to, hints, raw_sql)
}

/// Build a from-scratch `CREATE` migration for a slice of tables + enums, with
/// optional raw-SQL injections (e.g. `SECURITY DEFINER` functions, triggers,
/// grants). Lowers the specs to a `to` snapshot, diffs against an empty `from`,
/// and orders the result: `CREATE SCHEMA` → `CREATE TYPE` → `CREATE TABLE` →
/// foreign keys → indexes → **raw SQL** → `ENABLE ROW LEVEL SECURITY` → policies.
/// This is the one-call path for a downstream that wants to stand up a
/// non-`public` schema with the kit; pair it with
/// [`crate::write_drizzle_migration`] to emit the `.sql` file.
pub fn assemble_create_migration(
    tables: &[PgTableSpec],
    enums: &[EnumTypeSpec],
    extra_raw: &[String],
) -> Vec<DdlStatement> {
    let to = lower::lower(tables, enums, &[], &[], &[]);
    diff::diff_with_raw_sql(
        &SchemaSnapshot::default(),
        &to,
        &RenameHints::default(),
        extra_raw,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{ColumnSpec, PgTableSpec, PgType, PolicyFor, PolicySpec};

    #[test]
    fn empty_diff_is_noop() {
        let from = SchemaSnapshot::default();
        let to = SchemaSnapshot::default();
        let hints = RenameHints::default();
        assert!(diff(&from, &to, &hints).is_empty());
    }

    #[test]
    fn create_schema_is_emitted_first_for_a_non_public_table() {
        let to = lower(
            &[
                PgTableSpec::new("stores", vec![ColumnSpec::new("id", PgType::Uuid)])
                    .in_schema("rpm_pizza")
                    .primary_key(["id"]),
            ],
            &[],
            &[],
            &[],
            &[],
        );
        let stmts = diff(&SchemaSnapshot::default(), &to, &RenameHints::default());
        assert_eq!(
            stmts.first().map(DdlStatement::to_sql),
            Some(r#"CREATE SCHEMA IF NOT EXISTS "rpm_pizza";"#.to_string())
        );
    }

    #[test]
    fn public_only_schema_emits_no_create_schema() {
        let to = lower(
            &[
                PgTableSpec::new("widgets", vec![ColumnSpec::new("id", PgType::Uuid)])
                    .primary_key(["id"]),
            ],
            &[],
            &[],
            &[],
            &[],
        );
        let stmts = diff(&SchemaSnapshot::default(), &to, &RenameHints::default());
        assert!(stmts
            .iter()
            .all(|s| !matches!(s, DdlStatement::CreateSchema { .. })));
    }

    #[test]
    fn dropping_a_schema_emits_drop_schema_last() {
        let from = lower(
            &[
                PgTableSpec::new("stores", vec![ColumnSpec::new("id", PgType::Uuid)])
                    .in_schema("rpm_pizza")
                    .primary_key(["id"]),
            ],
            &[],
            &[],
            &[],
            &[],
        );
        let to = SchemaSnapshot::default();
        let stmts = diff(&from, &to, &RenameHints::default());
        assert_eq!(
            stmts.last().map(DdlStatement::to_sql),
            Some(r#"DROP SCHEMA IF EXISTS "rpm_pizza";"#.to_string())
        );
    }

    #[test]
    fn raw_sql_lands_after_tables_and_before_policies() {
        let table = PgTableSpec::new("docs", vec![ColumnSpec::new("id", PgType::Uuid)])
            .primary_key(["id"])
            .enable_rls()
            .policy(
                PolicySpec::new("p_select")
                    .for_command(PolicyFor::Select)
                    .using("is_manager(id)"),
            );
        let raw = vec![
            "CREATE FUNCTION is_manager(uuid) RETURNS boolean AS $$ SELECT true $$ LANGUAGE sql;"
                .to_string(),
        ];
        let stmts = assemble_create_migration(&[table], &[], &raw);

        let raw_idx = stmts
            .iter()
            .position(|s| matches!(s, DdlStatement::RawSql(_)))
            .expect("raw sql present");
        let table_idx = stmts
            .iter()
            .position(|s| matches!(s, DdlStatement::CreateTable(_)))
            .expect("create table present");
        let policy_idx = stmts
            .iter()
            .position(|s| matches!(s, DdlStatement::CreatePolicy { .. }))
            .expect("policy present");
        assert!(
            table_idx < raw_idx && raw_idx < policy_idx,
            "expected CREATE TABLE < RawSql < CREATE POLICY, got {table_idx} {raw_idx} {policy_idx}"
        );
    }
}
