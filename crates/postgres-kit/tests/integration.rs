//! Integration tests for the I/O layer against a REAL Postgres (testcontainers).
//!
//! Proves the engine end-to-end against a live server:
//!   1. a generated `CREATE TABLE` applies, and the forward-only migration runner
//!      applies a follow-up `*.sql` migration and is idempotent on a re-run;
//!   2. introspection round-trips — [`check_drift`] reports the live schema as
//!      clean against the spec it was generated from, and flags a `MissingColumn`
//!      when the spec gains a column the database lacks;
//!   3. a generated row-level-security policy actually blocks a cross-tenant read.
//!
//! Mirrors `clickhouse-kit/tests/integration_io.rs`. Gated behind `#[ignore]` so
//! `cargo test` stays Docker-free; CI runs it with `--ignored`. The whole file is
//! `cfg`-gated on the `sqlx` (BYO driver), `migrate`, and `drift` features it
//! exercises, so it compiles cleanly under `--all-features`.
#![cfg(all(feature = "sqlx", feature = "migrate", feature = "drift"))]

use std::future::Future;
use std::path::PathBuf;

use postgres_kit::{
    check_drift, create_policy_sql, run_migrations, to_create_table_sql, ColumnSpec, Drift,
    LiveColumn, PgError, PgExecutor, PgTableSpec, PgType, PolicyFor, PolicySpec, SchemaLimits,
};
use sqlx::{PgPool, Row};
use testcontainers_modules::{postgres::Postgres, testcontainers::runners::AsyncRunner};

/// Two fixed tenant ids so the cross-tenant assertion is deterministic.
const ORG_A: &str = "11111111-1111-1111-1111-111111111111";
const ORG_B: &str = "22222222-2222-2222-2222-222222222222";

/// Thin [`PgExecutor`] over a `sqlx::PgPool` — the BYO client a real consumer
/// would write. The kit generates SQL and reads shapes; the connection is ours.
struct PgPoolExec(PgPool);

#[allow(clippy::manual_async_fn)]
impl PgExecutor for PgPoolExec {
    fn command(&self, sql: &str) -> impl Future<Output = Result<(), PgError>> + Send {
        async move {
            sqlx::query(sql)
                .execute(&self.0)
                .await
                .map(|_| ())
                .map_err(|e| PgError::Backend(e.to_string()))
        }
    }

    fn fetch_strings(
        &self,
        sql: &str,
    ) -> impl Future<Output = Result<Vec<String>, PgError>> + Send {
        async move {
            let rows = sqlx::query(sql)
                .fetch_all(&self.0)
                .await
                .map_err(|e| PgError::Backend(e.to_string()))?;
            rows.into_iter()
                .map(|r| {
                    r.try_get::<String, _>(0)
                        .map_err(|e| PgError::Backend(e.to_string()))
                })
                .collect()
        }
    }

