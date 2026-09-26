use super::*;

fn sample_events() -> Vec<WireEvent> {
    vec![
        WireEvent::Request {
            id: 1,
            method: "POST".to_string(),
            url: "https://example.com/v1beta/interactions".to_string(),
            body: Some(serde_json::json!({
                "model": "test-model",
                "data": "A".repeat(200),
            })),
        },
        WireEvent::Request {
            id: 2,
            method: "GET".to_string(),
            url: "https://example.com/v1beta/interactions/abc".to_string(),
            body: None,
        },
        WireEvent::ResponseStatus { id: 1, status: 200 },
        WireEvent::ResponseStatus { id: 1, status: 500 },
        WireEvent::ResponseBody {
            id: 1,
            body: serde_json::json!({"status": "completed"}),
        },
        WireEvent::ResponseBody {
            id: 1,
            body: serde_json::Value::String("not json".repeat(300)),
        },
        WireEvent::ErrorBody {
            id: 1,
            status: 429,
            body: r#"{"error": {"message": "quota"}}"#.to_string(),
        },
        WireEvent::ErrorBody {
            id: 1,
            status: 503,
            body: "plain text error \u{4e16}\u{754c}".repeat(100),
        },
        WireEvent::SseFrame {
            id: 1,
            event_type: None,
            data: r#"{"event_type": "step.delta"}"#.to_string(),
        },
        WireEvent::SseFrame {
            id: 1,
            event_type: Some("interaction.completed".to_string()),
            data: String::new(),
        },
        WireEvent::SseFrame {
            id: 1,
            event_type: None,
            data: "not json".to_string(),
        },
        WireEvent::UploadStart {
            id: 3,
            file_name: "video.mp4".to_string(),
            mime_type: "video/mp4".to_string(),
            size_bytes: 157_286_400,
        },
        WireEvent::UploadComplete {
            id: 3,
            uri: "https://example.com/files/abc".to_string(),
        },
        WireEvent::HarnessSpawn {
            id: 4,
            path: "/usr/local/bin/localharness".to_string(),
            pid: Some(4242),
        },
        WireEvent::HarnessSpawn {
            id: 4,
            path: "/usr/local/bin/localharness".to_string(),
            pid: None,
        },
        WireEvent::WsSend {
            id: 4,
            payload: serde_json::json!({"userInput": "hello"}),
        },
        WireEvent::WsReceive {
            id: 4,
            payload: serde_json::json!({"stepUpdate": {"textDelta": "hi"}}),
        },
        WireEvent::WsReceive {
            id: 4,
            payload: serde_json::Value::String("not json".to_string()),
        },
        WireEvent::HarnessStderr {
            id: 4,
            line: "harness diagnostic \u{4e16}\u{754c}".repeat(100),
        },
    ]
}

#[test]
fn test_truncate_utf8_short_string() {
    assert_eq!(truncate_utf8("short", 100), "short");
}

#[test]
fn test_truncate_utf8_exact_boundary() {
    let s = "a".repeat(100);
    assert_eq!(truncate_utf8(&s, 100), s);
}

#[test]
fn test_truncate_utf8_ascii() {
    let s = "a".repeat(200);
    let result = truncate_utf8(&s, 100);
    assert_eq!(result.len(), 103); // 100 + "..."
    assert!(result.ends_with("..."));
}

#[test]
fn test_truncate_utf8_multibyte_no_panic() {
    // 4-byte emoji straddling the truncation point must not panic and
    // must not be split mid-character.
    let s = "x".repeat(99) + "🎉🎉🎉";
    let result = truncate_utf8(&s, 100);
    assert!(result.ends_with("..."));
    assert!(!result.contains('\u{FFFD}'));
    assert_eq!(&result[..99], &"x".repeat(99));
    // 99 x's, emoji doesn't fit in the last byte, so cut at 99.
    assert_eq!(result.len(), 102); // 99 + "..."

    // Also exercise a string that is entirely multibyte.
    let cjk = "\u{4e16}\u{754c}".repeat(60); // 3 bytes per char, 360 bytes
    let result = truncate_utf8(&cjk, 100);
    assert!(result.ends_with("..."));
    assert!(result.len() <= 103);
    // Must be valid UTF-8 by construction; check boundary integrity.
    assert!(result.is_char_boundary(result.len() - 3));
}

