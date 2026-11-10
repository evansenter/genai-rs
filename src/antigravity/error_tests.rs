use super::*;
use std::error::Error as _;

/// A cause whose text must reach a reader once: through `source()`.
const CAUSE: &str = "the-cause-text-7731";

fn cause() -> std::io::Error {
    std::io::Error::other(CAUSE)
}

/// Every variant that returns a source, built with one.
fn errors_with_a_source() -> Vec<AntigravityError> {
    vec![
        AntigravityError::WebSocket {
            message: "read failed".to_string(),
            source: Box::new(cause()),
        },
        AntigravityError::protocol_from("could not parse a message from the harness", cause()),
        AntigravityError::Io {
            message: "could not read media file /x.png".to_string(),
            source: cause(),
        },
        AntigravityError::init_failed_by(
            "connecting to the harness failed",
            String::new(),
            cause(),
        ),
    ]
}

#[test]
fn test_display_leaves_the_source_to_source() {
    // A reporter that walks the chain prints each cause once: the error's
    // Display, then its source's.
    for error in errors_with_a_source() {
        let source = error.source().expect("the cause is the source");
        assert_eq!(source.to_string(), CAUSE, "{error:?}");
        let display = error.to_string();
        assert!(
            !display.contains(CAUSE),
            "Display repeats the source's text: {display}"
        );
    }
}

#[test]
fn test_init_failed_from_a_connect_keeps_the_websocket_error_as_its_source() {
    let connect = AntigravityError::WebSocket {
        message: "failed to connect to harness WebSocket on port 1 after 5 attempts".to_string(),
        source: Box::new(cause()),
    };
    let error = AntigravityError::init_failed_by(
        "connecting to the harness failed",
        "tail".into(),
        connect,
    )
    .with_version_note(Some("0.0.1"));
    let AntigravityError::InitFailed {
        message, stderr, ..
    } = &error
    else {
        panic!("expected InitFailed: {error:?}");
    };
    assert!(message.contains("0.0.1"), "the note is kept: {message}");
    assert_eq!(stderr, "tail");
    let source = error.source().expect("the connect error is the source");
    let Some(AntigravityError::WebSocket { message, .. }) = source.downcast_ref() else {
        panic!("the source is the WebSocket error: {source:?}");
    };
    assert!(message.contains("port 1"), "{message}");
    assert_eq!(
        source.source().map(ToString::to_string).as_deref(),
        Some(CAUSE),
        "the transport error is one link further"
    );

    let bare = AntigravityError::init_failed("no initialization response", String::new());
    assert!(bare.source().is_none());
}

#[test]
fn test_a_backend_or_trajectory_failure_ends_the_session() {
    for (kind, ends) in [
        (TurnFailure::ModelBackend { status: 400 }, true),
        (TurnFailure::ModelBackend { status: 403 }, true),
        (TurnFailure::ModelBackend { status: 503 }, true),
        (TurnFailure::Trajectory, true),
        (TurnFailure::Denied, false),
        (TurnFailure::Cancelled, false),
        (TurnFailure::Hook, false),
    ] {
        assert_eq!(kind.ends_session(), ends, "{kind:?}");
    }
}

#[test]
fn test_a_turn_error_displays_its_kind_and_message() {
    let error = AntigravityError::turn(TurnFailure::ModelBackend { status: 401 }, "bad key");
    assert_eq!(
        error.to_string(),
        "agent turn failed (model backend error, HTTP 401): bad key"
    );
    let error = AntigravityError::turn(TurnFailure::Denied, "not today");
    assert_eq!(
        error.to_string(),
        "agent turn failed (denied by the pre-turn hook): not today"
    );
    assert!(error.source().is_none());
}
