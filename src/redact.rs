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
//! Each impl destructures its type without `..`, so a field added later
//! fails to compile until the impl prints it, or redacts it.

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

#[cfg(test)]
#[path = "redact_tests.rs"]
mod tests;
