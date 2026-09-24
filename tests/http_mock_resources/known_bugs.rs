//! Failing tests for known bugs, ignored until fixed.
//!
//! They live in a subdirectory so `ci_coverage.rs`, which requires every
//! ignore reason in a top-level test file to be the live-test one, does not
//! mistake them for unrun live tests. Run them with
//! `cargo nextest run --test http_mock_resources --run-ignored only`.

use super::common::http_stub::Stub;
use genai_rs::{EnvironmentFileUpload, GenaiError};

/// `http/file_search_stores.rs` validates the MIME header value up front for
/// exactly this reason; `http/environment_files.rs` passes it straight to
/// `RequestBuilder::header`, which defers the failure to `send()` as a
/// builder error, i.e. `GenaiError::Http`, which `is_retryable()` calls
/// transient. A retry loop would spin on input that can never succeed.
#[tokio::test]
#[ignore = "known bug: upload_environment_file reports an unheaderable MIME type as a retryable GenaiError::Http instead of InvalidInput"]
async fn upload_environment_file_rejects_an_unheaderable_mime_type_as_invalid_input() {
    let stub = Stub::replying(vec![]).await;

    let err = stub
        .client()
        .upload_environment_file(
            "env-1",
            "a.txt",
            b"x".to_vec(),
            "text/plain\nX-Injected: 1",
            EnvironmentFileUpload::default(),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, GenaiError::InvalidInput(_)), "{err:?}");
    assert!(!err.is_retryable(), "{err:?}");
    assert!(stub.requests().is_empty());
}
