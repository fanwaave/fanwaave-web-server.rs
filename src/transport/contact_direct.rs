#![forbid(unsafe_code)]
//! Stage 2: real typed direct-reader adapter, with authorization owned by
//! fanwaave-lib-core's signature verifier. This module MUST NOT be wired
//! into a public route until the trusted issuer and private dependency
//! admission are independently approved.

use async_trait::async_trait;
use fanwaave_interfaces::contact_read::ContactReadOperation;
use fanwaave_lib_core::{contact::ContactJobView, ReadContext, ReadError};
use ores_transport::{DirectReader, TransportError};
use uuid::Uuid;

/// One exact, service-owned, read-only operation. No arbitrary SQL,
/// mutations, role override, or API fallback is exposed here.
#[derive(Clone)]
pub struct ContactJobDirectReader {
    context: ReadContext,
}

impl ContactJobDirectReader {
    #[must_use]
    pub fn new(context: ReadContext) -> Self {
        Self { context }
    }
}

fn transport_error(error: ReadError) -> TransportError {
    match error {
        ReadError::Unauthorized => TransportError::Unauthorized,
        ReadError::InvalidData => TransportError::InvalidResponse,
        ReadError::Database
        | ReadError::NotReadOnly
        | ReadError::InvalidReadCredential
        | ReadError::UnsupportedFlavor
        | ReadError::MissingReadPrivilege
        | ReadError::ExcessivePrivileges
        | ReadError::IncompatibleSchema => TransportError::Upstream,
    }
}

#[async_trait]
impl DirectReader<ContactReadOperation, Option<ContactJobView>> for ContactJobDirectReader {
    async fn read(
        &self,
        operation: &ContactReadOperation,
        authorization: &str,
    ) -> Result<Option<ContactJobView>, TransportError> {
        operation
            .validate()
            .map_err(|_| TransportError::InvalidRequest)?;
        match operation {
            ContactReadOperation::GetContactJob { job_id } => {
                let id = Uuid::parse_str(job_id).map_err(|_| TransportError::InvalidRequest)?;
                // The core verifier enforces exact job binding, trusted kid,
                // HS256 signature, principal, scope and expiration BEFORE SQL.
                self.context
                    .get_contact_job(id, authorization)
                    .await
                    .map_err(transport_error)
            }
        }
    }

    fn serves(&self, operation: &ContactReadOperation) -> bool {
        matches!(operation, ContactReadOperation::GetContactJob { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_errors_cannot_be_presented_as_missing_data() {
        assert_eq!(
            transport_error(ReadError::Unauthorized),
            TransportError::Unauthorized
        );
        assert_eq!(
            transport_error(ReadError::InvalidData),
            TransportError::InvalidResponse
        );
        assert_eq!(
            transport_error(ReadError::Database),
            TransportError::Upstream
        );
        assert_eq!(
            transport_error(ReadError::ExcessivePrivileges),
            TransportError::Upstream
        );
    }
}
