//! Codegen scaffolding (feature `codegen`): emit serde/sqlx row types and
//! TS/Zod types from a [`crate::spec::PgTableSpec`]. The real generators land in
//! a later phase; this stub reserves the module and proves the feature compiles.

/// Marker for the codegen surface. Replaced by real emitters in a later phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CodegenStub;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codegen_feature_compiles() {
        let stub = CodegenStub;
        assert_eq!(stub, CodegenStub);
    }
}
