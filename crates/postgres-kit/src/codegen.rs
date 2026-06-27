//! Codegen (feature `codegen`): emit a serde/sqlx `FromRow` row type, its SELECT
//! `COLUMNS` const, an optional org-scoped table marker, and TS + Zod types from a
//! [`crate::spec::PgTableSpec`] — the single source of truth.
//!
//! This is the Rust-native port of the smooai `gen-rust-rows.mts` generator (which
//! read the Drizzle schema) folded together with the `@smooai/clickhouse-kit`
//! TS/Zod emitter. Because a [`PgTableSpec`] already knows the real Postgres column
//! types, the row is a drop-in decode target:
//!
//! - `enum` / `numeric` columns decode as `String` and are cast `col::text AS col`
//!   in `COLUMNS` (sqlx can't decode a Postgres enum or `numeric` into a scalar);
//! - `enum[]` / `numeric[]` arrays decode as `Vec<String>` and are cast `::text[]`;
//! - `jsonb`/`json` → [`serde_json::Value`], arrays → `Vec<_>`, nullable → `Option`.
//!
//! Field names are the snake_case DB column (so `FromRow` matches by name); a
//! `#[serde(rename = "camelCase")]` is emitted when the JS key differs; columns
//! whose names are Rust keywords become raw identifiers (`r#type`); columns whose
//! names are Postgres reserved words are quoted in the generated SQL (`"from"`).
//!
//! ## Redaction
//!
//! [`CodegenOptions::exclude`] drops secret-bearing columns from *both* the struct
//! and `COLUMNS`, so the generated row can never serialize or even select them.
//! Listing a column that doesn't exist is a **hard error** — a typo must not
//! silently fail to redact a secret.
//!
//! ## Org scope
//!
//! When a table has both an `id` column and the tenant column (default
//! `organization_id`), the emitter also produces a zero-sized marker + an
//! `impl OrgScopedTable` so list/find/delete bind the tenant filter themselves
//! (the anti-IDOR rule becomes structurally unskippable).

use std::collections::BTreeSet;

use crate::safety::{quote_identifier, validate_identifier, SchemaError, SchemaLimits};
use crate::spec::{ColumnSpec, PgTableSpec, PgType};

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors raised while generating code from a [`PgTableSpec`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodegenError {
    /// An identifier (table or column name) failed validation. Wraps the schema
    /// layer's [`SchemaError`] so codegen never emits an unsafe identifier.
    #[error(transparent)]
    Schema(#[from] SchemaError),

    /// One or more [`CodegenOptions::exclude`] entries name columns that don't
    /// exist on the table. Hard error: a typo must never silently leave a secret
    /// column in the generated row.
    #[error("exclude lists column(s) not present in table {table:?}: {columns}")]
    ExcludeColumnNotFound {
        table: String,
        /// The offending names, comma-joined and sorted for deterministic output.
        columns: String,
    },
}

// ── Options ──────────────────────────────────────────────────────────────────

/// Knobs for code generation. Construct with [`CodegenOptions::new`] and chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenOptions {
    /// Override the Rust struct / TS interface base name. Defaults to
    /// `PascalCase(table) + "Row"` (e.g. `managed_websites` → `ManagedWebsitesRow`).
    pub type_name: Option<String>,
    /// DB column names to drop from the row + `COLUMNS` (secret redaction).
    pub exclude: Vec<String>,
    /// The tenant column that, together with `id`, triggers the org-scoped marker.
    pub tenant_column: String,
    /// Provenance text for the `@generated` header. Defaults to the qualified name.
    pub provenance: Option<String>,
    /// The trait path the emitted `impl` targets for the org-scoped marker.
    pub orm_trait_path: String,
}

impl Default for CodegenOptions {
    fn default() -> Self {
        Self {
            type_name: None,
            exclude: Vec::new(),
            tenant_column: "organization_id".into(),
            provenance: None,
            orm_trait_path: "crate::orm::OrgScopedTable".into(),
        }
    }
}

impl CodegenOptions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the generated struct / interface base name.
    pub fn type_name(mut self, name: impl Into<String>) -> Self {
        self.type_name = Some(name.into());
        self
    }

    /// Redact the given DB columns from the row + `COLUMNS` (and TS/Zod).
    pub fn exclude(mut self, cols: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.exclude = cols.into_iter().map(Into::into).collect();
        self
    }

    /// Set the tenant column used for the org-scoped marker (default `organization_id`).
    pub fn tenant_column(mut self, name: impl Into<String>) -> Self {
        self.tenant_column = name.into();
        self
    }

    /// Set the `@generated` header provenance text.
    pub fn provenance(mut self, text: impl Into<String>) -> Self {
        self.provenance = Some(text.into());
        self
    }

    /// Set the trait path the org-scoped `impl` targets.
    pub fn orm_trait_path(mut self, path: impl Into<String>) -> Self {
        self.orm_trait_path = path.into();
        self
    }
}