    fn fetch_rows(
        &self,
        sql: &str,
    ) -> impl Future<Output = Result<Vec<Vec<Option<String>>>, PgError>> + Send {
        async move {
            let rows = sqlx::query(sql)
                .fetch_all(&self.0)
                .await
                .map_err(|e| PgError::Backend(e.to_string()))?;
            // The kit's introspection queries cast every column to text, so each
            // cell reads as an optional string (NULL ⇒ None).
            rows.into_iter()
                .map(|r| {
                    (0..r.columns().len())
                        .map(|i| {
                            r.try_get::<Option<String>, _>(i)
                                .map_err(|e| PgError::Backend(e.to_string()))
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .collect()
        }
    }

    fn fetch_columns(
        &self,
        table: &str,
    ) -> impl Future<Output = Result<Vec<LiveColumn>, PgError>> + Send {
        async move {
            // `check_drift` passes the schema-qualified `schema.table`; split it
            // for the catalog query (default to `public` if unqualified).
            let (schema, name) = match table.split_once('.') {
                Some((s, t)) => (s.to_string(), t.to_string()),
                None => ("public".to_string(), table.to_string()),
            };
            // `format_type` (pg_catalog) over `information_schema.columns`: the
            // latter reports user-defined enum columns as the opaque
            // `USER-DEFINED`, which would false-positive as drift. `format_type`
            // yields the real type name, matching what `introspect_schema` reads.
            let rows = sqlx::query(
                "SELECT a.attname, \
                        format_type(a.atttypid, a.atttypmod), \
                        (NOT a.attnotnull) \
                 FROM pg_attribute a \
                 JOIN pg_class c ON c.oid = a.attrelid \
                 JOIN pg_namespace n ON n.oid = c.relnamespace \
                 WHERE n.nspname = $1 AND c.relname = $2 \
                   AND a.attnum > 0 AND NOT a.attisdropped \
                 ORDER BY a.attnum",
            )
            .bind(&schema)
            .bind(&name)
            .fetch_all(&self.0)
            .await
            .map_err(|e| PgError::Backend(e.to_string()))?;

            rows.into_iter()
                .map(|r| {
                    let name: String = r.try_get(0).map_err(|e| PgError::Backend(e.to_string()))?;
                    let data_type: String =
                        r.try_get(1).map_err(|e| PgError::Backend(e.to_string()))?;
                    let is_nullable: bool =
                        r.try_get(2).map_err(|e| PgError::Backend(e.to_string()))?;
                    Ok(LiveColumn {
                        name,
                        data_type,
                        is_nullable,
                    })
                })
                .collect()
        }
    }
}

/// The spec that is the single source of truth for the test's table.
fn managed_websites_spec() -> PgTableSpec {
    PgTableSpec::new(
        "managed_websites",
        vec![
            ColumnSpec::new("id", PgType::Uuid).default_expr("gen_random_uuid()"),
            ColumnSpec::new("organization_id", PgType::Uuid),
            ColumnSpec::new("domain", PgType::Text),
            ColumnSpec::new("created_at", PgType::Timestamptz).default_expr("now()"),
        ],
    )
    .primary_key(["id"])
}

/// Write a single follow-up migration (a seed insert) into a fresh temp dir.
fn write_migrations() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pg_mig_test_{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();

    std::fs::write(
        dir.join("0001_seed.sql"),
        format!(
            "INSERT INTO managed_websites (organization_id, domain) \
             VALUES ('{ORG_A}', 'a.example.com');\n\
             INSERT INTO managed_websites (organization_id, domain) \
             VALUES ('{ORG_B}', 'b.example.com');\n"
        ),
    )
    .unwrap();

    dir
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker (Postgres testcontainer)"]
async fn engine_roundtrips_against_live_postgres() {
    let node = Postgres::default()
        .start()
        .await
        .expect("start postgres container");
    let port = node.get_host_port_ipv4(5432).await.expect("postgres port");
    // testcontainers-modules `Postgres::default` → user/pass/db all `postgres`.
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let pool = PgPool::connect(&url).await.expect("connect");
    let exec = PgPoolExec(pool.clone());
    let limits = SchemaLimits::default();
    let spec = managed_websites_spec();

    // The spec's `id` default uses `gen_random_uuid()`, which lives in pgcrypto
    // on older Postgres (it moved into core in PG 13+). Enable it defensively.
    exec.command("CREATE EXTENSION IF NOT EXISTS pgcrypto")
        .await
        .expect("create pgcrypto extension");

    // 1. Apply the generated CREATE TABLE directly through the executor seam.
    let create_sql = to_create_table_sql(&spec, &limits).expect("generate create table");
    exec.command(&create_sql).await.expect("create table");

    // 2. Forward-only migration runner: first pass applies, second is a no-op.
    let dir = write_migrations();
    let first = run_migrations(&exec, &dir)
        .await
        .expect("first migration run");
    assert_eq!(first.discovered, vec!["0001_seed.sql".to_string()]);
    assert_eq!(first.applied, first.discovered);
    assert!(first.skipped.is_empty(), "first pass should skip nothing");

    let second = run_migrations(&exec, &dir)
        .await
        .expect("second migration run");
    assert!(
        second.applied.is_empty(),
        "second pass must apply nothing, got {:?}",
        second.applied
    );
    assert_eq!(second.skipped, second.discovered);

    // The seed actually landed (two tenants, one row each).
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM managed_websites")
        .fetch_one(&pool)
        .await
        .expect("count rows");
    assert_eq!(total, 2);

    // 3. Introspection round-trip: the live schema matches the spec it was built
    //    from — clean drift.
    let clean = check_drift(&exec, std::slice::from_ref(&spec))
        .await
        .expect("drift check");
    assert!(clean.is_clean(), "expected no drift, got {:?}", clean.drift);

    // 4. A spec with a column the database lacks → MissingColumn drift.
    let mut with_extra = managed_websites_spec();
    with_extra
        .columns
        .push(ColumnSpec::new("price", PgType::Int4));
    let drifted = check_drift(&exec, &[with_extra])
        .await
        .expect("drift check w/ extra column");
    assert!(
        drifted.drift.contains(&Drift::MissingColumn {
            table: "public.managed_websites".into(),
            column: "price".into(),
            expected_type: "integer".into(),
        }),
        "expected MissingColumn drift for `price`, got {:?}",
        drifted.drift
    );

    // 5. RLS: a generated policy must block a cross-tenant read.
    //    A non-superuser role is subject to RLS (superusers bypass it), so we
    //    create one, grant it SELECT, enable RLS, and install a tenant-isolation
    //    policy generated by the kit.
    exec.command("CREATE ROLE tenant_reader NOLOGIN")
        .await
        .expect("create role");
    exec.command("GRANT SELECT ON managed_websites TO tenant_reader")
        .await
        .expect("grant select");
    exec.command("ALTER TABLE managed_websites ENABLE ROW LEVEL SECURITY")
        .await
        .expect("enable rls");

    let policy = PolicySpec::new("org_isolation")
        .for_command(PolicyFor::Select)
        .to_roles(["tenant_reader"])
        .using("organization_id = current_setting('app.current_org', true)::uuid");
    let policy_sql =
        create_policy_sql("public", "managed_websites", &policy, &limits).expect("generate policy");
    exec.command(&policy_sql).await.expect("create policy");

    // On a single dedicated connection: become the non-superuser, scope to ORG_A,
    // and read. RLS must hide ORG_B's row.
    let mut conn = pool.acquire().await.expect("acquire conn");
    sqlx::query("SET ROLE tenant_reader")
        .execute(&mut *conn)
        .await
        .expect("set role");
    sqlx::query("SELECT set_config('app.current_org', $1, false)")
        .bind(ORG_A)
        .execute(&mut *conn)
        .await
        .expect("set tenant");

    let visible: Vec<String> = sqlx::query("SELECT organization_id::text FROM managed_websites")
        .fetch_all(&mut *conn)
        .await
        .expect("scoped select")
        .into_iter()
        .map(|r| r.get::<String, _>(0))
        .collect();

    assert_eq!(
        visible,
        vec![ORG_A.to_string()],
        "RLS must expose only the scoped tenant's row, saw {visible:?}"
    );
    assert!(
        !visible.iter().any(|o| o == ORG_B),
        "cross-tenant row leaked through RLS: {visible:?}"
    );

    sqlx::query("RESET ROLE").execute(&mut *conn).await.ok();

    std::fs::remove_dir_all(&dir).ok();
}

/// The cutover guarantee, end-to-end: stand up a representative schema (enum,
/// two tables with a foreign key, unique / check constraints, a plain and a
/// partial index, an RLS policy + RLS enabled, plus `tsvector` and a `STORED`
/// generated column), then introspect it back with `introspect_schema` and feed
/// the introspected specs to `check_drift` / `check_enum_drift` against the same
/// database. Reading the source-of-truth out of the catalog the drift gate also
/// reads must be drift-clean — proving introspection and drift agree.
#[cfg(feature = "introspect")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker (Postgres testcontainer)"]
async fn introspect_round_trips_drift_clean() {
    use postgres_kit::{
        check_enum_drift, create_index_sql, create_policy_sql, create_type_sql, introspect_schema,
        CheckConstraintSpec, EnumTypeSpec, ForeignKeySpec, IndexColumn, IndexSpec,
        ReferentialAction, UniqueConstraintSpec,
    };
    use testcontainers_modules::testcontainers::ImageExt;

    // Pin a modern Postgres: introspection reads `pg_attribute.attgenerated`
    // (PG12+) and `pg_index.indnullsnotdistinct` (PG15+), matching the Supabase
    // (PG15+) target the cutover serves.
    let node = Postgres::default()
        .with_tag("16-alpine")
        .start()
        .await
        .expect("start postgres container");
    let port = node.get_host_port_ipv4(5432).await.expect("postgres port");
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let pool = PgPool::connect(&url).await.expect("connect");
    let exec = PgPoolExec(pool.clone());
    let limits = SchemaLimits::default();

    // ── Build the schema entirely from kit emitters ───────────────────────────
    let enum_spec = EnumTypeSpec::new("widget_status", ["active", "archived"]);
    exec.command(&create_type_sql(&enum_spec, &limits).expect("enum ddl"))
        .await
        .expect("create type");

    let orgs =
        PgTableSpec::new("orgs", vec![ColumnSpec::new("id", PgType::Uuid)]).primary_key(["id"]);
    exec.command(&to_create_table_sql(&orgs, &limits).expect("orgs ddl"))
        .await
        .expect("create orgs");

    // `widgets` exercises every introspected seam (the indexes / policy are
    // applied separately below, as they are not inline in CREATE TABLE).
    let widgets = PgTableSpec::new(
        "widgets",
        vec![
            ColumnSpec::new("id", PgType::Uuid),
            ColumnSpec::new("org_id", PgType::Uuid),
            ColumnSpec::new("name", PgType::Text),
            ColumnSpec::new("status", PgType::Enum("widget_status".into())),
            ColumnSpec::new("body", PgType::Tsvector).nullable(),
            ColumnSpec::new("created", PgType::Timestamptz),
        ],
    )
    .primary_key(["id"])
    .foreign_key(
        ForeignKeySpec::new("widgets_org_fk", ["org_id"], "public.orgs", ["id"])
            .on_delete(ReferentialAction::Cascade),
    )
    .unique_constraint(UniqueConstraintSpec::new("widgets_name_uq", ["name"]))
    .check(CheckConstraintSpec::new(
        "widgets_name_len",
        "char_length(name) > 0",
    ));
    exec.command(&to_create_table_sql(&widgets, &limits).expect("widgets ddl"))
        .await
        .expect("create widgets");

    // A plain btree index and a partial index (exercises the predicate path).
    let idx_org = IndexSpec::new("widgets_org_idx", [IndexColumn::column("org_id")]);
    exec.command(&create_index_sql("public", "widgets", &idx_org, &limits).expect("idx ddl"))
        .await
        .expect("create index");
    let idx_partial = IndexSpec::new("widgets_named_idx", [IndexColumn::column("name")])
        .where_clause("body IS NOT NULL");
    exec.command(
        &create_index_sql("public", "widgets", &idx_partial, &limits).expect("partial idx"),
    )
    .await
    .expect("create partial index");

    // RLS + a tenant-isolation policy.
    exec.command("ALTER TABLE widgets ENABLE ROW LEVEL SECURITY")
        .await
        .expect("enable rls");
    let policy = PolicySpec::new("widgets_org_isolation")
        .using("org_id = current_setting('app.current_org', true)::uuid");
    exec.command(&create_policy_sql("public", "widgets", &policy, &limits).expect("policy ddl"))
        .await
        .expect("create policy");

    // ── Introspect the live schema back into specs ────────────────────────────
    let introspected = introspect_schema(&exec, "public")
        .await
        .expect("introspect schema");

    // Sanity: both tables and the enum came back.
    assert_eq!(
        introspected
            .tables
            .iter()
            .map(|t| t.qualified_name())
            .collect::<Vec<_>>(),
        vec!["public.orgs".to_string(), "public.widgets".to_string()],
    );
    assert_eq!(introspected.enums.len(), 1);
    assert_eq!(introspected.enums[0].name, "widget_status");

    // ── The cutover guarantee: introspected specs are drift-clean ─────────────
    let table_drift = check_drift(&exec, &introspected.tables)
        .await
        .expect("drift check");
    assert!(
        table_drift.is_clean(),
        "introspected schema must be drift-clean, got {:?}",
        table_drift.drift
    );
    let enum_drift = check_enum_drift(&exec, &introspected.enums)
        .await
        .expect("enum drift check");
    assert!(
        enum_drift.is_empty(),
        "introspected enums must be drift-clean, got {enum_drift:?}"
    );
}
