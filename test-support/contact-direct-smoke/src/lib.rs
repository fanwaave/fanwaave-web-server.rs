#![forbid(unsafe_code)]

// Deliberate public-dependency-isolated trait shim. This harness compiles the
// exact production adapter file but DOES NOT compile the private upstream
// ores-transport crate. The latter remains an independent admission gate.
extern crate self as ores_transport;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportError {
    Unauthorized,
    AuthUnavailable,
    InvalidRequest,
    NotConfigured,
    Backpressure,
    Timeout,
    Upstream,
    InvalidResponse,
}

#[async_trait::async_trait]
pub trait DirectReader<O, T>: Send + Sync {
    async fn read(&self, operation: &O, authorization: &str) -> Result<T, TransportError>;

    fn serves(&self, _operation: &O) -> bool {
        true
    }
}

#[path = "../../../src/transport/contact_direct.rs"]
pub mod contact_direct;

#[cfg(test)]
#[path = "socket/live_contact_read.rs"]
mod live_contact_read;
