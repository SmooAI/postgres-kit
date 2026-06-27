//! Dogfood example: build a representative `rpm_pizza` slice — a multi-tenant app
//! whose tables live in a **dedicated, non-`public`** Postgres schema — entirely
//! with `smooai-postgres-kit`, and print the full generated migration.
//!
//! This file IS the consumer how-to. Run it with:
//!
//! ```bash
//! cargo run --example rpm_pizza_schema --all-features
//! ```
//!
//! What it demonstrates, end to end:
//!
//! - `PgTableSpec::in_schema("rpm_pizza")` — tables in a dedicated schema. The kit
//!   auto-emits `CREATE SCHEMA IF NOT EXISTS "rpm_pizza"` first.
//! - `EnumTypeSpec::in_schema("rpm_pizza")` — schema-local enum types, referenced
//!   from columns as `PgType::Enum("rpm_pizza.task_category".into())`.
//! - **Cross-schema foreign keys**: `rpm_pizza` tables referencing `public` tables
//!   (`ForeignKeySpec::new(.., "public.content_items", ..)`).
//! - Row-level security: the four org-scoped policies per table, plus an extra
//!   store-manager `FOR SELECT` policy on `task_instances` / `submissions`.
//! - A `SECURITY DEFINER` helper function injected via the [`DdlStatement::RawSql`]
//!   escape hatch (the kit does not model functions) — ordered *before* the policy
//!   that calls it.
//!
//! The migration is assembled with `differ::assemble_create_migration(tables,
//! enums, extra_raw)`, which lowers the specs, diffs against an empty schema, and
//! orders everything: `CREATE SCHEMA` → `CREATE TYPE` → `CREATE TABLE` → foreign
//! keys → indexes → raw SQL → `ENABLE ROW LEVEL SECURITY` → policies.

use postgres_kit::differ::{assemble_create_migration, DdlStatement};
use postgres_kit::{
    ColumnSpec, EnumTypeSpec, ForeignKeySpec, PgTableSpec, PgType, PolicyFor, PolicySpec,
    ReferentialAction,
};

/// The dedicated schema this app lives in.
const SCHEMA: &str = "rpm_pizza";

/// An org-member predicate: the row's tenant must be one the current user belongs
/// to. Trusted, developer-authored SQL (emitted verbatim into the policy).
const ORG_MEMBER: &str = "organization_id IN (SELECT organization_id FROM public.organization_members WHERE user_id = auth.uid())";

/// Build the four org-scoped RLS policies (SELECT / INSERT / UPDATE / DELETE) for
/// a table — each filtered by the org-member predicate, with distinct names.
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

/// `id uuid PK DEFAULT gen_random_uuid()` + `organization_id uuid NOT NULL` (the
/// tenant column) + `created_at` / `updated_at` timestamps — the shape every
/// table in this slice shares.
fn base_columns() -> Vec<ColumnSpec> {
    vec![
        ColumnSpec::new("id", PgType::Uuid).default_expr("gen_random_uuid()"),
        ColumnSpec::new("organization_id", PgType::Uuid),
        ColumnSpec::new("created_at", PgType::Timestamptz).default_expr("now()"),
        ColumnSpec::new("updated_at", PgType::Timestamptz).default_expr("now()"),
    ]
}

/// A column typed on a schema-local enum, referenced by its qualified name.
fn enum_col(name: &str, enum_name: &str) -> ColumnSpec {
    ColumnSpec::new(name, PgType::Enum(format!("{SCHEMA}.{enum_name}")))
}