#[test]
fn test_truncate_long_fields_char_boundary_safe() {
    // A "data" field where byte 100 falls inside a multibyte char.
    let payload = "x".repeat(99) + &"🎉".repeat(10);
    let mut value = serde_json::json!({ "data": payload, "text": "🎉".repeat(50) });
    truncate_long_fields(&mut value); // Must not panic.

    let data = value["data"].as_str().unwrap();
    assert!(data.ends_with("..."));
    // Text fields are never truncated.
    assert_eq!(value["text"].as_str().unwrap().chars().count(), 50);
}

#[test]
fn test_truncate_long_fields_nested() {
    let mut value = serde_json::json!({
        "model": "gemini",
        "content": {"data": "C".repeat(150), "signature": "S".repeat(150)},
        "items": [{"data": "D".repeat(150)}],
    });
    truncate_long_fields(&mut value);
    assert!(value["content"]["data"].as_str().unwrap().ends_with("..."));
    assert!(
        value["content"]["signature"]
            .as_str()
            .unwrap()
            .ends_with("...")
    );
    assert!(value["items"][0]["data"].as_str().unwrap().ends_with("..."));
    assert_eq!(value["model"], "gemini");
}

#[test]
fn test_truncate_long_fields_short_values_untouched() {
    let mut value = serde_json::json!({"data": "short", "signature": "sig"});
    truncate_long_fields(&mut value);
    assert_eq!(value["data"], "short");
    assert_eq!(value["signature"], "sig");
}

#[test]
fn test_redact_fields_api_key_fully_redacted() {
    // Short api_key values must be redacted, not merely truncated
    // (truncation leaves keys under the threshold fully intact).
    let mut value = serde_json::json!({
        "tools": [{
            "retrieval": {
                "exa_ai_search_config": {"api_key": "exa-secret-key"},
                "parallel_ai_search_config": {"api_key": "par-secret-key"}
            }
        }],
        "api_key": "top-level-secret"
    });
    truncate_long_fields(&mut value);
    let rendered = value.to_string();
    assert!(!rendered.contains("exa-secret-key"));
    assert!(!rendered.contains("par-secret-key"));
    assert!(!rendered.contains("top-level-secret"));
    assert_eq!(
        value["tools"][0]["retrieval"]["exa_ai_search_config"]["api_key"],
        "[REDACTED]"
    );
    assert_eq!(
        value["tools"][0]["retrieval"]["parallel_ai_search_config"]["api_key"],
        "[REDACTED]"
    );
    assert_eq!(value["api_key"], "[REDACTED]");
}

#[test]
fn test_truncate_fields_with_structured_values_still_redact_nested_secrets() {
    // A `data` (or `signature`) key can hold an object or array — e.g.
    // an Evergreen Unknown variant preserving raw JSON under `data`.
    // The walk must recurse into those subtrees so nested secrets are
    // still redacted, not skip them because the value is not a string.
    let mut value = serde_json::json!({
        "data": {"api_key": "nested-secret", "note": "kept"},
        "wrapper": {"data": [{"new_signing_secret": "whsec_nested"}]},
    });
    truncate_long_fields(&mut value);
    let rendered = value.to_string();
    assert!(!rendered.contains("nested-secret"), "leaked: {rendered}");
    assert!(!rendered.contains("whsec_nested"), "leaked: {rendered}");
    assert_eq!(value["data"]["api_key"], "[REDACTED]");
    assert_eq!(value["data"]["note"], "kept");
    assert_eq!(
        value["wrapper"]["data"][0]["new_signing_secret"],
        "[REDACTED]"
    );
}

#[test]
fn test_redact_fields_null_api_key_left_null() {
    // An absent/null key is not a secret; keep the JSON shape honest.
    let mut value = serde_json::json!({"api_key": null});
    truncate_long_fields(&mut value);
    assert!(value["api_key"].is_null());
}

