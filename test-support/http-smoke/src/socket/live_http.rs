use std::{collections::BTreeMap, io, net::TcpListener, time::Duration};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::oneshot,
    time::timeout,
};

use crate::{config::WebConfig, server};

async fn request_status(
    addr: std::net::SocketAddr,
    route: &str,
    expected_status: u16,
) -> io::Result<()> {
    let mut stream = TcpStream::connect(addr).await?;
    let request = format!("GET {route} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;
    // Read through connection close; a single TCP read may contain only a
    // partial header. Timeout bounds a stalled HTTP response.
    let mut buffer = Vec::new();
    timeout(Duration::from_secs(5), stream.read_to_end(&mut buffer))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "HTTP response timed out"))??;
    let response = String::from_utf8_lossy(&buffer);
    assert!(
        response.starts_with(&format!("HTTP/1.1 {expected_status}")),
        "unexpected response status"
    );
    if expected_status != 404 {
        let header_block = response.split("\r\n\r\n").next().unwrap_or("");
        let lowered = header_block.to_ascii_lowercase();
        for expected in [
            "cache-control: no-store",
            "x-content-type-options: nosniff",
            "x-frame-options: deny",
        ] {
            assert!(lowered.contains(expected), "missing response security header");
        }
    }
    Ok(())
}

#[tokio::test]
async fn live_routes_and_graceful_shutdown() -> io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        server::serve_prepared(listener, async move {
            let _ = stopped.await;
        })
        .await
    });
    for (route, status) in [
        ("/", 200),
        ("/healthz", 200),
        ("/readyz", 503),
        ("/v1/operations", 404),
        ("/unknown", 404),
    ] {
        request_status(addr, route, status).await?;
    }
    let _ = stop.send(());
    let outcome = timeout(Duration::from_secs(5), task)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "shutdown timed out"))?
        .map_err(|_| io::Error::other("server task terminated unexpectedly"))?;
    assert!(outcome.is_ok(), "server reported a shutdown failure");
    Ok(())
}

#[tokio::test]
async fn public_and_configured_backend_startup_fail_closed() {
    let empty = BTreeMap::new();
    let public = WebConfig {
        bind: "0.0.0.0:0".into(),
        api_http_base: None,
        database_url: None,
    };
    assert!(matches!(
        server::run(&public, &empty, &empty).await,
        Err(server::ServerError::Admission(
            crate::listener::AdmissionError::NonLoopbackBind
        ))
    ));

    let backend = WebConfig {
        bind: "127.0.0.1:0".into(),
        api_http_base: Some("https://api.example.invalid".into()),
        database_url: None,
    };
    assert!(matches!(
        server::run(&backend, &empty, &empty).await,
        Err(server::ServerError::Admission(
            crate::listener::AdmissionError::GatewayUnavailable
        ))
    ));
}
