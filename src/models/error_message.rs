//! `ErrorMessage` — port of the Lombok `@Data` DTO
//! `com.smartContact.model.ErrorMessage`.
//!
//! It is the JSON body of an error response:
//! `{"status":"NOT_FOUND","message":"..."}`. Jackson writes a Spring
//! `HttpStatus` as its enum constant name, so `status` holds that name as a
//! string (for example `"NOT_FOUND"`).
//!
//! Both fields are `Option` so that the no-args constructor (`new()` /
//! `Default`) leaves them unset, like Java's `null`. Unset fields serialize
//! as JSON `null`, which is what Jackson does.

use std::fmt;

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ErrorMessage {
    /// HTTP status as Spring's `HttpStatus` enum name, e.g. `"NOT_FOUND"`.
    status: Option<String>,
    message: Option<String>,
}

impl ErrorMessage {
    /// `@NoArgsConstructor` — all fields unset.
    pub fn new() -> Self {
        Self::default()
    }

    /// `@AllArgsConstructor(status, message)` — same parameter order.
    pub fn with(status: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: Some(status.into()),
            message: Some(message.into()),
        }
    }

    /// Build from an axum `StatusCode`, converting it to the Spring
    /// `HttpStatus` constant name (e.g. 404 → `"NOT_FOUND"`).
    pub fn from_status_code(status: StatusCode, message: impl Into<String>) -> Self {
        Self::with(http_status_name(status), message)
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn set_status(&mut self, status: Option<String>) {
        self.status = status;
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn set_message(&mut self, message: Option<String>) {
        self.message = message;
    }
}

/// Lombok-style `toString`: `ErrorMessage(status=NOT_FOUND, message=...)`.
/// Unset fields print as `null`, as in Java.
impl fmt::Display for ErrorMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ErrorMessage(status={}, message={})",
            self.status.as_deref().unwrap_or("null"),
            self.message.as_deref().unwrap_or("null")
        )
    }
}

/// Map a status code to its Spring `HttpStatus` enum constant name.
/// The name is derived from the canonical reason phrase, e.g.
/// "Not Found" → "NOT_FOUND". Codes with no reason phrase fall back to
/// the numeric code.
// MIGRATION_NOTE: a few Spring constant names differ from the IANA reason
// phrase (e.g. 413 is PAYLOAD_TOO_LARGE in Spring, and the phrase is
// "Payload Too Large"). Where they match, the result is identical. Review
// this if you depend on exotic codes.
pub fn http_status_name(status: StatusCode) -> String {
    match status.canonical_reason() {
        Some(reason) => reason
            .chars()
            .map(|c| match c {
                ' ' | '-' => '_',
                other => other.to_ascii_uppercase(),
            })
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect(),
        None => status.as_u16().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_has_unset_fields() {
        let e = ErrorMessage::new();
        assert_eq!(e.status(), None);
        assert_eq!(e.message(), None);
        assert_eq!(e.to_string(), "ErrorMessage(status=null, message=null)");
    }

    #[test]
    fn all_args_accessors_equality_and_json() {
        let mut e = ErrorMessage::with("NOT_FOUND", "User are not available");
        assert_eq!(e.status(), Some("NOT_FOUND"));
        assert_eq!(e.message(), Some("User are not available"));
        assert_eq!(
            e,
            ErrorMessage::from_status_code(StatusCode::NOT_FOUND, "User are not available")
        );
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"status":"NOT_FOUND","message":"User are not available"}"#
        );
        e.set_status(Some("BAD_REQUEST".into()));
        e.set_message(None);
        assert_eq!(e.to_string(), "ErrorMessage(status=BAD_REQUEST, message=null)");
    }
}
