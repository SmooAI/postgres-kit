//! End-to-end test for the `rpm_pizza` dogfood slice: build a non-`public`-schema
//! app with the kit and assert the generated migration is correctly ordered.
//!
//! This mirrors `examples/rpm_pizza_schema.rs` (the runnable how-to) and pins the
//! ordering contract the example relies on:
//!
//! `CREATE SCHEMA "rpm_pizza"` → `CREATE TYPE "rpm_pizza".*` → schema-qualified
//! `CREATE TABLE "rpm_pizza".*` → cross-schema FKs → RawSql function → policies
//! (org-scoped + the store-manager GM policy).

#![cfg(feature = "differ")]

use postgres_kit::differ::{assemble_create_migration, DdlStatement};
use postgres_kit::{
    ColumnSpec, EnumTypeSpec, ForeignKeySpec, PgTableSpec, PgType, PolicyFor, PolicySpec,
};

const SCHEMA: &str = "rpm_pizza";
const ORG_MEMBER: &str = "organization_id IN (SELECT organization_id FROM public.organization_members WHERE user_id = auth.uid())";

fn base_columns() -> Vec<ColumnSpec> {
    vec![
        ColumnSpec::new("id", PgType::Uuid).default_expr("gen_random_uuid()"),
        ColumnSpec::new("organization_id", PgType::Uuid),
        ColumnSpec::new("created_at", PgType::Timestamptz).default_expr("now()"),
        ColumnSpec::new("updated_at", PgType::Timestamptz).default_expr("now()"),
    ]
}

fn org_policies(table: &str) -> Vec<PolicySpec> {
    vec![
        PolicySpec::new(format!("{table}_org_select"))
            .for_command(PolicyFor::Select)
            .using(ORG_MEMBER),
        PolicySpec::new(format!("{table}_org_insert"))
            .for_command(PolicyFor::Insert)
            .with_check(ORG_MEMBER),
        PolicySpec::new(format!("{table}_org_update"))
            .for_command(PolicyFor::Update)
            .using(ORG_MEMBER)
            .with_check(ORG_MEMBER),
        PolicySpec::new(format!("{table}_org_delete"))
            .for_command(PolicyFor::Delete)
            .using(ORG_MEMBER),
    ]
}

/// A compact but representative slice: one schema-local enum, a `stores` table,
/// and a `task_instances` table with a cross-schema FK and a store-manager policy.
fn build() -> Vec<DdlStatement> {
    let enums = vec![EnumTypeSpec::new("task_category", ["prep", "cleaning"]).in_schema(SCHEMA)];

    let mut stores_cols = base_columns();
    stores_cols.push(ColumnSpec::new("name", PgType::Text));
    let mut stores = PgTableSpec::new("stores", stores_cols)
        .in_schema(SCHEMA)
        .primary_key(["id"])
        .enable_rls();
    for p in org_policies("stores") {
        stores = stores.policy(p);
    }

    let mut ti_cols = base_columns();
    ti_cols.push(ColumnSpec::new("store_id", PgType::Uuid));
    ti_cols.push(ColumnSpec::new("content_item_id", PgType::Uuid).nullable());
    ti_cols.push(ColumnSpec::new(
        "category",
        PgType::Enum(format!("{SCHEMA}.task_category")),
    ));
    let mut task_instances = PgTableSpec::new("task_instances", ti_cols)
        .in_schema(SCHEMA)
        .primary_key(["id"])
        // cross-schema FK: rpm_pizza.task_instances -> public.content_items
        .foreign_key(ForeignKeySpec::new(
            "ti_content_item_fk",
            ["content_item_id"],
            "public.content_items",
            ["id"],
        ))
        .enable_rls();
    for p in org_policies("task_instances") {
        task_instances = task_instances.policy(p);
    }
    task_instances = task_instances.policy(
        PolicySpec::new("task_instances_gm_select")
            .for_command(PolicyFor::Select)
            .using(format!("{SCHEMA}.is_store_manager(store_id)")),
    );

    let func = format!(
        "CREATE FUNCTION {SCHEMA}.is_store_manager(store uuid) RETURNS boolean LANGUAGE sql SECURITY DEFINER AS $$ SELECT true $$;"
    );

    assemble_create_migration(&[stores, task_instances], &enums, &[func])
}

#[test]
fn migration_is_ordered_schema_type_table_fk_raw_policy() {
    let stmts = build();
    let sql: Vec<String> = stmts.iter().map(DdlStatement::to_sql).collect();

    // 1. CREATE SCHEMA is first.
    assert_eq!(sql[0], r#"CREATE SCHEMA IF NOT EXISTS "rpm_pizza";"#);

    let pos = |pred: &dyn Fn(&DdlStatement) -> bool| stmts.iter().position(pred);

    let schema_at = pos(&|s| matches!(s, DdlStatement::CreateSchema { .. })).unwrap();
    let type_at = pos(&|s| matches!(s, DdlStatement::CreateEnum(_))).unwrap();
    let table_at = pos(&|s| matches!(s, DdlStatement::CreateTable(_))).unwrap();
    let fk_at = pos(&|s| matches!(s, DdlStatement::CreateForeignKey { .. })).unwrap();
    let raw_at = pos(&|s| matches!(s, DdlStatement::RawSql(_))).unwrap();
    let policy_at = pos(&|s| matches!(s, DdlStatement::CreatePolicy { .. })).unwrap();

    // 2. Phase order: schema < type < table < fk < raw < policy.
    assert!(schema_at < type_at, "schema before type");
    assert!(type_at < table_at, "type before table");
    assert!(table_at < fk_at, "table before fk");
    assert!(fk_at < raw_at, "fk before raw sql");
    assert!(raw_at < policy_at, "raw sql before policy");

    // 3. The enum type is schema-qualified.
    assert_eq!(
        sql[type_at],
        r#"CREATE TYPE "rpm_pizza"."task_category" AS ENUM('prep', 'cleaning');"#
    );

    // 4. CREATE TABLE statements are schema-qualified.
    assert!(sql
        .iter()
        .any(|s| s.starts_with(r#"CREATE TABLE "rpm_pizza"."stores" ("#)));
    assert!(sql
        .iter()
        .any(|s| s.starts_with(r#"CREATE TABLE "rpm_pizza"."task_instances" ("#)));

    // 5. The cross-schema FK targets a public table. FK *targets* are always
    //    fully qualified (drizzle + the differ both qualify FK targets).
    assert!(sql
        .iter()
        .any(|s| s.contains("ADD CONSTRAINT \"ti_content_item_fk\"")
            && s.contains("REFERENCES \"public\".\"content_items\"")));

    // 6. The SECURITY DEFINER function is present verbatim, before its GM policy.
    assert!(sql[raw_at].contains("SECURITY DEFINER"));
    let gm_at = sql
        .iter()
        .position(|s| s.contains("\"task_instances_gm_select\""))
        .unwrap();
    assert!(
        raw_at < gm_at,
        "function must precede the GM policy that calls it"
    );

    // 7. Both org-scoped and GM policies materialize.
    assert!(sql.iter().any(|s| s.contains("\"stores_org_select\"")));
    assert!(sql
        .iter()
        .any(|s| s.contains("\"task_instances_gm_select\"")));
    assert!(sql
        .iter()
        .any(|s| s.contains("rpm_pizza.is_store_manager(store_id)")));
}
