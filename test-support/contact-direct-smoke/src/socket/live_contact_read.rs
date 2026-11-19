#![forbid(unsafe_code)]

use std::time::{SystemTime, UNIX_EPOCH};

use fanwaave_interfaces::contact_read::{
    ContactReadGrantClaims, ContactReadOperation, CONTACT_READ_AUDIENCE, CONTACT_READ_ISSUER,
    CONTACT_READ_SCOPE,
};
use fanwaave_lib_core::{config::CoreConfig, ReadContext, ReadVerifierKey};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use uuid::Uuid;

use crate::{contact_direct::ContactJobDirectReader, DirectReader, TransportError};

const ROW: &str = "00000000-0000-4000-8000-000000000001";
const OTHER: &str = "00000000-0000-4000-8000-000000000002";
const SIGNING_KEY: &str = "public-ci-test-only-scoped-read-signing-key";
const KID: &str = "ci-current";

fn read_context_config() -> CoreConfig {
    let dsn = std::env::var("FANWAAVE_READ_TEST_DSN")
        .expect("isolated source job requires its own dedicated test PostgreSQL service");
    CoreConfig::from_values(dsn, None).expect("test DSN is PostgreSQL")
}

fn auth_for(job: Uuid, key_id: &str, secret: &str, scope: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("CI clock is after Unix epoch")
        .as_secs() as i64;
    let claims = ContactReadGrantClaims {
        iss: CONTACT_READ_ISSUER.into(),
        aud: CONTACT_READ_AUDIENCE.into(),
        sub: "trusted-ci-issuer-fixture".into(),
        scope: scope.into(),
        job_id: job.to_string(),
        iat: now - 1,
        nbf: now - 1,
        exp: now + 120,
        jti: "00000000-0000-4000-8000-000000000003".into(),
    };
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some(key_id.to_owned());
    let signed = encode(
        &header,
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("CI-only token issuer can sign fixture");
    format!("Bearer {signed}")
}

#[tokio::test]
async fn production_adapter_returns_real_postgresql_job_and_none_for_unknown() {
    let keys = [ReadVerifierKey {
        kid: KID,
        secret: SIGNING_KEY.as_bytes(),
    }];
    let context = ReadContext::connect(&read_context_config(), &keys)
        .await
        .expect("real limited database role admits connection");
    let adapter = ContactJobDirectReader::new(context);
    let job = Uuid::parse_str(ROW).expect("canonical UUID");
    let operation = ContactReadOperation::GetContactJob { job_id: ROW.into() };
    assert!(adapter.serves(&operation));
    let result = adapter
        .read(&operation, &auth_for(job, KID, SIGNING_KEY, CONTACT_READ_SCOPE))
        .await
        .expect("real source-backed read succeeds")
        .expect("fixture row exists");
    assert_eq!(result.job_id, ROW);
    assert_eq!(result.idempotency_key, "fixture-contact-job-0001");

    let missing = Uuid::parse_str(OTHER).expect("canonical other UUID");
    let missing_operation = ContactReadOperation::GetContactJob { job_id: OTHER.into() };
    assert!(adapter
        .read(
            &missing_operation,
            &auth_for(missing, KID, SIGNING_KEY, CONTACT_READ_SCOPE)
        )
        .await
        .expect("signed read for missing UUID is valid")
        .is_none());
}

#[tokio::test]
async fn actual_adapter_denies_wrong_job_scope_and_unknown_kid_before_lookup() {
    let keys = [ReadVerifierKey {
        kid: KID,
        secret: SIGNING_KEY.as_bytes(),
    }];
    let context = ReadContext::connect(&read_context_config(), &keys)
        .await
        .expect("real limited database role admits connection");
    let adapter = ContactJobDirectReader::new(context);
    let job = Uuid::parse_str(ROW).expect("canonical UUID");
    let other = Uuid::parse_str(OTHER).expect("canonical UUID");
    let operation = ContactReadOperation::GetContactJob { job_id: ROW.into() };

    for auth in [
        auth_for(other, KID, SIGNING_KEY, CONTACT_READ_SCOPE),
        auth_for(job, KID, SIGNING_KEY, "contact_job.write"),
        auth_for(job, "untrusted", SIGNING_KEY, CONTACT_READ_SCOPE),
    ] {
        assert!(matches!(
            adapter.read(&operation, &auth).await,
            Err(TransportError::Unauthorized)
        ));
    }
    let malformed = ContactReadOperation::GetContactJob {
        job_id: "not-uuid".into(),
    };
    assert!(matches!(
        adapter
            .read(
                &malformed,
                &auth_for(job, KID, SIGNING_KEY, CONTACT_READ_SCOPE)
            )
            .await,
        Err(TransportError::InvalidRequest)
    ));
}
