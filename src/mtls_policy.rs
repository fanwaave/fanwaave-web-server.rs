//! The mTLS downgrade rule used by transport startup.
//!
//! Keep this predicate dependency-free so the real production decision can be
//! compiled and tested even when private upstream Cargo sources are inaccessible.

/// Whether enabling this TCP endpoint would silently downgrade requested mTLS.
#[must_use]
pub(crate) const fn missing_required_connector(
    mtls_requested: bool,
    connector_available: bool,
) -> bool {
    mtls_requested && !connector_available
}

#[cfg(test)]
mod tests {
    use super::missing_required_connector;

    #[test]
    fn mtls_without_a_connector_must_fail_closed() {
        assert!(missing_required_connector(true, false));
    }

    #[test]
    fn mtls_with_a_connector_is_not_a_downgrade() {
        assert!(!missing_required_connector(true, true));
    }

    #[test]
    fn non_mtls_mode_does_not_require_a_connector() {
        assert!(!missing_required_connector(false, false));
        assert!(!missing_required_connector(false, true));
    }
}
