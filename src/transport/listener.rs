#![forbid(unsafe_code)]
//! Fail-closed admission for the temporary static-only web listener.
//! A configured backend must never silently degrade to a static-only service.

use std::{
    collections::BTreeMap,
    fmt, io,
    net::{SocketAddr, TcpListener},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionError {
    InvalidBind,
    NonLoopbackBind,
    GatewayUnavailable,
    Bind(io::ErrorKind),
    Nonblocking(io::ErrorKind),
}

impl fmt::Display for AdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidBind => "web listener bind address is invalid",
            Self::NonLoopbackBind => {
                "public web binding requires authenticated gateway integration"
            }
            Self::GatewayUnavailable => {
                "configured web backend requires unavailable authenticated gateway"
            }
            Self::Bind(_) => "web listener could not bind",
            Self::Nonblocking(_) => "web listener could not enter nonblocking mode",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for AdmissionError {}

fn has_unwired_backend(environment: &BTreeMap<String, String>) -> bool {
    environment.keys().any(|key| {
        key.starts_with("FANWAAVE_API_")
            || key.starts_with("FANWAAVE_WEB_CLIENT_")
            || matches!(
                key.as_str(),
                "FANWAAVE_NATS_URL" | "FANWAAVE_READONLY_DATABASE_URL" | "FANWAAVE_DATABASE_URL"
            )
    })
}

/// Reject unsafe or unwired modes before binding a real, nonblocking listener.
/// Diagnostics retain only an OS error *kind*, never raw error text or secrets.
pub fn prepare_listener(
    bind: &str,
    domain_backend_configured: bool,
    ambient: &BTreeMap<String, String>,
    applied: &BTreeMap<String, String>,
) -> Result<TcpListener, AdmissionError> {
    let address = bind
        .parse::<SocketAddr>()
        .map_err(|_| AdmissionError::InvalidBind)?;
    if !address.ip().is_loopback() {
        return Err(AdmissionError::NonLoopbackBind);
    }
    if domain_backend_configured || has_unwired_backend(ambient) || has_unwired_backend(applied) {
        return Err(AdmissionError::GatewayUnavailable);
    }

    let listener =
        TcpListener::bind(address).map_err(|error| AdmissionError::Bind(error.kind()))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| AdmissionError::Nonblocking(error.kind()))?;
    Ok(listener)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean() -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    #[test]
    fn actually_binds_loopback_and_sets_nonblocking() -> Result<(), Box<dyn std::error::Error>> {
        let listener = prepare_listener("127.0.0.1:0", false, &clean(), &clean())?;
        let address = listener.local_addr()?;
        assert!(address.ip().is_loopback());
        assert_ne!(address.port(), 0);
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        Ok(())
    }

    #[test]
    fn refuses_public_bind_before_opening_socket() {
        for address in ["0.0.0.0:0", "[::]:0", "192.0.2.1:80"] {
            assert_eq!(
                prepare_listener(address, false, &clean(), &clean()).unwrap_err(),
                AdmissionError::NonLoopbackBind
            );
        }
    }

    #[test]
    fn malformed_bind_fails_with_typed_error() {
        assert_eq!(
            prepare_listener("secret-user:token@host:8080", false, &clean(), &clean()).unwrap_err(),
            AdmissionError::InvalidBind
        );
    }

    #[test]
    fn rejects_unwired_database_or_http_configuration_even_if_empty() {
        assert_eq!(
            prepare_listener("127.0.0.1:0", true, &clean(), &clean()).unwrap_err(),
            AdmissionError::GatewayUnavailable
        );
        let applied = BTreeMap::from([("FANWAAVE_API_HTTP_BASE".into(), String::new())]);
        assert_eq!(
            prepare_listener("127.0.0.1:0", false, &clean(), &applied).unwrap_err(),
            AdmissionError::GatewayUnavailable
        );
    }

    #[test]
    fn rejects_configured_transport_family_in_both_sources() {
        for key in [
            "FANWAAVE_API_URL",
            "FANWAAVE_API_MTLS_ADDR",
            "FANWAAVE_API_TLS_SERVER_NAME",
            "FANWAAVE_API_TCP_SIDECAR_PLAINTEXT",
            "FANWAAVE_NATS_URL",
            "FANWAAVE_WEB_CLIENT_CERT_FILE",
            "FANWAAVE_READONLY_DATABASE_URL",
            "FANWAAVE_DATABASE_URL",
        ] {
            let ambient = BTreeMap::from([(key.to_owned(), "secret-value".to_owned())]);
            assert_eq!(
                prepare_listener("127.0.0.1:0", false, &ambient, &clean()).unwrap_err(),
                AdmissionError::GatewayUnavailable,
                "{key}"
            );
        }
    }

    #[test]
    fn occupied_port_is_a_typed_failure_not_success() -> Result<(), Box<dyn std::error::Error>> {
        let occupied = TcpListener::bind("127.0.0.1:0")?;
        let port = occupied.local_addr()?.port();
        assert_eq!(
            prepare_listener(&format!("127.0.0.1:{port}"), false, &clean(), &clean()).unwrap_err(),
            AdmissionError::Bind(io::ErrorKind::AddrInUse)
        );
        Ok(())
    }

    #[test]
    fn errors_never_echo_addresses_or_credentials() {
        let ambient = BTreeMap::from([("FANWAAVE_API_URL".into(), "token=do-not-echo".into())]);
        let error = prepare_listener("127.0.0.1:0", false, &ambient, &clean()).unwrap_err();
        assert!(!error.to_string().contains("do-not-echo"));
        assert!(!format!("{error:?}").contains("do-not-echo"));
    }
}