#[test]
fn test_redact_fields_webhook_signing_secrets() {
    // create_webhook returns new_signing_secret; rotate returns secret.
    // Both are one-time values and must never reach inspector output.
    let mut value = serde_json::json!({
        "id": "wh1bare0pq",
        "new_signing_secret": "whsec_create-secret",
        "secret": "whsec_rotated-secret"
    });
    truncate_long_fields(&mut value);
    let rendered = value.to_string();
    assert!(!rendered.contains("whsec_"), "secret leaked: {rendered}");
    assert_eq!(value["new_signing_secret"], "[REDACTED]");
    assert_eq!(value["secret"], "[REDACTED]");
    assert_eq!(value["id"], "wh1bare0pq");
}

#[test]
fn test_redact_fields_credential_secrets() {
    // Create bodies for each credential type, and an update body.
    let mut value = serde_json::json!([
        {"type": "bearer_token", "token": "tok-1", "header_name": "Authorization"},
        {"type": "environment_variable", "value": "val-1", "injection_location": ["header"]},
        {"type": "oauth2", "client_id": "cid", "client_secret": "csec-1",
         "refresh_token": "rtok-1", "token_url": "https://oauth.example/token"},
        {"type": "environment_variable", "value": "val-2"},
    ]);
    truncate_long_fields(&mut value);
    let rendered = value.to_string();
    for secret in ["tok-1", "val-1", "csec-1", "rtok-1", "val-2"] {
        assert!(!rendered.contains(secret), "{secret} leaked: {rendered}");
    }
    assert_eq!(value[0]["header_name"], "Authorization");
    assert_eq!(value[2]["client_id"], "cid");
    assert_eq!(value[2]["token_url"], "https://oauth.example/token");
}

#[test]
fn test_redact_fields_env_var_values_in_both_env_forms() {
    // `env` is sent as a map and echoed as a list of single-key maps.
    let mut value = serde_json::json!({
        "environment": {
            "type": "remote",
            "env": {"PLAIN": {"value": "env-secret-1"}, "REF": {"credential": "cred-1"}}
        },
        "echo": {"env": [{"PLAIN": {"value": "env-secret-2"}}]},
    });
    truncate_long_fields(&mut value);
    let rendered = value.to_string();
    assert!(!rendered.contains("env-secret-"), "leaked: {rendered}");
    assert_eq!(value["environment"]["env"]["REF"]["credential"], "cred-1");
}

#[test]
fn test_redact_fields_ordinary_value_keys_still_print() {
    // `value` is redacted only in the two secret contexts.
    let mut value = serde_json::json!({
        "type": "function_result",
        "result": {"value": 42},
        "metadata": [{"key": "k", "value": "v"}],
    });
    truncate_long_fields(&mut value);
    assert_eq!(value["result"]["value"], 42);
    assert_eq!(value["metadata"][0]["value"], "v");
}

#[test]
fn test_loud_wire_printer_smoke_all_variants() {
    // No assertions on the output itself (it goes to stderr); this
    // guards against panics in formatting, including UTF-8 truncation.
    let printer = LoudWirePrinter::new();
    for event in sample_events() {
        printer.on_event(&event);
    }
}

#[test]
fn test_tracing_forwarder_smoke_all_variants() {
    let forwarder = TracingForwarder::new();
    for event in sample_events() {
        forwarder.on_event(&event);
    }
}

#[test]
fn test_wire_event_id_accessor() {
    for event in sample_events() {
        assert!(event.id() > 0);
    }
}

#[test]
fn test_wire_event_serializes_with_kind_tag() {
    let event = WireEvent::ResponseStatus { id: 4, status: 200 };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["kind"], "response_status");
    assert_eq!(json["id"], 4);
    assert_eq!(json["status"], 200);

    let event = WireEvent::SseFrame {
        id: 1,
        event_type: Some("interaction.completed".to_string()),
        data: "{}".to_string(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["kind"], "sse_frame");
    assert_eq!(json["event_type"], "interaction.completed");

    let event = WireEvent::HarnessStderr {
        id: 9,
        line: "harness log".to_string(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["kind"], "harness_stderr");
    assert_eq!(json["id"], 9);
    assert_eq!(json["line"], "harness log");

    let event = WireEvent::WsSend {
        id: 9,
        payload: serde_json::json!({"userInput": "hi"}),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["kind"], "ws_send");
    assert_eq!(json["payload"]["userInput"], "hi");
}
