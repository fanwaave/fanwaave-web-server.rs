#![forbid(unsafe_code)]

use std::{collections::BTreeMap, future::Future, io, net::TcpListener};

use axum::{http::StatusCode, response::Html, routing::get, Router};

use crate::{
    config::WebConfig,
    pages,
    transport::listener::{prepare_listener, AdmissionError},
};

// These static-only responses must not be cached or embedded as authority.
const STATIC_RESPONSE_HEADERS: [(&str, &str); 3] = [
    ("cache-control", "no-store"),
    ("x-content-type-options", "nosniff"),
    ("x-frame-options", "DENY"),
];

/// Limited static-only service until issue #19 provides an authenticated gateway.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error(transparent)]
    Admission(#[from] AdmissionError),
    #[error("web listener runtime initialization failed")]
    ListenerRuntime(io::ErrorKind),
    #[error("web listener stopped unexpectedly")]
    Serve(io::ErrorKind),
}

pub async fn run(
    config: &WebConfig,
    ambient: &BTreeMap<String, String>,
    applied: &BTreeMap<String, String>,
) -> Result<(), ServerError> {
    // All admission runs before bind. Do not pretend that an unwired API
    // gateway or a publicly reachable listener is ready for service.
    let listener = prepare_listener(
        &config.bind,
        config.database_url.is_some() || config.api_http_base.is_some(),
        ambient,
        applied,
    )?;
    serve_prepared(listener, shutdown_signal()).await
}

/// Runs the production router on an already admitted socket. Kept internal
/// to prevent API callers from bypassing fail-closed listener admission.
/// The explicit shutdown future allows real HTTP and graceful-shutdown tests
/// without registering process-global signal handlers.
pub(crate) async fn serve_prepared<S>(listener: TcpListener, shutdown: S) -> Result<(), ServerError>
where
    S: Future<Output = ()> + Send + 'static,
{
    let listener = tokio::net::TcpListener::from_std(listener)
        .map_err(|error| ServerError::ListenerRuntime(error.kind()))?;

    let router = Router::new()
        .route(
            "/",
            get(|| async { (STATIC_RESPONSE_HEADERS, Html(pages::home::markup())) }),
        )
        .route(
            "/healthz",
            get(|| async { (STATIC_RESPONSE_HEADERS, Html(pages::health::markup())) }),
        )
        .route(
            "/readyz",
            get(|| async { (STATIC_RESPONSE_HEADERS, StatusCode::SERVICE_UNAVAILABLE) }),
        );

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|error| ServerError::Serve(error.kind()))
}

/// Allow Kubernetes/Unix SIGTERM to drain active requests, as well as Ctrl-C.
/// A closed or unregistrable signal stream must never falsely report shutdown.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(termination) => {
                tokio::select! {
                    _ = await_termination(termination) => {},
                    _ = await_ctrl_c() => {},
                }
            }
            Err(_) => await_ctrl_c().await,
        }
    }

    #[cfg(not(unix))]
    await_ctrl_c().await;
}

#[cfg(unix)]
async fn await_termination(mut termination: tokio::signal::unix::Signal) {
    if termination.recv().await.is_none() {
        std::future::pending::<()>().await;
    }
}

async fn await_ctrl_c() {
    if tokio::signal::ctrl_c().await.is_err() {
        // Failed signal registration cannot masquerade as orderly shutdown.
        std::future::pending::<()>().await;
    }
}