// ── Naming helpers ───────────────────────────────────────────────────────────

/// `snake_case` / `kebab-case` → `camelCase` (e.g. `organization_id` → `organizationId`).
fn to_camel_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut upper_next = false;
    let mut first = true;
    for c in s.chars() {
        if c == '_' || c == '-' {
            // Don't uppercase across a leading separator; keep it as a boundary.
            upper_next = !first;
            continue;
        }
        if upper_next {
            out.extend(c.to_uppercase());
            upper_next = false;
        } else {
            out.push(c);
        }
        first = false;
    }
    out
}

/// `snake_case` → `PascalCase` (e.g. `managed_websites` → `ManagedWebsites`).
fn to_pascal_case(s: &str) -> String {
    let camel = to_camel_case(s);
    let mut chars = camel.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => camel,
    }
}

/// `ManagedWebsiteRow` → `MANAGED_WEBSITE_ROW_COLUMNS` (mirrors the TS generator).
fn to_const_name(type_name: &str) -> String {
    let chars: Vec<char> = type_name.chars().collect();
    let mut out = String::with_capacity(type_name.len() + 8);
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            let prev = chars[i - 1];
            if prev.is_ascii_lowercase() || prev.is_ascii_digit() {
                out.push('_');
            }
        }
        out.push(c);
    }
    format!("{}_COLUMNS", out.to_uppercase())
}

/// Default struct / interface base name for a table.
pub fn row_type_name(spec: &PgTableSpec, opts: &CodegenOptions) -> String {
    opts.type_name
        .clone()
        .unwrap_or_else(|| format!("{}Row", to_pascal_case(&spec.name)))
}

/// The Zod select-schema const name (e.g. `managedWebsitesSelectSchema`).
pub fn select_schema_name(spec: &PgTableSpec) -> String {
    format!("{}SelectSchema", to_camel_case(&spec.name))
}

/// The Zod insert-schema const name (e.g. `managedWebsitesInsertSchema`).
pub fn insert_schema_name(spec: &PgTableSpec) -> String {
    format!("{}InsertSchema", to_camel_case(&spec.name))
}

// ── Keyword / reserved-word sets ──────────────────────────────────────────────

/// Rust keywords that can appear as DB column names → emit as raw idents (`r#type`).
fn is_rust_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "try"
    )
}

/// Postgres reserved words that can appear as column names → quote in SQL (`"from"`).
fn is_pg_reserved(s: &str) -> bool {
    matches!(
        s,
        "from"
            | "to"
            | "user"
            | "order"
            | "group"
            | "select"
            | "where"
            | "table"
            | "column"
            | "check"
            | "default"
            | "references"
            | "primary"
            | "foreign"
            | "unique"
            | "constraint"
            | "limit"
            | "offset"
            | "using"
            | "when"
            | "then"
            | "case"
            | "end"
            | "all"
            | "and"
            | "or"
            | "not"
            | "null"
            | "on"
            | "in"
    )
}

// ── Type mapping ──────────────────────────────────────────────────────────────

