#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use fanwaave_lib_core::fanwaave_config::{ConfigValue, ResolvedFanwaaveConfig};

#[derive(Clone, Debug)]
pub struct WebConfig {
    pub bind: String,
    pub api_http_base: Option<String>,
    pub database_url: Option<String>,
}

impl WebConfig {
    pub fn from_env() -> Self {
        Self::from_map(&std::env::vars().collect())
    }

    pub fn from_map(environment: &BTreeMap<String, String>) -> Self {
        Self {
            bind: environment
                .get("FANWAAVE_WEB_BIND")
                .cloned()
                .unwrap_or_else(|| "127.0.0.1:8081".into()),
            api_http_base: environment.get("FANWAAVE_API_HTTP_BASE").cloned(),
            database_url: environment.get("FANWAAVE_DATABASE_URL").cloned(),
        }
    }

    /// Compose the existing server-specific environment surface with the
    /// TJSV-admitted Fanwaave domain policy. Domain-owned bindings win; values
    /// outside the domain contract remain on the canonical flags-2-env map.
    pub fn from_sources(
        environment: &BTreeMap<String, String>,
        fanwaave: &ResolvedFanwaaveConfig,
    ) -> Result<Self, String> {
        let bind = match fanwaave.binding("bind_addr").map(|binding| binding.value()) {
            Some(ConfigValue::String(value)) => value.clone(),
            Some(_) => return Err("Fanwaave bind_addr binding must resolve as a string".into()),
            None => environment
                .get("FANWAAVE_WEB_BIND")
                .cloned()
                .unwrap_or_else(|| "127.0.0.1:8081".into()),
        };
        let database_url = match fanwaave
            .binding("database_url")
            .map(|binding| binding.value())
        {
            Some(ConfigValue::Url(value)) => Some(value.clone()),
            Some(_) => return Err("Fanwaave database_url binding must resolve as a URL".into()),
            None => None,
        };
        Ok(Self {
            bind,
            api_http_base: environment.get("FANWAAVE_API_HTTP_BASE").cloned(),
            database_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanwaave_lib_core::fanwaave_config::{parse_fanwaave_config, resolve_fanwaave_config};

    const DOMAIN_CONFIG: &str = include_str!("../.fanwaave-cfg.toml");

    #[test]
    fn domain_default_and_secret_environment_compose() {
        let policy = parse_fanwaave_config(DOMAIN_CONFIG).expect("tracked domain config parses");
        let ambient = BTreeMap::from([
            (
                "FANWAAVE_DATABASE_URL".to_owned(),
                "postgres://user:pass@127.0.0.1/fanwaave".to_owned(),
            ),
            (
                "FANWAAVE_API_HTTP_BASE".to_owned(),
                "https://api.example.invalid".to_owned(),
            ),
        ]);
        let resolved = resolve_fanwaave_config(&policy, &ambient, &BTreeMap::new())
            .expect("tracked domain config resolves");
        let config = WebConfig::from_sources(&ambient, &resolved).expect("web config resolves");
        assert_eq!(config.bind, "127.0.0.1:8081");
        assert_eq!(
            config.api_http_base.as_deref(),
            Some("https://api.example.invalid")
        );
        assert_eq!(
            config.database_url.as_deref(),
            Some("postgres://user:pass@127.0.0.1/fanwaave")
        );
    }

    #[test]
    fn secret_database_binding_rejects_argv_delivery() {
        let policy = parse_fanwaave_config(DOMAIN_CONFIG).expect("tracked domain config parses");
        let error = resolve_fanwaave_config(
            &policy,
            &BTreeMap::new(),
            &BTreeMap::from([(
                "FANWAAVE_DATABASE_URL".to_owned(),
                "postgres://do-not-accept-on-argv".to_owned(),
            )]),
        )
        .expect_err("secret argv must fail closed");
        assert!(error.to_string().contains("may not be supplied through argv"));
    }
}
