//! Full `rpm_pizza` schema: the complete 12-table multi-tenant slice for the RPM
//! Pizza app, built entirely with `smooai-postgres-kit`. Mirrors the patterns in
//! `rpm_pizza_schema.rs` (the worked how-to) but for the production-shaped spec.
//!
//! Run with:
//!
//! ```bash
//! cargo run --example rpm_pizza_full --all-features
//! ```
//!
//! Prints, in order:
//! - `==== MIGRATION ====`: every DDL statement + `--> statement-breakpoint`.
//! - `==== TS CODEGEN ====`: the Zod TS module (interface + select/insert schemas)
//!   for each of the 12 tables.

use postgres_kit::codegen::{emit_ts_module, CodegenOptions};
use postgres_kit::differ::{assemble_create_migration, DdlStatement};
use postgres_kit::{
    ColumnSpec, EnumTypeSpec, ForeignKeySpec, PgTableSpec, PgType, PolicyFor, PolicySpec,
    ReferentialAction,
};

/// The dedicated schema this app lives in.
const SCHEMA: &str = "rpm_pizza";

/// RLS role: all policies target `authenticated` (per kit note).
const ROLE: &str = "authenticated";

/// An org-member predicate: the row's tenant must be one the current user belongs
/// to. Trusted, developer-authored SQL (emitted verbatim into the policy).
const ORG_MEMBER: &str = "organization_id IN (SELECT organization_id FROM public.organization_members WHERE user_id = auth.uid())";

/// Build the four org-scoped RLS policies (SELECT / INSERT / UPDATE / DELETE) for
/// a table — each filtered by the org-member predicate, scoped to `authenticated`.
fn org_policies(table: &str) -> Vec<PolicySpec> {
    vec![
        PolicySpec::new(format!("{table}_org_select"))
            .for_command(PolicyFor::Select)
            .to_roles([ROLE])
            .using(ORG_MEMBER),
        PolicySpec::new(format!("{table}_org_insert"))
            .for_command(PolicyFor::Insert)
            .to_roles([ROLE])
            .with_check(ORG_MEMBER),
        PolicySpec::new(format!("{table}_org_update"))
            .for_command(PolicyFor::Update)
            .to_roles([ROLE])
            .using(ORG_MEMBER)
            .with_check(ORG_MEMBER),
        PolicySpec::new(format!("{table}_org_delete"))
            .for_command(PolicyFor::Delete)
            .to_roles([ROLE])
            .using(ORG_MEMBER),
    ]
}

/// `id uuid PK DEFAULT gen_random_uuid()` + `organization_id uuid` + `created_at`
/// / `updated_at` timestamps — the shape every table in this slice shares.
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

/// Apply base + org policies + RLS, returning the finished table.
fn with_org_policies(mut table: PgTableSpec) -> PgTableSpec {
    let name = table.name.clone();
    table = table.in_schema(SCHEMA).primary_key(["id"]).enable_rls();
    for p in org_policies(&name) {
        table = table.policy(p);
    }
    table
}

