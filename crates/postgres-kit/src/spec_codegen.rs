//! Spec-source codegen (feature `codegen`): emit a Rust module that
//! *reconstructs* a set of [`PgTableSpec`]/[`EnumTypeSpec`] values via the public
//! builder API. Where [`crate::codegen`] turns a spec into a row/decode target,
//! this turns a spec back into the *source code that builds it* — so an
//! introspected live schema can be frozen into a committed Rust source-of-truth.
//!
//! The intended use is the **cutover** (SMOODEV-2150): introspect the live DB into
//! `Vec<PgTableSpec>` + `Vec<EnumTypeSpec>` (see [`crate::introspect`]), run
//! [`emit_spec_rust`] over them, and commit the result as `generated.rs`. The
//! committed module then exposes `tables()` / `enums()` that rebuild exactly the
//! introspected specs, which [`crate::check_drift`] / [`crate::check_enum_drift`]
//! validate drift-clean against the same database.
//!
//! Output is deterministic: tables and enums are sorted by qualified name; every
//! optional builder call is emitted only when its field is set, in a fixed order.
//! The emitter covers every [`PgType`] variant and every spec field
//! introspection can populate (columns with defaults / `STORED` generated /
//! identity, primary keys, foreign keys with `ON DELETE`/`ON UPDATE`, unique and
//! check constraints, indexes with method / `WHERE` / per-column direction /
//! nulls / opclass, RLS policies with `AS`/`FOR`/`TO`/`USING`/`WITH CHECK`, and
//! the RLS-enabled flag).

use crate::spec::{
    ColumnSpec, EnumTypeSpec, ForeignKeySpec, IdentityKind, IdentitySpec, IndexColumn, IndexSpec,
    PgTableSpec, PgType, PolicyAs, PolicyFor, PolicySpec, ReferentialAction, SequenceOptions,
    UniqueConstraintSpec,
};

/// Render a Rust double-quoted string literal, escaping the metacharacters that
/// can appear in trusted-but-arbitrary spec text (default exprs, generated
/// expressions, policy/index predicates, check bodies).
fn lit(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Render an array literal of string literals, e.g. `["a", "b"]`.
fn str_array<'a>(items: impl IntoIterator<Item = &'a String>) -> String {
    let parts: Vec<String> = items.into_iter().map(|s| lit(s)).collect();
    format!("[{}]", parts.join(", "))
}

/// Emit a [`PgType`] constructor expression. Covers every variant, including the
/// parameterized ones (`Varchar`/`Numeric`/`Vector`), the named `Enum`, and the
/// recursive `Array`.
fn emit_pg_type(ty: &PgType) -> String {
    match ty {
        PgType::Uuid => "PgType::Uuid".into(),
        PgType::Text => "PgType::Text".into(),
        PgType::Varchar(None) => "PgType::Varchar(None)".into(),
        PgType::Varchar(Some(n)) => format!("PgType::Varchar(Some({n}))"),
        PgType::Bool => "PgType::Bool".into(),
        PgType::Int2 => "PgType::Int2".into(),
        PgType::Int4 => "PgType::Int4".into(),
        PgType::Int8 => "PgType::Int8".into(),
        PgType::Float4 => "PgType::Float4".into(),
        PgType::Float8 => "PgType::Float8".into(),
        PgType::Numeric(None) => "PgType::Numeric(None)".into(),
        PgType::Numeric(Some((p, s))) => format!("PgType::Numeric(Some(({p}, {s})))"),
        PgType::Timestamptz => "PgType::Timestamptz".into(),
        PgType::Timestamp => "PgType::Timestamp".into(),
        PgType::Date => "PgType::Date".into(),
        PgType::Jsonb => "PgType::Jsonb".into(),
        PgType::Json => "PgType::Json".into(),
        PgType::Bytea => "PgType::Bytea".into(),
        PgType::Tsvector => "PgType::Tsvector".into(),
        PgType::Vector(None) => "PgType::Vector(None)".into(),
        PgType::Vector(Some(n)) => format!("PgType::Vector(Some({n}))"),
        PgType::Enum(name) => format!("PgType::Enum({}.to_string())", lit(name)),
        PgType::Array(inner) => format!("PgType::Array(Box::new({}))", emit_pg_type(inner)),
    }
}

