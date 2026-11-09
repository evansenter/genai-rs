//! `Debug` helpers for the Antigravity types that hold a secret: an API
//! key, an HTTP header value or an environment variable's value. The
//! crate's rule is that such a type prints the secret redacted, as `Client`
//! and `CredentialConfig` do, so `dbg!` or `tracing::debug!(?value)` cannot
//! undo the wire inspector's redaction. A secret's name (a header's, a
//! variable's) stays readable; only its value is hidden.

use std::collections::BTreeMap;
use std::fmt;

/// What a redacted value prints as.
pub(crate) const REDACTED: &str = "[REDACTED]";

/// Prints a map's keys, each with a redacted value: the names of headers or
/// environment variables, never their values.
pub(crate) struct RedactedValues<'a>(pub(crate) &'a BTreeMap<String, String>);

impl fmt::Debug for RedactedValues<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.0.keys().map(|name| (name, REDACTED)))
            .finish()
    }
}

/// An optional secret as it prints: whether it is set, not what it is.
pub(crate) fn redacted<T>(secret: Option<&T>) -> Option<&'static str> {
    secret.map(|_| REDACTED)
}

#[cfg(test)]
#[path = "redact_tests.rs"]
mod tests;
