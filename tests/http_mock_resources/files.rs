//! The Files API: upload, metadata, list, delete and the wait.

use super::*;

/// A path upload streams the file as the finalize body, byte for byte.
#[tokio::test]
async fn files_upload_from_path_streams_the_file_from_disk_then_finalizes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.txt");
    // Under one 8 MB read buffer, but it spans many TCP writes, and the
    // pattern catches reordering or truncation.
    let data: Vec<u8> = (0..200_003u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&path, &data).unwrap();
    let size = data.len().to_string();

    let stub = Stub::replying(vec![
        Reply::json(200, json!({})).header("x-goog-upload-url", "{base}/upload-session/f1"),
        Reply::json(
            200,
            json!({"file": {"name": "files/f1", "mimeType": "text/plain", "uri": "u", "sizeBytes": size}}),
        ),
    ])
    .await;

    let file = stub
        .client()
        .files()
        .upload(FileUpload::from_path(&path))
        .await
        .unwrap();
    assert_eq!(file.name, "files/f1");
    assert_eq!(file.size_bytes, Some(data.len() as i64));

    let [start, finish] = stub.requests().try_into().unwrap();
    assert_eq!(start.method, "POST");
    assert_eq!(start.target, "/upload/v1beta/files");
    assert_eq!(start.header("x-goog-api-key"), Some("test-key"));
    assert_eq!(start.header("api-revision"), Some(API_REVISION));
    assert_eq!(start.header("x-goog-upload-protocol"), Some("resumable"));
    assert_eq!(start.header("x-goog-upload-command"), Some("start"));
    assert_eq!(
        start.header("x-goog-upload-header-content-length"),
        Some(size.as_str())
    );
    assert_eq!(
        start.header("x-goog-upload-header-content-type"),
        Some("text/plain")
    );
    assert_eq!(start.json(), json!({"file": {"displayName": "notes.txt"}}));

    assert_eq!(finish.method, "POST");
    assert_eq!(finish.target, "/upload-session/f1");
    assert_eq!(
        finish.header("x-goog-upload-command"),
        Some("upload, finalize")
    );
    assert_eq!(finish.header("x-goog-upload-offset"), Some("0"));
    assert_eq!(finish.header("content-length"), Some(size.as_str()));
    assert!(finish.body == data, "the finalize body is the file");
}

#[tokio::test]
async fn files_endpoints_send_the_documented_requests() {
    let stub = ok_stub().await;
    let client = stub.client();
    let c = &client;
    assert_wire(
        &stub,
        vec![
            wire("GET", "/v1beta/files/abc", None, c.files().get("files/abc")),
            wire("GET", "/v1beta/files", None, c.files().list().send()),
            // The Files API spells its paging parameters in camelCase.
            wire(
                "GET",
                "/v1beta/files?pageSize=10&pageToken=a%2Bb",
                None,
                c.files()
                    .list()
                    .with_page_size(10)
                    .with_page_token("a+b")
                    .send(),
            ),
            wire(
                "DELETE",
                "/v1beta/files/abc",
                None,
                c.files().delete("files/abc"),
            ),
        ],
    )
    .await;
}

/// `with_mime_type` and `with_display_name` replace what a path upload
/// would infer from the file name.
#[tokio::test]
async fn files_upload_from_path_sends_the_mime_type_and_display_name_set() {
    let (_dir, path) = temp_file("data.xyz", b"hello");
    let stub = Stub::replying(vec![
        Reply::json(200, json!({})).header("x-goog-upload-url", "{base}/upload-session/f1"),
        Reply::json(
            200,
            json!({"file": {"name": "files/f1", "mimeType": "application/octet-stream", "uri": "u"}}),
        ),
    ])
    .await;

    stub.client()
        .files()
        .upload(
            FileUpload::from_path(&path)
                .with_mime_type("application/octet-stream")
                .with_display_name("Q4 data"),
        )
        .await
        .unwrap();

    let [start, finish] = stub.requests().try_into().unwrap();
    assert_eq!(
        start.header("x-goog-upload-header-content-type"),
        Some("application/octet-stream")
    );
    assert_eq!(start.json(), json!({"file": {"displayName": "Q4 data"}}));
    assert_eq!(finish.body, b"hello");
}