/// Build the full `rpm_pizza` slice (tables + enums) and the raw `SECURITY
/// DEFINER` function, then assemble the ordered migration.
fn build_migration() -> Vec<DdlStatement> {
    // ---- schema-local enum types ----
    let enums = vec![
        EnumTypeSpec::new("task_category", ["prep", "cleaning", "inventory"]).in_schema(SCHEMA),
        EnumTypeSpec::new("task_cadence", ["daily", "weekly", "monthly"]).in_schema(SCHEMA),
        EnumTypeSpec::new("store_status", ["open", "closed", "remodeling"]).in_schema(SCHEMA),
    ];

    // ---- stores ----
    let mut stores_cols = base_columns();
    stores_cols.push(ColumnSpec::new("name", PgType::Text));
    stores_cols.push(enum_col("status", "store_status").default_expr("'open'"));
    let mut stores = PgTableSpec::new("stores", stores_cols)
        .in_schema(SCHEMA)
        .primary_key(["id"])
        .enable_rls();
    for p in org_policies("stores") {
        stores = stores.policy(p);
    }

    // ---- task_lists (cross-schema FK -> public.content_items) ----
    let mut task_lists_cols = base_columns();
    task_lists_cols.push(ColumnSpec::new("store_id", PgType::Uuid));
    task_lists_cols.push(ColumnSpec::new("content_item_id", PgType::Uuid).nullable());
    task_lists_cols.push(enum_col("category", "task_category"));
    task_lists_cols.push(enum_col("cadence", "task_cadence"));
    let mut task_lists = PgTableSpec::new("task_lists", task_lists_cols)
        .in_schema(SCHEMA)
        .primary_key(["id"])
        .foreign_key(
            ForeignKeySpec::new(
                "task_lists_store_fk",
                ["store_id"],
                "rpm_pizza.stores",
                ["id"],
            )
            .on_delete(ReferentialAction::Cascade),
        )
        .foreign_key(ForeignKeySpec::new(
            "task_lists_content_item_fk",
            ["content_item_id"],
            "public.content_items",
            ["id"],
        ))
        .enable_rls();
    for p in org_policies("task_lists") {
        task_lists = task_lists.policy(p);
    }

    // ---- task_instances (store-manager SELECT policy in addition to org ones) ----
    let mut task_instances_cols = base_columns();
    task_instances_cols.push(ColumnSpec::new("store_id", PgType::Uuid));
    task_instances_cols.push(ColumnSpec::new("task_list_id", PgType::Uuid));
    let mut task_instances = PgTableSpec::new("task_instances", task_instances_cols)
        .in_schema(SCHEMA)
        .primary_key(["id"])
        .foreign_key(
            ForeignKeySpec::new(
                "task_instances_list_fk",
                ["task_list_id"],
                "rpm_pizza.task_lists",
                ["id"],
            )
            .on_delete(ReferentialAction::Cascade),
        )
        .enable_rls();
    for p in org_policies("task_instances") {
        task_instances = task_instances.policy(p);
    }
    // Store managers can additionally read instances for stores they manage.
    task_instances = task_instances.policy(
        PolicySpec::new("task_instances_gm_select")
            .for_command(PolicyFor::Select)
            .using(format!("{SCHEMA}.is_store_manager(store_id)")),
    );

    // ---- submissions (cross-schema FK -> public.form_submissions, + GM policy) ----
    let mut submissions_cols = base_columns();
    submissions_cols.push(ColumnSpec::new("store_id", PgType::Uuid));
    submissions_cols.push(ColumnSpec::new("task_instance_id", PgType::Uuid));
    submissions_cols.push(ColumnSpec::new("form_submission_id", PgType::Uuid).nullable());
    let mut submissions = PgTableSpec::new("submissions", submissions_cols)
        .in_schema(SCHEMA)
        .primary_key(["id"])
        .foreign_key(ForeignKeySpec::new(
            "submissions_form_submission_fk",
            ["form_submission_id"],
            "public.form_submissions",
            ["id"],
        ))
        .enable_rls();
    for p in org_policies("submissions") {
        submissions = submissions.policy(p);
    }
    submissions = submissions.policy(
        PolicySpec::new("submissions_gm_select")
            .for_command(PolicyFor::Select)
            .using(format!("{SCHEMA}.is_store_manager(store_id)")),
    );

    let tables = vec![stores, task_lists, task_instances, submissions];

    // ---- the SECURITY DEFINER helper, injected via the RawSql escape hatch ----
    let is_store_manager = format!(
        "CREATE FUNCTION {SCHEMA}.is_store_manager(store uuid) RETURNS boolean\n\
         LANGUAGE sql SECURITY DEFINER STABLE AS $$\n\
         \x20 SELECT EXISTS (\n\
         \x20   SELECT 1 FROM {SCHEMA}.store_managers m\n\
         \x20   WHERE m.store_id = store AND m.user_id = auth.uid()\n\
         \x20 );\n\
         $$;"
    );

    assemble_create_migration(&tables, &enums, &[is_store_manager])
}

fn main() {
    let statements = build_migration();
    for stmt in &statements {
        println!("{}", stmt.to_sql());
        println!("--> statement-breakpoint");
    }
}
