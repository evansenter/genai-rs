//! Property tests: webhook, trigger and environment types.

use super::*;

pub(super) fn arb_trigger_status() -> impl Strategy<Value = TriggerStatus> {
    prop_oneof![
        Just(TriggerStatus::Active),
        Just(TriggerStatus::Paused),
        Just(TriggerStatus::Error),
        unknown_or(TriggerStatus::Active, |status_type| {
            TriggerStatus::Unknown {
                data: serde_json::Value::String(status_type.clone()),
                status_type,
            }
        }),
    ]
}

pub(super) fn arb_trigger_execution_status() -> impl Strategy<Value = TriggerExecutionStatus> {
    prop_oneof![
        Just(TriggerExecutionStatus::InProgress),
        Just(TriggerExecutionStatus::Completed),
        Just(TriggerExecutionStatus::Failed),
        Just(TriggerExecutionStatus::Skipped),
        Just(TriggerExecutionStatus::TimedOut),
        unknown_or(TriggerExecutionStatus::InProgress, |status_type| {
            TriggerExecutionStatus::Unknown {
                data: serde_json::Value::String(status_type.clone()),
                status_type,
            }
        }),
    ]
}

pub(super) fn arb_environment_status() -> impl Strategy<Value = EnvironmentStatus> {
    prop_oneof![
        Just(EnvironmentStatus::Active),
        Just(EnvironmentStatus::Expired),
        unknown_or(EnvironmentStatus::Active, |status_type| {
            EnvironmentStatus::Unknown {
                data: serde_json::Value::String(status_type.clone()),
                status_type,
            }
        }),
    ]
}

pub(super) fn arb_webhook_event() -> impl Strategy<Value = WebhookEvent> {
    prop_oneof![
        Just(WebhookEvent::BatchSucceeded),
        Just(WebhookEvent::BatchExpired),
        Just(WebhookEvent::BatchFailed),
        Just(WebhookEvent::InteractionRequiresAction),
        Just(WebhookEvent::InteractionCompleted),
        Just(WebhookEvent::InteractionFailed),
        Just(WebhookEvent::VideoGenerated),
        unknown_or(WebhookEvent::BatchSucceeded, |event_type| {
            WebhookEvent::Unknown {
                data: serde_json::Value::String(event_type.clone()),
                event_type,
            }
        }),
    ]
}

pub(super) fn arb_webhook_state() -> impl Strategy<Value = WebhookState> {
    prop_oneof![
        Just(WebhookState::Enabled),
        Just(WebhookState::Disabled),
        Just(WebhookState::DisabledDueToFailedDeliveries),
        unknown_or(WebhookState::Enabled, |state_type| WebhookState::Unknown {
            data: serde_json::Value::String(state_type.clone()),
            state_type,
        }),
    ]
}

pub(super) fn arb_revocation_behavior() -> impl Strategy<Value = RevocationBehavior> {
    prop_oneof![
        Just(RevocationBehavior::RevokePreviousSecretsAfterH24),
        Just(RevocationBehavior::RevokePreviousSecretsImmediately),
        unknown_or(
            RevocationBehavior::RevokePreviousSecretsAfterH24,
            |behavior_type| RevocationBehavior::Unknown {
                data: serde_json::Value::String(behavior_type.clone()),
                behavior_type,
            }
        ),
    ]
}

pub(crate) fn arb_webhook_config() -> impl Strategy<Value = WebhookConfig> {
    (
        proptest::option::of(proptest::collection::vec(arb_text(), 0..3)),
        proptest::option::of(arb_unknown_object()),
    )
        .prop_map(|(uris, user_metadata)| WebhookConfig {
            uris,
            user_metadata,
            ..Default::default()
        })
}

pub(super) fn arb_source_type() -> impl Strategy<Value = SourceType> {
    prop_oneof![
        Just(SourceType::Gcs),
        Just(SourceType::Inline),
        Just(SourceType::Repository),
        Just(SourceType::SkillRegistry),
        unknown_or(SourceType::Gcs, |source_type| SourceType::Unknown {
            data: serde_json::Value::String(source_type.clone()),
            source_type,
        }),
    ]
}

