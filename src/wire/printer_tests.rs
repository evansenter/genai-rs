use super::{WireEvent, WireFilter};
use serde_json::json;

fn ws(payload: serde_json::Value) -> WireEvent {
    WireEvent::WsReceive { id: 1, payload }
}

fn request() -> WireEvent {
    WireEvent::Request {
        id: 1,
        method: "POST".into(),
        url: "https://x/y".into(),
        body: None,
    }
}

#[test]
fn on_values_keep_everything() {
    // The historical spellings must not start filtering anything out.
    for raw in ["1", "true", "yes", "on", "all", "", "  "] {
        let f = WireFilter::parse(raw);
        assert!(f.allows(&request()), "{raw:?} should keep requests");
        assert!(
            f.allows(&ws(json!({"stepUpdate": {}}))),
            "{raw:?} should keep ws"
        );
        assert!(!f.is_summary(), "{raw:?} should not imply summary");
    }
}

#[test]
fn category_selectors_filter_by_event_kind() {
    let f = WireFilter::parse("request");
    assert!(f.allows(&request()));
    assert!(!f.allows(&ws(json!({"stepUpdate": {}}))));
    assert!(!f.allows(&WireEvent::ResponseStatus { id: 1, status: 200 }));

    let f = WireFilter::parse("response");
    assert!(f.allows(&WireEvent::ResponseStatus { id: 1, status: 200 }));
    assert!(!f.allows(&request()));

    // `ws` keeps every WebSocket message regardless of payload key.
    let f = WireFilter::parse("ws");
    assert!(f.allows(&ws(json!({"toolCall": {}}))));
    assert!(f.allows(&ws(json!({"anythingElse": {}}))));
    assert!(!f.allows(&request()));
}

#[test]
fn payload_key_selectors_pick_out_one_message_type() {
    // The granularity that actually matters when reading a harness
    // session: which oneof arm, not which transport.
    let f = WireFilter::parse("stepUpdate");
    assert!(f.allows(&ws(json!({"stepUpdate": {"text": "hi"}}))));
    assert!(!f.allows(&ws(json!({"toolCall": {}}))));
    assert!(!f.allows(&request()));

    // Case-insensitive, and envelope metadata does not count as a match.
    let f = WireFilter::parse("STEPUPDATE");
    assert!(f.allows(&ws(json!({"stepUpdate": {}}))));
    let f = WireFilter::parse("seqNum");
    assert!(!f.allows(&ws(json!({"seqNum": "1", "toolCall": {}}))));

    // Usage rides along with a message rather than being one, and the
    // deserializer strips it before picking the oneof arm — so it must
    // not select either, or it would keep every message carrying usage.
    for envelope in ["usageUpdate", "usageMetadata"] {
        let f = WireFilter::parse(envelope);
        assert!(
            !f.allows(&ws(json!({"stepUpdate": {}, envelope: {"total": {}}}))),
            "{envelope:?} must not act as a selector"
        );
    }
    // ...and the message it rides on still selects normally.
    let f = WireFilter::parse("stepUpdate");
    assert!(f.allows(&ws(json!({"stepUpdate": {}, "usageMetadata": {}}))));
}

#[test]
fn selectors_compose_and_summary_is_a_modifier() {
    let f = WireFilter::parse("stepUpdate,toolCall");
    assert!(f.allows(&ws(json!({"stepUpdate": {}}))));
    assert!(f.allows(&ws(json!({"toolCall": {}}))));
    assert!(!f.allows(&ws(json!({"userInput": "x"}))));

    // `summary` alone changes rendering, not selection.
    let f = WireFilter::parse("summary");
    assert!(f.is_summary());
    assert!(f.allows(&request()));
    assert!(f.allows(&ws(json!({"anything": {}}))));

    // ...and combines with selectors.
    let f = WireFilter::parse("toolCall,summary");
    assert!(f.is_summary());
    assert!(f.allows(&ws(json!({"toolCall": {}}))));
    assert!(!f.allows(&request()));
}

