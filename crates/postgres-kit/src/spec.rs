//! The declarative schema DSL. A [`PgTableSpec`] is the single source of truth;
//! everything else the kit produces is derived from it.

/// A Postgres column type. The closed set keeps DDL generation total and the
/// future differ/codegen exhaustive. `Enum` names a user-defined enum type;
/// `Array` wraps any inner type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PgType {
    Uuid,
    Text,
    Varchar(Option<u32>),
    Bool,
    Int2,
    Int4,
    Int8,
    Float4,
    Float8,
    Numeric(Option<(u32, u32)>),
    Timestamptz,
    Timestamp,
    Date,
    Jsonb,
    Json,
    Bytea,
    /// A named Postgres enum type (e.g. `CREATE TYPE managed_website_status AS ENUM (...)`).
    Enum(String),
    /// An array of the inner type, rendered as `<inner>[]`.
    Array(Box<PgType>),
}

impl PgType {
    /// Render the canonical Postgres type text.
    pub fn to_sql_type(&self) -> String {
        match self {
            PgType::Uuid => "uuid".into(),
            PgType::Text => "text".into(),
            PgType::Varchar(None) => "varchar".into(),
            PgType::Varchar(Some(n)) => format!("varchar({n})"),
            PgType::Bool => "boolean".into(),
            PgType::Int2 => "smallint".into(),
            PgType::Int4 => "integer".into(),
            PgType::Int8 => "bigint".into(),
            PgType::Float4 => "real".into(),
            PgType::Float8 => "double precision".into(),
            PgType::Numeric(None) => "numeric".into(),
            PgType::Numeric(Some((p, s))) => format!("numeric({p},{s})"),
            PgType::Timestamptz => "timestamptz".into(),
            PgType::Timestamp => "timestamp".into(),
            PgType::Date => "date".into(),
            PgType::Jsonb => "jsonb".into(),
            PgType::Json => "json".into(),
            PgType::Bytea => "bytea".into(),
            PgType::Enum(name) => name.clone(),
            PgType::Array(inner) => format!("{}[]", inner.to_sql_type()),
        }
    }
}

/// A single column. Construct with [`ColumnSpec::new`] and chain the builder
/// helpers (`.nullable()`, `.default_expr(...)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSpec {
    pub name: String,
    pub ty: PgType,
    pub nullable: bool,
    /// A trusted, developer-authored default expression (emitted verbatim).
    pub default: Option<String>,
}

impl ColumnSpec {
    pub fn new(name: impl Into<String>, ty: PgType) -> Self {
        Self {
            name: name.into(),
            ty,
            nullable: false,
            default: None,
        }
    }

    pub fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }

    pub fn default_expr(mut self, expr: impl Into<String>) -> Self {
        self.default = Some(expr.into());
        self
    }
}

/// A table declaration — the unit of the schema source of truth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgTableSpec {
    pub name: String,
    pub columns: Vec<ColumnSpec>,
    pub primary_key: Vec<String>,
}

impl PgTableSpec {
    pub fn new(name: impl Into<String>, columns: Vec<ColumnSpec>) -> Self {
        Self {
            name: name.into(),
            columns,
            primary_key: Vec::new(),
        }
    }

    pub fn primary_key(mut self, cols: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.primary_key = cols.into_iter().map(Into::into).collect();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_sql_types() {
        assert_eq!(PgType::Uuid.to_sql_type(), "uuid");
        assert_eq!(PgType::Varchar(Some(255)).to_sql_type(), "varchar(255)");
        assert_eq!(
            PgType::Numeric(Some((10, 2))).to_sql_type(),
            "numeric(10,2)"
        );
        assert_eq!(
            PgType::Array(Box::new(PgType::Text)).to_sql_type(),
            "text[]"
        );
        assert_eq!(PgType::Enum("status".into()).to_sql_type(), "status");
        assert_eq!(PgType::Timestamptz.to_sql_type(), "timestamptz");
        assert_eq!(PgType::Float8.to_sql_type(), "double precision");
    }

    #[test]
    fn builder_sets_flags() {
        let c = ColumnSpec::new("x", PgType::Int4)
            .nullable()
            .default_expr("0");
        assert!(c.nullable);
        assert_eq!(c.default.as_deref(), Some("0"));

        let pk = PgTableSpec::new("t", vec![c]).primary_key(["x"]);
        assert_eq!(pk.primary_key, vec!["x".to_string()]);
    }
}
