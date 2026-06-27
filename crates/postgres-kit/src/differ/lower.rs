//! Lowering: turn the authoring DSL ([`crate::spec`]) into the diffable
//! [`SchemaSnapshot`] IR. This is the bridge the differ consumes — author in the
//! ergonomic DSL, lower to the normalized form, diff two normalized forms.

use crate::differ::ir::*;
use crate::spec::{EnumTypeSpec, PgTableSpec, RoleSpec, SequenceSpec, ViewSpec};

/// Lower a set of [`PgTableSpec`]s into a [`SchemaSnapshot`] (tables only).
/// Top-level enums/views/sequences/roles are empty; use [`lower`] to include them.
/// Every non-`public` schema referenced by a table is collected into
/// [`SchemaSnapshot::schemas`] so the differ can emit `CREATE SCHEMA`.
pub fn lower_tables(tables: &[PgTableSpec]) -> SchemaSnapshot {
    let mut builder = SchemaSnapshot::builder();
    for table in tables {
        builder = builder.schema(table.schema.clone());
        builder = builder.table(lower_table(table));
    }
    builder.build()
}

/// Lower a full schema — tables plus top-level enums, views, sequences, roles.
/// Every distinct non-`public` schema referenced by a table, enum, view, or
/// sequence is collected into [`SchemaSnapshot::schemas`] so the differ emits a
/// `CREATE SCHEMA` for it first. (Roles are cluster-global, not schema-scoped.)
pub fn lower(
    tables: &[PgTableSpec],
    enums: &[EnumTypeSpec],
    views: &[ViewSpec],
    sequences: &[SequenceSpec],
    roles: &[RoleSpec],
) -> SchemaSnapshot {
    let mut builder = SchemaSnapshot::builder();
    for table in tables {
        builder = builder.schema(table.schema.clone());
        builder = builder.table(lower_table(table));
    }
    for e in enums {
        builder = builder.schema(e.schema.clone());
        builder = builder.enum_type(lower_enum(e));
    }
    for v in views {
        builder = builder.schema(v.schema.clone());
        builder = builder.view(lower_view(v));
    }
    for s in sequences {
        builder = builder.schema(s.schema.clone());
        builder = builder.sequence(lower_sequence(s));
    }
    for r in roles {
        builder = builder.role(lower_role(r));
    }
    builder.build()
}

/// Lower a single table spec.
pub fn lower_table(table: &PgTableSpec) -> SnapTable {
    let single_pk: Option<&String> = if table.primary_key.len() == 1 {
        table.primary_key.first()
    } else {
        None
    };

    let mut snap = SnapTable::new(table.qualified_name());

    for col in &table.columns {
        let is_pk = single_pk.is_some_and(|pk| pk == &col.name);
        snap = snap.col(lower_column(col, is_pk));
    }

    // A multi-column PK becomes a composite-PK construct.
    if table.primary_key.len() > 1 {
        let name = format!("{}_pkey", table.name);
        snap = snap.composite_pk(SnapCompositePk::new(name, table.primary_key.clone()));
    }

    for fk in &table.foreign_keys {
        let mut snap_fk = SnapForeignKey::new(
            fk.name.clone(),
            fk.columns_from.clone(),
            fk.table_to.clone(),
            fk.columns_to.clone(),
        );
        if let Some(a) = fk.on_update {
            snap_fk = snap_fk.on_update(a.to_sql());
        }
        if let Some(a) = fk.on_delete {
            snap_fk = snap_fk.on_delete(a.to_sql());
        }
        snap = snap.foreign_key(snap_fk);
    }

    for uc in &table.unique_constraints {
        let mut snap_uc = SnapUnique::new(uc.name.clone(), uc.columns.clone());
        if uc.nulls_not_distinct {
            snap_uc = snap_uc.nulls_not_distinct();
        }
        snap = snap.unique(snap_uc);
    }

    for cc in &table.check_constraints {
        snap = snap.check(SnapCheck::new(cc.name.clone(), cc.value.clone()));
    }

    for idx in &table.indexes {
        let cols = idx.columns.iter().map(|c| {
            let mut sc = if c.is_expression {
                SnapIndexColumn::expr(c.expression.clone())
            } else {
                SnapIndexColumn::column(c.expression.clone())
            };
            if !c.asc {
                sc = sc.desc();
            }
            if let Some(n) = &c.nulls {
                sc = sc.nulls(n.clone());
            }
            if let Some(o) = &c.opclass {
                sc = sc.opclass(o.clone());
            }
            sc
        });
        let mut snap_idx = SnapIndex::new(idx.name.clone(), cols).method(idx.method.clone());
        if idx.unique {
            snap_idx = snap_idx.unique();
        }
        if let Some(w) = &idx.where_clause {
            snap_idx = snap_idx.where_clause(w.clone());
        }
        snap = snap.index(snap_idx);
    }

    for p in &table.policies {
        let mut snap_p = SnapPolicy::new(p.name.clone());
        if let Some(a) = p.as_ {
            snap_p = snap_p.as_permissiveness(a);
        }
        if let Some(f) = p.for_ {
            snap_p = snap_p.for_command(f);
        }
        if !p.to.is_empty() {
            snap_p = snap_p.to_roles(p.to.clone());
        }
        if let Some(u) = &p.using {
            snap_p = snap_p.using(u.clone());
        }
        if let Some(w) = &p.with_check {
            snap_p = snap_p.with_check(w.clone());
        }
        snap = snap.policy(snap_p);
    }

    snap.rls(table.rls_enabled)
}

