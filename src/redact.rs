//! What a hand-written `Debug` prints in place of a secret (D-NEXT-I4-a).
//!
//! A type that holds a secret prints it as [`REDACTED`] and keeps the names
//! around it: the field's, and a header's or an environment variable's. The
//! secrets are the ones the `LOUD_WIRE` printer redacts on the wire
//! (`crate::wire`): an API key, a webhook signing secret, credential
//! material, an environment variable's value, and every header value under
//! an MCP server's `headers` or an allowlist entry's `transform`. So
//! `dbg!(request)` or `tracing::debug!(?request)` cannot undo the wire's
//! redaction.
//!
//! A modeled secret field is redacted by its type's impl. JSON a type
//! carries where these secrets travel (a `WireEvent`'s bodies, a retrieval
//! config's `custom_config`) goes through the wire's own walk ([`json`]),
//! so there the two share one list.
//!
//! Each impl destructures its type without `..`, so a field added later
//! fails to compile until the impl prints it, or redacts it.

use std::borrow::Cow;
use std::collections::BTreeMap;

/// What a secret prints as: the wire printer's placeholder too.
pub(crate) const REDACTED: &str = "[REDACTED]";

/// `Some("[REDACTED]")` for a secret that is set and `None` for one that
/// is not, so `Debug` still shows which.
pub(crate) fn optional<T>(secret: Option<&T>) -> Option<&'static str> {
    secret.map(|_| REDACTED)
}

/// A header (or environment) map with its names kept and every value
/// redacted, ordered by name, so the output does not depend on a
/// `HashMap`'s order.
pub(crate) fn values<'a, I>(map: I) -> BTreeMap<&'a str, &'static str>
where
    I: IntoIterator<Item = (&'a String, &'a String)>,
{
    map.into_iter()
        .map(|(name, _)| (name.as_str(), REDACTED))
        .collect()
}

/// A JSON value with the secrets `LOUD_WIRE` redacts replaced, by the
/// wire's own walk (`crate::wire::redact_secrets`); nothing is truncated.
/// The value is cloned only to be printed.
pub(crate) fn json(value: &serde_json::Value) -> serde_json::Value {
    let mut value = value.clone();
    crate::wire::redact_secrets(&mut value);
    value
}

/// [`json`] for a payload carried as text (an error body, an SSE frame's
/// `data`): JSON with a secret prints redacted and re-serialized, and any
/// other text as it is.
pub(crate) fn raw_json(raw: &str) -> Cow<'_, str> {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(value) => {
            let redacted = json(&value);
            if redacted == value {
                Cow::Borrowed(raw)
            } else {
                Cow::Owned(redacted.to_string())
            }
        }
        Err(_) => Cow::Borrowed(raw),
    }
}

/// A JSON value with every string replaced and the keys kept: for a map the
/// crate could not read, where a secret cannot be told from a name.
pub(crate) fn json_strings(value: &serde_json::Value) -> serde_json::Value {
    let mut value = value.clone();
    crate::wire::redact_strings(&mut value);
    value
}

#[cfg(test)]
#[path = "redact_tests.rs"]
mod tests;