fn emit_ref_action(a: ReferentialAction) -> &'static str {
    match a {
        ReferentialAction::NoAction => "ReferentialAction::NoAction",
        ReferentialAction::Restrict => "ReferentialAction::Restrict",
        ReferentialAction::Cascade => "ReferentialAction::Cascade",
        ReferentialAction::SetNull => "ReferentialAction::SetNull",
        ReferentialAction::SetDefault => "ReferentialAction::SetDefault",
    }
}

/// Emit an [`IdentitySpec`] expression (kind + any non-default sequence options).
fn emit_identity(id: &IdentitySpec) -> String {
    let base = match id.kind {
        IdentityKind::Always => "IdentitySpec::always()",
        IdentityKind::ByDefault => "IdentitySpec::by_default()",
    };
    let seq: &SequenceOptions = &id.sequence;
    let has_seq = seq.increment.is_some()
        || seq.min_value.is_some()
        || seq.max_value.is_some()
        || seq.start_with.is_some()
        || seq.cache.is_some()
        || seq.cycle;
    if !has_seq {
        return base.to_string();
    }
    let mut so = String::from("SequenceOptions::new()");
    if let Some(v) = &seq.increment {
        so.push_str(&format!(".increment({})", lit(v)));
    }
    if let Some(v) = &seq.min_value {
        so.push_str(&format!(".min_value({})", lit(v)));
    }
    if let Some(v) = &seq.max_value {
        so.push_str(&format!(".max_value({})", lit(v)));
    }
    if let Some(v) = &seq.start_with {
        so.push_str(&format!(".start_with({})", lit(v)));
    }
    if let Some(v) = &seq.cache {
        so.push_str(&format!(".cache({})", lit(v)));
    }
    if seq.cycle {
        so.push_str(".cycle(true)");
    }
    format!("{base}.with_sequence({so})")
}

/// Emit a single [`ColumnSpec`] builder chain.
fn emit_column(c: &ColumnSpec) -> String {
    let mut s = format!("ColumnSpec::new({}, {})", lit(&c.name), emit_pg_type(&c.ty));
    if c.nullable {
        s.push_str(".nullable()");
    }
    if let Some(d) = &c.default {
        s.push_str(&format!(".default_expr({})", lit(d)));
    }
    if let Some(g) = &c.generated {
        s.push_str(&format!(".generated_stored({})", lit(&g.expression)));
    }
    if let Some(id) = &c.identity {
        s.push_str(&format!(".identity({})", emit_identity(id)));
    }
    if let Some(u) = &c.unique {
        // Column-level UNIQUE. Introspection folds column uniqueness into
        // table-level unique constraints, so this path is for hand-authored
        // specs; the builders cover the name / no-name cases.
        match &u.name {
            Some(n) => s.push_str(&format!(".unique_named({})", lit(n))),
            None => s.push_str(".unique()"),
        }
    }
    s
}

fn emit_foreign_key(fk: &ForeignKeySpec) -> String {
    let mut s = format!(
        "ForeignKeySpec::new({}, {}, {}, {})",
        lit(&fk.name),
        str_array(&fk.columns_from),
        lit(&fk.table_to),
        str_array(&fk.columns_to),
    );
    if let Some(a) = fk.on_delete {
        s.push_str(&format!(".on_delete({})", emit_ref_action(a)));
    }
    if let Some(a) = fk.on_update {
        s.push_str(&format!(".on_update({})", emit_ref_action(a)));
    }
    s
}

fn emit_unique_constraint(uc: &UniqueConstraintSpec) -> String {
    let mut s = format!(
        "UniqueConstraintSpec::new({}, {})",
        lit(&uc.name),
        str_array(&uc.columns),
    );
    if uc.nulls_not_distinct {
        s.push_str(".nulls_not_distinct()");
    }
    s
}

fn emit_index_column(ic: &IndexColumn) -> String {
    let mut s = if ic.is_expression {
        format!("IndexColumn::expr({})", lit(&ic.expression))
    } else {
        format!("IndexColumn::column({})", lit(&ic.expression))
    };
    if !ic.asc {
        s.push_str(".desc()");
    }
    if let Some(n) = &ic.nulls {
        s.push_str(&format!(".nulls({})", lit(n)));
    }
    if let Some(o) = &ic.opclass {
        s.push_str(&format!(".opclass({})", lit(o)));
    }
    s
}

