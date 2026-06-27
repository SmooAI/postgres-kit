//! The diffable schema IR — [`SchemaSnapshot`] and its `Snap*` constituents.
//!
//! A snapshot is the *normalized*, comparison-ready form of a schema. Unlike the
//! [`crate::spec`] DSL (which is authoring-ergonomic), every type here is keyed by
//! name in a [`BTreeMap`] so iteration is deterministic, and every column type is
//! a plain `String` (rendered via [`crate::PgType::to_sql_type`]). The differ
//! ([`crate::differ::diff`]) takes a `from` and a `to` snapshot and emits the
//! [`crate::differ::DdlStatement`]s that turn one into the other.
//!
//! # Builder API (read this to write snapshot literals)
//!
//! Everything is builder-style and chainable. Top-level:
//!
//! ```
//! use postgres_kit::differ::ir::*;
//!
//! let snap = SchemaSnapshot::builder()
//!     .table(
//!         SnapTable::new("public.users")
//!             .col(SnapColumn::new("id", "uuid").primary_key())
//!             .col(SnapColumn::new("email", "text").not_null())
//!             .col(
//!                 SnapColumn::new("created_at", "timestamptz")
//!                     .not_null()
//!                     .default("now()"),
//!             )
//!             .unique(SnapUnique::new("u_email", ["email"]))
//!             .index(SnapIndex::new("idx_email", [SnapIndexColumn::column("email")]))
//!             .foreign_key(SnapForeignKey::new(
//!                 "fk_org",
//!                 ["org_id"],
//!                 "public.orgs",
//!                 ["id"],
//!             ))
//!             .check(SnapCheck::new("c_pos", "n > 0"))
//!             .policy(SnapPolicy::new("p_select").using("org_id = current_org()"))
//!             .enable_rls(),
//!     )
//!     .enum_type(SnapEnum::new("public.role", ["admin", "member"]))
//!     .view(SnapView::new("public.active_users", "SELECT * FROM users"))
//!     .sequence(SnapSequence::new("public.order_seq"))
//!     .role(SnapRole::new("app_user"))
//!     .build();
//!
//! assert!(snap.tables.contains_key("public.users"));
//! ```
//!
//! ## Naming keys
//!
//! - Tables, enums, views, sequences are keyed by their `schema.name` (pass that
//!   to `new`, e.g. `SnapTable::new("public.users")`; the schema and bare name are
//!   split out for you). A name with no `.` is treated as schema `public`.
//! - Roles are keyed by bare name (roles are cluster-global, not schema-scoped).
//! - Columns / constraints / indexes / policies are keyed by their own name
//!   inside the owning [`SnapTable`].
//!
//! ## Composite primary keys
//!
//! A single-column PK lives on the column ([`SnapColumn::primary_key`]); a
//! multi-column PK is a [`SnapCompositePk`] added via [`SnapTable::composite_pk`].

use std::collections::BTreeMap;

use crate::spec::{IdentityKind, PolicyAs, PolicyFor};

/// Split a `schema.name` key into `(schema, name)`. A bare name (no dot) defaults
/// to the `public` schema. Only the first dot splits, so `public.my.weird` →
/// `("public", "my.weird")` — schema names never contain dots in our DSL.
fn split_qualified(qualified: &str) -> (String, String) {
    match qualified.split_once('.') {
        Some((schema, name)) => (schema.to_string(), name.to_string()),
        None => ("public".to_string(), qualified.to_string()),
    }
}

/// A normalized, diffable schema. Every map is a [`BTreeMap`] for deterministic
/// iteration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SchemaSnapshot {
    /// Tables keyed by `schema.table`.
    pub tables: BTreeMap<String, SnapTable>,
    /// Enum types keyed by `schema.name`.
    pub enums: BTreeMap<String, SnapEnum>,
    /// Views keyed by `schema.name`.
    pub views: BTreeMap<String, SnapView>,
    /// Sequences keyed by `schema.name`.
    pub sequences: BTreeMap<String, SnapSequence>,
    /// Roles keyed by bare name.
    pub roles: BTreeMap<String, SnapRole>,
}

