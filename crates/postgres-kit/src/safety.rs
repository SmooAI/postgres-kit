//! Identifier safety and schema bounds — the boundary that makes injection and
//! unbounded tables unrepresentable on the happy path.

use thiserror::Error;

/// Structural limits applied during DDL generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaLimits {
    pub max_columns: usize,
    pub max_identifier_length: usize,
}

impl Default for SchemaLimits {
    fn default() -> Self {
        // Postgres hard limits: identifiers truncate at 63 bytes (NAMEDATALEN - 1),
        // and a table caps near 1600 columns.
        Self {
            max_columns: 1600,
            max_identifier_length: 63,
        }
    }
}

/// Errors raised while validating a spec or rendering DDL.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SchemaError {
    #[error("identifier for {kind} is empty")]
    EmptyIdentifier { kind: &'static str },
    #[error("{kind} identifier {name:?} exceeds {max} characters")]
    IdentifierTooLong {
        kind: &'static str,
        name: String,
        max: usize,
    },
    #[error("{kind} identifier {name:?} is not a valid unquoted Postgres identifier")]
    InvalidIdentifier { kind: &'static str, name: String },
    #[error("table {table:?} has no columns")]
    NoColumns { table: String },
    #[error("table {table:?} has {count} columns, exceeding the limit of {max}")]
    TooManyColumns {
        table: String,
        count: usize,
        max: usize,
    },
    #[error("duplicate column {name:?} in table {table:?}")]
    DuplicateColumn { table: String, name: String },
    #[error("no columns to set for table {table:?}")]
    EmptyColumnSet { table: String },
    #[error("column {column:?} is reserved on table {table:?} and cannot be assigned here")]
    ReservedColumn { table: String, column: String },
    #[error("primary key references unknown column {name:?} in table {table:?}")]
    UnknownPrimaryKeyColumn { table: String, name: String },
    #[error("invalid rename hint {hint:?}: {reason}")]
    InvalidRenameHint { hint: String, reason: &'static str },
}

/// Validate that `name` is a legal *unquoted* Postgres identifier within bounds.
///
/// Grammar: `^[A-Za-z_][A-Za-z0-9_]*$` (ASCII-only by policy — we never emit
/// Unicode or quoted-only identifiers, so the surface stays small and safe).
pub fn validate_identifier<'a>(
    name: &'a str,
    kind: &'static str,
    limits: &SchemaLimits,
) -> Result<&'a str, SchemaError> {
    if name.is_empty() {
        return Err(SchemaError::EmptyIdentifier { kind });
    }
    if name.len() > limits.max_identifier_length {
        return Err(SchemaError::IdentifierTooLong {
            kind,
            name: name.to_string(),
            max: limits.max_identifier_length,
        });
    }
    let mut chars = name.chars();
    let first = chars.next().expect("non-empty checked above");
    let first_ok = first.is_ascii_alphabetic() || first == '_';
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !(first_ok && rest_ok) {
        return Err(SchemaError::InvalidIdentifier {
            kind,
            name: name.to_string(),
        });
    }
    Ok(name)
}

/// Double-quote an identifier, escaping embedded quotes by doubling them.
pub fn quote_identifier(name: &str) -> String {
    let escaped = name.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_identifier() {
        let l = SchemaLimits::default();
        assert!(validate_identifier("organization_id", "column", &l).is_ok());
        assert!(validate_identifier("_private", "column", &l).is_ok());
    }

    #[test]
    fn rejects_empty() {
        let l = SchemaLimits::default();
        assert!(matches!(
            validate_identifier("", "column", &l),
            Err(SchemaError::EmptyIdentifier { .. })
        ));
    }

    #[test]
    fn rejects_too_long() {
        let l = SchemaLimits::default();
        let name = "a".repeat(64);
        assert!(matches!(
            validate_identifier(&name, "column", &l),
            Err(SchemaError::IdentifierTooLong { .. })
        ));
    }

    #[test]
    fn rejects_special_chars_and_leading_digit() {
        let l = SchemaLimits::default();
        assert!(matches!(
            validate_identifier("foo-bar", "column", &l),
            Err(SchemaError::InvalidIdentifier { .. })
        ));
        assert!(matches!(
            validate_identifier("1abc", "column", &l),
            Err(SchemaError::InvalidIdentifier { .. })
        ));
        assert!(matches!(
            validate_identifier("a\"; DROP TABLE x;--", "column", &l),
            Err(SchemaError::InvalidIdentifier { .. })
        ));
    }

    #[test]
    fn quotes_and_escapes() {
        assert_eq!(quote_identifier("table"), "\"table\"");
        assert_eq!(quote_identifier("we\"ird"), "\"we\"\"ird\"");
    }
}