fn emit_index(idx: &IndexSpec) -> String {
    let cols: Vec<String> = idx.columns.iter().map(emit_index_column).collect();
    let mut s = format!(
        "IndexSpec::new({}, vec![{}])",
        lit(&idx.name),
        cols.join(", "),
    );
    if idx.unique {
        s.push_str(".unique()");
    }
    // Always emit the access method for fidelity (defaults to `btree`).
    s.push_str(&format!(".method({})", lit(&idx.method)));
    if let Some(w) = &idx.where_clause {
        s.push_str(&format!(".where_clause({})", lit(w)));
    }
    s
}

fn emit_policy_as(a: PolicyAs) -> &'static str {
    match a {
        PolicyAs::Permissive => "PolicyAs::Permissive",
        PolicyAs::Restrictive => "PolicyAs::Restrictive",
    }
}

fn emit_policy_for(f: PolicyFor) -> &'static str {
    match f {
        PolicyFor::All => "PolicyFor::All",
        PolicyFor::Select => "PolicyFor::Select",
        PolicyFor::Insert => "PolicyFor::Insert",
        PolicyFor::Update => "PolicyFor::Update",
        PolicyFor::Delete => "PolicyFor::Delete",
    }
}

fn emit_policy(p: &PolicySpec) -> String {
    let mut s = format!("PolicySpec::new({})", lit(&p.name));
    if let Some(a) = p.as_ {
        s.push_str(&format!(".as_permissiveness({})", emit_policy_as(a)));
    }
    if let Some(f) = p.for_ {
        s.push_str(&format!(".for_command({})", emit_policy_for(f)));
    }
    if !p.to.is_empty() {
        s.push_str(&format!(".to_roles({})", str_array(&p.to)));
    }
    if let Some(u) = &p.using {
        s.push_str(&format!(".using({})", lit(u)));
    }
    if let Some(w) = &p.with_check {
        s.push_str(&format!(".with_check({})", lit(w)));
    }
    s
}

/// Emit the body of a `fn() -> PgTableSpec` that rebuilds `t`.
fn emit_table(t: &PgTableSpec) -> String {
    let mut s = String::from("    PgTableSpec::new(");
    s.push_str(&lit(&t.name));
    s.push_str(", vec![\n");
    for c in &t.columns {
        s.push_str("        ");
        s.push_str(&emit_column(c));
        s.push_str(",\n");
    }
    s.push_str("    ])\n");
    s.push_str(&format!("    .in_schema({})\n", lit(&t.schema)));
    if !t.primary_key.is_empty() {
        s.push_str(&format!(
            "    .primary_key({})\n",
            str_array(&t.primary_key)
        ));
    }
    for fk in &t.foreign_keys {
        s.push_str(&format!("    .foreign_key({})\n", emit_foreign_key(fk)));
    }
    for uc in &t.unique_constraints {
        s.push_str(&format!(
            "    .unique_constraint({})\n",
            emit_unique_constraint(uc)
        ));
    }
    for cc in &t.check_constraints {
        s.push_str(&format!(
            "    .check(CheckConstraintSpec::new({}, {}))\n",
            lit(&cc.name),
            lit(&cc.value),
        ));
    }
    for idx in &t.indexes {
        s.push_str(&format!("    .index({})\n", emit_index(idx)));
    }
    for p in &t.policies {
        s.push_str(&format!("    .policy({})\n", emit_policy(p)));
    }
    if t.rls_enabled {
        s.push_str("    .enable_rls()\n");
    }
    s
}

fn emit_enum(e: &EnumTypeSpec) -> String {
    format!(
        "    EnumTypeSpec::new({}, {}).in_schema({})",
        lit(&e.name),
        str_array(&e.values),
        lit(&e.schema),
    )
}