impl SchemaSnapshot {
    pub fn builder() -> SchemaSnapshotBuilder {
        SchemaSnapshotBuilder::default()
    }
}

/// Builder for [`SchemaSnapshot`]. Add constructs in any order; keys are derived
/// from each construct's qualified name.
#[derive(Debug, Default)]
pub struct SchemaSnapshotBuilder {
    snapshot: SchemaSnapshot,
}

impl SchemaSnapshotBuilder {
    pub fn table(mut self, table: SnapTable) -> Self {
        self.snapshot.tables.insert(table.key(), table);
        self
    }

    pub fn enum_type(mut self, e: SnapEnum) -> Self {
        self.snapshot.enums.insert(e.key(), e);
        self
    }

    pub fn view(mut self, v: SnapView) -> Self {
        self.snapshot.views.insert(v.key(), v);
        self
    }

    pub fn sequence(mut self, s: SnapSequence) -> Self {
        self.snapshot.sequences.insert(s.key(), s);
        self
    }

    pub fn role(mut self, r: SnapRole) -> Self {
        self.snapshot.roles.insert(r.name.clone(), r);
        self
    }

    pub fn build(self) -> SchemaSnapshot {
        self.snapshot
    }
}

/// A normalized table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapTable {
    pub schema: String,
    pub name: String,
    pub columns: BTreeMap<String, SnapColumn>,
    pub foreign_keys: BTreeMap<String, SnapForeignKey>,
    pub unique_constraints: BTreeMap<String, SnapUnique>,
    pub check_constraints: BTreeMap<String, SnapCheck>,
    pub indexes: BTreeMap<String, SnapIndex>,
    pub composite_primary_keys: BTreeMap<String, SnapCompositePk>,
    pub policies: BTreeMap<String, SnapPolicy>,
    pub rls_enabled: bool,
}

impl SnapTable {
    /// Construct from a `schema.table` (or bare `table`, defaulting to `public`).
    pub fn new(qualified: impl AsRef<str>) -> Self {
        let (schema, name) = split_qualified(qualified.as_ref());
        Self {
            schema,
            name,
            columns: BTreeMap::new(),
            foreign_keys: BTreeMap::new(),
            unique_constraints: BTreeMap::new(),
            check_constraints: BTreeMap::new(),
            indexes: BTreeMap::new(),
            composite_primary_keys: BTreeMap::new(),
            policies: BTreeMap::new(),
            rls_enabled: false,
        }
    }

    /// The `schema.table` map key.
    pub fn key(&self) -> String {
        format!("{}.{}", self.schema, self.name)
    }

    pub fn col(mut self, mut column: SnapColumn) -> Self {
        column.position = self.columns.len() as u32;
        self.columns.insert(column.name.clone(), column);
        self
    }

    pub fn foreign_key(mut self, fk: SnapForeignKey) -> Self {
        self.foreign_keys.insert(fk.name.clone(), fk);
        self
    }

    pub fn unique(mut self, uc: SnapUnique) -> Self {
        self.unique_constraints.insert(uc.name.clone(), uc);
        self
    }

    pub fn check(mut self, cc: SnapCheck) -> Self {
        self.check_constraints.insert(cc.name.clone(), cc);
        self
    }

    pub fn index(mut self, idx: SnapIndex) -> Self {
        self.indexes.insert(idx.name.clone(), idx);
        self
    }

    pub fn composite_pk(mut self, pk: SnapCompositePk) -> Self {
        self.composite_primary_keys.insert(pk.name.clone(), pk);
        self
    }

    pub fn policy(mut self, policy: SnapPolicy) -> Self {
        self.policies.insert(policy.name.clone(), policy);
        self
    }

    pub fn rls(mut self, enabled: bool) -> Self {
        self.rls_enabled = enabled;
        self
    }

