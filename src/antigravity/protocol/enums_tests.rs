use super::*;
use serde_json::json;

/// A non-string value read from the harness goes back out as the
/// original JSON (C2 of the 2026-11-07 grading); it used to be written
/// as a string, `7` becoming `"7"`.
#[test]
fn a_non_string_unknown_round_trips_as_its_json() {
    for value in [
        json!(7),
        json!(null),
        json!(true),
        json!({"state": "x"}),
        json!(["a"]),
    ] {
        let state: TrajectoryState = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(
            state.unknown_state_type(),
            Some(crate::wire_enum::non_string_descriptor(&value).as_str())
        );
        assert_eq!(state.unknown_data(), Some(&value));
        assert_eq!(serde_json::to_value(&state).unwrap(), value);
        let hook: LifecycleHook = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&hook).unwrap(), value);
    }
}

#[test]
fn a_string_unknown_round_trips_and_is_recorded_as_drift() {
    let state: TrajectoryState = serde_json::from_value(json!("STATE_WP13_HIBERNATING")).unwrap();
    assert_eq!(state.unknown_state_type(), Some("STATE_WP13_HIBERNATING"));
    assert_eq!(
        serde_json::to_value(&state).unwrap(),
        "STATE_WP13_HIBERNATING"
    );
    assert!(super::super::drift_report().contains_key("TrajectoryState=STATE_WP13_HIBERNATING"));
}

/// A hand-built `Unknown` writes its type string, whatever `data` holds.
#[test]
fn a_hand_built_unknown_serializes_its_type() {
    let state = TrajectoryState::Unknown {
        state_type: "7".to_string(),
        data: json!(7),
    };
    assert_eq!(serde_json::to_value(&state).unwrap(), "7");
    let state = TrajectoryState::Unknown {
        state_type: "STATE_X".to_string(),
        data: Value::Null,
    };
    assert_eq!(serde_json::to_value(&state).unwrap(), "STATE_X");
    assert_eq!(state.as_wire_str(), "STATE_X");
}
