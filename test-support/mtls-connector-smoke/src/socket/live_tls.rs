#![forbid(unsafe_code)]

use std::{io::Cursor, path::PathBuf, sync::Arc, time::Duration};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
use tokio_rustls::{
    rustls::{
        pki_types::{CertificateDer, PrivateKeyDer, ServerName},
        server::WebPkiClientVerifier,
        ClientConfig, RootCertStore, ServerConfig,
    },
    TlsAcceptor, TlsConnector,
};

use crate::mtls_connector::{client_mtls_connector, MtlsConnectorError};

#[derive(Debug, thiserror::Error)]
enum TestError {
    #[error("TLS fixture I/O failed")]
    Io(#[from] std::io::Error),
    #[error("TLS configuration was invalid")]
    Tls(#[from] tokio_rustls::rustls::Error),
    #[error("TLS fixture environment was missing")]
    Environment(#[from] std::env::VarError),
    #[error("TLS connector rejected an identity")]
    Connector(#[from] MtlsConnectorError),
    #[error("TLS handshake exceeded the deadline")]
    Timeout(#[from] tokio::time::error::Elapsed),
    #[error("TLS handshake background task failed")]
    Task(#[from] tokio::task::JoinError),
    #[error("fixture PEM private key is missing")]
    MissingPrivateKey,
    #[error("invalid TLS verifier or server name")]
    InvalidTlsConfig,
}

type TestResult<T> = Result<T, TestError>;

fn fixture(name: &str) -> TestResult<Vec<u8>> {
    let root = PathBuf::from(std::env::var("FANWAAVE_TLS_FIXTURES")?);
    Ok(std::fs::read(root.join(name))?)
}

fn certificates(raw: &[u8]) -> TestResult<Vec<CertificateDer<'static>>> {
    Ok(rustls_pemfile::certs(&mut Cursor::new(raw)).collect::<Result<Vec<_>, _>>()?)
}

fn private_key(raw: &[u8]) -> TestResult<PrivateKeyDer<'static>> {
    rustls_pemfile::private_key(&mut Cursor::new(raw))?.ok_or(TestError::MissingPrivateKey)
}

fn trust_roots() -> TestResult<RootCertStore> {
    let mut roots = RootCertStore::empty();
    for cert in certificates(&fixture("ca.pem")?)? {
        roots.add(cert)?;
    }
    Ok(roots)
}

fn strict_server() -> TestResult<TlsAcceptor> {
    let root = Arc::new(trust_roots()?);
    let verifier = WebPkiClientVerifier::builder(root)
        .build()
        .map_err(|_| TestError::InvalidTlsConfig)?;
    let config = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(
            certificates(&fixture("server.pem")?)?,
            private_key(&fixture("server.key")?)?,
        )?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

fn client(cert_path: &str, key_path: &str) -> TestResult<TlsConnector> {
    Ok(client_mtls_connector(
        &fixture("ca.pem")?,
        &fixture(cert_path)?,
        &fixture(key_path)?,
    )?)
}

async fn exchange(client: TlsConnector, hostname: &str) -> TestResult<(bool, bool)> {
    let server = strict_server()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = listener.local_addr()?;
    let task = tokio::spawn(async move {
        match listener.accept().await {
            Ok((socket, _)) => match server.accept(socket).await {
                Ok(mut tls) => tls.write_all(b"OK").await.is_ok(),
                Err(_) => false,
            },
            Err(_) => false,
        }
    });
    let socket = TcpStream::connect(endpoint).await?;
    let name =
        ServerName::try_from(hostname.to_owned()).map_err(|_| TestError::InvalidTlsConfig)?;
    let client_received = match timeout(Duration::from_secs(5), client.connect(name, socket)).await
    {
        Ok(Ok(mut tls)) => {
            let mut reply = [0_u8; 2];
            matches!(
                timeout(Duration::from_secs(5), tls.read_exact(&mut reply)).await,
                Ok(Ok(_))
            ) && reply == *b"OK"
        }
        Ok(Err(_)) | Err(_) => false,
    };
    let server_authenticated = timeout(Duration::from_secs(5), task).await??;
    Ok((client_received, server_authenticated))
}

#[tokio::test]
async fn trusted_client_and_server_complete_real_mutual_tls() -> TestResult<()> {
    let negotiated = exchange(client("client.pem", "client.key")?, "localhost").await?;
    assert_eq!(negotiated, (true, true));
    Ok(())
}

#[tokio::test]
async fn server_refuses_client_with_no_certificate() -> TestResult<()> {
    let anonymous = ClientConfig::builder()
        .with_root_certificates(trust_roots()?)
        .with_no_client_auth();
    let negotiated = exchange(TlsConnector::from(Arc::new(anonymous)), "localhost").await?;
    assert_eq!(negotiated, (false, false));
    Ok(())
}

#[tokio::test]
async fn server_refuses_certificate_from_unknown_client_ca() -> TestResult<()> {
    let negotiated = exchange(client("rogue-client.pem", "rogue-client.key")?, "localhost").await?;
    assert_eq!(negotiated, (false, false));
    Ok(())
}

#[tokio::test]
async fn client_refuses_wrong_server_name_even_with_trusted_ca() -> TestResult<()> {
    let negotiated = exchange(client("client.pem", "client.key")?, "not-localhost.invalid").await?;
    assert_eq!(negotiated.0, false);
    Ok(())
}

#[test]
fn empty_oversized_and_broken_identity_fails_before_network() -> TestResult<()> {
    let root = fixture("ca.pem")?;
    let chain = fixture("client.pem")?;
    let key = fixture("client.key")?;
    assert!(matches!(
        client_mtls_connector(b"", &chain, &key),
        Err(MtlsConnectorError::InvalidRoots)
    ));
    assert!(matches!(
        client_mtls_connector(&root, b"", &key),
        Err(MtlsConnectorError::InvalidClientChain)
    ));
    assert!(matches!(
        client_mtls_connector(&root, &chain, b""),
        Err(MtlsConnectorError::InvalidClientKey)
    ));
    assert!(matches!(
        client_mtls_connector(&vec![b'X'; 65_537], &chain, &key),
        Err(MtlsConnectorError::InvalidRoots)
    ));
    assert!(client_mtls_connector(b"not a PEM", &chain, &key).is_err());
    assert!(client_mtls_connector(&root, &chain, b"not a key").is_err());
    Ok(())
}
