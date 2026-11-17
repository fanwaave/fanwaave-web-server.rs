#![forbid(unsafe_code)]

use fanwaave_lib_core::fanwaave_config::{parse_fanwaave_config, resolve_fanwaave_config};
use fanwaave_web_server::{config::WebConfig, flags, server};
use std::{path::Path, process::ExitCode};

/// Do not print arbitrary parser or config errors: they may contain credentials.
#[derive(Debug, thiserror::Error)]
enum StartupError {
    #[error("command-line or environment configuration is invalid")]
    Flags,
    #[error("could not read Fanwaave domain configuration")]
    DomainRead(#[source] std::io::Error),
    #[error("Fanwaave domain configuration is invalid")]
    DomainParse,
    #[error("Fanwaave domain configuration could not be resolved")]
    DomainResolve,
    #[error("Fanwaave web configuration is invalid")]
    WebConfig,
}

fn read_domain_text(path: &Path) -> Result<String, StartupError> {
    std::fs::read_to_string(path).map_err(StartupError::DomainRead)
}

fn run() -> Result<(), StartupError> {
    let applied = flags::resolve_sources().map_err(|_| StartupError::Flags)?;
    let domain_text = read_domain_text(Path::new(".fanwaave-cfg.toml"))?;
    let domain = parse_fanwaave_config(&domain_text).map_err(|_| StartupError::DomainParse)?;
    let resolved = resolve_fanwaave_config(&domain, &applied.ambient, &applied.argv_overrides)
        .map_err(|_| StartupError::DomainResolve)?;
    let cfg =
        WebConfig::from_sources(&applied.merged, &resolved).map_err(|_| StartupError::WebConfig)?;
    server::run(&cfg);
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_domain_file_returns_typed_error_without_panic() {
        let result = read_domain_text(Path::new("test/fixtures/nonexistent-domain.toml"));
        assert!(matches!(result, Err(StartupError::DomainRead(_))));
    }

    #[test]
    fn startup_error_messages_do_not_include_inner_file_paths() {
        let error = StartupError::DomainRead(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "secret-token=do-not-echo",
        ));
        assert_eq!(
            error.to_string(),
            "could not read Fanwaave domain configuration"
        );
        assert!(!error.to_string().contains("do-not-echo"));
    }
}
