//! Replays real API responses, captured into `tests/fixtures/captured/`,
//! through the client against the local stub (#492).
//!
//! The other offline tests replay hand-written bodies, which can drift from
//! what the API sends without any of them noticing. These are recorded
//! from the live API by `.github/scripts/capture_fixtures.py`, each with its
//! capture date, and re-captured by the weekly `captured-fixtures` job in
//! `rust.yml`, which runs this binary against the fresh copies. So the
//! checks are on shape, not wording: every event parses to a known chunk,
//! nothing comes back `Unknown`, the streamed text reassembles, an error
//! envelope maps to its status, and no captured key is dropped when the
//! parsed value is serialized again (an unmodeled one lands in `extra` and
//! comes back out).
//!
//! As of 2026-11-05 no stream sends `steps` in `interaction.completed` (a
//! stored and an unstored create, code execution, and a retrieval stream
//! were checked), so the "with steps" case stays hand-written
//! (`http_mock_tests.rs`); the accumulator rebuilds the steps here.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a failure by panicking"
)]

mod common;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use common::http_stub::{Reply, Stub};
use futures_util::StreamExt;
use genai_rs::{GenaiError, InteractionStatus, StreamChunk, StreamEvent};
use serde_json::Value;

const FIXTURES: &[&str] = &[
    "create_stream",
    "retrieve_stream",
    "create_interaction",
    "get_interaction",
    "get_environment",
    "list_voices",
    "error_unknown_parameter",
    "error_not_found",
];

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/captured")
}

fn fixture(name: &str) -> Value {
    let path = dir().join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
}

/// The captured response as a stub reply: JSON whole, an SSE body one event
/// per write, so the client's framing is exercised too.
fn reply(fixture: &Value) -> Reply {
    let response = &fixture["response"];
    let status = response["status"].as_u64().expect("status") as u16;
    if let Some(sse) = response["sse"].as_str() {
        let parts = sse
            .split_inclusive("\n\n")
            .map(|event| (Duration::ZERO, event.as_bytes().to_vec()))
            .collect();
        Reply {
            status,
            headers: vec![("content-type".into(), "text/event-stream".into())],
            delay: Duration::ZERO,
            parts,
            content_length: false,
        }
    } else {
        Reply::json(status, response["json"].clone())
    }
}

/// The path the fixture's request went to, as the stub records it.
fn request_path(fixture: &Value) -> &str {
    fixture["request"]["path"].as_str().expect("request path")
}

/// The id in the captured request path's last segment.
fn path_id(fixture: &Value) -> String {
    let path = request_path(fixture).split('?').next().unwrap();
    path.rsplit('/').next().unwrap().to_string()
}

/// Every object key path in `value` (`/usage/total_tokens`, array indices
/// included), for the "nothing dropped" check.
fn key_paths(value: &Value, at: &str, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let path = format!("{at}/{key}");
                out.insert(path.clone());
                key_paths(child, &path, out);
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                key_paths(child, &format!("{at}/{i}"), out);
            }
        }
        _ => {}
    }
}

/// Asserts that every key the API sent survives a parse and a serialize.
fn assert_nothing_dropped(what: &str, captured: &Value, reserialized: &Value) {
    let mut sent = BTreeSet::new();
    key_paths(captured, "", &mut sent);
    let mut kept = BTreeSet::new();
    key_paths(reserialized, "", &mut kept);
    let dropped: Vec<_> = sent.difference(&kept).collect();
    assert!(
        dropped.is_empty(),
        "{what}: keys the API sent were dropped by a parse and serialize: {dropped:?}"
    );
}

/// The text of every text delta in a captured SSE body, in order.
fn captured_text(sse: &str) -> String {
    sse.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .filter(|event| event["event_type"] == "step.delta")
        .filter_map(|event| event["delta"]["text"].as_str().map(str::to_string))
        .collect()
}

/// The events a stream yielded: all `Ok`, none unknown, ending at
/// `Completed` with the text reassembled.
fn assert_stream(what: &str, fixture: &Value, events: Vec<Result<StreamEvent, GenaiError>>) {
    let events: Vec<StreamEvent> = events
        .into_iter()
        .map(|e| e.unwrap_or_else(|e| panic!("{what}: stream error {e:?}")))
        .collect();
    let unknown: Vec<_> = events.iter().filter(|e| e.is_unknown()).collect();
    assert!(unknown.is_empty(), "{what}: unknown chunks {unknown:?}");
    let streamed: String = events
        .iter()
        .filter_map(|e| match &e.chunk {
            StreamChunk::StepDelta { delta, .. } => delta.as_text().map(str::to_string),
            _ => None,
        })
        .collect();
    let expected = captured_text(fixture["response"]["sse"].as_str().unwrap());
    assert!(
        !expected.is_empty(),
        "{what}: the capture has no text deltas"
    );
    assert_eq!(streamed, expected, "{what}: streamed text");
    let last = events.last().expect("no events");
    let StreamChunk::Completed(done) = &last.chunk else {
        panic!("{what}: the stream did not end at Completed: {last:?}");
    };
    assert_eq!(done.status, InteractionStatus::Completed, "{what}");
    assert!(!done.has_unknown(), "{what}: {done:?}");
    assert_eq!(
        done.as_text(),
        Some(expected.as_str()),
        "{what}: the completed interaction does not carry the streamed text"
    );
}

