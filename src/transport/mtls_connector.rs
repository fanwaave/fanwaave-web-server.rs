#![forbid(unsafe_code)]
//! Construct a real mutual-TLS client connector without accepting plaintext.
//! The caller must separately validate endpoint/server name with mtls_policy.
//! The connector is inert until an approved gateway wires it.

use std::{io::Cursor, sync::Arc};
use tokio_rustls::{
    rustls::{ClientConfig, RootCertStore},
    TlsConnector,
};

const MAX_PEM_BYTES: usize = 64 * 1024;

/// Redacted errors: never retain or echo private key material.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MtlsConnectorError {
    #[error("missing or oversized trusted TLS roots")]
    InvalidRoots,
    #[error("unusable trusted TLS root certificate")]
    MalformedRoot,
    #[error("missing or oversized TLS client certificate chain")]
    InvalidClientChain,
    #[error("missing or oversized TLS client private key")]
    InvalidClientKey,
    #[error("TLS client certificate/key pairing rejected")]
    InvalidIdentity,
}

/// Build a genuine Rustls client connector from a dedicated CA and identity.
/// Never fall back to a system trust store or anonymous TLS.
///
/// # Errors
/// Refuses absent, malformed, or oversized trusted CA, certificate, key,
/// and mismatched identity without exposing sensitive bytes.
pub fn client_mtls_connector(
    trusted_ca_pem: &[u8],
    client_cert_chain_pem: &[u8],
    client_key_pem: &[u8],
) -> Result<TlsConnector, MtlsConnectorError> {
    if trusted_ca_pem.is_empty() || trusted_ca_pem.len() > MAX_PEM_BYTES {
        return Err(MtlsConnectorError::InvalidRoots);
    }
    if client_cert_chain_pem.is_empty() || client_cert_chain_pem.len() > MAX_PEM_BYTES {
        return Err(MtlsConnectorError::InvalidClientChain);
    }
    if client_key_pem.is_empty() || client_key_pem.len() > MAX_PEM_BYTES {
        return Err(MtlsConnectorError::InvalidClientKey);
    }

    let trusted = rustls_pemfile::certs(&mut Cursor::new(trusted_ca_pem))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| MtlsConnectorError::MalformedRoot)?;
    if trusted.is_empty() {
        return Err(MtlsConnectorError::InvalidRoots);
    }
    let trust = trusted.into_iter().try_fold(
        RootCertStore::empty(),
        |mut store, cert| -> Result<RootCertStore, MtlsConnectorError> {
            store
                .add(cert)
                .map_err(|_| MtlsConnectorError::MalformedRoot)?;
            Ok(store)
        },
    )?;

    let identity = rustls_pemfile::certs(&mut Cursor::new(client_cert_chain_pem))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| MtlsConnectorError::InvalidClientChain)?;
    if identity.is_empty() {
        return Err(MtlsConnectorError::InvalidClientChain);
    }
    let private_key = rustls_pemfile::private_key(&mut Cursor::new(client_key_pem))
        .map_err(|_| MtlsConnectorError::InvalidClientKey)?
        .ok_or(MtlsConnectorError::InvalidClientKey)?;

    let client = ClientConfig::builder()
        .with_root_certificates(trust)
        .with_client_auth_cert(identity, private_key)
        .map_err(|_| MtlsConnectorError::InvalidIdentity)?;
    Ok(TlsConnector::from(Arc::new(client)))
}
