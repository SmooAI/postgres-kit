//! Rename hints. The differ can't tell a rename from a drop+add by structure
//! alone, so the operator supplies hints in a small text grammar:
//!
//! ```text
//! public.users->public.users1                 # table rename
//! public.users.name->public.users.name1       # column rename
//! enum:public.status->public.status1          # enum-type rename
//! policy:public.users.old->public.users.new   # policy rename (table-scoped)
//! ```
//!
//! Untagged hints are disambiguated by dotted arity: 1 = role (cluster-global
//! bare name), 2 = table/relation, 3 = column. Enum/policy hints carry an
//! `enum:` / `policy:` tag. Untagged arity-2 hints double as the *move/rename*
//! signal for any schema-scoped relation (table, enum, sequence, view) — the two
//! sides may differ in schema, so they cover both `RENAME TO` and `SET SCHEMA`.
//! Parse with [`RenameHints::parse`].

use crate::safety::SchemaError;

/// A relation rename and/or schema move. The two sides may differ in both schema
/// and name; the differ matches a hint against an object's `from` qualified name
/// and resolves whichever of `SET SCHEMA` / `RENAME TO` is implied. Used for
/// tables, enums, sequences and views (the untagged arity-2 hint form).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRename {
    pub from_schema: String,
    pub from: String,
    pub to_schema: String,
    pub to: String,
}

/// A role rename (roles are cluster-global bare names — untagged arity-1 hints).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleRename {
    pub from: String,
    pub to: String,
}

/// A column rename within a table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRename {
    pub schema: String,
    pub table: String,
    pub from: String,
    pub to: String,
}

/// An enum-type rename within a schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumRename {
    pub schema: String,
    pub from: String,
    pub to: String,
}

/// A policy rename within a table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyRename {
    pub schema: String,
    pub table: String,
    pub from: String,
    pub to: String,
}

/// All parsed rename hints, grouped by kind.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RenameHints {
    pub tables: Vec<TableRename>,
    pub columns: Vec<ColumnRename>,
    pub enums: Vec<EnumRename>,
    pub policies: Vec<PolicyRename>,
    pub roles: Vec<RoleRename>,
}

impl RenameHints {
    /// Parse a slice of raw hint strings.
    pub fn parse(raw: &[&str]) -> Result<RenameHints, SchemaError> {
        let mut hints = RenameHints::default();
        for &item in raw {
            parse_one(item, &mut hints)?;
        }
        Ok(hints)
    }

    /// Look up a relation rename/move by its source `(schema, from)`.
    pub fn find_table_rename(&self, schema: &str, from: &str) -> Option<&TableRename> {
        self.tables
            .iter()
            .find(|r| r.from_schema == schema && r.from == from)
    }

    /// Look up a role rename by its source name.
    pub fn find_role_rename(&self, from: &str) -> Option<&RoleRename> {
        self.roles.iter().find(|r| r.from == from)
    }

    /// Look up a column rename by its source `(schema, table, from)`.
    pub fn find_column_rename(
        &self,
        schema: &str,
        table: &str,
        from: &str,
    ) -> Option<&ColumnRename> {
        self.columns
            .iter()
            .find(|r| r.schema == schema && r.table == table && r.from == from)
    }

    /// Look up an enum rename by its source `(schema, from)`.
    pub fn find_enum_rename(&self, schema: &str, from: &str) -> Option<&EnumRename> {
        self.enums
            .iter()
            .find(|r| r.schema == schema && r.from == from)
    }

    /// Look up a policy rename by its source `(schema, table, from)`.
    pub fn find_policy_rename(
        &self,
        schema: &str,
        table: &str,
        from: &str,
    ) -> Option<&PolicyRename> {
        self.policies
            .iter()
            .find(|r| r.schema == schema && r.table == table && r.from == from)
    }
}