    pub fn enable_rls(self) -> Self {
        self.rls(true)
    }
}

/// A normalized identity declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapIdentity {
    pub kind: IdentityKind,
    pub increment: Option<String>,
    pub min_value: Option<String>,
    pub max_value: Option<String>,
    pub start_with: Option<String>,
    pub cache: Option<String>,
    pub cycle: bool,
}

impl SnapIdentity {
    pub fn always() -> Self {
        Self::with_kind(IdentityKind::Always)
    }

    pub fn by_default() -> Self {
        Self::with_kind(IdentityKind::ByDefault)
    }

    fn with_kind(kind: IdentityKind) -> Self {
        Self {
            kind,
            increment: None,
            min_value: None,
            max_value: None,
            start_with: None,
            cache: None,
            cycle: false,
        }
    }
}

/// Column-level uniqueness, normalized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapColumnUnique {
    pub name: Option<String>,
    pub nulls_not_distinct: bool,
}

/// A normalized column. `ty` is the rendered Postgres type string.
///
/// `position` records declaration order within the owning table so `CREATE TABLE`
/// renders columns in author order (the `columns` map itself is a `BTreeMap`, which
/// would otherwise iterate name-sorted). It is assigned by [`SnapTable::col`] and is
/// *not* part of a column's logical identity — the differ compares columns by their
/// fields, ignoring `position`, so reordering alone never emits DDL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapColumn {
    pub name: String,
    pub ty: String,
    pub not_null: bool,
    pub primary_key: bool,
    pub default: Option<String>,
    /// A `STORED` generated-column expression, if any.
    pub generated: Option<String>,
    pub identity: Option<SnapIdentity>,
    pub unique: Option<SnapColumnUnique>,
    /// Declaration order within the owning table (assigned by [`SnapTable::col`]).
    pub position: u32,
}

impl SnapColumn {
    pub fn new(name: impl Into<String>, ty: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ty: ty.into(),
            not_null: false,
            primary_key: false,
            default: None,
            generated: None,
            identity: None,
            unique: None,
            position: 0,
        }
    }

    pub fn not_null(mut self) -> Self {
        self.not_null = true;
        self
    }

    pub fn primary_key(mut self) -> Self {
        self.primary_key = true;
        self
    }

    pub fn default(mut self, expr: impl Into<String>) -> Self {
        self.default = Some(expr.into());
        self
    }

    pub fn generated(mut self, expr: impl Into<String>) -> Self {
        self.generated = Some(expr.into());
        self
    }

    pub fn identity(mut self, identity: SnapIdentity) -> Self {
        self.identity = Some(identity);
        self
    }

    pub fn unique(mut self) -> Self {
        self.unique = Some(SnapColumnUnique {
            name: None,
            nulls_not_distinct: false,
        });
        self
    }

    pub fn unique_named(mut self, name: impl Into<String>) -> Self {
        self.unique = Some(SnapColumnUnique {
            name: Some(name.into()),
            nulls_not_distinct: false,
        });
        self
    }
}

/// A normalized foreign key. `on_update`/`on_delete` are stored as their SQL
/// keyword form (e.g. `CASCADE`) or `None` for the default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapForeignKey {
    pub name: String,
    pub columns_from: Vec<String>,
    pub table_to: String,
    pub columns_to: Vec<String>,
    pub on_update: Option<String>,
    pub on_delete: Option<String>,
}

impl SnapForeignKey {
    pub fn new(
        name: impl Into<String>,
        columns_from: impl IntoIterator<Item = impl Into<String>>,
        table_to: impl Into<String>,
        columns_to: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            name: name.into(),
            columns_from: columns_from.into_iter().map(Into::into).collect(),
            table_to: table_to.into(),
            columns_to: columns_to.into_iter().map(Into::into).collect(),
            on_update: None,
            on_delete: None,
        }
    }

    pub fn on_update(mut self, action: impl Into<String>) -> Self {
        self.on_update = Some(action.into());
        self
    }

    pub fn on_delete(mut self, action: impl Into<String>) -> Self {
        self.on_delete = Some(action.into());
        self
    }
}

