use super::*;
use crate::request::InteractionInput;

fn probe_interaction() -> InteractionRequest {
    InteractionRequest {
        agent: Some("my-agent".to_string()),
        input: InteractionInput::Text("Say OK".to_string()),
        ..Default::default()
    }
}

#[test]
fn list_triggers_setters_fill_the_query() {
    let client = Client::new("k".to_string());
    let list = client.triggers().list();
    assert_eq!(list.page_size, None);
    assert_eq!(list.page_token, None);

    let list = list
        .with_page_size(5)
        .with_page_token("t1")
        // `with_*` replaces.
        .with_page_token("t2");
    assert_eq!(list.page_size, Some(5));
    assert_eq!(list.page_token.as_deref(), Some("t2"));
}

#[test]
fn list_trigger_executions_owns_its_trigger_id_and_query() {
    let client = Client::new("k".to_string());
    let trigger_id = String::from("trig-1");
    let list = client.triggers().list_executions(&trigger_id);
    // The builder owns its ID, so the caller's string can go.
    drop(trigger_id);
    assert_eq!(list.trigger_id, "trig-1");
    assert_eq!(list.page_size, None);
    assert_eq!(list.page_token, None);

    let list = list
        .with_page_size(10)
        .with_page_size(2)
        .with_page_token("next");
    assert_eq!(list.page_size, Some(2));
    assert_eq!(list.page_token.as_deref(), Some("next"));
}

#[test]
fn triggers_handle_and_lists_debug_redact_the_api_key() {
    let client = Client::new("secret-api-key".to_string());
    for debug in [
        format!("{:?}", client.triggers()),
        format!("{:?}", client.triggers().list().with_page_size(1)),
        format!("{:?}", client.triggers().list_executions("trig-1")),
    ] {
        assert!(!debug.contains("secret-api-key"), "{debug}");
        assert!(debug.contains("[REDACTED]"), "{debug}");
    }
}

#[test]
fn create_params_serialize_minimal() {
    let params = TriggerCreateParams::new("0 5 1 1 *", "UTC", probe_interaction());
    let json = serde_json::to_value(&params).unwrap();
    assert_eq!(json["schedule"], "0 5 1 1 *");
    assert_eq!(json["time_zone"], "UTC");
    assert_eq!(json["interaction"]["agent"], "my-agent");
    assert!(json.get("display_name").is_none());
}

#[test]
fn trigger_update_serializes_partial() {
    // The send direction for TriggerStatus: pause/resume rides on this
    // exact wire value.
    let update = TriggerUpdate::new().with_status(TriggerStatus::Paused);
    assert_eq!(
        serde_json::to_value(&update).unwrap(),
        serde_json::json!({"status": "paused"})
    );

    // An empty update must serialize to an empty object. There is no
    // update_mask on this endpoint (see the TriggerUpdate docs), so
    // omitting unset fields from the body is the only partial-update
    // mechanism the wire offers — this pins that we never send nulls.
    assert_eq!(
        serde_json::to_value(TriggerUpdate::new()).unwrap(),
        serde_json::json!({})
    );

    let named = TriggerUpdate::new()
        .with_display_name("renamed")
        .with_status(TriggerStatus::Active);
    let json = serde_json::to_value(&named).unwrap();
    assert_eq!(json["display_name"], "renamed");
    assert_eq!(json["status"], "active");
}

#[test]
fn list_envelopes_deserialize_under_spec_keys() {
    // Pins the envelope keys the crate is betting on for the resource
    // ENUM_WIRE_FORMATS.md marks as unverified: `triggers` and (the one
    // that diverges from its path segment) `trigger_executions`. If the
    // live wire turns out to use `executions`, the fix lands as a
    // visible diff here rather than as a list that quietly reads zero.
    let list: TriggerListResponse =
        serde_json::from_value(serde_json::json!({"triggers": [{"id": "t1"}]})).unwrap();
    assert_eq!(list.triggers.len(), 1);

    let executions: TriggerExecutionListResponse = serde_json::from_value(serde_json::json!({
        "trigger_executions": [{"id": "e1", "status": "completed"}]
    }))
    .unwrap();
    assert_eq!(executions.trigger_executions.len(), 1);
    assert_eq!(
        executions.trigger_executions[0].status,
        Some(TriggerExecutionStatus::Completed)
    );
}

