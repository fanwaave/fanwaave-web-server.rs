#![forbid(unsafe_code)]

use fanwaave_lib_core::fanwaave_config::{parse_fanwaave_config, resolve_fanwaave_config};
use fanwaave_web_server::{config::WebConfig, flags, server};

mod startup_error_policy;
use std::{path::Path, process::ExitCode};

/// Do not print arbitrary parser or config errors: they may contain credentials.
#[derive(Debug, thiserror::Error)]
enum StartupError {
    #[error("command-line or environment configuration is invalid")]
    Flags,
    #[error("could not read Fanwaave domain configuration")]
    DomainRead(std::io::ErrorKind),
    #[error("Fanwaave domain configuration is invalid")]
    DomainParse,
    #[error("Fanwaave domain configuration could not be resolved")]
    DomainResolve,
    #[error("Fanwaave web configuration is invalid")]
    WebConfig,
    #[error(transparent)]
    Server(#[from] server::ServerError),
}

fn read_domain_text(path: &Path) -> Result<String, StartupError> {
    std::fs::read_to_string(path)
        .map_err(|error| StartupError::DomainRead(startup_error_policy::redact_io_error(&error)))
}

async fn run() -> Result<(), StartupError> {
    let applied = flags::resolve_sources().map_err(|_| StartupError::Flags)?;
    let domain_text = read_domain_text(Path::new(".fanwaave-cfg.toml"))?;
    let domain = parse_fanwaave_config(&domain_text).map_err(|_| StartupError::DomainParse)?;
    let resolved = resolve_fanwaave_config(&domain, &applied.ambient, &applied.argv_overrides)
        .map_err(|_| StartupError::DomainResolve)?;
    let cfg =
        WebConfig::from_sources(&applied.merged, &resolved).map_err(|_| StartupError::WebConfig)?;
    server::run(&cfg, &applied.ambient, &applied.merged).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
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
    fn startup_error_messages_and_debug_do_not_include_inner_file_paths() {
        let raw = std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "secret-token=do-not-echo",
        );
        let error = StartupError::DomainRead(startup_error_policy::redact_io_error(&raw));
        assert_eq!(
            error.to_string(),
            "could not read Fanwaave domain configuration"
        );
        assert!(!error.to_string().contains("do-not-echo"));
        assert!(!format!("{error:?}").contains("do-not-echo"));
    }
}
