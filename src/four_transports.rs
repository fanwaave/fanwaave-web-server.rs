#![forbid(unsafe_code)]
//! The four avenues this web server can use to reach `fanwaave-api-server`.
//!
//! Generated from the fleet-wide contract in
//! <https://github.com/ORESoftware/ores-transport>, which owns the parts that
//! are shared *and* subtle: the envelope, the byte ceilings, absolute
//! deadlines, reply correlation, global backpressure, and the
//! redeliver-versus-terminate decision. This module owns only what is specific
//! to fanwaave — the environment prefix, the service slug that names its NATS
//! subjects, and the api route that accepts an envelope.
//!
//! | # | Mode | What happens |
//! |---|------|--------------|
//! | 1 | `direct_read` | this server queries Postgres itself through `fanwaave-lib-core`, read-only |
//! | 2 | `http` | stateless request to the `fanwaave-api-server` cluster |
//! | 3 | `tcp` | framed, held-open connection to the `fanwaave-api-server` cluster |
//! | 4 | `jet_stream` | published to NATS; the api server answers asynchronously |
//!
//! # Finishing the wiring
//!
//! One thing is left to this repository, because only this repository knows
//! what an operation *is*: implement [`ores_transport::DirectReader`] over
//! `fanwaave-lib-core`'s read context, and hand it to [`gateway_from_env`]. See
//! `docs/four-transports.md`.

use crate::mtls_policy::{decide_tcp_security_mode, TcpPolicyError, TcpSecurityMode};

use ores_transport::{
    DirectReader, Gateway, HttpRoute, HttpTransport, NatsTransport, PersistentTcpClient,
    TcpEndpoint, TlsConnector, TransportConfig, TransportMode, DEFAULT_TRANSPORT_TIMEOUT,
};
use serde::{de::DeserializeOwned, Serialize};
use std::{net::SocketAddr, sync::Arc};

/// Environment prefix for every variable this service reads.
///
/// `FANWAAVE_API_URL`, `FANWAAVE_NATS_URL`, and the rest; see the
/// table in `ores-transport`'s README.
pub const ENV_PREFIX: &str = "FANWAAVE";

/// Service slug, which derives the NATS subjects and JetStream stream names.
///
/// It must match the slug the api server uses, or the two will be publishing
/// and consuming on different subjects while both look healthy.
pub const SERVICE_SLUG: &str = "fanwaave";

/// The api server route that accepts an envelope on avenue 2.
pub const ENVELOPE_PATH: &str = "/v1/operations";

/// A typed, credential-safe failure while wiring an avenue at startup.
#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    /// An unsafe or incomplete environment configuration was supplied.
    #[error(transparent)]
    InvalidConfig(#[from] ores_transport::ConfigError),
    /// The upstream HTTP client could not be configured.
    #[error("HTTP transport client configuration failed")]
    HttpClient,
    /// The TCP endpoint address is malformed.
    #[error("TCP endpoint address is invalid")]
    TcpAddress,
    /// A requested mutual-TLS connection has no injected connector.
    #[error("TCP mTLS requested but no TLS connector was provided")]
    MissingTlsConnector,
    /// A TLS connector or TLS fields were supplied without a TCP endpoint.
    #[error("TCP TLS setup supplied without TCP endpoint")]
    OrphanTlsConfiguration,
    /// TLS fields are incomplete for a configured TCP endpoint.
    #[error("TCP mTLS configuration incomplete")]
    IncompleteTlsConfiguration,
    /// A connector was supplied but the environment did not request mTLS.
    #[error("TCP TLS connector supplied without matching mTLS configuration")]
    UnexpectedTlsConnector,
    /// Configured security mode and available connector contradicted each other.
    #[error("TCP security setup was inconsistent")]
    InconsistentTlsConfiguration,
    /// The NATS connection could not be established.
    #[error("NATS transport unavailable")]
    NatsUnavailable,
}

impl From<TcpPolicyError> for SetupError {
    fn from(value: TcpPolicyError) -> Self {
        match value {
            TcpPolicyError::OrphanTlsConfiguration => Self::OrphanTlsConfiguration,
            TcpPolicyError::IncompleteTlsConfiguration => Self::IncompleteTlsConfiguration,
            TcpPolicyError::MissingTlsConnector => Self::MissingTlsConnector,
            TcpPolicyError::UnexpectedTlsConnector => Self::UnexpectedTlsConnector,
        }
    }
}