#[test]
fn create_params_and_update_pass_through_unmodeled_fields() {
    // Trigger bodies can't be live-verified while creation is
    // agent-gated, so the Evergreen extra map is the release valve for
    // fields the crate doesn't model yet (same shape as
    // CreateEnvironmentRequest::extra next door).
    let mut params = TriggerCreateParams::new("0 9 * * *", "UTC", probe_interaction());
    params
        .extra
        .insert("future_field".into(), serde_json::json!("x"));
    let json = serde_json::to_value(&params).unwrap();
    assert_eq!(json["future_field"], "x");

    // A colliding key wins on serialize (the flattened map is emitted
    // last) — pinned so the precedence reads as a decision rather than
    // an artifact of field-declaration order.
    params
        .extra
        .insert("schedule".into(), serde_json::json!("*/5 * * * *"));
    let json = serde_json::to_value(&params).unwrap();
    assert_eq!(json["schedule"], "*/5 * * * *");

    let mut update = TriggerUpdate::new();
    update.extra.insert("other".into(), serde_json::json!(1));
    let json = serde_json::to_value(&update).unwrap();
    assert_eq!(json, serde_json::json!({"other": 1}));
    // An empty map keeps the empty-update-is-{} contract.
    assert_eq!(
        serde_json::to_value(TriggerUpdate::new()).unwrap(),
        serde_json::json!({})
    );

    // The deserialize direction: an unmodeled key on the way in lands
    // in `extra` — the documented absorption behavior a config-file
    // typo relies on, and the direction where a flatten regression
    // (flatten buffers every sibling through serde's Content, and the
    // nested interaction deserializers are custom) would be silent.
    let params: TriggerCreateParams = serde_json::from_value(serde_json::json!({
        "schedule": "0 9 * * *",
        "time_zone": "UTC",
        "interaction": {"agent": "my-agent", "input": "hi"},
        "future_field": "x"
    }))
    .unwrap();
    assert_eq!(params.extra["future_field"], "x");

    // The int64s accept the protobuf-JSON string form on the way in
    // (a config seeded from a stored Trigger re-serializes them as
    // strings) while still sending plain numbers on the way out.
    let params: TriggerCreateParams = serde_json::from_value(serde_json::json!({
        "schedule": "0 9 * * *",
        "time_zone": "UTC",
        "interaction": {"agent": "my-agent", "input": "hi"},
        "max_consecutive_failures": "3",
        "execution_timeout_seconds": "600"
    }))
    .unwrap();
    assert_eq!(params.max_consecutive_failures, Some(3));
    assert_eq!(params.execution_timeout_seconds, Some(600));
    let json = serde_json::to_value(&params).unwrap();
    assert_eq!(json["max_consecutive_failures"], serde_json::json!(3));
    assert_eq!(json["execution_timeout_seconds"], serde_json::json!(600));

    // Unlike the response side there is no page to protect here, so a
    // malformed value stays a clean load-time error rather than
    // silently dropping the field (which would create a trigger with
    // no auto-disable cap).
    for bad in [
        serde_json::json!("three"),
        serde_json::json!(""),
        serde_json::json!(3.5),
        serde_json::json!(true),
    ] {
        let result: Result<TriggerCreateParams, _> = serde_json::from_value(serde_json::json!({
            "schedule": "0 9 * * *",
            "time_zone": "UTC",
            "interaction": {"agent": "my-agent", "input": "hi"},
            "max_consecutive_failures": bad
        }));
        assert!(result.is_err(), "send-side int64 must stay strict");
    }
    let update: TriggerUpdate = serde_json::from_value(serde_json::json!({"other": 1})).unwrap();
    assert_eq!(update.extra["other"], 1);
}