/// The `::text` / `::text[]` cast a column needs so sqlx can decode it as
/// `String` / `Vec<String>` (Postgres enums and `numeric` can't decode raw).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cast {
    Text,
    TextArray,
}

/// The Rust element type for a [`PgType`] (without the nullable `Option` wrapper).
fn rust_element_type(ty: &PgType) -> String {
    match ty {
        PgType::Uuid => "::uuid::Uuid".into(),
        PgType::Text | PgType::Varchar(_) => "::std::string::String".into(),
        PgType::Bool => "bool".into(),
        PgType::Int2 => "i16".into(),
        PgType::Int4 => "i32".into(),
        PgType::Int8 => "i64".into(),
        PgType::Float4 => "f32".into(),
        PgType::Float8 => "f64".into(),
        // numeric has no lossless scalar without an extra crate; decode as text.
        PgType::Numeric(_) => "::std::string::String".into(),
        PgType::Timestamptz => "::chrono::DateTime<::chrono::Utc>".into(),
        PgType::Timestamp => "::chrono::NaiveDateTime".into(),
        PgType::Date => "::chrono::NaiveDate".into(),
        PgType::Jsonb | PgType::Json => "::serde_json::Value".into(),
        PgType::Bytea => "::std::vec::Vec<u8>".into(),
        // enums decode as text.
        PgType::Enum(_) => "::std::string::String".into(),
        PgType::Array(inner) => format!("::std::vec::Vec<{}>", rust_element_type(inner)),
    }
}

/// The full Rust field type, wrapping in `Option<_>` when the column is nullable.
fn rust_field_type(col: &ColumnSpec) -> String {
    let base = rust_element_type(&col.ty);
    if col.nullable {
        format!("::std::option::Option<{base}>")
    } else {
        base
    }
}

/// The cast a column needs in the SELECT `COLUMNS` list, if any.
fn cast_for(ty: &PgType) -> Option<Cast> {
    match ty {
        PgType::Enum(_) | PgType::Numeric(_) => Some(Cast::Text),
        PgType::Array(inner) => match **inner {
            PgType::Enum(_) | PgType::Numeric(_) => Some(Cast::TextArray),
            _ => None,
        },
        _ => None,
    }
}

/// The TS element type for a [`PgType`] (without the `| null` widening).
fn ts_element_type(ty: &PgType) -> String {
    match ty {
        PgType::Uuid
        | PgType::Text
        | PgType::Varchar(_)
        | PgType::Numeric(_)
        | PgType::Timestamptz
        | PgType::Timestamp
        | PgType::Date
        | PgType::Bytea
        | PgType::Enum(_) => "string".into(),
        PgType::Bool => "boolean".into(),
        PgType::Int2 | PgType::Int4 | PgType::Int8 | PgType::Float4 | PgType::Float8 => {
            "number".into()
        }
        PgType::Jsonb | PgType::Json => "unknown".into(),
        PgType::Array(inner) => format!("{}[]", ts_element_type(inner)),
    }
}

/// The Zod element expression for a [`PgType`] (without `.nullable()`).
fn zod_element_type(ty: &PgType) -> String {
    match ty {
        PgType::Uuid
        | PgType::Text
        | PgType::Varchar(_)
        | PgType::Numeric(_)
        | PgType::Timestamptz
        | PgType::Timestamp
        | PgType::Date
        | PgType::Bytea
        | PgType::Enum(_) => "z.string()".into(),
        PgType::Bool => "z.boolean()".into(),
        PgType::Int2 | PgType::Int4 | PgType::Int8 | PgType::Float4 | PgType::Float8 => {
            "z.number()".into()
        }
        PgType::Jsonb | PgType::Json => "z.unknown()".into(),
        PgType::Array(inner) => format!("z.array({})", zod_element_type(inner)),
    }
}

// ── Column selection / redaction ─────────────────────────────────────────────

