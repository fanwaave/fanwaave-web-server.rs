//! Reduce raw I/O errors to non-sensitive diagnostics at the startup boundary.
//!
//! An io::Error can contain paths, credentials or remote response text in its
//! custom message. Keeping only the error kind prevents even Debug output of
//! StartupError from displaying that untrusted material.

/// Retain a meaningful diagnostic category without retaining raw error text.
#[must_use]
pub(crate) fn redact_io_error(error: &std::io::Error) -> std::io::ErrorKind {
    error.kind()
}

#[cfg(test)]
mod tests {
    use super::redact_io_error;
    use std::io::{Error, ErrorKind};

    #[test]
    fn custom_io_error_text_does_not_survive_redaction() {
        let raw = Error::new(ErrorKind::PermissionDenied, "secret-token=do-not-echo");
        assert!(raw.to_string().contains("secret-token"));
        let sanitized = redact_io_error(&raw);
        assert_eq!(sanitized, ErrorKind::PermissionDenied);
        assert!(!format!("{sanitized:?}").contains("secret-token"));
    }

    #[test]
    fn filesystem_error_categories_survive_without_paths() {
        let raw = Error::new(ErrorKind::NotFound, "/sensitive/path/credential.pem");
        assert_eq!(redact_io_error(&raw), ErrorKind::NotFound);
        assert!(!format!("{:?}", redact_io_error(&raw)).contains("credential.pem"));
    }
}