#[test]
fn store_warn_fires_on_both_construction_paths() {
    // The store rejection is server-verified but masked by the agent
    // gate, so the pre-flight warn is the only signal most users get.
    // Pin that it fires from new() AND from a config-file load — a
    // value assertion cannot express either.
    let messages = crate::test_subscriber::capture_messages(|| {
        let mut interaction = probe_interaction();
        interaction.store = Some(true);
        let _ = TriggerCreateParams::new("0 9 * * *", "UTC", interaction);
    });
    assert!(
        messages.iter().any(|m| m.contains("store")),
        "new() must warn on store; got: {messages:?}"
    );

    let messages = crate::test_subscriber::capture_messages(|| {
        let params: TriggerCreateParams = serde_json::from_value(serde_json::json!({
            "schedule": "0 9 * * *",
            "time_zone": "UTC",
            "interaction": {"agent": "my-agent", "input": "hi", "store": true}
        }))
        .unwrap();
        assert_eq!(params.interaction.store, Some(true));
    });
    assert!(
        messages.iter().any(|m| m.contains("store")),
        "the deserialize path must warn on store; got: {messages:?}"
    );
}

#[test]
fn missing_agent_warn_fires_on_model_only_interactions() {
    // The other live-verified rejection: a model-only interaction is
    // refused ("Agent '' is invalid or not found"), and the agent
    // gate masks it until the round-trip. Pin that the funnel warns,
    // and that an agent-targeting interaction stays quiet.
    let messages = crate::test_subscriber::capture_messages(|| {
        let interaction = crate::request::InteractionRequest {
            model: Some("test-model".to_string()),
            input: InteractionInput::Text("Daily audit".to_string()),
            ..Default::default()
        };
        let _ = TriggerCreateParams::new("0 9 * * *", "UTC", interaction);
    });
    assert!(
        messages.iter().any(|m| m.contains("custom agent")),
        "new() must warn on a model-only interaction; got: {messages:?}"
    );

    let messages = crate::test_subscriber::capture_messages(|| {
        let _ = TriggerCreateParams::new("0 9 * * *", "UTC", probe_interaction());
    });
    assert!(
        !messages.iter().any(|m| m.contains("custom agent")),
        "an agent-targeting interaction must not warn; got: {messages:?}"
    );
}

#[test]
fn empty_input_warn_fires_on_the_struct_literal_shape() {
    // The strict `input` deserialize can't catch a struct literal that
    // rides `..Default::default()` — the field is present and
    // well-formed, just the empty zero value — nor the explicit
    // empty-vector spellings of the same mistake. Pin that the funnel
    // warns on all three, and that real input stays quiet.
    let empty_inputs = [
        InteractionInput::default(),
        InteractionInput::Content(Vec::new()),
        InteractionInput::Steps(Vec::new()),
    ];
    for input in empty_inputs {
        let variant = format!("{input:?}");
        let messages = crate::test_subscriber::capture_messages(|| {
            let interaction = crate::request::InteractionRequest {
                agent: Some("my-agent".to_string()),
                input,
                ..Default::default()
            };
            let _ = TriggerCreateParams::new("0 9 * * *", "UTC", interaction);
        });
        assert!(
            messages.iter().any(|m| m.contains("empty prompt")),
            "new() must warn on empty {variant}; got: {messages:?}"
        );
    }

    // The config-file spelling of the same mistake: an empty JSON
    // array parses cleanly as empty `Steps` (input_from_value's
    // is-content check is vacuously false for it), so the deserialize
    // funnel must warn too — this is the shape a user hits without
    // ever writing a struct literal.
    let messages = crate::test_subscriber::capture_messages(|| {
        let params: TriggerCreateParams = serde_json::from_value(serde_json::json!({
            "schedule": "0 9 * * *",
            "time_zone": "UTC",
            "interaction": {"agent": "my-agent", "input": []}
        }))
        .unwrap();
        assert!(matches!(
            &params.interaction.input,
            InteractionInput::Steps(steps) if steps.is_empty()
        ));
    });
    assert!(
        messages.iter().any(|m| m.contains("empty prompt")),
        "the deserialize path must warn on an empty-array input; got: {messages:?}"
    );

    let messages = crate::test_subscriber::capture_messages(|| {
        let _ = TriggerCreateParams::new("0 9 * * *", "UTC", probe_interaction());
    });
    assert!(
        !messages.iter().any(|m| m.contains("empty prompt")),
        "real input must not warn; got: {messages:?}"
    );
}