#[test]
fn nested_selectors_reach_step_actions() {
    // The case this exists for: every builtin action lives one level
    // under `stepUpdate`, so a top-level-only match made the most
    // useful selector ("show me the MCP calls") match nothing.
    let step_with_mcp = ws(json!({
        "seqNum": "3",
        "stepUpdate": {
            "stepIndex": 2,
            "mcpTool": {"serverName": "widgets", "toolName": "list_widgets"},
        },
    }));
    let step_with_view = ws(json!({
        "stepUpdate": {"stepIndex": 4, "viewFile": {"path": "/tmp/x"}},
    }));

    let f = WireFilter::parse("mcpTool");
    assert!(f.allows(&step_with_mcp));
    assert!(
        !f.allows(&step_with_view),
        "a nested selector must still discriminate between actions"
    );

    // The enclosing key keeps working, and still matches both.
    let f = WireFilter::parse("stepUpdate");
    assert!(f.allows(&step_with_mcp));
    assert!(f.allows(&step_with_view));

    // Only one level deep: a leaf inside the action is not a selector,
    // or selectors would start colliding across unrelated messages.
    let f = WireFilter::parse("serverName");
    assert!(!f.allows(&step_with_mcp));

    // Scalar fields are not selectors either: `allows` and the summary
    // qualifier share `nested_action_keys`, so a selector can only
    // match something the label is able to name.
    let f = WireFilter::parse("stepIndex");
    assert!(!f.allows(&step_with_mcp));
}

#[test]
fn payload_keys_names_the_message_and_labels_the_empty_case() {
    use super::LoudWirePrinter;

    // The case that matters: on the harness path, envelope and payload
    // keys together must render as the payload alone, using the same
    // `is_envelope_key` filter selection uses. A divergence between
    // what a summary shows and what a selector matches would surface
    // here first.
    assert_eq!(
        LoudWirePrinter::ws_payload_keys(&json!({
            "seqNum": "7",
            "timestampMicros": "1",
            "stepUpdate": {"text": "hi"},
        })),
        "stepUpdate"
    );

    // A real message whose only content is usage — not a bug, so it
    // gets a label rather than a blank detail column. Neutral wording
    // because this renders HTTP bodies too.
    assert_eq!(
        LoudWirePrinter::ws_payload_keys(&json!({"seqNum": "7", "usageUpdate": {"total": {}}})),
        "(no payload keys)"
    );

    // Non-JSON frames arrive as a bare string.
    assert_eq!(
        LoudWirePrinter::payload_keys(&json!("not an object")),
        "(non-object)"
    );

    // A step carrying an action is qualified with it, so summary lines
    // say which action ran and agree with what a nested selector
    // matched on.
    assert_eq!(
        LoudWirePrinter::ws_payload_keys(&json!({
            "seqNum": "3",
            "stepUpdate": {
                "stepIndex": 2,
                "text": "List widgets",
                "mcpTool": {"serverName": "widgets"},
            },
        })),
        "stepUpdate/mcpTool"
    );
    // Scalar-only payloads are unqualified, exactly as before.
    assert_eq!(
        LoudWirePrinter::ws_payload_keys(&json!({
            "trajectoryStateUpdate": {"state": "STATE_FULLY_IDLE", "trajectoryId": "t-0"},
        })),
        "trajectoryStateUpdate"
    );
    // HTTP bodies go through the unqualified path, so a nested object
    // in a response body is not dressed up as an action.
    assert_eq!(
        LoudWirePrinter::payload_keys(&json!({"interaction": {"outputs": {}}})),
        "interaction"
    );
}