/// A normalized unique constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapUnique {
    pub name: String,
    pub columns: Vec<String>,
    pub nulls_not_distinct: bool,
}

impl SnapUnique {
    pub fn new(
        name: impl Into<String>,
        columns: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            name: name.into(),
            columns: columns.into_iter().map(Into::into).collect(),
            nulls_not_distinct: false,
        }
    }

    pub fn nulls_not_distinct(mut self) -> Self {
        self.nulls_not_distinct = true;
        self
    }
}

/// A normalized check constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapCheck {
    pub name: String,
    pub value: String,
}

impl SnapCheck {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// A normalized composite primary key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapCompositePk {
    pub name: String,
    pub columns: Vec<String>,
}

impl SnapCompositePk {
    pub fn new(
        name: impl Into<String>,
        columns: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            name: name.into(),
            columns: columns.into_iter().map(Into::into).collect(),
        }
    }
}

/// A normalized index column / expression member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapIndexColumn {
    pub expression: String,
    pub is_expression: bool,
    pub asc: bool,
    pub nulls: Option<String>,
    pub opclass: Option<String>,
}

impl SnapIndexColumn {
    pub fn column(name: impl Into<String>) -> Self {
        Self {
            expression: name.into(),
            is_expression: false,
            asc: true,
            nulls: None,
            opclass: None,
        }
    }

    pub fn expr(expr: impl Into<String>) -> Self {
        Self {
            expression: expr.into(),
            is_expression: true,
            asc: true,
            nulls: None,
            opclass: None,
        }
    }

    pub fn desc(mut self) -> Self {
        self.asc = false;
        self
    }

    pub fn nulls(mut self, nulls: impl Into<String>) -> Self {
        self.nulls = Some(nulls.into());
        self
    }

    pub fn opclass(mut self, opclass: impl Into<String>) -> Self {
        self.opclass = Some(opclass.into());
        self
    }
}

/// A normalized index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapIndex {
    pub name: String,
    pub columns: Vec<SnapIndexColumn>,
    pub unique: bool,
    pub method: String,
    pub where_clause: Option<String>,
}

impl SnapIndex {
    pub fn new(
        name: impl Into<String>,
        columns: impl IntoIterator<Item = SnapIndexColumn>,
    ) -> Self {
        Self {
            name: name.into(),
            columns: columns.into_iter().collect(),
            unique: false,
            method: "btree".into(),
            where_clause: None,
        }
    }

    pub fn unique(mut self) -> Self {
        self.unique = true;
        self
    }

    pub fn method(mut self, method: impl Into<String>) -> Self {
        self.method = method.into();
        self
    }

    pub fn where_clause(mut self, predicate: impl Into<String>) -> Self {
        self.where_clause = Some(predicate.into());
        self
    }
}

/// A normalized RLS policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapPolicy {
    pub name: String,
    pub as_: Option<PolicyAs>,
    pub for_: Option<PolicyFor>,
    pub to: Vec<String>,
    pub using: Option<String>,
    pub with_check: Option<String>,
}

impl SnapPolicy {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            as_: None,
            for_: None,
            to: Vec::new(),
            using: None,
            with_check: None,
        }
    }

    pub fn as_permissiveness(mut self, as_: PolicyAs) -> Self {
        self.as_ = Some(as_);
        self
    }

    pub fn for_command(mut self, for_: PolicyFor) -> Self {
        self.for_ = Some(for_);
        self
    }

    pub fn to_roles(mut self, roles: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.to = roles.into_iter().map(Into::into).collect();
        self
    }

    pub fn using(mut self, predicate: impl Into<String>) -> Self {
        self.using = Some(predicate.into());
        self
    }

    pub fn with_check(mut self, predicate: impl Into<String>) -> Self {
        self.with_check = Some(predicate.into());
        self
    }
}