/// Emit a self-contained Rust module that reconstructs `tables` and `enums` via
/// the public builder API. The module exposes `pub fn tables() -> Vec<PgTableSpec>`
/// and `pub fn enums() -> Vec<EnumTypeSpec>`; each table/enum is built by a small
/// private helper so the generated file stays readable and per-item diffable.
///
/// Output is deterministic: inputs are sorted by qualified name, and every
/// optional builder call is emitted in a fixed order only when its field is set.
pub fn emit_spec_rust(tables: &[PgTableSpec], enums: &[EnumTypeSpec]) -> String {
    let mut sorted_tables: Vec<&PgTableSpec> = tables.iter().collect();
    sorted_tables.sort_by_key(|a| a.qualified_name());
    let mut sorted_enums: Vec<&EnumTypeSpec> = enums.iter().collect();
    sorted_enums.sort_by_key(|a| a.qualified_name());

    let mut out = String::new();
    out.push_str(
        "// @generated by postgres_kit::emit_spec_rust — DO NOT EDIT BY HAND.\n\
         //\n\
         // Committed Rust source-of-truth for the Postgres schema, reconstructed from\n\
         // a live-database introspection (SMOODEV-2150 cutover). Each function rebuilds\n\
         // the introspected spec via the kit's public builder API; `check_drift` /\n\
         // `check_enum_drift` validate these specs drift-clean against the live DB.\n\n",
    );
    out.push_str(
        "#[allow(unused_imports, clippy::all)]\n\
         use postgres_kit::{\n    \
         CheckConstraintSpec, ColumnSpec, EnumTypeSpec, ForeignKeySpec, IdentityKind, IdentitySpec,\n    \
         IndexColumn, IndexSpec, PgTableSpec, PgType, PolicyAs, PolicyFor, PolicySpec,\n    \
         ReferentialAction, SequenceOptions, UniqueConstraintSpec,\n};\n\n",
    );

    // ── tables() ──────────────────────────────────────────────────────────────
    out.push_str("/// Every introspected table, sorted by qualified name.\n");
    out.push_str("pub fn tables() -> Vec<PgTableSpec> {\n    vec![\n");
    for i in 0..sorted_tables.len() {
        out.push_str(&format!("        table_{i}(),\n"));
    }
    out.push_str("    ]\n}\n\n");

    // ── enums() ───────────────────────────────────────────────────────────────
    out.push_str("/// Every introspected user-defined enum type, sorted by qualified name.\n");
    out.push_str("pub fn enums() -> Vec<EnumTypeSpec> {\n    vec![\n");
    for i in 0..sorted_enums.len() {
        out.push_str(&format!("        enum_{i}(),\n"));
    }
    out.push_str("    ]\n}\n\n");

    // ── per-table builders ────────────────────────────────────────────────────
    for (i, t) in sorted_tables.iter().enumerate() {
        out.push_str(&format!("// {}\n", t.qualified_name()));
        out.push_str(&format!(
            "#[rustfmt::skip]\nfn table_{i}() -> PgTableSpec {{\n"
        ));
        out.push_str(&emit_table(t));
        out.push_str("}\n\n");
    }

    // ── per-enum builders ─────────────────────────────────────────────────────
    for (i, e) in sorted_enums.iter().enumerate() {
        out.push_str(&format!("// {}\n", e.qualified_name()));
        out.push_str(&format!(
            "#[rustfmt::skip]\nfn enum_{i}() -> EnumTypeSpec {{\n"
        ));
        out.push_str(&emit_enum(e));
        out.push('\n');
        out.push_str("}\n\n");
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{CheckConstraintSpec, GeneratedColumn};

    /// A representative spec set exercising the variants and fields the emitter
    /// must cover.
    fn representative() -> (Vec<PgTableSpec>, Vec<EnumTypeSpec>) {
        let widgets = PgTableSpec::new(
            "widgets",
            vec![
                ColumnSpec::new("id", PgType::Uuid).default_expr("gen_random_uuid()"),
                ColumnSpec::new("org_id", PgType::Uuid),
                ColumnSpec::new("name", PgType::Varchar(Some(255))).nullable(),
                ColumnSpec::new("price", PgType::Numeric(Some((10, 2)))).nullable(),
                ColumnSpec::new("status", PgType::Enum("widget_status".into()))
                    .default_expr("'active'::widget_status"),
                ColumnSpec::new("tags", PgType::Array(Box::new(PgType::Text))).nullable(),
                ColumnSpec::new("embedding", PgType::Vector(Some(1536))).nullable(),
                ColumnSpec::new("body", PgType::Tsvector).nullable(),
                ColumnSpec {
                    name: "search".into(),
                    ty: PgType::Tsvector,
                    nullable: false,
                    default: None,
                    generated: Some(GeneratedColumn {
                        expression: "to_tsvector('english'::regconfig, name)".into(),
                    }),
                    identity: None,
                    unique: None,
                },
                ColumnSpec::new("seq", PgType::Int8).identity(IdentitySpec::always()),
                ColumnSpec::new("created", PgType::Timestamptz).default_expr("now()"),
            ],
        )
        .in_schema("public")
        .primary_key(["id"])
        .foreign_key(
            ForeignKeySpec::new("widgets_org_id_fkey", ["org_id"], "public.orgs", ["id"])
                .on_delete(ReferentialAction::Cascade)
                .on_update(ReferentialAction::NoAction),
        )
        .unique_constraint(UniqueConstraintSpec::new("widgets_name_key", ["name"]))
        .check(CheckConstraintSpec::new(
            "widgets_name_check",
            "(char_length(name) > 0)",
        ))
        .index(IndexSpec::new("widgets_org_id_idx", [IndexColumn::expr("org_id")]).method("btree"))
        .policy(
            PolicySpec::new("org_isolation")
                .for_command(PolicyFor::All)
                .to_roles(["public"])
                .using("(org_id = current_org())"),
        )
        .enable_rls();

        let enums =
            vec![EnumTypeSpec::new("widget_status", ["active", "inactive"]).in_schema("public")];

        (vec![widgets], enums)
    }

    #[test]
    fn emits_module_entrypoints() {
        let (tables, enums) = representative();
        let src = emit_spec_rust(&tables, &enums);
        assert!(src.contains("pub fn tables() -> Vec<PgTableSpec>"));
        assert!(src.contains("pub fn enums() -> Vec<EnumTypeSpec>"));
        assert!(src.contains("use postgres_kit::{"));
        assert!(src.contains("table_0(),"));
        assert!(src.contains("enum_0(),"));
    }

    #[test]
    fn emits_every_pg_type_variant_form() {
        // Direct unit coverage of the type emitter for all variants.
        assert_eq!(emit_pg_type(&PgType::Uuid), "PgType::Uuid");
        assert_eq!(emit_pg_type(&PgType::Text), "PgType::Text");
        assert_eq!(
            emit_pg_type(&PgType::Varchar(None)),
            "PgType::Varchar(None)"
        );
        assert_eq!(
            emit_pg_type(&PgType::Varchar(Some(255))),
            "PgType::Varchar(Some(255))"
        );
        assert_eq!(emit_pg_type(&PgType::Bool), "PgType::Bool");
        assert_eq!(emit_pg_type(&PgType::Int2), "PgType::Int2");
        assert_eq!(emit_pg_type(&PgType::Int4), "PgType::Int4");
        assert_eq!(emit_pg_type(&PgType::Int8), "PgType::Int8");
        assert_eq!(emit_pg_type(&PgType::Float4), "PgType::Float4");
        assert_eq!(emit_pg_type(&PgType::Float8), "PgType::Float8");
        assert_eq!(
            emit_pg_type(&PgType::Numeric(None)),
            "PgType::Numeric(None)"
        );
        assert_eq!(
            emit_pg_type(&PgType::Numeric(Some((10, 2)))),
            "PgType::Numeric(Some((10, 2)))"
        );
        assert_eq!(emit_pg_type(&PgType::Timestamptz), "PgType::Timestamptz");
        assert_eq!(emit_pg_type(&PgType::Timestamp), "PgType::Timestamp");
        assert_eq!(emit_pg_type(&PgType::Date), "PgType::Date");
        assert_eq!(emit_pg_type(&PgType::Jsonb), "PgType::Jsonb");
        assert_eq!(emit_pg_type(&PgType::Json), "PgType::Json");
        assert_eq!(emit_pg_type(&PgType::Bytea), "PgType::Bytea");
        assert_eq!(emit_pg_type(&PgType::Tsvector), "PgType::Tsvector");
        assert_eq!(emit_pg_type(&PgType::Vector(None)), "PgType::Vector(None)");
        assert_eq!(
            emit_pg_type(&PgType::Vector(Some(1536))),
            "PgType::Vector(Some(1536))"
        );
        assert_eq!(
            emit_pg_type(&PgType::Enum("status".into())),
            "PgType::Enum(\"status\".to_string())"
        );
        assert_eq!(
            emit_pg_type(&PgType::Array(Box::new(PgType::Text))),
            "PgType::Array(Box::new(PgType::Text))"
        );
        // Nested array of enum.
        assert_eq!(
            emit_pg_type(&PgType::Array(Box::new(PgType::Enum("status".into())))),
            "PgType::Array(Box::new(PgType::Enum(\"status\".to_string())))"
        );
    }

    #[test]
    fn emits_column_and_table_builder_calls() {
        let (tables, enums) = representative();
        let src = emit_spec_rust(&tables, &enums);

        // Columns: type, default, nullable, generated, identity.
        assert!(src
            .contains("ColumnSpec::new(\"id\", PgType::Uuid).default_expr(\"gen_random_uuid()\")"));
        assert!(src.contains("ColumnSpec::new(\"name\", PgType::Varchar(Some(255))).nullable()"));
        assert!(
            src.contains("ColumnSpec::new(\"price\", PgType::Numeric(Some((10, 2)))).nullable()")
        );
        assert!(src.contains("PgType::Enum(\"widget_status\".to_string())"));
        assert!(src.contains("PgType::Array(Box::new(PgType::Text))"));
        assert!(src.contains("PgType::Vector(Some(1536))"));
        assert!(src.contains(".generated_stored(\"to_tsvector('english'::regconfig, name)\")"));
        assert!(src.contains(".identity(IdentitySpec::always())"));

        // Table-level builder calls.
        assert!(src.contains(".in_schema(\"public\")"));
        assert!(src.contains(".primary_key([\"id\"])"));
        assert!(src.contains(
            ".foreign_key(ForeignKeySpec::new(\"widgets_org_id_fkey\", [\"org_id\"], \"public.orgs\", [\"id\"]).on_delete(ReferentialAction::Cascade).on_update(ReferentialAction::NoAction))"
        ));
        assert!(src.contains(
            ".unique_constraint(UniqueConstraintSpec::new(\"widgets_name_key\", [\"name\"]))"
        ));
        assert!(src.contains(
            ".check(CheckConstraintSpec::new(\"widgets_name_check\", \"(char_length(name) > 0)\"))"
        ));
        assert!(src.contains(".index(IndexSpec::new(\"widgets_org_id_idx\", vec![IndexColumn::expr(\"org_id\")]).method(\"btree\"))"));
        assert!(src.contains(
            ".policy(PolicySpec::new(\"org_isolation\").for_command(PolicyFor::All).to_roles([\"public\"]).using(\"(org_id = current_org())\"))"
        ));
        assert!(src.contains(".enable_rls()"));

        // Enum reconstruction.
        assert!(src.contains("EnumTypeSpec::new(\"widget_status\", [\"active\", \"inactive\"]).in_schema(\"public\")"));
    }

    #[test]
    fn output_is_deterministic_and_sorted() {
        // Two tables provided out of qualified-name order must emit sorted.
        let a = PgTableSpec::new("alpha", vec![ColumnSpec::new("id", PgType::Uuid)])
            .in_schema("public");
        let z =
            PgTableSpec::new("zeta", vec![ColumnSpec::new("id", PgType::Uuid)]).in_schema("public");
        let src1 = emit_spec_rust(&[z.clone(), a.clone()], &[]);
        let src2 = emit_spec_rust(&[a, z], &[]);
        assert_eq!(src1, src2, "emit must be order-independent (sorted)");
        let alpha_at = src1.find("// public.alpha").unwrap();
        let zeta_at = src1.find("// public.zeta").unwrap();
        assert!(
            alpha_at < zeta_at,
            "tables must be sorted by qualified name"
        );
    }

    #[test]
    fn escapes_string_metacharacters() {
        // A default expr containing a double quote and backslash must escape.
        let t = PgTableSpec::new(
            "t",
            vec![ColumnSpec::new("c", PgType::Text).default_expr("a\"b\\c")],
        )
        .in_schema("public");
        let src = emit_spec_rust(&[t], &[]);
        assert!(src.contains(".default_expr(\"a\\\"b\\\\c\")"));
    }
}
