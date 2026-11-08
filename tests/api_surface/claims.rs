//! The date check on the coverage oracle's reasoned exceptions: a reason
//! that states what the API did, or does, carries the `YYYY-MM` date of the
//! observation (D-010), since a live fact goes stale and the date says how
//! stale.
//!
//! Shared by the in-crate oracle's lists (`src/coverage_oracle/`), the wire
//! oracle's (`tests/request_wire_oracle.rs`) and the endpoint map's
//! (`tests/ci_coverage.rs`, which includes this file alone).

/// Whether a reason states what the API did, or does (a word like
/// "served", "rejected", "accepted" or "live", or an HTTP error status),
/// without the `YYYY-MM` date of the observation.
pub fn undated_live_claim(reason: &str) -> bool {
    const STEMS: &[&str] = &[
        "live", "serv", "reject", "accept", "ignor", "observ", "seen", "probe",
    ];
    let lower = reason.to_lowercase();
    let claims = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| {
            STEMS.iter().any(|stem| word.starts_with(stem))
                || (word.len() == 3
                    && word.starts_with(['4', '5'])
                    && word.bytes().all(|b| b.is_ascii_digit()))
        });
    let dated = reason.as_bytes().windows(7).any(|w| {
        w.starts_with(b"20")
            && w[2..4].iter().all(u8::is_ascii_digit)
            && w[4] == b'-'
            && w[5..7].iter().all(u8::is_ascii_digit)
    });
    claims && !dated
}

/// Every `list: entry: reason` among `(list, entries)` whose reason is an
/// undated live claim, for a test to fail on.
pub fn undated_live_claims(lists: &[(&str, Vec<(&str, &str)>)]) -> Vec<String> {
    lists
        .iter()
        .flat_map(|(list, entries)| {
            entries
                .iter()
                .filter(|(_, reason)| undated_live_claim(reason))
                .map(move |(entry, reason)| format!("{list}: {entry}: {reason}"))
        })
        .collect()
}