fn build_tables() -> Vec<PgTableSpec> {
    // 1. regions
    let mut cols = base_columns();
    cols.push(ColumnSpec::new("name", PgType::Text));
    let regions = with_org_policies(PgTableSpec::new("regions", cols));

    // 2. areas -> regions
    let mut cols = base_columns();
    cols.push(ColumnSpec::new("name", PgType::Text));
    cols.push(ColumnSpec::new("region_id", PgType::Uuid));
    let areas = with_org_policies(
        PgTableSpec::new("areas", cols).foreign_key(
            ForeignKeySpec::new(
                "areas_region_fk",
                ["region_id"],
                "rpm_pizza.regions",
                ["id"],
            )
            .on_delete(ReferentialAction::Cascade),
        ),
    );

    // 3. stores -> areas
    let mut cols = base_columns();
    cols.push(ColumnSpec::new("name", PgType::Text));
    cols.push(ColumnSpec::new("store_number", PgType::Text));
    cols.push(ColumnSpec::new("address", PgType::Text));
    cols.push(ColumnSpec::new("city", PgType::Text));
    cols.push(ColumnSpec::new("state", PgType::Text));
    cols.push(ColumnSpec::new("lat", PgType::Float8));
    cols.push(ColumnSpec::new("lng", PgType::Float8));
    cols.push(ColumnSpec::new("area_id", PgType::Uuid));
    cols.push(enum_col("status", "store_status").default_expr("'open'"));
    let stores = with_org_policies(
        PgTableSpec::new("stores", cols).foreign_key(
            ForeignKeySpec::new("stores_area_fk", ["area_id"], "rpm_pizza.areas", ["id"])
                .on_delete(ReferentialAction::Cascade),
        ),
    );

    // 4. app_users (no role column — roles live in public.custom_app_role_assignments)
    let mut cols = base_columns();
    cols.push(ColumnSpec::new("supabase_user_id", PgType::Uuid));
    cols.push(ColumnSpec::new("email", PgType::Text));
    cols.push(ColumnSpec::new("display_name", PgType::Text));
    let app_users = with_org_policies(PgTableSpec::new("app_users", cols));

    // 5. store_managers -> stores, app_users
    let mut cols = base_columns();
    cols.push(ColumnSpec::new("store_id", PgType::Uuid));
    cols.push(ColumnSpec::new("app_user_id", PgType::Uuid));
    let store_managers = with_org_policies(
        PgTableSpec::new("store_managers", cols)
            .foreign_key(
                ForeignKeySpec::new(
                    "store_managers_store_fk",
                    ["store_id"],
                    "rpm_pizza.stores",
                    ["id"],
                )
                .on_delete(ReferentialAction::Cascade),
            )
            .foreign_key(
                ForeignKeySpec::new(
                    "store_managers_app_user_fk",
                    ["app_user_id"],
                    "rpm_pizza.app_users",
                    ["id"],
                )
                .on_delete(ReferentialAction::Cascade),
            ),
    );

    // 6. task_lists -> public.content_items (nullable)
    let mut cols = base_columns();
    cols.push(ColumnSpec::new("name", PgType::Text));
    cols.push(enum_col("category", "task_category"));
    cols.push(enum_col("cadence", "task_cadence"));
    cols.push(ColumnSpec::new("content_item_id", PgType::Uuid).nullable());
    let task_lists = with_org_policies(PgTableSpec::new("task_lists", cols).foreign_key(
        ForeignKeySpec::new(
            "task_lists_content_item_fk",
            ["content_item_id"],
            "public.content_items",
            ["id"],
        ),
    ));

    // 7. task_list_assignments -> task_lists
    let mut cols = base_columns();
    cols.push(ColumnSpec::new("task_list_id", PgType::Uuid));
    cols.push(enum_col("scope_type", "assignment_scope"));
    cols.push(ColumnSpec::new("scope_id", PgType::Uuid));
    cols.push(ColumnSpec::new("due_local_time", PgType::Text));
    cols.push(ColumnSpec::new("active", PgType::Bool).default_expr("true"));
    let task_list_assignments = with_org_policies(
        PgTableSpec::new("task_list_assignments", cols).foreign_key(
            ForeignKeySpec::new(
                "task_list_assignments_list_fk",
                ["task_list_id"],
                "rpm_pizza.task_lists",
                ["id"],
            )
            .on_delete(ReferentialAction::Cascade),
        ),
    );

    // 8. task_instances -> task_lists (+ GM SELECT policy)
    let mut cols = base_columns();
    cols.push(ColumnSpec::new("task_list_id", PgType::Uuid));
    cols.push(ColumnSpec::new("store_id", PgType::Uuid));
    cols.push(ColumnSpec::new("due_at", PgType::Timestamptz));
    cols.push(enum_col("status", "task_status").default_expr("'pending'"));
    cols.push(ColumnSpec::new("generated_at", PgType::Timestamptz).default_expr("now()"));
    let mut task_instances = with_org_policies(
        PgTableSpec::new("task_instances", cols).foreign_key(
            ForeignKeySpec::new(
                "task_instances_list_fk",
                ["task_list_id"],
                "rpm_pizza.task_lists",
                ["id"],
            )
            .on_delete(ReferentialAction::Cascade),
        ),
    );
    task_instances = task_instances.policy(
        PolicySpec::new("task_instances_gm_select")
            .for_command(PolicyFor::Select)
            .to_roles([ROLE])
            .using(format!("{SCHEMA}.is_store_manager(store_id)")),
    );

    // 9. submissions -> task_instances, public.form_submissions (+ GM SELECT policy)
    let mut cols = base_columns();
    cols.push(ColumnSpec::new("task_instance_id", PgType::Uuid));
    cols.push(ColumnSpec::new("store_id", PgType::Uuid));
    cols.push(ColumnSpec::new("form_submission_id", PgType::Uuid).nullable());
    cols.push(ColumnSpec::new("submitted_by_app_user_id", PgType::Uuid).nullable());
    cols.push(ColumnSpec::new("photo_refs", PgType::Jsonb));
    cols.push(ColumnSpec::new("vision_validation", PgType::Jsonb));
    cols.push(enum_col("result", "submission_result").nullable());
    cols.push(ColumnSpec::new("exception_flag", PgType::Bool).default_expr("false"));
    cols.push(ColumnSpec::new("submitted_at", PgType::Timestamptz));
    let mut submissions = with_org_policies(
        PgTableSpec::new("submissions", cols)
            .foreign_key(
                ForeignKeySpec::new(
                    "submissions_task_instance_fk",
                    ["task_instance_id"],
                    "rpm_pizza.task_instances",
                    ["id"],
                )
                .on_delete(ReferentialAction::Cascade),
            )
            .foreign_key(ForeignKeySpec::new(
                "submissions_form_submission_fk",
                ["form_submission_id"],
                "public.form_submissions",
                ["id"],
            )),
    );
    submissions = submissions.policy(
        PolicySpec::new("submissions_gm_select")
            .for_command(PolicyFor::Select)
            .to_roles([ROLE])
            .using(format!("{SCHEMA}.is_store_manager(store_id)")),
    );

    // 10. exceptions -> submissions
    let mut cols = base_columns();
    cols.push(ColumnSpec::new("store_id", PgType::Uuid));
    cols.push(ColumnSpec::new("submission_id", PgType::Uuid).nullable());
    cols.push(enum_col("type", "exception_type"));
    cols.push(ColumnSpec::new("category", PgType::Text));
    cols.push(enum_col("status", "exception_status").default_expr("'open'"));
    cols.push(ColumnSpec::new("raised_at", PgType::Timestamptz).default_expr("now()"));
    cols.push(ColumnSpec::new("resolved_at", PgType::Timestamptz).nullable());
    let exceptions = with_org_policies(PgTableSpec::new("exceptions", cols).foreign_key(
        ForeignKeySpec::new(
            "exceptions_submission_fk",
            ["submission_id"],
            "rpm_pizza.submissions",
            ["id"],
        ),
    ));

    // 11. store_performance_rollups -> stores
    let mut cols = base_columns();
    cols.push(ColumnSpec::new("store_id", PgType::Uuid));
    cols.push(ColumnSpec::new("period_date", PgType::Date));
    cols.push(ColumnSpec::new("compliance_pct", PgType::Float8));
    cols.push(ColumnSpec::new("overdue_count", PgType::Int4));
    cols.push(ColumnSpec::new("exception_count", PgType::Int4));
    cols.push(ColumnSpec::new("on_time_pct", PgType::Float8));
    cols.push(ColumnSpec::new("revenue", PgType::Float8));
    cols.push(ColumnSpec::new("headcount", PgType::Int4));
    let store_performance_rollups = with_org_policies(
        PgTableSpec::new("store_performance_rollups", cols).foreign_key(
            ForeignKeySpec::new(
                "store_performance_rollups_store_fk",
                ["store_id"],
                "rpm_pizza.stores",
                ["id"],
            )
            .on_delete(ReferentialAction::Cascade),
        ),
    );

    // 12. audit_links
    let mut cols = base_columns();
    cols.push(ColumnSpec::new("audit_event_id", PgType::Uuid));
    cols.push(ColumnSpec::new("entity_type", PgType::Text));
    cols.push(ColumnSpec::new("entity_id", PgType::Uuid));
    let audit_links = with_org_policies(PgTableSpec::new("audit_links", cols));

    vec![
        regions,
        areas,
        stores,
        app_users,
        store_managers,
        task_lists,
        task_list_assignments,
        task_instances,
        submissions,
        exceptions,
        store_performance_rollups,
        audit_links,
    ]
}

