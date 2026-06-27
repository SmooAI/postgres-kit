//! Tenant scaffolding (feature `tenant`): a safe-by-construction, tenant-scoped
//! typed query layer over `sqlx`. The real layer lands in a later phase; this
//! stub reserves the module and proves the feature compiles.

/// Marker for the tenant-scoping surface. Replaced in a later phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TenantStub;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenant_feature_compiles() {
        let stub = TenantStub;
        assert_eq!(stub, TenantStub);
    }
}