/// Validate identifiers, apply the `exclude` redaction, and return the kept
/// columns in declaration order. A typo in `exclude` is a hard error.
fn kept_columns<'a>(
    spec: &'a PgTableSpec,
    opts: &CodegenOptions,
) -> Result<(Vec<&'a ColumnSpec>, Vec<String>), CodegenError> {
    let limits = SchemaLimits::default();
    validate_identifier(&spec.name, "table", &limits)?;

    let exclude: BTreeSet<&str> = opts.exclude.iter().map(String::as_str).collect();
    let present: BTreeSet<&str> = spec.columns.iter().map(|c| c.name.as_str()).collect();

    // A typo in `exclude` must NOT silently leave a secret column in the row.
    let not_found: Vec<&str> = exclude
        .iter()
        .copied()
        .filter(|c| !present.contains(c))
        .collect();
    if !not_found.is_empty() {
        return Err(CodegenError::ExcludeColumnNotFound {
            table: spec.name.clone(),
            columns: not_found.join(", "),
        });
    }

    let mut kept = Vec::new();
    let mut redacted = Vec::new();
    for col in &spec.columns {
        validate_identifier(&col.name, "column", &limits)?;
        if exclude.contains(col.name.as_str()) {
            redacted.push(col.name.clone());
        } else {
            kept.push(col);
        }
    }
    Ok((kept, redacted))
}

// ── Rust emit ─────────────────────────────────────────────────────────────────

/// Emit a complete, `rustfmt`-clean Rust module for a table: the `@generated`
/// header, the serde/sqlx `FromRow` struct, the SELECT `COLUMNS` const, and (when
/// the table has `id` + the tenant column) a zero-sized marker + org-scoped `impl`.
pub fn emit_rust_module(spec: &PgTableSpec, opts: &CodegenOptions) -> Result<String, CodegenError> {
    let (kept, redacted) = kept_columns(spec, opts)?;
    let type_name = row_type_name(spec, opts);
    let const_name = to_const_name(&type_name);

    // Struct fields + SELECT column expressions, in declaration order.
    let mut field_lines = Vec::new();
    let mut col_exprs = Vec::new();
    for col in &kept {
        let camel = to_camel_case(&col.name);
        if camel != col.name {
            field_lines.push(format!("    #[serde(rename = \"{camel}\")]"));
        }
        let ident = if is_rust_keyword(&col.name) {
            format!("r#{}", col.name)
        } else {
            col.name.clone()
        };
        field_lines.push(format!("    pub {ident}: {},", rust_field_type(col)));

        let sql_name = if is_pg_reserved(&col.name) {
            quote_identifier(&col.name)
        } else {
            col.name.clone()
        };
        col_exprs.push(match cast_for(&col.ty) {
            Some(Cast::Text) => format!("{sql_name}::text AS {sql_name}"),
            Some(Cast::TextArray) => format!("{sql_name}::text[] AS {sql_name}"),
            None => sql_name,
        });
    }

    let src = opts
        .provenance
        .clone()
        .unwrap_or_else(|| format!("PgTableSpec `{}`", spec.qualified_name()));
    let redaction_doc = if redacted.is_empty() {
        String::new()
    } else {
        format!(
            "// REDACTED (excluded — never selected or serialized): {}.\n",
            redacted.join(", ")
        )
    };

    let mut out = String::new();
    out.push_str(&format!(
        "// @generated by postgres-kit codegen from {src}.\n"
    ));
    out.push_str(
        "// Do not edit by hand. Enum/numeric columns are cast to text in COLUMNS so sqlx\n",
    );
    out.push_str("// decodes them into String.\n");
    out.push_str(&redaction_doc);
    out.push_str("#![allow(dead_code)]\n\n");

    out.push_str(&format!(
        "/// sqlx FromRow row mirroring `{}`, camelCase JSON.\n",
        spec.name
    ));
    out.push_str(
        "#[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize, ::sqlx::FromRow)]\n",
    );
    out.push_str(&format!("pub struct {type_name} {{\n"));
    out.push_str(&field_lines.join("\n"));
    out.push_str("\n}\n\n");

    out.push_str(&format!(
        "/// SELECT column list for `{type_name}` (enum/numeric columns cast to text).\n"
    ));
    out.push_str(&format!(
        "pub const {const_name}: &str = r#\"{}\"#;\n",
        col_exprs.join(", ")
    ));

    // Org-scoped marker + impl when `id` and the tenant column are both present.
    let kept_names: BTreeSet<&str> = kept.iter().map(|c| c.name.as_str()).collect();
    if kept_names.contains("id") && kept_names.contains(opts.tenant_column.as_str()) {
        let marker = to_pascal_case(&spec.name);
        let table_ref = if spec.schema == "public" {
            spec.name.clone()
        } else {
            spec.qualified_name()
        };
        out.push('\n');
        out.push_str(&format!(
            "/// Org-scoped CRUD marker — `list_by_org` / `find_by_id` / `delete_by_id` bind\n\
             /// `{}` themselves (see [`{}`]).\n",
            opts.tenant_column, opts.orm_trait_path
        ));
        out.push_str(&format!("pub struct {marker};\n"));
        out.push_str(&format!("impl {} for {marker} {{\n", opts.orm_trait_path));
        out.push_str(&format!("    type Row = {type_name};\n"));
        out.push_str(&format!(
            "    const NAME: &'static str = \"{table_ref}\";\n"
        ));
        out.push_str(&format!(
            "    const COLUMNS: &'static str = {const_name};\n"
        ));
        out.push_str("}\n");
    }

    Ok(out)
}