#[test]
fn every_captured_fixture_is_dated_and_complete() {
    let mut on_disk: Vec<String> = std::fs::read_dir(dir())
        .expect("tests/fixtures/captured")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    on_disk.sort();
    let mut expected: Vec<String> = FIXTURES
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    expected.sort();
    assert_eq!(
        on_disk, expected,
        "every capture is replayed below, and every replay has its capture"
    );
    for name in FIXTURES {
        let f = fixture(name);
        let date = f["captured"].as_str().unwrap_or_default();
        assert!(
            chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok(),
            "{name}: `captured` is not a YYYY-MM-DD date: {date:?}"
        );
        assert!(f["request"]["method"].is_string(), "{name}: request.method");
        assert!(f["response"]["status"].is_u64(), "{name}: response.status");
        assert!(
            f["response"]["json"].is_object() || f["response"]["sse"].is_string(),
            "{name}: response has neither json nor sse"
        );
    }
}

#[tokio::test]
async fn a_captured_create_stream_replays() {
    let f = fixture("create_stream");
    let stub = Stub::replying(vec![reply(&f)]).await;
    let events: Vec<_> = stub
        .client()
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_text("Count from 1 to 5, one number per line.")
        .with_store_disabled()
        .create_stream()
        .collect()
        .await;

    assert_eq!(stub.requests()[0].target, request_path(&f));
    assert_stream("create_stream", &f, events);
}

#[tokio::test]
async fn a_captured_retrieval_stream_replays() {
    let f = fixture("retrieve_stream");
    let stub = Stub::replying(vec![reply(&f)]).await;
    let id = path_id(&f);
    let events: Vec<_> = stub.client().interactions().stream(&id).collect().await;

    assert_eq!(stub.requests()[0].target, request_path(&f));
    assert_stream("retrieve_stream", &f, events);
}

#[tokio::test]
async fn a_captured_create_parses_and_keeps_every_key() {
    let f = fixture("create_interaction");
    let stub = Stub::replying(vec![reply(&f)]).await;
    let response = stub
        .client()
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_text("Name three primary colors, comma-separated.")
        .create()
        .await
        .expect("the captured create parses");

    assert_eq!(stub.requests()[0].target, request_path(&f));
    assert_eq!(response.status, InteractionStatus::Completed);
    assert!(response.as_text().is_some_and(|t| !t.is_empty()));
    assert!(!response.has_unknown(), "{response:?}");
    assert_nothing_dropped(
        "create_interaction",
        &f["response"]["json"],
        &serde_json::to_value(&response).unwrap(),
    );
}

#[tokio::test]
async fn a_captured_stored_interaction_parses_and_keeps_every_key() {
    let f = fixture("get_interaction");
    let stub = Stub::replying(vec![reply(&f)]).await;
    let response = stub
        .client()
        .interactions()
        .get(&path_id(&f))
        .await
        .expect("the captured read parses");

    assert_eq!(stub.requests()[0].target, request_path(&f));
    assert_eq!(response.status, InteractionStatus::Completed);
    assert!(response.as_text().is_some_and(|t| !t.is_empty()));
    assert!(!response.has_unknown(), "{response:?}");
    assert_nothing_dropped(
        "get_interaction",
        &f["response"]["json"],
        &serde_json::to_value(&response).unwrap(),
    );
}

#[tokio::test]
async fn a_captured_environment_parses_and_keeps_every_key() {
    let f = fixture("get_environment");
    let stub = Stub::replying(vec![reply(&f)]).await;
    let env = stub
        .client()
        .environments()
        .get(&path_id(&f))
        .await
        .expect("the captured environment parses");

    assert_eq!(stub.requests()[0].target, request_path(&f));
    assert_eq!(env.id.as_deref(), Some(path_id(&f).as_str()));
    assert_nothing_dropped(
        "get_environment",
        &f["response"]["json"],
        &serde_json::to_value(&env).unwrap(),
    );
}

#[tokio::test]
async fn a_captured_voice_listing_parses_and_keeps_every_key() {
    let f = fixture("list_voices");
    let stub = Stub::replying(vec![reply(&f)]).await;
    let page = stub
        .client()
        .voices()
        .list()
        .with_page_size(3)
        .send()
        .await
        .expect("the captured listing parses");

    assert_eq!(stub.requests()[0].target, request_path(&f));
    assert!(!page.voices.is_empty());
    assert!(page.voices.iter().all(|v| {
        !v.voice_type
            .as_ref()
            .is_some_and(genai_rs::VoiceType::is_unknown)
    }));
    assert_nothing_dropped(
        "list_voices",
        &f["response"]["json"],
        &serde_json::to_value(&page).unwrap(),
    );
}

/// The two error envelopes map to `GenaiError::Api` with their status and
/// the server's message.
#[tokio::test]
async fn captured_error_envelopes_map_to_api_errors() {
    let unknown = fixture("error_unknown_parameter");
    let stub = Stub::replying(vec![reply(&unknown)]).await;
    let err = stub
        .client()
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_text("hi")
        .create()
        .await
        .unwrap_err();
    let server_message = unknown["response"]["json"]["error"]["message"]
        .as_str()
        .unwrap();
    assert!(
        matches!(&err, GenaiError::Api { status_code: 400, message, .. }
            if message.contains(server_message)),
        "{err:?}"
    );
    assert!(!err.is_retryable());

    let missing = fixture("error_not_found");
    let stub = Stub::replying(vec![reply(&missing)]).await;
    let err = stub
        .client()
        .interactions()
        .get(&path_id(&missing))
        .await
        .unwrap_err();
    let server_message = missing["response"]["json"]["error"]["message"]
        .as_str()
        .unwrap();
    assert!(
        matches!(&err, GenaiError::Api { status_code: 404, message, .. }
            if message.contains(server_message)),
        "{err:?}"
    );
    assert_eq!(stub.requests()[0].target, request_path(&missing));
}
