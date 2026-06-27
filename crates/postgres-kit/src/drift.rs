//! Drift scaffolding (feature `drift`): compare expected specs against the live
//! schema introspected via [`crate::PgExecutor`], reporting divergence read-only
//! (mirrors the clickhouse-kit drift gate). The real gate lands in a later phase;
//! this stub reserves the module, the `Drift`/`DriftResult` contract, and proves
//! the feature compiles.

/// A single schema divergence between the expected spec and the live database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    MissingTable {
        table: String,
    },
    MissingColumn {
        table: String,
        column: String,
        expected_type: String,
    },
    ExtraColumn {
        table: String,
        column: String,
        actual_type: String,
    },
    TypeMismatch {
        table: String,
        column: String,
        expected_type: String,
        actual_type: String,
    },
}

/// Result of a drift check.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DriftResult {
    pub checked: Vec<String>,
    pub drift: Vec<Drift>,
}

impl DriftResult {
    /// Whether the live schema matches every expected spec.
    pub fn is_clean(&self) -> bool {
        self.drift.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_result_is_clean() {
        let r = DriftResult::default();
        assert!(r.is_clean());
        let dirty = DriftResult {
            checked: vec!["t".into()],
            drift: vec![Drift::MissingTable { table: "t".into() }],
        };
        assert!(!dirty.is_clean());
    }
}
