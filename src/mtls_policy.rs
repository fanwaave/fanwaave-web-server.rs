//! Fail-closed TCP TLS policy shared by production wiring and isolated CI tests.
//!
//! This module has no Cargo dependencies: CI can compile and exercise the
//! actual production decision even before private Cargo sources are admitted.

/// Which TCP avenue, if any, startup may construct.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TcpSecurityMode {
    Disabled,
    PlaintextSidecar,
    MutualTls,
}

/// A contradictory transport/TLS setup must fail startup, not downgrade.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TcpPolicyError {
    OrphanTlsConfiguration,
    IncompleteTlsConfiguration,
    MissingTlsConnector,
    UnexpectedTlsConnector,
}

/// Choose a TCP transport only when all four pieces of TLS intent agree.
///
/// A connector without mTLS configuration is *not* permission to use
/// plaintext. TLS files without a TCP endpoint are also not silently ignored.
/// The sole plaintext case is an explicitly unconfigured sidecar transport.
pub(crate) const fn decide_tcp_security_mode(
    tcp_endpoint: bool,
    tls_material: bool,
    mtls_requested: bool,
    connector_available: bool,
) -> Result<TcpSecurityMode, TcpPolicyError> {
    if !tcp_endpoint {
        if tls_material || mtls_requested || connector_available {
            return Err(TcpPolicyError::OrphanTlsConfiguration);
        }
        return Ok(TcpSecurityMode::Disabled);
    }
    if mtls_requested {
        if !tls_material {
            return Err(TcpPolicyError::IncompleteTlsConfiguration);
        }
        if !connector_available {
            return Err(TcpPolicyError::MissingTlsConnector);
        }
        return Ok(TcpSecurityMode::MutualTls);
    }
    if tls_material {
        return Err(TcpPolicyError::IncompleteTlsConfiguration);
    }
    if connector_available {
        return Err(TcpPolicyError::UnexpectedTlsConnector);
    }
    Ok(TcpSecurityMode::PlaintextSidecar)
}

#[cfg(test)]
mod tests {
    use super::{decide_tcp_security_mode, TcpPolicyError, TcpSecurityMode};

    #[test]
    fn all_sixteen_tls_startup_states_are_accounted_for() {
        use TcpPolicyError::{
            IncompleteTlsConfiguration as Incomplete, MissingTlsConnector as Missing,
            OrphanTlsConfiguration as Orphan, UnexpectedTlsConnector as Unexpected,
        };
        use TcpSecurityMode::{Disabled, MutualTls, PlaintextSidecar};

        let cases = [
            (false, false, false, false, Ok(Disabled)),
            (false, false, false, true, Err(Orphan)),
            (false, false, true, false, Err(Orphan)),
            (false, false, true, true, Err(Orphan)),
            (false, true, false, false, Err(Orphan)),
            (false, true, false, true, Err(Orphan)),
            (false, true, true, false, Err(Orphan)),
            (false, true, true, true, Err(Orphan)),
            (true, false, false, false, Ok(PlaintextSidecar)),
            (true, false, false, true, Err(Unexpected)),
            (true, false, true, false, Err(Incomplete)),
            (true, false, true, true, Err(Incomplete)),
            (true, true, false, false, Err(Incomplete)),
            (true, true, false, true, Err(Incomplete)),
            (true, true, true, false, Err(Missing)),
            (true, true, true, true, Ok(MutualTls)),
        ];
        for (endpoint, material, requested, connector, expected) in cases {
            assert_eq!(
                decide_tcp_security_mode(endpoint, material, requested, connector),
                expected,
                "endpoint={endpoint}, material={material}, requested={requested}, connector={connector}"
            );
        }
    }

    #[test]
    fn supplied_connector_cannot_be_discarded_for_plaintext() {
        assert_eq!(
            decide_tcp_security_mode(true, false, false, true),
            Err(TcpPolicyError::UnexpectedTlsConnector)
        );
    }

    #[test]
    fn m_tls_missing_connector_refuses_downgrade() {
        assert_eq!(
            decide_tcp_security_mode(true, true, true, false),
            Err(TcpPolicyError::MissingTlsConnector)
        );
    }

    #[test]
    fn tls_without_endpoint_is_never_silently_ignored() {
        assert_eq!(
            decide_tcp_security_mode(false, true, true, true),
            Err(TcpPolicyError::OrphanTlsConfiguration)
        );
    }

    #[test]
    fn plaintext_requires_explicit_sidecar_case() {
        assert_eq!(
            decide_tcp_security_mode(true, false, false, false),
            Ok(TcpSecurityMode::PlaintextSidecar)
        );
    }
}