fn build_enums() -> Vec<EnumTypeSpec> {
    vec![
        EnumTypeSpec::new("store_status", ["open", "closed", "remodeling"]).in_schema(SCHEMA),
        EnumTypeSpec::new(
            "task_category",
            [
                "financial",
                "human_resources",
                "maintenance",
                "operations",
                "quality",
                "safety",
                "training",
                "test_forms",
                "testing",
                "uncategorized",
            ],
        )
        .in_schema(SCHEMA),
        EnumTypeSpec::new("task_cadence", ["daily", "weekly", "monthly"]).in_schema(SCHEMA),
        EnumTypeSpec::new(
            "task_status",
            ["pending", "in_progress", "submitted", "overdue"],
        )
        .in_schema(SCHEMA),
        EnumTypeSpec::new("submission_result", ["pass", "fail"]).in_schema(SCHEMA),
        EnumTypeSpec::new("exception_type", ["vision_fail", "overdue", "manual"]).in_schema(SCHEMA),
        EnumTypeSpec::new("exception_status", ["open", "resolved"]).in_schema(SCHEMA),
        EnumTypeSpec::new("assignment_scope", ["region", "area", "store"]).in_schema(SCHEMA),
    ]
}

fn build_migration(tables: &[PgTableSpec], enums: &[EnumTypeSpec]) -> Vec<DdlStatement> {
    // The SECURITY DEFINER helper, injected via the RawSql escape hatch. Joins
    // app_users on supabase_user_id = auth.uid() (roles/identity live there).
    let is_store_manager = format!(
        "CREATE FUNCTION {SCHEMA}.is_store_manager(store uuid) RETURNS boolean\n\
         LANGUAGE sql SECURITY DEFINER STABLE AS $$\n\
         \x20 SELECT EXISTS (\n\
         \x20   SELECT 1 FROM {SCHEMA}.store_managers m\n\
         \x20   JOIN {SCHEMA}.app_users u ON u.id = m.app_user_id\n\
         \x20   WHERE m.store_id = store AND u.supabase_user_id = auth.uid()\n\
         \x20 );\n\
         $$;"
    );

    assemble_create_migration(tables, enums, &[is_store_manager])
}

fn main() {
    let tables = build_tables();
    let enums = build_enums();

    println!("==== MIGRATION ====");
    for stmt in &build_migration(&tables, &enums) {
        println!("{}", stmt.to_sql());
        println!("--> statement-breakpoint");
    }

    println!();
    println!("==== TS CODEGEN ====");
    let opts = CodegenOptions::new();
    for table in &tables {
        println!("// ---- {} ----", table.name);
        match emit_ts_module(table, &opts) {
            Ok(ts) => println!("{ts}"),
            Err(e) => println!("// codegen error for {}: {e}", table.name),
        }
    }
}
