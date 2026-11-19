#![forbid(unsafe_code)]

// Test-only child process: exercise the real production server::run and
// OS signal handling without opening any public listener.
use std::collections::BTreeMap;

use fanwaave_web_http_smoke::{config::WebConfig, server};

#[tokio::main]
async fn main() {
    let Some(bind) = std::env::args().nth(1) else {
        std::process::exit(2);
    };
    let config = WebConfig {
        bind,
        api_http_base: None,
        database_url: None,
    };
    let empty = BTreeMap::new();
    if server::run(&config, &empty, &empty).await.is_err() {
        std::process::exit(1);
    }
}
