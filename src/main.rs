#![forbid(unsafe_code)]

use fanwaave_lib_core::fanwaave_config::{parse_fanwaave_config, resolve_fanwaave_config};
use fanwaave_web_server::{config::WebConfig, flags, server};

fn main() {
    let applied = flags::resolve_sources().unwrap_or_else(|error| panic!("{error}"));
    let domain_text = std::fs::read_to_string(".fanwaave-cfg.toml")
        .unwrap_or_else(|error| panic!("cannot read .fanwaave-cfg.toml: {error}"));
    let domain = parse_fanwaave_config(&domain_text)
        .unwrap_or_else(|error| panic!("invalid .fanwaave-cfg.toml: {error}"));
    let resolved = resolve_fanwaave_config(
        &domain,
        &applied.ambient,
        &applied.argv_overrides,
    )
    .unwrap_or_else(|error| panic!("Fanwaave domain config resolution failed: {error}"));
    let cfg = WebConfig::from_sources(&applied.merged, &resolved)
        .unwrap_or_else(|error| panic!("Fanwaave web config resolution failed: {error}"));
    server::run(&cfg);
}