fn parse_one(item: &str, hints: &mut RenameHints) -> Result<(), SchemaError> {
    let invalid = |reason: &'static str| SchemaError::InvalidRenameHint {
        hint: item.to_string(),
        reason,
    };

    // A prefix tag is only honored if its body still contains the rename arrow,
    // so a bare `schema.table` (no arrow) doesn't get mis-parsed by a stray colon.
    let (kind, body) = match item.split_once(':') {
        Some(("enum", rest)) if rest.contains("->") => (Kind::Enum, rest),
        Some(("policy", rest)) if rest.contains("->") => (Kind::Policy, rest),
        Some(("table", rest)) if rest.contains("->") => (Kind::Table, rest),
        Some(("column", rest)) if rest.contains("->") => (Kind::Column, rest),
        Some(("enum" | "policy" | "table" | "column", _)) => return Err(invalid("missing '->'")),
        Some((_, _)) if !item.contains("->") => return Err(invalid("missing '->'")),
        Some((other, _)) if !other.is_empty() && !other.contains('.') => {
            return Err(invalid("unknown rename hint prefix"))
        }
        _ => (Kind::Untagged, item),
    };

    let (from, to) = body
        .split_once("->")
        .ok_or_else(|| invalid("missing '->'"))?;
    let from_parts: Vec<&str> = from.split('.').collect();
    let to_parts: Vec<&str> = to.split('.').collect();

    if from.is_empty() || to.is_empty() {
        return Err(invalid("empty side"));
    }
    if from_parts.len() != to_parts.len() {
        return Err(invalid("both sides must have the same dotted arity"));
    }
    if from_parts.iter().any(|p| p.is_empty()) || to_parts.iter().any(|p| p.is_empty()) {
        return Err(invalid("empty path segment"));
    }

    let resolved = match kind {
        Kind::Untagged => match from_parts.len() {
            1 => Kind::Role,
            2 => Kind::Table,
            3 => Kind::Column,
            _ => {
                return Err(invalid(
                    "untagged hint must be role, schema.table, or schema.table.column",
                ))
            }
        },
        other => other,
    };

    match resolved {
        Kind::Table => {
            if from_parts.len() != 2 {
                return Err(invalid("table rename must be schema.table->schema.table"));
            }
            // The two sides may differ in schema (a SET SCHEMA move) and/or name.
            hints.tables.push(TableRename {
                from_schema: from_parts[0].to_string(),
                from: from_parts[1].to_string(),
                to_schema: to_parts[0].to_string(),
                to: to_parts[1].to_string(),
            });
        }
        Kind::Role => {
            if from_parts.len() != 1 {
                return Err(invalid("role rename must be name->name"));
            }
            hints.roles.push(RoleRename {
                from: from_parts[0].to_string(),
                to: to_parts[0].to_string(),
            });
        }
        Kind::Column => {
            if from_parts.len() != 3 {
                return Err(invalid(
                    "column rename must be schema.table.col->schema.table.col",
                ));
            }
            if from_parts[0] != to_parts[0] || from_parts[1] != to_parts[1] {
                return Err(invalid("column rename cannot change schema or table"));
            }
            hints.columns.push(ColumnRename {
                schema: from_parts[0].to_string(),
                table: from_parts[1].to_string(),
                from: from_parts[2].to_string(),
                to: to_parts[2].to_string(),
            });
        }
        Kind::Enum => {
            if from_parts.len() != 2 {
                return Err(invalid("enum rename must be schema.name->schema.name"));
            }
            if from_parts[0] != to_parts[0] {
                return Err(invalid("enum rename cannot change schema"));
            }
            hints.enums.push(EnumRename {
                schema: from_parts[0].to_string(),
                from: from_parts[1].to_string(),
                to: to_parts[1].to_string(),
            });
        }
        Kind::Policy => {
            if from_parts.len() != 3 {
                return Err(invalid(
                    "policy rename must be schema.table.policy->schema.table.policy",
                ));
            }
            if from_parts[0] != to_parts[0] || from_parts[1] != to_parts[1] {
                return Err(invalid("policy rename cannot change schema or table"));
            }
            hints.policies.push(PolicyRename {
                schema: from_parts[0].to_string(),
                table: from_parts[1].to_string(),
                from: from_parts[2].to_string(),
                to: to_parts[2].to_string(),
            });
        }
        Kind::Untagged => unreachable!("resolved above"),
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum Kind {
    Untagged,
    Table,
    Column,
    Enum,
    Policy,
    Role,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_untagged_table_and_column() {
        let h = RenameHints::parse(&[
            "public.users->public.members",
            "public.users.name->public.users.full_name",
        ])
        .unwrap();
        assert_eq!(
            h.tables,
            vec![TableRename {
                from_schema: "public".into(),
                from: "users".into(),
                to_schema: "public".into(),
                to: "members".into(),
            }]
        );
        assert_eq!(
            h.columns,
            vec![ColumnRename {
                schema: "public".into(),
                table: "users".into(),
                from: "name".into(),
                to: "full_name".into(),
            }]
        );
    }

    #[test]
    fn parses_enum_and_policy_tags() {
        let h = RenameHints::parse(&[
            "enum:public.status->public.state",
            "policy:public.docs.old->public.docs.new",
        ])
        .unwrap();
        assert_eq!(h.enums[0].from, "status");
        assert_eq!(h.enums[0].to, "state");
        assert_eq!(h.policies[0].table, "docs");
        assert_eq!(h.policies[0].to, "new");
    }

    #[test]
    fn accepts_explicit_table_column_tags() {
        let h = RenameHints::parse(&["table:public.a->public.b", "column:public.a.x->public.a.y"])
            .unwrap();
        assert_eq!(h.tables.len(), 1);
        assert_eq!(h.columns.len(), 1);
    }

    #[test]
    fn rejects_arity_mismatch() {
        assert!(matches!(
            RenameHints::parse(&["public.users->public.users.name"]),
            Err(SchemaError::InvalidRenameHint { .. })
        ));
    }

    #[test]
    fn rejects_missing_arrow() {
        assert!(matches!(
            RenameHints::parse(&["public.users"]),
            Err(SchemaError::InvalidRenameHint { .. })
        ));
    }

    #[test]
    fn accepts_cross_schema_move() {
        // An untagged arity-2 hint may change schema — it is the move/rename
        // signal for relations (tables, enums, sequences, views).
        let h = RenameHints::parse(&["folder1.enum->folder2.enum"]).unwrap();
        assert_eq!(h.tables[0].from_schema, "folder1");
        assert_eq!(h.tables[0].to_schema, "folder2");
        assert_eq!(h.tables[0].from, "enum");
        assert_eq!(h.tables[0].to, "enum");
    }

    #[test]
    fn parses_role_rename() {
        let h = RenameHints::parse(&["manager->admin"]).unwrap();
        assert_eq!(
            h.roles,
            vec![RoleRename {
                from: "manager".into(),
                to: "admin".into(),
            }]
        );
        assert!(h.find_role_rename("manager").is_some());
    }

    #[test]
    fn rejects_unknown_prefix() {
        assert!(matches!(
            RenameHints::parse(&["bogus:public.a->public.b"]),
            Err(SchemaError::InvalidRenameHint { .. })
        ));
    }

    #[test]
    fn lookups_work() {
        let h = RenameHints::parse(&["public.users.name->public.users.full_name"]).unwrap();
        assert!(h.find_column_rename("public", "users", "name").is_some());
        assert!(h.find_column_rename("public", "users", "missing").is_none());
    }
}