/// Pins the third roundtrip asymmetry documented on
/// [`Trigger::interaction`]: because that field is an
/// [`InteractionRequest`], a trigger read back with a bare `[Content]`
/// input re-serializes as a `user_input` step (#427).
///
/// Asserted rather than left to prose so that moving the wrap back onto
/// `InteractionInput`'s own `Serialize` — or removing it — cannot make
/// that doc comment stale in silence.
#[test]
fn trigger_interaction_reshapes_a_bare_content_input() {
    let trigger: Trigger = serde_json::from_value(serde_json::json!({
        "name": "triggers/abc",
        "interaction": {
            "model": "test-model",
            "input": [{"type": "text", "text": "hi"}],
        },
    }))
    .expect("a trigger with a bare content-array input should deserialize");

    // Read back as `Content` — the shape the server sent.
    assert!(
        matches!(
            trigger.interaction.as_ref().map(|i| &i.input),
            Some(InteractionInput::Content(c)) if c.len() == 1
        ),
        "expected a Content input, got {:?}",
        trigger.interaction.as_ref().map(|i| &i.input)
    );

    // Re-serialized as a step — the reshape the doc comment describes.
    let json = serde_json::to_value(&trigger).unwrap();
    assert_eq!(
        json["interaction"]["input"],
        serde_json::json!([{
            "type": "user_input",
            "content": [{"type": "text", "text": "hi"}],
        }]),
        "a bare content array must come back out as a user_input step"
    );
}

#[test]
fn trigger_int64s_tolerate_string_wire_form() {
    // The environments resource live-verified that this API family
    // serializes int64s as protobuf-JSON strings; a trigger doing the
    // same must degrade per-field, not fail the whole list response.
    let json = serde_json::json!({
        "id": "trig-1",
        "max_consecutive_failures": "3",
        "consecutive_failure_count": "1",
        "execution_timeout_seconds": 600
    });
    let trigger: Trigger = serde_json::from_value(json).unwrap();
    assert_eq!(trigger.max_consecutive_failures, Some(3));
    assert_eq!(trigger.consecutive_failure_count, Some(1));
    assert_eq!(trigger.execution_timeout_seconds, Some(600));

    // Re-serialization is uniform with Environment's counts: the
    // protobuf-JSON string form, whichever form arrived.
    let back = serde_json::to_value(&trigger).unwrap();
    assert_eq!(back["max_consecutive_failures"], serde_json::json!("3"));
    assert_eq!(back["execution_timeout_seconds"], serde_json::json!("600"));
}

#[test]
fn trigger_timestamps_degrade_per_field() {
    // Same posture as the int64s on this wire-unverified resource: a
    // timestamp arriving in an unexpected encoding (epoch number,
    // proto-style object, garbage string) drops that field to None
    // instead of failing the whole list response.
    let json = serde_json::json!({
        "id": "trig-1",
        "create_time": "2026-08-08T12:30:00Z",
        "update_time": "not-a-time",
        "last_run_time": 1754656200,
        "next_run_time": {"seconds": 1754656200}
    });
    let trigger: Trigger = serde_json::from_value(json).unwrap();
    assert_eq!(trigger.id.as_deref(), Some("trig-1"));
    assert!(trigger.create_time.is_some());
    assert_eq!(trigger.update_time, None);
    assert_eq!(trigger.last_run_time, None);
    assert_eq!(trigger.next_run_time, None);

    let json = serde_json::json!({
        "id": "exec-1",
        "scheduled_time": "2026-08-08T12:30:00Z",
        "end_time": "garbage"
    });
    let execution: TriggerExecution = serde_json::from_value(json).unwrap();
    assert!(execution.scheduled_time.is_some());
    assert_eq!(execution.end_time, None);
}

