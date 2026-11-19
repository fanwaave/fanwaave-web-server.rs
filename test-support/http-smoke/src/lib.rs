#![forbid(unsafe_code)]

// This harness compiles actual production modules. Only WebConfig is a shim:
// its concrete production type depends on an inaccessible private Cargo tree.
pub mod config {
    #[derive(Clone, Debug)]
    pub struct WebConfig {
        pub bind: String,
        pub api_http_base: Option<String>,
        pub database_url: Option<String>,
    }
}

#[path = "../../../src/pages/mod.rs"]
pub mod pages;
#[path = "../../../src/transport/listener.rs"]
pub mod listener;
pub mod transport {
    pub use crate::listener;
}
#[path = "../../../src/server.rs"]
pub mod server;

#[cfg(test)]
#[path = "socket/live_http.rs"]
mod live_http;
