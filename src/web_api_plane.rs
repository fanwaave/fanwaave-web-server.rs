//! Four-avenue web ↔ API binding for `fanwaave`.
//!
//! 1. Direct read-only DB via `*-lib-core` named queries (no migrations).
//! 2. Stateless HTTP from `app.fanwaave.dev` to `api.fanwaave.dev`.
//! 3. Stateful TLS 1.3/mTLS TCP to `api.fanwaave.dev:7443`.
//! 4. JetStream: in-cluster producers publish directly to
//!    `nats://dd-nats.messaging.svc.cluster.local:4222`. External producers
//!    use named HTTPS routes on `dd-nats-bridge` (not raw subjects). The
//!    `dd-remote-queue-consumer` in k8s-cluster is the agent-task consumer,
//!    not the product-web producer path.

use k8s_web_api_data_plane::{
    DataPlaneCapabilities, DataPlaneError, DirectDatabasePolicy, InteractionMode, JetStreamPolicy,
    OrgIdentity, StatefulMtlsTcpPolicy, StatelessHttpPolicy,
};

pub const GITHUB_ORG: &str = "fanwaave";
pub const ORG_SLUG: &str = "fanwaave";
pub const DNS_ZONE: &str = "fanwaave.dev";

/// Preserve upstream validation failures instead of obscuring their variants.
pub fn identity() -> Result<OrgIdentity, DataPlaneError> {
    OrgIdentity::new(GITHUB_ORG, ORG_SLUG, DNS_ZONE)
}

pub fn capabilities() -> Result<DataPlaneCapabilities, DataPlaneError> {
    Ok(DataPlaneCapabilities::for_identity(&identity()?))
}

pub fn policies() -> Result<
    (
        DirectDatabasePolicy,
        StatelessHttpPolicy,
        StatefulMtlsTcpPolicy,
        JetStreamPolicy,
    ),
    DataPlaneError,
> {
    let identity = identity()?;
    Ok((
        DirectDatabasePolicy::for_identity(&identity),
        StatelessHttpPolicy::for_identity(&identity),
        StatefulMtlsTcpPolicy::for_identity(&identity, 7443),
        JetStreamPolicy::for_identity(&identity),
    ))
}

pub fn validate_four_avenues() -> Result<(), DataPlaneError> {
    let identity = identity()?;
    let (db, http, tcp, nats) = policies()?;
    db.validate(&identity)?;
    http.validate()?;
    tcp.validate()?;
    nats.validate()?;
    debug_assert_eq!(InteractionMode::ALL.len(), 4);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_avenues_are_named_and_fail_closed() -> Result<(), DataPlaneError> {
        validate_four_avenues()?;
        let caps = capabilities()?;
        assert_eq!(caps.app_host, format!("app.{DNS_ZONE}"));
        assert_eq!(caps.api_host, format!("api.{DNS_ZONE}"));
        assert_eq!(
            caps.nats_request_subject,
            format!("dd.remote.web_api.{ORG_SLUG}.request")
        );
        assert_eq!(
            caps.nats_url_in_cluster,
            "nats://dd-nats.messaging.svc.cluster.local:4222"
        );
        Ok(())
    }

    #[test]
    fn policy_validation_retains_upstream_error_type() {
        // The public validation API used DataPlaneError before the panic cleanup.
        let checked: Result<(), DataPlaneError> = validate_four_avenues();
        assert!(checked.is_ok());
        assert!(matches!(
            OrgIdentity::new(GITHUB_ORG, "invalid.slug", DNS_ZONE),
            Err(DataPlaneError::InvalidIdentifier("org_slug"))
        ));
    }
}