pub(super) fn arb_environment_source() -> impl Strategy<Value = EnvironmentSource> {
    (
        proptest::option::of(arb_source_type()),
        proptest::option::of(arb_identifier()),
        proptest::option::of(arb_identifier()),
        proptest::option::of(arb_text()),
        proptest::option::of(arb_identifier()),
    )
        .prop_map(
            |(source_type, source, target, content, encoding)| EnvironmentSource {
                source_type,
                source,
                target,
                content,
                encoding,
                ..Default::default()
            },
        )
}

pub(super) fn arb_network_config() -> impl Strategy<Value = NetworkConfig> {
    prop_oneof![
        Just(NetworkConfig::Disabled),
        Just(NetworkConfig::AllowlistDisabled),
        proptest::collection::vec(
            (
                arb_identifier(),
                proptest::option::of(proptest::collection::vec(
                    proptest::collection::btree_map(arb_identifier(), arb_text(), 1..3)
                        .prop_map(|m| m.into_iter().collect::<std::collections::HashMap<_, _>>()),
                    1..2,
                )),
            )
                .prop_map(|(domain, transform)| AllowlistEntry {
                    domain,
                    transform,
                    ..Default::default()
                }),
            0..3,
        )
        .prop_map(NetworkConfig::allowlist),
    ]
}

pub(crate) fn arb_environment_spec() -> impl Strategy<Value = EnvironmentSpec> {
    prop_oneof![
        arb_identifier().prop_map(EnvironmentSpec::Id),
        (
            proptest::collection::vec(arb_environment_source(), 0..3),
            proptest::option::of(arb_network_config()),
        )
            .prop_map(|(sources, network)| {
                EnvironmentSpec::Remote(RemoteEnvironment {
                    sources,
                    network,
                    ..Default::default()
                })
            }),
    ]
}

proptest! {
    /// Test that TriggerStatus roundtrips correctly through JSON.
    #[test]
    fn trigger_status_roundtrip(status in arb_trigger_status()) {
        let json = serde_json::to_string(&status).expect("Serialization should succeed");
        let restored: TriggerStatus = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(status, restored);
    }

    /// Test that TriggerExecutionStatus roundtrips correctly through JSON.
    #[test]
    fn trigger_execution_status_roundtrip(status in arb_trigger_execution_status()) {
        let json = serde_json::to_string(&status).expect("Serialization should succeed");
        let restored: TriggerExecutionStatus = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(status, restored);
    }

    /// Test that EnvironmentStatus roundtrips correctly through JSON.
    #[test]
    fn environment_status_roundtrip(status in arb_environment_status()) {
        let json = serde_json::to_string(&status).expect("Serialization should succeed");
        let restored: EnvironmentStatus = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(status, restored);
    }

    /// Test that WebhookEvent roundtrips correctly through JSON.
    #[test]
    fn webhook_event_roundtrip(event in arb_webhook_event()) {
        let json = serde_json::to_string(&event).expect("Serialization should succeed");
        let restored: WebhookEvent = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(event, restored);
    }

    /// Test that WebhookState roundtrips correctly through JSON.
    #[test]
    fn webhook_state_roundtrip(state in arb_webhook_state()) {
        let json = serde_json::to_string(&state).expect("Serialization should succeed");
        let restored: WebhookState = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(state, restored);
    }

    /// Test that RevocationBehavior roundtrips correctly through JSON.
    #[test]
    fn revocation_behavior_roundtrip(behavior in arb_revocation_behavior()) {
        let json = serde_json::to_string(&behavior).expect("Serialization should succeed");
        let restored: RevocationBehavior = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(behavior, restored);
    }

    /// Test that WebhookConfig roundtrips correctly through JSON.
    #[test]
    fn webhook_config_roundtrip(config in arb_webhook_config()) {
        let json = serde_json::to_string(&config).expect("Serialization should succeed");
        let restored: WebhookConfig = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(config, restored);
    }

    /// Test that SourceType roundtrips correctly through JSON.
    #[test]
    fn source_type_roundtrip(source_type in arb_source_type()) {
        let json = serde_json::to_string(&source_type).expect("Serialization should succeed");
        let restored: SourceType = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(source_type, restored);
    }

    /// Test that EnvironmentSpec (string ID or remote environment)
    /// roundtrips correctly through JSON.
    #[test]
    fn environment_spec_roundtrip(spec in arb_environment_spec()) {
        let json = serde_json::to_string(&spec).expect("Serialization should succeed");
        let restored: EnvironmentSpec = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(spec, restored);
    }
}