fn lower_column(col: &crate::spec::ColumnSpec, is_pk: bool) -> SnapColumn {
    let mut sc = SnapColumn::new(col.name.clone(), col.ty.to_sql_type());
    if !col.nullable {
        sc = sc.not_null();
    }
    if is_pk {
        sc = sc.primary_key();
    }
    if let Some(d) = &col.default {
        sc = sc.default(d.clone());
    }
    if let Some(g) = &col.generated {
        sc = sc.generated(g.expression.clone());
    }
    if let Some(id) = &col.identity {
        sc = sc.identity(SnapIdentity {
            kind: id.kind,
            name: None,
            increment: id.sequence.increment.clone(),
            min_value: id.sequence.min_value.clone(),
            max_value: id.sequence.max_value.clone(),
            start_with: id.sequence.start_with.clone(),
            cache: id.sequence.cache.clone(),
            cycle: id.sequence.cycle,
        });
    }
    if let Some(u) = &col.unique {
        sc.unique = Some(SnapColumnUnique {
            name: u.name.clone(),
            nulls_not_distinct: u.nulls_not_distinct,
        });
    }
    sc
}

fn lower_enum(e: &EnumTypeSpec) -> SnapEnum {
    SnapEnum::new(e.qualified_name(), e.values.clone())
}

fn lower_view(v: &ViewSpec) -> SnapView {
    let mut sv = SnapView {
        schema: v.schema.clone(),
        name: v.name.clone(),
        definition: v.definition.clone(),
        materialized: false,
        existing: false,
        with_options: std::collections::BTreeMap::new(),
        tablespace: None,
        using: None,
        with_no_data: false,
    };
    if v.materialized {
        sv = sv.materialized();
    }
    sv
}

fn lower_sequence(s: &SequenceSpec) -> SnapSequence {
    SnapSequence {
        schema: s.schema.clone(),
        name: s.name.clone(),
        increment: s.options.increment.clone(),
        min_value: s.options.min_value.clone(),
        max_value: s.options.max_value.clone(),
        start_with: s.options.start_with.clone(),
        cache: s.options.cache.clone(),
        cycle: s.options.cycle,
    }
}

fn lower_role(r: &RoleSpec) -> SnapRole {
    SnapRole::new(r.name.clone())
        .create_db(r.create_db)
        .create_role(r.create_role)
        .inherit(r.inherit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{ColumnSpec, ForeignKeySpec, PgType, ReferentialAction};

    #[test]
    fn single_pk_lands_on_column() {
        let t = PgTableSpec::new("users", vec![ColumnSpec::new("id", PgType::Uuid)])
            .primary_key(["id"]);
        let snap = lower_table(&t);
        assert!(snap.columns.get("id").unwrap().primary_key);
        assert!(snap.composite_primary_keys.is_empty());
    }

    #[test]
    fn multi_pk_becomes_composite() {
        let t = PgTableSpec::new(
            "members",
            vec![
                ColumnSpec::new("org_id", PgType::Uuid),
                ColumnSpec::new("user_id", PgType::Uuid),
            ],
        )
        .primary_key(["org_id", "user_id"]);
        let snap = lower_table(&t);
        assert!(!snap.columns.get("org_id").unwrap().primary_key);
        let pk = snap.composite_primary_keys.get("members_pkey").unwrap();
        assert_eq!(pk.columns, vec!["org_id", "user_id"]);
    }

    #[test]
    fn fk_actions_lower_to_sql_keywords() {
        let t = PgTableSpec::new("t", vec![ColumnSpec::new("org_id", PgType::Uuid)]).foreign_key(
            ForeignKeySpec::new("fk", ["org_id"], "public.orgs", ["id"])
                .on_delete(ReferentialAction::Cascade),
        );
        let snap = lower_table(&t);
        assert_eq!(
            snap.foreign_keys.get("fk").unwrap().on_delete.as_deref(),
            Some("CASCADE")
        );
    }

    #[test]
    fn lower_collects_non_public_schemas() {
        let snap = lower(
            &[
                PgTableSpec::new("stores", vec![ColumnSpec::new("id", PgType::Uuid)])
                    .in_schema("rpm_pizza"),
                PgTableSpec::new("widgets", vec![ColumnSpec::new("id", PgType::Uuid)]),
            ],
            &[EnumTypeSpec::new("task_category", ["a", "b"]).in_schema("rpm_pizza")],
            &[],
            &[],
            &[],
        );
        // Only the non-public schema is collected; public stays implicit.
        assert_eq!(snap.schemas.len(), 1);
        assert!(snap.schemas.contains("rpm_pizza"));

        // lower_tables collects table schemas too.
        let snap2 =
            lower_tables(&[
                PgTableSpec::new("t", vec![ColumnSpec::new("id", PgType::Uuid)])
                    .in_schema("rpm_pizza"),
            ]);
        assert!(snap2.schemas.contains("rpm_pizza"));
    }

    #[test]
    fn lower_full_schema_collects_top_level() {
        let snap = lower(
            &[PgTableSpec::new(
                "t",
                vec![ColumnSpec::new("a", PgType::Int4)],
            )],
            &[EnumTypeSpec::new("role", ["a", "b"])],
            &[ViewSpec::new("v", "SELECT 1")],
            &[SequenceSpec::new("s")],
            &[RoleSpec::new("app")],
        );
        assert!(snap.tables.contains_key("public.t"));
        assert!(snap.enums.contains_key("public.role"));
        assert!(snap.views.contains_key("public.v"));
        assert!(snap.sequences.contains_key("public.s"));
        assert!(snap.roles.contains_key("app"));
    }
}