// ── TS / Zod emit ─────────────────────────────────────────────────────────────

/// Emit the TS row `interface` (one field per kept column, camelCase keys).
fn emit_ts_interface(spec: &PgTableSpec, opts: &CodegenOptions) -> Result<String, CodegenError> {
    let (kept, _) = kept_columns(spec, opts)?;
    let mut out = format!("export interface {} {{\n", row_type_name(spec, opts));
    for col in &kept {
        let optional = if col.nullable { "?" } else { "" };
        let mut ts = ts_element_type(&col.ty);
        if col.nullable {
            ts.push_str(" | null");
        }
        out.push_str(&format!(
            "    {}{}: {};\n",
            to_camel_case(&col.name),
            optional,
            ts
        ));
    }
    out.push('}');
    Ok(out)
}

/// Whether a column is server-filled on insert (default / identity / generated),
/// making it `.optional()` in the Zod insert schema.
fn server_filled(col: &ColumnSpec) -> bool {
    col.default.is_some() || col.identity.is_some() || col.generated.is_some()
}

fn emit_zod_object(
    name: &str,
    spec: &PgTableSpec,
    opts: &CodegenOptions,
    insert: bool,
) -> Result<String, CodegenError> {
    let (kept, _) = kept_columns(spec, opts)?;
    let mut out = format!("export const {name} = z.object({{\n");
    for col in &kept {
        let mut zod = zod_element_type(&col.ty);
        if col.nullable {
            zod.push_str(".nullable()");
        }
        // Server-filled columns (DEFAULT / IDENTITY / GENERATED) are optional on insert.
        if insert && server_filled(col) {
            zod.push_str(".optional()");
        }
        out.push_str(&format!("    {}: {},\n", to_camel_case(&col.name), zod));
    }
    out.push_str("});");
    Ok(out)
}

/// Emit the Zod **select** schema (`z.object(...)`) for a table.
pub fn emit_select_schema(
    spec: &PgTableSpec,
    opts: &CodegenOptions,
) -> Result<String, CodegenError> {
    emit_zod_object(&select_schema_name(spec), spec, opts, false)
}

/// Emit the Zod **insert** schema — server-filled columns become `.optional()`.
pub fn emit_insert_schema(
    spec: &PgTableSpec,
    opts: &CodegenOptions,
) -> Result<String, CodegenError> {
    emit_zod_object(&insert_schema_name(spec), spec, opts, true)
}