#[test]
fn create_params_serialize_all_fields() {
    let params = TriggerCreateParams::new("0 9 * * *", "UTC", probe_interaction())
        .with_display_name("daily-audit")
        .with_environment_id("env-123")
        .with_max_consecutive_failures(3)
        .with_execution_timeout_seconds(600);
    let json = serde_json::to_value(&params).unwrap();
    assert_eq!(json["display_name"], "daily-audit");
    assert_eq!(json["environment_id"], "env-123");
    assert_eq!(json["max_consecutive_failures"], 3);
    assert_eq!(json["execution_timeout_seconds"], 600);
}

#[test]
fn empty_list_response_deserializes() {
    // GET /v1beta/triggers returns `{}` when nothing exists.
    let list: TriggerListResponse = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(list.triggers.is_empty());

    // Present-but-degenerate list keys — the shapes the struct-level
    // serde default does not reach — degrade rather than zeroing the
    // page with an error: null and non-array values read as empty.
    let list: TriggerListResponse =
        serde_json::from_value(serde_json::json!({"triggers": null})).unwrap();
    assert!(list.triggers.is_empty());
    let list: TriggerListResponse =
        serde_json::from_value(serde_json::json!({"triggers": "corrupted"})).unwrap();
    assert!(list.triggers.is_empty());
    let executions: TriggerExecutionListResponse =
        serde_json::from_value(serde_json::json!({"trigger_executions": null})).unwrap();
    assert!(executions.trigger_executions.is_empty());
    let executions: TriggerExecutionListResponse =
        serde_json::from_value(serde_json::json!({"trigger_executions": {"a": 1}})).unwrap();
    assert!(executions.trigger_executions.is_empty());

    // The alias hedges on the wire-unverified spellings: the
    // path-segment envelope key and the environments-convention
    // timestamps deserialize too.
    let executions: TriggerExecutionListResponse = serde_json::from_value(
        serde_json::json!({"executions": [{"id": "e1", "status": "completed"}]}),
    )
    .unwrap();
    assert_eq!(executions.trigger_executions.len(), 1);
    let trigger: Trigger = serde_json::from_value(serde_json::json!({
        "id": "t1",
        "created": "2026-08-08T12:30:00Z",
        "updated": "2026-08-08T12:31:00Z"
    }))
    .unwrap();
    assert!(trigger.create_time.is_some());
    assert!(trigger.update_time.is_some());
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn unknown_statuses_roundtrip() {
    let status: TriggerStatus = serde_json::from_value(serde_json::json!("snoozing")).unwrap();
    assert!(status.is_unknown());
    assert_eq!(serde_json::to_value(&status).unwrap(), "snoozing");

    let exec: TriggerExecutionStatus =
        serde_json::from_value(serde_json::json!("requeued")).unwrap();
    assert!(exec.is_unknown());
    assert_eq!(serde_json::to_value(&exec).unwrap(), "requeued");
}

#[test]
fn trigger_deserializes_sdk_shape() {
    let json = serde_json::json!({
        "id": "trig-1",
        "schedule": "0 9 * * *",
        "time_zone": "UTC",
        "interaction": {"agent": "my-agent", "input": "audit"},
        "status": "active",
        "next_run_time": "2026-08-09T09:00:00Z"
    });
    let trigger: Trigger = serde_json::from_value(json).unwrap();
    assert_eq!(trigger.id.as_deref(), Some("trig-1"));
    assert_eq!(trigger.status, Some(TriggerStatus::Active));
    assert!(trigger.next_run_time.is_some());
}

#[test]
fn sparse_trigger_projection_degrades_per_field() {
    // A list projection that elides the nested interaction (or any
    // other field) must still deserialize — Evergreen posture.
    let trigger: Trigger = serde_json::from_value(serde_json::json!({"id": "t"})).unwrap();
    assert_eq!(trigger.id.as_deref(), Some("t"));
    assert!(trigger.interaction.is_none());

    let execution: TriggerExecution =
        serde_json::from_value(serde_json::json!({"status": "completed"})).unwrap();
    assert!(execution.id.is_none());
    assert_eq!(execution.status, Some(TriggerExecutionStatus::Completed));

    // Present-but-partial interaction (identity fields without input)
    // must also degrade rather than fail the trigger.
    let trigger: Trigger = serde_json::from_value(serde_json::json!({
        "id": "t2",
        "interaction": {"agent": "my-agent"}
    }))
    .unwrap();
    let interaction = trigger.interaction.expect("interaction present");
    assert_eq!(interaction.agent.as_deref(), Some("my-agent"));
    // An absent input reads as empty text on this path (documented on
    // the field): indistinguishable from a genuinely empty prompt.
    assert_eq!(
        interaction.input,
        crate::request::InteractionInput::Text(String::new())
    );

    // An interaction carrying an undeserializable `input` — explicit
    // null (serde defaults only cover the key-absent case) or a stray
    // scalar — degrades to empty text too, instead of failing the
    // whole list response. (A malformed steps *array* is deliberately
    // not in this list: under default features the Evergreen Step
    // deserializer absorbs unrecognized elements as Unknown steps, so
    // only scalar shapes are rejectable in every feature mode.)
    for bad_input in [serde_json::Value::Null, serde_json::json!(0)] {
        let trigger: Trigger = serde_json::from_value(serde_json::json!({
            "id": "t3",
            "interaction": {"agent": "my-agent", "input": bad_input}
        }))
        .unwrap();
        let interaction = trigger.interaction.expect("interaction present");
        assert_eq!(
            interaction.input,
            crate::request::InteractionInput::Text(String::new())
        );
    }

    // A non-object `interaction` (stray scalar, array) or one with a
    // type mismatch on a modeled field degrades to None wholesale —
    // the catch-all arms, uniform with the serde_util helpers.
    for bad_interaction in [
        serde_json::json!(0),
        serde_json::json!([5]),
        serde_json::json!({"model": 5}),
    ] {
        let trigger: Trigger = serde_json::from_value(serde_json::json!({
            "id": "t4",
            "interaction": bad_interaction
        }))
        .unwrap();
        assert_eq!(trigger.id.as_deref(), Some("t4"));
        assert!(trigger.interaction.is_none());
    }

    // The leniency is scoped to the response side: the same malformed
    // input in a send-side TriggerCreateParams (e.g. loaded from a
    // config file) is a clean parse error, not a silently scheduled
    // empty prompt.
    let result: Result<TriggerCreateParams, _> = serde_json::from_value(serde_json::json!({
        "schedule": "0 9 * * *",
        "time_zone": "UTC",
        "interaction": {"agent": "my-agent", "input": 0}
    }));
    assert!(result.is_err(), "send-side input must stay strict");
    // Absent (or typo'd, e.g. "inputs") is equally a clean parse
    // error on the send side — `input` is a required field there, so
    // a config mistake cannot silently schedule an empty prompt.
    let result: Result<TriggerCreateParams, _> = serde_json::from_value(serde_json::json!({
        "schedule": "0 9 * * *",
        "time_zone": "UTC",
        "interaction": {"agent": "my-agent", "inputs": "typo"}
    }));
    assert!(result.is_err(), "send-side absent input must stay strict");
}

#[test]
fn execution_status_wire_values() {
    for (status, wire) in [
        (TriggerExecutionStatus::InProgress, "in_progress"),
        (TriggerExecutionStatus::Completed, "completed"),
        (TriggerExecutionStatus::Failed, "failed"),
        (TriggerExecutionStatus::Skipped, "skipped"),
        (TriggerExecutionStatus::TimedOut, "timed_out"),
    ] {
        assert_eq!(serde_json::to_value(&status).unwrap(), wire);
        // Display is public API and must agree with the wire value.
        assert_eq!(status.to_string(), wire);
    }
    for (status, wire) in [
        (TriggerStatus::Active, "active"),
        (TriggerStatus::Paused, "paused"),
        (TriggerStatus::Error, "error"),
    ] {
        assert_eq!(serde_json::to_value(&status).unwrap(), wire);
        assert_eq!(status.to_string(), wire);
    }
}

// --- Evergreen `extra` passthrough on response shapes (#406) ---

#[test]
fn trigger_preserves_unknown_response_fields() {
    // The response shape is unverified while trigger creation is
    // agent-gated, so a field the API returns today would otherwise be
    // both invisible and unrecoverable to a caller.
    let wire = serde_json::json!({
        "id": "trig_123",
        "display_name": "nightly",
        "future_field": {"nested": [1, 2]}
    });

    let trigger: Trigger = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        trigger.extra.get("future_field"),
        Some(&serde_json::json!({"nested": [1, 2]}))
    );
    assert_eq!(serde_json::to_value(&trigger).unwrap(), wire);
}

