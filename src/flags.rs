//! Fail-closed argv and environment resolution through flags-2-env.

use std::collections::BTreeMap;

use flags2env::BundledFlags2Env;
use tempfile::NamedTempFile;

const CONTRACT: &str = include_str!("../.cli-flags.toml");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedFlags {
    /// dotenv + process environment + explicit dotenv overrides, before argv.
    pub ambient: BTreeMap<String, String>,
    /// Only options explicitly supplied on argv. Domain resolvers use this to
    /// reject secret values crossing the command-line boundary.
    pub argv_overrides: BTreeMap<String, String>,
    /// Fully typed/coerced argv-over-environment view for legacy consumers.
    pub merged: BTreeMap<String, String>,
}

pub fn resolve() -> Result<BTreeMap<String, String>, String> {
    Ok(resolve_sources()?.merged)
}

pub fn resolve_sources() -> Result<AppliedFlags, String> {
    resolve_sources_from(&std::env::args().collect::<Vec<_>>(), std::env::vars())
}

fn resolve_sources_from(
    argv: &[String],
    environment: impl IntoIterator<Item = (String, String)>,
) -> Result<AppliedFlags, String> {
    let contract = NamedTempFile::new()
        .map_err(|error| format!("cannot create embedded flags-2-env contract: {error}"))?
        .into_temp_path();
    std::fs::write(&contract, CONTRACT.as_bytes())
        .map_err(|error| format!("cannot materialize embedded flags-2-env contract: {error}"))?;
    let path = contract
        .to_str()
        .ok_or_else(|| "flags-2-env contract path is not valid UTF-8".to_owned())?;
    let parser = BundledFlags2Env::new();
    parser
        .audit_config(Some(path))
        .map_err(|error| format!("flags-2-env contract audit failed: {error}"))?;
    let parsed = parser
        .parse_structured(argv, Some(path))
        .map_err(|error| format!("flags-2-env parsing failed: {error}"))?;

    if !parsed.unknown_options.is_empty() {
        let names = parsed
            .unknown_options
            .iter()
            .map(|option| option.split('=').next().unwrap_or_default())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!("unknown command-line option(s): {names}"));
    }
    if !parsed.errors.is_empty() {
        return Err(format!(
            "invalid command-line value(s): {}",
            parsed.errors.join("; ")
        ));
    }
    if !parsed.extras.is_empty() {
        return Err(format!(
            "unexpected positional argument(s): {}",
            parsed.extras.len()
        ));
    }

    // Later sources take precedence, including explicit dotenv overrides.
    let ambient = parsed
        .dotenv
        .into_iter()
        .chain(environment)
        .chain(parsed.dotenv_overrides)
        .collect::<BTreeMap<_, _>>();
    let argv_overrides = parsed
        .provided_flags
        .into_iter()
        .collect::<BTreeMap<_, _>>();

    let raw = ambient
        .clone()
        .into_iter()
        .chain(argv_overrides.clone())
        .collect::<BTreeMap<_, _>>();
    let typed = parser
        .coerce::<serde_json::Map<String, serde_json::Value>, _>(&raw, Some(path))
        .map_err(|error| format!("flags-2-env typed configuration failed: {error}"))?;
    let merged = typed
        .into_iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(name, value)| scalar_string(&name, value).map(|value| (name, value)))
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    Ok(AppliedFlags {
        ambient,
        argv_overrides,
        merged,
    })
}

fn scalar_string(name: &str, value: serde_json::Value) -> Result<String, String> {
    match value {
        serde_json::Value::String(value) => Ok(value),
        serde_json::Value::Bool(value) => Ok(value.to_string()),
        serde_json::Value::Number(value) => Ok(value.to_string()),
        serde_json::Value::Null | serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            Err(format!(
                "flags-2-env returned a non-scalar value for {name}"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_options_fail_closed_without_echoing_values() -> Result<(), String> {
        let error = resolve_sources_from(
            &[
                "server".to_owned(),
                "--definitely-unknown=do-not-echo".to_owned(),
            ],
            std::iter::empty(),
        )
        .err()
        .ok_or_else(|| "unknown option was accepted".to_owned())?;
        assert!(error.contains("--definitely-unknown"));
        assert!(!error.contains("do-not-echo"));
        Ok(())
    }

    #[test]
    fn explicit_argv_is_preserved_separately_from_ambient_environment() -> Result<(), String> {
        let applied = resolve_sources_from(
            &[
                "server".to_owned(),
                "--fanwaave-web-bind=127.0.0.1:9090".to_owned(),
            ],
            [("FANWAAVE_WEB_BIND".to_owned(), "127.0.0.1:8081".to_owned())],
        )?;
        assert_eq!(
            applied.ambient.get("FANWAAVE_WEB_BIND").map(String::as_str),
            Some("127.0.0.1:8081")
        );
        assert_eq!(
            applied
                .argv_overrides
                .get("FANWAAVE_WEB_BIND")
                .map(String::as_str),
            Some("127.0.0.1:9090")
        );
        assert_eq!(
            applied.merged.get("FANWAAVE_WEB_BIND").map(String::as_str),
            Some("127.0.0.1:9090")
        );
        Ok(())
    }
}