/// A bytes upload has no display name unless one is set.
#[tokio::test]
async fn files_upload_from_bytes_without_a_display_name_sends_empty_metadata() {
    let stub = Stub::replying(vec![
        Reply::json(200, json!({})).header("x-goog-upload-url", "{base}/upload-session/f1"),
        Reply::json(
            200,
            json!({"file": {"name": "files/f1", "mimeType": "text/csv", "uri": "u"}}),
        ),
    ])
    .await;

    let file = stub
        .client()
        .files()
        .upload(FileUpload::from_bytes(b"a,b\n".to_vec(), "text/csv"))
        .await
        .unwrap();

    assert_eq!(file.name, "files/f1");
    let [start, finish] = stub.requests().try_into().unwrap();
    assert_eq!(
        start.header("x-goog-upload-header-content-type"),
        Some("text/csv")
    );
    assert_eq!(start.json(), json!({"file": {}}));
    assert_eq!(finish.body, b"a,b\n");
}

fn files_page(names: &[&str], next: Option<&str>) -> Reply {
    let files: Vec<Value> = names
        .iter()
        .map(|name| json!({"name": name, "mimeType": "text/plain", "uri": "u"}))
        .collect();
    let mut body = json!({"files": files});
    if let Some(next) = next {
        body["nextPageToken"] = json!(next);
    }
    Reply::json(200, body)
}

pub(super) fn file_names(files: &[FileMetadata]) -> Vec<&str> {
    files.iter().map(|f| f.name.as_str()).collect()
}

/// Files paging is camelCase both ways: `pageSize` / `pageToken` go out and
/// `nextPageToken` comes back. The first token has the live shape
/// (unpadded URL-safe base64, 2026-09-27).
#[tokio::test]
async fn file_list_items_resend_the_camel_case_page_size_on_every_page() {
    let stub = Stub::replying(vec![
        files_page(
            &["files/b", "files/a"],
            Some("ciAKDoIBCwjX2uHVBhDQ_75OCg5CDGJ2eXU4bHQ1dzZpcw"),
        ),
        // An empty page with a token is followed.
        files_page(&[], Some("p+3")),
        files_page(&["files/c"], None),
    ])
    .await;

    let files: Vec<FileMetadata> = stub
        .client()
        .files()
        .list()
        .with_page_size(2)
        .items()
        .try_collect()
        .await
        .unwrap();

    assert_eq!(file_names(&files), ["files/b", "files/a", "files/c"]);
    assert_eq!(
        targets(&stub),
        [
            "/v1beta/files?pageSize=2",
            "/v1beta/files?pageSize=2&pageToken=ciAKDoIBCwjX2uHVBhDQ_75OCg5CDGJ2eXU4bHQ1dzZpcw",
            "/v1beta/files?pageSize=2&pageToken=p%2B3",
        ]
    );
}

#[tokio::test]
async fn file_list_pages_resume_from_with_page_token() {
    let stub = Stub::replying(vec![
        files_page(&["files/c"], Some("p3")),
        files_page(&["files/d"], None),
    ])
    .await;

    let pages: Vec<_> = stub
        .client()
        .files()
        .list()
        .with_page_token("p2")
        .pages()
        .try_collect()
        .await
        .unwrap();

    assert_eq!(pages.len(), 2);
    assert_eq!(file_names(&pages[0].files), ["files/c"]);
    assert_eq!(
        targets(&stub),
        ["/v1beta/files?pageToken=p2", "/v1beta/files?pageToken=p3"]
    );
}

#[cfg(not(genai_strict_unknown))]
#[tokio::test]
async fn file_list_parses_metadata_and_preserves_unknown_states() {
    let stub = Stub::replying(vec![Reply::json(
        200,
        json!({
            "files": [
                {"name": "files/abc", "displayName": "clip.mp4", "mimeType": "video/mp4", "sizeBytes": "1048576",
                 "createTime": "2026-09-24T01:00:00Z", "expirationTime": "2026-09-26T01:00:00Z",
                 "sha256Hash": "ZmFrZQ==", "uri": "https://generativelanguage.googleapis.com/v1beta/files/abc",
                 "state": "ACTIVE", "videoMetadata": {"videoDuration": "12s"}},
                {"name": "files/def", "mimeType": "text/plain", "state": "ARCHIVED"}
            ],
            "nextPageToken": "p2"
        }),
    )])
    .await;

    let list = stub.client().files().list().send().await.unwrap();

    let clip = &list.files[0];
    assert!(clip.is_active());
    assert_eq!(clip.display_name.as_deref(), Some("clip.mp4"));
    assert_eq!(clip.size_bytes, Some(1_048_576));
    assert!(clip.create_time.is_some() && clip.expiration_time.is_some());
    assert_eq!(
        clip.video_metadata
            .as_ref()
            .unwrap()
            .video_duration
            .as_deref(),
        Some("12s")
    );
    let archived = list.files[1].state.as_ref().unwrap();
    assert_eq!(archived.unknown_state_type(), Some("ARCHIVED"));
    assert!(list.files[1].uri.is_empty(), "uri defaults when absent");
    assert_eq!(list.next_page_token.as_deref(), Some("p2"));
}

