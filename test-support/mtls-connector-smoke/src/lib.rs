#![forbid(unsafe_code)]

// Include the actual production connector code. No private source or
// alternate implementation is substituted by this isolated crate.
#[path = "../../../src/transport/mtls_connector.rs"]
pub mod mtls_connector;

#[cfg(test)]
#[path = "socket/live_tls.rs"]
mod live_tls;