/// A normalized enum type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapEnum {
    pub schema: String,
    pub name: String,
    pub values: Vec<String>,
}

impl SnapEnum {
    pub fn new(
        qualified: impl AsRef<str>,
        values: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let (schema, name) = split_qualified(qualified.as_ref());
        Self {
            schema,
            name,
            values: values.into_iter().map(Into::into).collect(),
        }
    }

    pub fn key(&self) -> String {
        format!("{}.{}", self.schema, self.name)
    }
}

/// A normalized view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapView {
    pub schema: String,
    pub name: String,
    pub definition: Option<String>,
    pub materialized: bool,
}

impl SnapView {
    pub fn new(qualified: impl AsRef<str>, definition: impl Into<String>) -> Self {
        let (schema, name) = split_qualified(qualified.as_ref());
        Self {
            schema,
            name,
            definition: Some(definition.into()),
            materialized: false,
        }
    }

    pub fn key(&self) -> String {
        format!("{}.{}", self.schema, self.name)
    }

    pub fn materialized(mut self) -> Self {
        self.materialized = true;
        self
    }
}

/// A normalized sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapSequence {
    pub schema: String,
    pub name: String,
    pub increment: Option<String>,
    pub min_value: Option<String>,
    pub max_value: Option<String>,
    pub start_with: Option<String>,
    pub cache: Option<String>,
    pub cycle: bool,
}

impl SnapSequence {
    pub fn new(qualified: impl AsRef<str>) -> Self {
        let (schema, name) = split_qualified(qualified.as_ref());
        Self {
            schema,
            name,
            increment: None,
            min_value: None,
            max_value: None,
            start_with: None,
            cache: None,
            cycle: false,
        }
    }

    pub fn key(&self) -> String {
        format!("{}.{}", self.schema, self.name)
    }

    pub fn increment(mut self, v: impl Into<String>) -> Self {
        self.increment = Some(v.into());
        self
    }

    pub fn start_with(mut self, v: impl Into<String>) -> Self {
        self.start_with = Some(v.into());
        self
    }

    pub fn cycle(mut self, on: bool) -> Self {
        self.cycle = on;
        self
    }
}

/// A normalized role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapRole {
    pub name: String,
    pub create_db: bool,
    pub create_role: bool,
    pub inherit: bool,
}

impl SnapRole {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            create_db: false,
            create_role: false,
            inherit: true,
        }
    }

    pub fn create_db(mut self, on: bool) -> Self {
        self.create_db = on;
        self
    }

    pub fn create_role(mut self, on: bool) -> Self {
        self.create_role = on;
        self
    }

    pub fn inherit(mut self, on: bool) -> Self {
        self.inherit = on;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_assembles_and_keys_correctly() {
        let snap = SchemaSnapshot::builder()
            .table(
                SnapTable::new("public.users")
                    .col(SnapColumn::new("id", "uuid").primary_key())
                    .col(SnapColumn::new("email", "text").not_null().unique())
                    .index(SnapIndex::new(
                        "idx_email",
                        [SnapIndexColumn::column("email")],
                    ))
                    .enable_rls(),
            )
            .enum_type(SnapEnum::new("role", ["admin", "member"]))
            .view(SnapView::new("public.v", "SELECT 1"))
            .sequence(SnapSequence::new("seq"))
            .role(SnapRole::new("app"))
            .build();

        let users = snap.tables.get("public.users").unwrap();
        assert!(users.columns.contains_key("id"));
        assert!(users.rls_enabled);
        assert!(snap.enums.contains_key("public.role"));
        assert!(snap.views.contains_key("public.v"));
        assert!(snap.sequences.contains_key("public.seq"));
        assert!(snap.roles.contains_key("app"));
    }

    #[test]
    fn bare_name_defaults_to_public() {
        let t = SnapTable::new("widgets");
        assert_eq!(t.schema, "public");
        assert_eq!(t.name, "widgets");
        assert_eq!(t.key(), "public.widgets");
    }
}