/// Build the gateway for this process from the environment.
///
/// Every avenue past the first is attached only if its variables are set, so a
/// deployment without a NATS cluster is a normal deployment: asking it for
/// `jet_stream` yields `TransportError::NotConfigured` rather than failing to
/// start. A variable that is present and *unsafe* — a plaintext api URL off
/// loopback, half-configured mutual TLS — does fail startup, because that is a
/// misconfiguration wearing a working service's clothes.
///
/// `tls` is the client side of mutual TLS for avenue 3. Pass `None` in a mesh
/// that terminates TLS in a sidecar.
///
/// # Errors
/// [`SetupError`] if a variable is present and unusable, if the stateful
/// address will not parse, or if NATS refuses the connection.
pub async fn gateway_from_env<O, T>(
    direct: Arc<dyn DirectReader<O, T>>,
    tls: Option<TlsConnector>,
) -> Result<Gateway<O, T>, SetupError>
where
    O: Serialize + Send + Sync,
    T: DeserializeOwned + Send,
{
    let config = TransportConfig::from_env(ENV_PREFIX)?;
    let tls_material_present = config.api_tls_server_name.is_some()
        || config.client_cert_file.is_some()
        || config.client_key_file.is_some()
        || config.ca_file.is_some();
    // Reject contradictory TLS intent before constructing any transport.
    let tcp_mode = decide_tcp_security_mode(
        config.api_tcp_addr.is_some(),
        tls_material_present,
        config.uses_mtls(),
        tls.is_some(),
    )?;
    let gateway = Gateway::new(direct);

    // Avenue 2.
    let gateway = if let Some(api_url) = config.api_url.as_deref() {
        let client = HttpTransport::client_for(api_url, DEFAULT_TRANSPORT_TIMEOUT)
            .map_err(|_| SetupError::HttpClient)?;
        gateway.with_http(
            HttpTransport::new(client, api_url, DEFAULT_TRANSPORT_TIMEOUT),
            HttpRoute {
                envelope_path: ENVELOPE_PATH.to_owned(),
            },
        )
    } else {
        gateway
    };

    // Avenue 3.
    let gateway = if let Some(raw) = config.api_tcp_addr.as_deref() {
        let address: SocketAddr = raw.parse().map_err(|_| SetupError::TcpAddress)?;
        let endpoint = TcpEndpoint {
            address,
            server_name: config.api_tls_server_name.clone(),
            timeout: DEFAULT_TRANSPORT_TIMEOUT,
        };
        let client = match (tcp_mode, tls) {
            (TcpSecurityMode::MutualTls, Some(connector)) => {
                PersistentTcpClient::tls(endpoint, connector)
            }
            (TcpSecurityMode::PlaintextSidecar, None) => PersistentTcpClient::plaintext(endpoint),
            (TcpSecurityMode::MutualTls, None) => {
                return Err(SetupError::MissingTlsConnector);
            }
            (TcpSecurityMode::PlaintextSidecar, Some(_)) => {
                return Err(SetupError::UnexpectedTlsConnector);
            }
            (TcpSecurityMode::Disabled, None) | (TcpSecurityMode::Disabled, Some(_)) => {
                return Err(SetupError::InconsistentTlsConfiguration);
            }
        };
        gateway.with_tcp(Arc::new(client))
    } else {
        gateway
    };

    // Avenue 4.
    let gateway = if let Some(nats_url) = config.nats_url.as_deref() {
        let context = ores_transport::connect_nats(nats_url)
            .await
            .map_err(|_| SetupError::NatsUnavailable)?;
        gateway.with_nats(NatsTransport::new(
            context,
            SERVICE_SLUG,
            DEFAULT_TRANSPORT_TIMEOUT,
        ))
    } else {
        gateway
    };

    Ok(gateway)
}

/// The avenues this process can actually serve, for a capabilities endpoint.
///
/// Worth exposing: an operator reading "tcp is missing" from the service
/// itself beats inferring it from a 503 at 3am.
#[must_use]
pub fn advertised_modes<O, T>(gateway: &Gateway<O, T>) -> Vec<&'static str>
where
    O: Serialize + Send + Sync,
    T: DeserializeOwned + Send,
{
    gateway
        .available_modes()
        .into_iter()
        .map(|mode| mode.as_wire())
        .collect()
}

/// Parse a caller-supplied `?mode=` value.
///
/// Refuses an unknown spelling rather than defaulting to one. Silently falling
/// back to HTTP would make an end-to-end suite that thinks it is exercising
/// all four avenues actually exercise one.
#[must_use]
pub fn requested_mode(raw: Option<&str>) -> Option<TransportMode> {
    match raw {
        None => Some(TransportMode::Http),
        Some(value) => TransportMode::from_wire(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_service_slug_matches_the_environment_prefix() {
        // These two drift apart in exactly one way: someone renames the
        // service and updates one of them. Then the web server publishes to
        // subjects the api server does not consume, and both look healthy.
        assert_eq!(SERVICE_SLUG.replace('-', "_").to_uppercase(), ENV_PREFIX);
    }

    #[test]
    fn security_policy_errors_map_to_startup_errors() {
        assert!(matches!(
            SetupError::from(TcpPolicyError::MissingTlsConnector),
            SetupError::MissingTlsConnector
        ));
        assert!(matches!(
            SetupError::from(TcpPolicyError::UnexpectedTlsConnector),
            SetupError::UnexpectedTlsConnector
        ));
        assert!(matches!(
            SetupError::from(TcpPolicyError::OrphanTlsConfiguration),
            SetupError::OrphanTlsConfiguration
        ));
        assert!(matches!(
            SetupError::from(TcpPolicyError::IncompleteTlsConfiguration),
            SetupError::IncompleteTlsConfiguration
        ));
    }

    #[test]
    fn an_absent_mode_defaults_to_http_and_an_unknown_one_is_refused() {
        assert_eq!(requested_mode(None), Some(TransportMode::Http));
        assert_eq!(requested_mode(Some("tcp")), Some(TransportMode::Tcp));
        assert_eq!(
            requested_mode(Some("jet_stream")),
            Some(TransportMode::JetStream)
        );
        assert_eq!(requested_mode(Some("grpc")), None);
        assert_eq!(requested_mode(Some("")), None);
    }
}