/// Emit a full TS module for a table: the `zod` import, the row interface, and the
/// select + insert schemas, separated by blank lines.
pub fn emit_ts_module(spec: &PgTableSpec, opts: &CodegenOptions) -> Result<String, CodegenError> {
    Ok(format!(
        "import {{ z }} from \"zod\";\n\n{}\n\n{}\n\n{}\n",
        emit_ts_interface(spec, opts)?,
        emit_select_schema(spec, opts)?,
        emit_insert_schema(spec, opts)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{ColumnSpec, IdentitySpec, PgTableSpec, PgType};

    fn col(name: &str, ty: PgType) -> ColumnSpec {
        ColumnSpec::new(name, ty)
    }

    /// A representative table: uuid PK, tenant column, single-word + multi-word
    /// columns, an enum (→ ::text), a nullable numeric (→ String + ::text), an
    /// array of text, an array of an enum (→ ::text[]), jsonb, a timestamptz, a
    /// Rust-keyword column (`type` → raw ident), and a Postgres-reserved column
    /// (`order` → quoted in SQL).
    fn sample() -> PgTableSpec {
        PgTableSpec::new(
            "managed_websites",
            vec![
                col("id", PgType::Uuid),
                col("organization_id", PgType::Uuid),
                col("domain", PgType::Text),
                col("status", PgType::Enum("managed_website_status".into())),
                col("production_url", PgType::Text).nullable(),
                col("price", PgType::Numeric(Some((10, 2)))).nullable(),
                col("tags", PgType::Array(Box::new(PgType::Text))),
                col(
                    "roles",
                    PgType::Array(Box::new(PgType::Enum("member_role".into()))),
                ),
                col("metadata", PgType::Jsonb),
                col("type", PgType::Text),
                col("order", PgType::Int4),
                col("created_at", PgType::Timestamptz).default_expr("now()"),
            ],
        )
        .primary_key(["id"])
    }

    #[test]
    fn name_helpers() {
        assert_eq!(to_camel_case("organization_id"), "organizationId");
        assert_eq!(to_camel_case("id"), "id");
        assert_eq!(to_camel_case("_leading"), "leading");
        assert_eq!(to_pascal_case("managed_websites"), "ManagedWebsites");
        assert_eq!(
            to_const_name("ManagedWebsitesRow"),
            "MANAGED_WEBSITES_ROW_COLUMNS"
        );
        let opts = CodegenOptions::new();
        assert_eq!(row_type_name(&sample(), &opts), "ManagedWebsitesRow");
        assert_eq!(select_schema_name(&sample()), "managedWebsitesSelectSchema");
        assert_eq!(insert_schema_name(&sample()), "managedWebsitesInsertSchema");
    }

    #[test]
    fn golden_rust_module() {
        let opts = CodegenOptions::new();
        let expected = "\
// @generated by postgres-kit codegen from PgTableSpec `public.managed_websites`.
// Do not edit by hand. Enum/numeric columns are cast to text in COLUMNS so sqlx
// decodes them into String.
#![allow(dead_code)]

/// sqlx FromRow row mirroring `managed_websites`, camelCase JSON.
#[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize, ::sqlx::FromRow)]
pub struct ManagedWebsitesRow {
    pub id: ::uuid::Uuid,
    #[serde(rename = \"organizationId\")]
    pub organization_id: ::uuid::Uuid,
    pub domain: ::std::string::String,
    pub status: ::std::string::String,
    #[serde(rename = \"productionUrl\")]
    pub production_url: ::std::option::Option<::std::string::String>,
    pub price: ::std::option::Option<::std::string::String>,
    pub tags: ::std::vec::Vec<::std::string::String>,
    pub roles: ::std::vec::Vec<::std::string::String>,
    pub metadata: ::serde_json::Value,
    pub r#type: ::std::string::String,
    pub order: i32,
    #[serde(rename = \"createdAt\")]
    pub created_at: ::chrono::DateTime<::chrono::Utc>,
}

/// SELECT column list for `ManagedWebsitesRow` (enum/numeric columns cast to text).
pub const MANAGED_WEBSITES_ROW_COLUMNS: &str = r#\"id, organization_id, domain, status::text AS status, production_url, price::text AS price, tags, roles::text[] AS roles, metadata, type, \"order\", created_at\"#;

/// Org-scoped CRUD marker — `list_by_org` / `find_by_id` / `delete_by_id` bind
/// `organization_id` themselves (see [`crate::orm::OrgScopedTable`]).
pub struct ManagedWebsites;
impl crate::orm::OrgScopedTable for ManagedWebsites {
    type Row = ManagedWebsitesRow;
    const NAME: &'static str = \"managed_websites\";
    const COLUMNS: &'static str = MANAGED_WEBSITES_ROW_COLUMNS;
}
";
        assert_eq!(emit_rust_module(&sample(), &opts).unwrap(), expected);
    }

    #[test]
    fn golden_ts_module() {
        let opts = CodegenOptions::new();
        let expected = "\
import { z } from \"zod\";

export interface ManagedWebsitesRow {
    id: string;
    organizationId: string;
    domain: string;
    status: string;
    productionUrl?: string | null;
    price?: string | null;
    tags: string[];
    roles: string[];
    metadata: unknown;
    type: string;
    order: number;
    createdAt: string;
}

export const managedWebsitesSelectSchema = z.object({
    id: z.string(),
    organizationId: z.string(),
    domain: z.string(),
    status: z.string(),
    productionUrl: z.string().nullable(),
    price: z.string().nullable(),
    tags: z.array(z.string()),
    roles: z.array(z.string()),
    metadata: z.unknown(),
    type: z.string(),
    order: z.number(),
    createdAt: z.string(),
});

export const managedWebsitesInsertSchema = z.object({
    id: z.string(),
    organizationId: z.string(),
    domain: z.string(),
    status: z.string(),
    productionUrl: z.string().nullable(),
    price: z.string().nullable(),
    tags: z.array(z.string()),
    roles: z.array(z.string()),
    metadata: z.unknown(),
    type: z.string(),
    order: z.number(),
    createdAt: z.string().optional(),
});
";
        assert_eq!(emit_ts_module(&sample(), &opts).unwrap(), expected);
    }

    #[test]
    fn exclude_redacts_columns_from_struct_and_columns_const() {
        let spec = PgTableSpec::new(
            "organizations",
            vec![
                col("id", PgType::Uuid),
                col("organization_id", PgType::Uuid),
                col("name", PgType::Text),
                col("encryption_key", PgType::Text),
            ],
        );
        let opts = CodegenOptions::new().exclude(["encryption_key"]);
        let out = emit_rust_module(&spec, &opts).unwrap();
        assert!(out.contains("REDACTED (excluded — never selected or serialized): encryption_key."));
        assert!(!out.contains("pub encryption_key"));
        // The redaction doc names it, but it never appears in the struct or COLUMNS.
        assert!(!out.contains("encryption_key,"));
        assert!(!out.contains("encryption_key\""));
        // TS is redacted too.
        let ts = emit_ts_module(&spec, &opts).unwrap();
        assert!(!ts.contains("encryptionKey"));
    }

    #[test]
    fn exclude_typo_is_a_hard_error() {
        let spec = PgTableSpec::new(
            "t",
            vec![col("id", PgType::Uuid), col("secret", PgType::Text)],
        );
        let opts = CodegenOptions::new().exclude(["secrt", "nope"]);
        let err = emit_rust_module(&spec, &opts).unwrap_err();
        assert_eq!(
            err,
            CodegenError::ExcludeColumnNotFound {
                table: "t".into(),
                // sorted for determinism
                columns: "nope, secrt".into(),
            }
        );
    }

    #[test]
    fn no_org_marker_without_id_and_tenant() {
        // Has organization_id but no id → no marker.
        let spec = PgTableSpec::new(
            "events",
            vec![
                col("organization_id", PgType::Uuid),
                col("kind", PgType::Text),
            ],
        );
        let out = emit_rust_module(&spec, &CodegenOptions::new()).unwrap();
        assert!(!out.contains("OrgScopedTable"));
        assert!(!out.contains("pub struct Events;"));
    }

    #[test]
    fn custom_tenant_column_and_trait_path() {
        let spec = PgTableSpec::new(
            "widgets",
            vec![col("id", PgType::Uuid), col("tenant_id", PgType::Uuid)],
        );
        let opts = CodegenOptions::new()
            .tenant_column("tenant_id")
            .orm_trait_path("postgres_kit::tenant::TenantScoped");
        let out = emit_rust_module(&spec, &opts).unwrap();
        assert!(out.contains("impl postgres_kit::tenant::TenantScoped for Widgets {"));
        assert!(out.contains("bind\n/// `tenant_id` themselves"));
    }

    #[test]
    fn non_public_schema_qualifies_the_table_name() {
        let spec = PgTableSpec::new(
            "users",
            vec![
                col("id", PgType::Uuid),
                col("organization_id", PgType::Uuid),
            ],
        )
        .in_schema("app");
        let out = emit_rust_module(&spec, &CodegenOptions::new()).unwrap();
        assert!(out.contains("const NAME: &'static str = \"app.users\";"));
    }

    #[test]
    fn type_name_override() {
        let spec = PgTableSpec::new("managed_websites", vec![col("id", PgType::Uuid)]);
        let opts = CodegenOptions::new().type_name("ManagedWebsiteRow");
        let out = emit_rust_module(&spec, &opts).unwrap();
        assert!(out.contains("pub struct ManagedWebsiteRow {"));
        assert!(out.contains("pub const MANAGED_WEBSITE_ROW_COLUMNS: &str"));
    }

    #[test]
    fn type_mapping_covers_every_pgtype() {
        let spec = PgTableSpec::new(
            "t",
            vec![
                col("c_uuid", PgType::Uuid),
                col("c_varchar", PgType::Varchar(Some(20))),
                col("c_bool", PgType::Bool),
                col("c_i2", PgType::Int2),
                col("c_i4", PgType::Int4),
                col("c_i8", PgType::Int8),
                col("c_f4", PgType::Float4),
                col("c_f8", PgType::Float8),
                col("c_ts", PgType::Timestamp),
                col("c_date", PgType::Date),
                col("c_json", PgType::Json),
                col("c_bytea", PgType::Bytea),
            ],
        );
        let out = emit_rust_module(&spec, &CodegenOptions::new()).unwrap();
        assert!(out.contains("pub c_uuid: ::uuid::Uuid,"));
        assert!(out.contains("pub c_varchar: ::std::string::String,"));
        assert!(out.contains("pub c_bool: bool,"));
        assert!(out.contains("pub c_i2: i16,"));
        assert!(out.contains("pub c_i4: i32,"));
        assert!(out.contains("pub c_i8: i64,"));
        assert!(out.contains("pub c_f4: f32,"));
        assert!(out.contains("pub c_f8: f64,"));
        assert!(out.contains("pub c_ts: ::chrono::NaiveDateTime,"));
        assert!(out.contains("pub c_date: ::chrono::NaiveDate,"));
        assert!(out.contains("pub c_json: ::serde_json::Value,"));
        assert!(out.contains("pub c_bytea: ::std::vec::Vec<u8>,"));
    }

    #[test]
    fn identity_and_generated_columns_are_optional_on_insert() {
        let spec = PgTableSpec::new(
            "t",
            vec![
                col("id", PgType::Int8).identity(IdentitySpec::always()),
                col("name", PgType::Text),
                col("slug", PgType::Text).generated_stored("lower(name)"),
            ],
        );
        let insert = emit_insert_schema(&spec, &CodegenOptions::new()).unwrap();
        assert!(insert.contains("id: z.number().optional(),"));
        assert!(insert.contains("name: z.string(),"));
        assert!(insert.contains("slug: z.string().optional(),"));
        let select = emit_select_schema(&spec, &CodegenOptions::new()).unwrap();
        assert!(select.contains("id: z.number(),"));
        assert!(!select.contains(".optional()"));
    }
}
