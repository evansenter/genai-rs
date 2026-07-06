//! Client-side triggers (stub).
//!
//! The wire protocol already supports trigger delivery — the
//! [`InputEvent::AutomatedTrigger`](super::protocol::InputEvent) event
//! injects a message into the conversation outside a user turn, and
//! [`TriggerConfig`] captures the intended configuration surface — but the
//! scheduling runtime (interval timers driving `automated_trigger` sends,
//! mirroring the reference SDK's `TriggerRunner`) is not implemented yet.
//!
//! Planned follow-up: `AgentBuilder::add_trigger(TriggerConfig)` spawning a
//! per-trigger timer task that sends `automated_trigger` events whenever the
//! agent is idle, with jitter and overlap suppression.

use std::time::Duration;

/// Configuration for a recurring client-side trigger.
///
/// Not yet wired into [`AgentBuilder`](super::AgentBuilder) — see the
/// module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TriggerConfig {
    /// Message injected into the conversation on each firing.
    pub message: String,
    /// Interval between firings.
    pub interval: Duration,
}

impl TriggerConfig {
    /// Creates a trigger that injects `message` every `interval`.
    #[must_use]
    pub fn new(message: impl Into<String>, interval: Duration) -> Self {
        Self {
            message: message.into(),
            interval,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trigger_config_construction() {
        let trigger = TriggerConfig::new("check the queue", Duration::from_secs(60));
        assert_eq!(trigger.message, "check the queue");
        assert_eq!(trigger.interval, Duration::from_secs(60));
    }
}