async fn wait_for_file(stub: &Stub, timeout: Duration) -> Result<FileMetadata, GenaiError> {
    let poll = PollOptions::new()
        .with_poll_interval(Duration::from_millis(10))
        .with_timeout(timeout);
    stub.client()
        .files()
        .wait_until_active("files/abc", poll)
        .await
}

/// On the paused clock, with each request holding it while in flight: polls
/// at 0, 10, ..., 50 ms, and the one at 60 would start at the deadline.
#[tokio::test(start_paused = true)]
async fn wait_until_active_times_out_with_the_last_state() {
    let stub = Stub::start(|_, _| Reply::json(200, file("PROCESSING"))).await;
    let poll = PollOptions::new()
        .with_poll_interval(Duration::from_millis(10))
        .with_timeout(Duration::from_millis(60));

    let err = stub
        .paused_clock_client()
        .files()
        .wait_until_active("files/abc", poll)
        .await
        .unwrap_err();

    assert!(matches!(err, GenaiError::PollTimeout { .. }), "{err:?}");
    let message = err.to_string();
    assert!(message.contains("Timeout"), "{message}");
    assert!(message.contains("waited 50ms"), "{message}");
    assert!(message.contains("Processing"), "{message}");
    assert_eq!(stub.requests().len(), 6, "{message}");
}

/// A retryable error mid-poll (here a 503) is polled through: one bad
/// status read must not end a long processing wait.
#[tokio::test]
async fn wait_until_active_polls_through_a_retryable_error() {
    let stub = Stub::replying(vec![
        Reply::json(200, file("PROCESSING")),
        Reply::json(
            503,
            json!({"error": {"code": 503, "message": "Backend unavailable", "status": "UNAVAILABLE"}}),
        ),
        Reply::json(200, file("ACTIVE")),
    ])
    .await;

    let ready = wait_for_file(&stub, Duration::from_secs(5)).await.unwrap();

    assert!(ready.is_active());
    assert_eq!(stub.requests().len(), 3);
}

/// A status request that never answers is cut off at the deadline, not
/// left to hang (the default client has no HTTP timeout).
///
/// On the paused clock (#501), held until the stub has the request: from
/// then on the only timers are the wait's deadline and the stub's 60 s
/// delay, so the request is cut off at exactly the 200 ms deadline.
#[tokio::test(start_paused = true)]
async fn wait_until_active_bounds_a_stalled_request() {
    let (gate, release) = hold_clock();
    let stub = Stub::start(move |_, _| {
        release();
        Reply::json(200, file("PROCESSING")).delayed(Duration::from_secs(60))
    })
    .await;

    let started = tokio::time::Instant::now();
    let err = wait_for_file(&stub, Duration::from_millis(200))
        .await
        .unwrap_err();
    gate.await.unwrap();

    assert!(matches!(err, GenaiError::PollTimeout { .. }), "{err:?}");
    let message = err.to_string();
    assert!(message.contains("Timeout"), "{message}");
    assert!(
        message.contains("cut off by the wait's deadline"),
        "{message}"
    );
    assert_eq!(started.elapsed(), Duration::from_millis(200), "{message}");
    assert_eq!(stub.requests().len(), 1, "{message}");
}

#[cfg(not(genai_strict_unknown))]
#[tokio::test]
async fn wait_until_active_keeps_polling_through_unknown_states() {
    let stub = Stub::replying(vec![
        Reply::json(200, file("TRANSCODING")),
        Reply::json(200, file("ACTIVE")),
    ])
    .await;

    let ready = wait_for_file(&stub, Duration::from_secs(5)).await.unwrap();

    assert!(ready.is_active());
    assert_eq!(stub.requests().len(), 2);
}

#[tokio::test]
async fn wait_until_active_propagates_api_errors() {
    let stub = Stub::replying(vec![Reply::json(200, file("PROCESSING")), not_found()]).await;

    let err = wait_for_file(&stub, Duration::from_secs(5))
        .await
        .unwrap_err();

    assert!(
        matches!(
            err,
            GenaiError::Api {
                status_code: 404,
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(stub.requests().len(), 2);
}