#[test]
fn trigger_without_unknown_fields_has_empty_extra() {
    let trigger: Trigger = serde_json::from_value(serde_json::json!({"id": "trig_123"})).unwrap();
    assert!(trigger.extra.is_empty());
    // An empty map must not add a key on serialize.
    assert_eq!(
        serde_json::to_value(&trigger).unwrap(),
        serde_json::json!({"id": "trig_123"})
    );
}

#[test]
fn trigger_execution_preserves_unknown_response_fields() {
    let wire = serde_json::json!({
        "id": "exec_1",
        "trigger_id": "trig_123",
        "future_metric": 42
    });

    let execution: TriggerExecution = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        execution.extra.get("future_metric"),
        Some(&serde_json::json!(42))
    );
    assert_eq!(serde_json::to_value(&execution).unwrap(), wire);
}

#[test]
fn trigger_extra_wins_on_collision() {
    // The doc comment on all five new `extra` fields states this as a
    // guarantee, and it is not intrinsic — it holds only because the
    // flattened map is emitted last, which is a consequence of `extra`
    // being declared after the modeled fields. Moving the declaration up
    // is a plausible tidy-up that nothing else in these structs depends
    // on, and it would silently flip the documented behaviour on all
    // five while every other test still passed. Pinned on `Trigger` as
    // the representative; the mechanism is identical across the five.
    let mut trigger: Trigger =
        serde_json::from_value(serde_json::json!({"id": "trig_123"})).unwrap();
    trigger
        .extra
        .insert("id".into(), serde_json::json!("from_extra"));
    let json = serde_json::to_value(&trigger).unwrap();
    assert_eq!(
        json["id"], "from_extra",
        "a colliding key must win on serialize, as the field doc promises"
    );
}

#[test]
fn trigger_execution_without_unknown_fields_has_empty_extra() {
    let execution: TriggerExecution =
        serde_json::from_value(serde_json::json!({"id": "exec_1"})).unwrap();
    assert!(execution.extra.is_empty());
    // An empty map must not add a key on serialize.
    assert_eq!(
        serde_json::to_value(&execution).unwrap(),
        serde_json::json!({"id": "exec_1"})
    );
}

#[test]
fn trigger_extra_does_not_disturb_equality_for_identical_wire() {
    // PartialEq includes the map, so two triggers parsed from the same
    // wire stay equal — existing equality-based tests are unaffected.
    let wire = serde_json::json!({"id": "trig_123", "unknown": true});
    let a: Trigger = serde_json::from_value(wire.clone()).unwrap();
    let b: Trigger = serde_json::from_value(wire).unwrap();
    assert_eq!(a, b);
}
