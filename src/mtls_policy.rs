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

/// Contradictory TLS settings must fail startup, never choose plaintext.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TcpPolicyError {
    OrphanTlsConfiguration,
    IncompleteTlsConfiguration,
    MissingTlsConnector,
    UnexpectedTlsConnector,
    MissingSidecarPlaintextOptIn,
    ConflictingSidecarPlaintextOptIn,
    InvalidSidecarPlaintextOptIn,
}

/// Accept only an explicit lowercase `true` for TLS-terminating sidecar mode.
pub(crate) fn parse_sidecar_plaintext_opt_in(
    raw: Option<&str>,
) -> Result<bool, TcpPolicyError> {
    match raw {
        None | Some("false") => Ok(false),
        Some("true") => Ok(true),
        Some(_) => Err(TcpPolicyError::InvalidSidecarPlaintextOptIn),
    }
}

/// Select TCP security mode only when the five inputs are consistent.
///
/// Omitting all TLS configuration does not authorize plaintext: that requires
/// `FANWAAVE_API_TCP_SIDECAR_PLAINTEXT=true` as a deliberate operator opt-in.
pub(crate) const fn decide_tcp_security_mode(
    tcp_endpoint: bool,
    tls_material: bool,
    mtls_requested: bool,
    connector_available: bool,
    sidecar_plaintext_opt_in: bool,
) -> Result<TcpSecurityMode, TcpPolicyError> {
    if !tcp_endpoint {
        if tls_material || mtls_requested || connector_available || sidecar_plaintext_opt_in {
            return Err(TcpPolicyError::OrphanTlsConfiguration);
        }
        return Ok(TcpSecurityMode::Disabled);
    }
    if sidecar_plaintext_opt_in && (tls_material || mtls_requested || connector_available) {
        return Err(TcpPolicyError::ConflictingSidecarPlaintextOptIn);
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
    if !sidecar_plaintext_opt_in {
        return Err(TcpPolicyError::MissingSidecarPlaintextOptIn);
    }
    Ok(TcpSecurityMode::PlaintextSidecar)
}

#[cfg(test)]
mod tests {
    use super::{
        decide_tcp_security_mode, parse_sidecar_plaintext_opt_in, TcpPolicyError, TcpSecurityMode,
    };

    #[test]
    fn all_thirty_two_tls_startup_states_are_accounted_for() {
        use TcpPolicyError::{
            ConflictingSidecarPlaintextOptIn as Conflict, IncompleteTlsConfiguration as Incomplete,
            MissingSidecarPlaintextOptIn as NoOptIn, MissingTlsConnector as Missing,
            OrphanTlsConfiguration as Orphan, UnexpectedTlsConnector as Unexpected,
        };
        use TcpSecurityMode::{Disabled, MutualTls, PlaintextSidecar};

        // Endpoint, TLS fields, mTLS request, connector, opt-in, expected mode.
        let cases = [
            (false, false, false, false, false, Ok(Disabled)),
            (false, false, false, false, true, Err(Orphan)),
            (false, false, false, true, false, Err(Orphan)),
            (false, false, false, true, true, Err(Orphan)),
            (false, false, true, false, false, Err(Orphan)),
            (false, false, true, false, true, Err(Orphan)),
            (false, false, true, true, false, Err(Orphan)),
            (false, false, true, true, true, Err(Orphan)),
            (false, true, false, false, false, Err(Orphan)),
            (false, true, false, false, true, Err(Orphan)),
            (false, true, false, true, false, Err(Orphan)),
            (false, true, false, true, true, Err(Orphan)),
            (false, true, true, false, false, Err(Orphan)),
            (false, true, true, false, true, Err(Orphan)),
            (false, true, true, true, false, Err(Orphan)),
            (false, true, true, true, true, Err(Orphan)),
            (true, false, false, false, false, Err(NoOptIn)),
            (true, false, false, false, true, Ok(PlaintextSidecar)),
            (true, false, false, true, false, Err(Unexpected)),
            (true, false, false, true, true, Err(Conflict)),
            (true, false, true, false, false, Err(Incomplete)),
            (true, false, true, false, true, Err(Conflict)),
            (true, false, true, true, false, Err(Incomplete)),
            (true, false, true, true, true, Err(Conflict)),
            (true, true, false, false, false, Err(Incomplete)),
            (true, true, false, false, true, Err(Conflict)),
            (true, true, false, true, false, Err(Incomplete)),
            (true, true, false, true, true, Err(Conflict)),
            (true, true, true, false, false, Err(Missing)),
            (true, true, true, false, true, Err(Conflict)),
            (true, true, true, true, false, Ok(MutualTls)),
            (true, true, true, true, true, Err(Conflict)),
        ];
        assert_eq!(cases.len(), 32);
        for (endpoint, material, requested, connector, sidecar, expected) in cases {
            assert_eq!(
                decide_tcp_security_mode(endpoint, material, requested, connector, sidecar),
                expected,
                "endpoint={endpoint}, material={material}, requested={requested}, connector={connector}, sidecar={sidecar}"
            );
        }
    }

    #[test]
    fn sidecar_plaintext_is_never_implicit() {
        assert_eq!(
            decide_tcp_security_mode(true, false, false, false, false),
            Err(TcpPolicyError::MissingSidecarPlaintextOptIn)
        );
        assert_eq!(
            decide_tcp_security_mode(true, false, false, false, true),
            Ok(TcpSecurityMode::PlaintextSidecar)
        );
    }

    #[test]
    fn connector_cannot_be_dropped_for_plaintext() {
        assert_eq!(
            decide_tcp_security_mode(true, false, false, true, true),
            Err(TcpPolicyError::ConflictingSidecarPlaintextOptIn)
        );
        assert_eq!(
            decide_tcp_security_mode(true, false, false, true, false),
            Err(TcpPolicyError::UnexpectedTlsConnector)
        );
    }

    #[test]
    fn mtls_requires_a_connector_and_never_downgrades() {
        assert_eq!(
            decide_tcp_security_mode(true, true, true, false, false),
            Err(TcpPolicyError::MissingTlsConnector)
        );
        assert_eq!(
            decide_tcp_security_mode(true, true, true, true, false),
            Ok(TcpSecurityMode::MutualTls)
        );
    }

    #[test]
    fn orphaned_or_partial_tls_intent_fails_closed() {
        assert_eq!(
            decide_tcp_security_mode(false, false, false, false, true),
            Err(TcpPolicyError::OrphanTlsConfiguration)
        );
        assert_eq!(
            decide_tcp_security_mode(true, true, false, false, false),
            Err(TcpPolicyError::IncompleteTlsConfiguration)
        );
    }

    #[test]
    fn sidecar_opt_in_is_strictly_parsed_without_echoing_values() {
        assert_eq!(parse_sidecar_plaintext_opt_in(None), Ok(false));
        assert_eq!(parse_sidecar_plaintext_opt_in(Some("false")), Ok(false));
        assert_eq!(parse_sidecar_plaintext_opt_in(Some("true")), Ok(true));
        for input in ["", "TRUE", "1", "yes", " true", "false ", "secret-token"] {
            assert_eq!(
                parse_sidecar_plaintext_opt_in(Some(input)),
                Err(TcpPolicyError::InvalidSidecarPlaintextOptIn)
            );
        }
    }
}