#[test]
fn send_frames_and_http_bodies_are_not_dressed_up_as_actions() {
    use super::LoudWirePrinter;

    // An InputEvent arm carrying an object-valued field is still just
    // that arm. Qualifying it would print `questionResponse/response`
    // and collide with `response`, the HTTP category selector.
    let question_response = json!({
        "questionResponse": {"response": {"answers": []}},
    });
    assert_eq!(
        LoudWirePrinter::payload_keys(&question_response),
        "questionResponse"
    );

    // usageMetadata is envelope bookkeeping on the harness wire...
    assert_eq!(
        LoudWirePrinter::ws_payload_keys(&json!({"seqNum": "1", "usageMetadata": {}})),
        "(no payload keys)"
    );
    // ...but a real field on a Gemini HTTP response, so the HTTP path
    // must still name it rather than reporting an empty body.
    assert_eq!(
        LoudWirePrinter::payload_keys(&json!({"usageMetadata": {"totalTokens": 7}})),
        "usageMetadata"
    );

    // Selection has to agree with that labelling, or `LOUD_WIRE=response`
    // would keep a line rendered `questionResponse` — a selector matching
    // nothing the printed label contains.
    let send = WireEvent::WsSend {
        id: 1,
        payload: question_response.clone(),
    };
    assert!(
        !WireFilter::parse("response").allows(&send),
        "a send frame must not be selected by an action nested inside its arm"
    );
    // The arm itself still selects it.
    assert!(WireFilter::parse("questionResponse").allows(&send));
    // And a received frame keeps the nested granularity it was added for.
    assert!(WireFilter::parse("mcpTool").allows(&WireEvent::WsReceive {
        id: 1,
        payload: json!({"stepUpdate": {"mcpTool": {"name": "x"}}}),
    }));

    // Envelope stripping is gated the same way on both sides. A
    // received frame's seqNum neither renders nor selects...
    let received = WireEvent::WsReceive {
        id: 1,
        payload: json!({"seqNum": "7", "stepUpdate": {"text": "hi"}}),
    };
    assert!(!WireFilter::parse("seqNum").allows(&received));
    // ...while on a send it does both, rather than one without the other.
    let send_with_envelope = WireEvent::WsSend {
        id: 1,
        payload: json!({"seqNum": "7", "userInput": {}}),
    };
    assert!(WireFilter::parse("seqNum").allows(&send_with_envelope));
    assert!(
        LoudWirePrinter::payload_keys(&json!({"seqNum": "7", "userInput": {}})).contains("seqNum")
    );
}

#[test]
fn env_inspector_applies_the_filter_from_the_variable() {
    use crate::wire::env_inspector;

    // The nextest-per-process claim that used to justify unguarded
    // mutation here was false under `cargo test` — `Client::builder()`
    // reads LOUD_WIRE at construction, so a client built concurrently
    // with this window sees a stray printer (#418). Same guard as the
    // client.rs mutator; it also restores the ambient value on drop.
    let mut guard = crate::test_subscriber::LoudWireGuard::acquire();

    guard.set("toolCall,summary");
    let printer = env_inspector().expect("LOUD_WIRE set should yield a printer");
    assert!(printer.filter.is_summary());
    assert!(printer.filter.allows(&ws(json!({"toolCall": {}}))));
    assert!(!printer.filter.allows(&request()));

    // The historical spelling still means everything.
    guard.set("1");
    let printer = env_inspector().expect("printer");
    assert_eq!(printer.filter, WireFilter::all());

    guard.unset();
    assert!(
        env_inspector().is_none(),
        "an unset LOUD_WIRE must install nothing"
    );
}

#[test]
fn summary_survives_an_on_value_in_either_order() {
    // Even alongside an "on" value, summary survives — in either order.
    // `summary` is a modifier, so token position must not change what
    // the value means.
    for raw in ["summary,1", "1,summary", "stepUpdate,1,summary"] {
        let f = WireFilter::parse(raw);
        assert!(f.is_summary(), "{raw:?} should keep summary");
        assert!(f.allows(&request()), "{raw:?} should keep everything");
        assert!(
            f.allows(&ws(json!({"toolCall": {}}))),
            "{raw:?}: an \"on\" value overrides narrower selectors"
        );
    }
}
