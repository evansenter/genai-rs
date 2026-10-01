use super::*;

#[test]
fn test_file_metadata_deserialization() {
    let json = r#"{
        "name": "files/abc123",
        "displayName": "test.mp4",
        "mimeType": "video/mp4",
        "sizeBytes": "1234567",
        "createTime": "2024-01-01T00:00:00Z",
        "expirationTime": "2024-01-03T00:00:00Z",
        "uri": "https://generativelanguage.googleapis.com/v1beta/files/abc123",
        "state": "ACTIVE"
    }"#;

    let file: FileMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(file.name, "files/abc123");
    assert_eq!(file.display_name.as_deref(), Some("test.mp4"));
    assert_eq!(file.mime_type, "video/mp4");
    assert!(file.is_active());
    assert!(!file.is_processing());
}

#[test]
fn test_file_state_processing() {
    let json =
        r#"{"name": "files/test", "mimeType": "video/mp4", "state": "PROCESSING", "uri": ""}"#;
    let file: FileMetadata = serde_json::from_str(json).unwrap();
    assert!(file.is_processing());
    assert!(!file.is_active());
}

#[test]
fn test_file_state_failed() {
    let json = r#"{"name": "files/test", "mimeType": "video/mp4", "state": "FAILED", "uri": ""}"#;
    let file: FileMetadata = serde_json::from_str(json).unwrap();
    assert!(file.is_failed());
    assert!(!file.is_active());
}

#[test]
fn test_list_files_response_deserialization() {
    let json = r#"{
        "files": [
            {"name": "files/a", "mimeType": "video/mp4", "uri": ""},
            {"name": "files/b", "mimeType": "image/png", "uri": ""}
        ],
        "nextPageToken": "token123"
    }"#;

    let response: ListFilesResponse = serde_json::from_str(json).unwrap();
    assert_eq!(response.files.len(), 2);
    assert_eq!(response.next_page_token.as_deref(), Some("token123"));
}

#[test]
fn test_list_files_response_drops_only_the_undeserializable_entry() {
    // `mimeType` is required; the second entry lacks it.
    let json = r#"{
        "files": [
            {"name": "files/a", "mimeType": "text/plain", "uri": "u"},
            {"name": "files/b", "uri": "u"}
        ],
        "nextPageToken": "p2"
    }"#;
    let list: ListFilesResponse = serde_json::from_str(json).unwrap();
    assert_eq!(list.files.len(), 1);
    assert_eq!(list.files[0].name, "files/a");
    assert_eq!(list.next_page_token.as_deref(), Some("p2"));
}

#[test]
fn test_list_files_response_explicit_null_list_is_empty() {
    let list: ListFilesResponse = serde_json::from_str(r#"{"files": null}"#).unwrap();
    assert!(list.files.is_empty());
}

#[test]
fn test_empty_list_files_response() {
    let json = r#"{}"#;
    let response: ListFilesResponse = serde_json::from_str(json).unwrap();
    assert!(response.files.is_empty());
    assert!(response.next_page_token.is_none());
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_file_state_unknown_preserves_data() {
    // Test that unknown states preserve the original value
    let json =
        r#"{"name": "files/test", "mimeType": "video/mp4", "state": "UPLOADING", "uri": ""}"#;
    let file: FileMetadata = serde_json::from_str(json).unwrap();

    assert!(!file.is_active());
    assert!(!file.is_processing());
    assert!(!file.is_failed());

    // Check the Unknown variant captured the state
    if let Some(FileState::Unknown { state_type, data }) = &file.state {
        assert_eq!(state_type, "UPLOADING");
        assert_eq!(data.as_str(), Some("UPLOADING"));
    } else {
        panic!("Expected FileState::Unknown variant, got {:?}", file.state);
    }
}

#[test]
fn test_file_state_unknown_helper_methods() {
    let unknown = FileState::Unknown {
        state_type: "NEW_STATE".to_string(),
        data: serde_json::json!("NEW_STATE"),
    };

    assert!(unknown.is_unknown());
    assert_eq!(unknown.unknown_state_type(), Some("NEW_STATE"));
    assert_eq!(
        unknown.unknown_data(),
        Some(&serde_json::json!("NEW_STATE"))
    );

    // Known states should return None for unknown helpers
    let active = FileState::Active;
    assert!(!active.is_unknown());
    assert_eq!(active.unknown_state_type(), None);
    assert_eq!(active.unknown_data(), None);
}

#[test]
fn test_file_state_roundtrip_serialization() {
    // Known state roundtrips
    let active = FileState::Active;
    let json = serde_json::to_string(&active).unwrap();
    assert_eq!(json, r#""ACTIVE""#);
    let deserialized: FileState = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, FileState::Active);

    // Unknown state roundtrips
    let unknown = FileState::Unknown {
        state_type: "QUEUED".to_string(),
        data: serde_json::json!("QUEUED"),
    };
    let json = serde_json::to_string(&unknown).unwrap();
    assert_eq!(json, r#""QUEUED""#);
}

#[test]
fn test_file_metadata_failed_state_with_error() {
    let json = r#"{
        "name": "files/failed123",
        "mimeType": "video/mp4",
        "state": "FAILED",
        "uri": "",
        "error": {
            "code": 400,
            "message": "Unsupported video codec"
        }
    }"#;
    let file: FileMetadata = serde_json::from_str(json).unwrap();
    assert!(file.is_failed());
    assert!(file.error.is_some());

    let error = file.error.unwrap();
    assert_eq!(error.code, Some(400));
    assert_eq!(error.message.as_deref(), Some("Unsupported video codec"));
}

#[test]
fn test_file_error_partial_fields() {
    // Error with only code
    let json = r#"{"code": 500}"#;
    let error: FileError = serde_json::from_str(json).unwrap();
    assert_eq!(error.code, Some(500));
    assert_eq!(error.message, None);

    // Error with only message
    let json = r#"{"message": "Something went wrong"}"#;
    let error: FileError = serde_json::from_str(json).unwrap();
    assert_eq!(error.code, None);
    assert_eq!(error.message.as_deref(), Some("Something went wrong"));

    // Empty error (edge case)
    let json = r#"{}"#;
    let error: FileError = serde_json::from_str(json).unwrap();
    assert_eq!(error.code, None);
    assert_eq!(error.message, None);
}

#[test]
fn test_file_error_display() {
    // Both code and message
    let error = FileError {
        code: Some(400),
        message: Some("Invalid file format".to_string()),
    };
    assert_eq!(error.to_string(), "error 400: Invalid file format");

    // Only code
    let error = FileError {
        code: Some(500),
        message: None,
    };
    assert_eq!(error.to_string(), "error 500");

    // Only message
    let error = FileError {
        code: None,
        message: Some("Something went wrong".to_string()),
    };
    assert_eq!(error.to_string(), "Something went wrong");

    // Neither code nor message
    let error = FileError {
        code: None,
        message: None,
    };
    assert_eq!(error.to_string(), "unknown error");
}

#[test]
fn test_size_bytes_as_u64() {
    // Valid size_bytes parses correctly
    let file = FileMetadata {
        name: "files/test".to_string(),
        display_name: None,
        mime_type: "video/mp4".to_string(),
        size_bytes: Some("1234567890".to_string()),
        create_time: None,
        expiration_time: None,
        sha256_hash: None,
        uri: "".to_string(),
        state: None,
        error: None,
        video_metadata: None,
    };
    assert_eq!(file.size_bytes_as_u64(), Some(1234567890));

    // None size_bytes returns None
    let file = FileMetadata {
        name: "files/test".to_string(),
        display_name: None,
        mime_type: "video/mp4".to_string(),
        size_bytes: None,
        create_time: None,
        expiration_time: None,
        sha256_hash: None,
        uri: "".to_string(),
        state: None,
        error: None,
        video_metadata: None,
    };
    assert_eq!(file.size_bytes_as_u64(), None);

    // Invalid size_bytes (non-numeric) returns None
    let file = FileMetadata {
        name: "files/test".to_string(),
        display_name: None,
        mime_type: "video/mp4".to_string(),
        size_bytes: Some("not a number".to_string()),
        create_time: None,
        expiration_time: None,
        sha256_hash: None,
        uri: "".to_string(),
        state: None,
        error: None,
        video_metadata: None,
    };
    assert_eq!(file.size_bytes_as_u64(), None);

    // Large file size (2GB+) parses correctly
    let file = FileMetadata {
        name: "files/test".to_string(),
        display_name: None,
        mime_type: "video/mp4".to_string(),
        size_bytes: Some("2147483648".to_string()), // 2GB
        create_time: None,
        expiration_time: None,
        sha256_hash: None,
        uri: "".to_string(),
        state: None,
        error: None,
        video_metadata: None,
    };
    assert_eq!(file.size_bytes_as_u64(), Some(2147483648));
}

#[tokio::test]
async fn test_upload_file_unknown_extension_error() {
    let client = Client::new("test_key".to_string());

    // Create a temp file with an unknown extension
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("data.xyz");
    std::fs::write(&file_path, b"test data").unwrap();

    // A path upload without a MIME type fails with InvalidInput for an
    // unknown extension, naming the setter that fixes it.
    let err = client
        .files()
        .upload(FileUpload::from_path(&file_path))
        .await
        .expect_err("Should fail for unknown extension");
    assert!(matches!(err, GenaiError::InvalidInput(_)), "{err:?}");

    let err_string = err.to_string();
    assert!(
        err_string.contains("Could not determine MIME type"),
        "Error should mention MIME type issue: {}",
        err_string
    );
    assert!(
        err_string.contains("data.xyz"),
        "Error should include filename: {}",
        err_string
    );
    assert!(
        err_string.contains("FileUpload::with_mime_type()"),
        "Error should name the setter: {}",
        err_string
    );
}

#[tokio::test]
async fn test_upload_file_nonexistent_file_error() {
    let client = Client::new("test_key".to_string());

    // Try to upload a file that doesn't exist
    let result = client
        .files()
        .upload(FileUpload::from_path("/nonexistent/path/to/file.txt"))
        .await;
    assert!(result.is_err(), "Should fail for nonexistent file");

    let err = result.unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("Failed to read file"),
        "Error should mention file read failure: {}",
        err_string
    );
}

#[tokio::test]
async fn test_upload_file_bytes_empty_file_error() {
    let client = Client::new("test_key".to_string());

    // Try to upload empty bytes
    let result = client
        .files()
        .upload(FileUpload::from_bytes(Vec::new(), "text/plain").with_display_name("empty.txt"))
        .await;
    assert!(result.is_err(), "Should fail for empty file");

    let err = result.unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("Cannot upload empty file"),
        "Error should mention empty file: {}",
        err_string
    );
}

#[tokio::test]
async fn test_upload_file_bytes_validates_before_network() {
    // This test verifies that validation happens before any network call
    // by using an invalid API key - if we reach the network, we'd get auth error
    let client = Client::new("invalid_key".to_string());

    // Empty file should fail with validation error, not auth error
    let result = client
        .files()
        .upload(FileUpload::from_bytes(Vec::new(), "text/plain"))
        .await;
    assert!(result.is_err());
    let err_string = result.unwrap_err().to_string();
    assert!(
        err_string.contains("Cannot upload empty file"),
        "Should fail validation before hitting network: {}",
        err_string
    );
}

// =============================================================================
// FileUpload, PollOptions, and the handle
// =============================================================================

#[test]
fn file_upload_from_path_leaves_mime_type_and_display_name_to_the_upload() {
    let upload = FileUpload::from_path("clips/video.mp4");
    assert!(matches!(&upload.source, UploadSource::Path(p) if p == Path::new("clips/video.mp4")));
    assert_eq!(upload.mime_type, None);
    assert_eq!(upload.display_name, None);
    // The MIME type comes from the extension when unset.
    assert_eq!(upload.resolved_mime_type().unwrap(), "video/mp4");

    // `impl Into<PathBuf>` takes a `&Path` and a `PathBuf` too.
    let path = std::path::PathBuf::from("notes.md");
    for upload in [
        FileUpload::from_path(path.as_path()),
        FileUpload::from_path(path.clone()),
    ] {
        assert!(matches!(&upload.source, UploadSource::Path(p) if *p == path));
        assert_eq!(upload.resolved_mime_type().unwrap(), "text/markdown");
    }
}

#[test]
fn file_upload_from_bytes_carries_the_data_and_mime_type() {
    let upload = FileUpload::from_bytes(b"a,b\n".to_vec(), "text/csv");
    assert!(matches!(&upload.source, UploadSource::Bytes(d) if d == b"a,b\n"));
    assert_eq!(upload.mime_type.as_deref(), Some("text/csv"));
    assert_eq!(upload.display_name, None);
    assert_eq!(upload.resolved_mime_type().unwrap(), "text/csv");
    // `impl Into<String>` takes an owned string too.
    let owned = FileUpload::from_bytes(Vec::new(), String::from("application/pdf"));
    assert_eq!(owned.mime_type.as_deref(), Some("application/pdf"));
}

#[test]
fn file_upload_setters_replace() {
    let upload = FileUpload::from_path("data.xyz")
        .with_mime_type("application/octet-stream")
        .with_display_name("first")
        .with_display_name(String::from("second"));
    assert_eq!(upload.display_name.as_deref(), Some("second"));
    // An explicit MIME type wins over the extension, even one that maps to
    // nothing.
    assert_eq!(
        upload.resolved_mime_type().unwrap(),
        "application/octet-stream"
    );

    let upload = FileUpload::from_bytes(b"x".to_vec(), "text/plain").with_mime_type("text/csv");
    assert_eq!(upload.resolved_mime_type().unwrap(), "text/csv");
}

#[test]
fn file_upload_without_a_mappable_extension_names_the_setter() {
    let err = FileUpload::from_path("data.xyz")
        .resolved_mime_type()
        .unwrap_err();
    assert!(matches!(err, GenaiError::InvalidInput(_)), "{err:?}");
    let message = err.to_string();
    assert!(message.contains("'data.xyz'"), "{message}");
    assert!(
        message.contains("FileUpload::with_mime_type()"),
        "{message}"
    );
}

#[test]
fn file_upload_debug_shows_the_length_not_the_bytes() {
    let upload =
        FileUpload::from_bytes(b"top-secret".to_vec(), "text/plain").with_display_name("notes");
    let debug = format!("{upload:?}");
    assert!(!debug.contains("top-secret"), "{debug}");
    assert!(!debug.contains("116, 111"), "no byte values: {debug}");
    assert!(debug.contains("data_len: 10"), "{debug}");
    assert!(debug.contains("text/plain"), "{debug}");
    assert!(debug.contains("notes"), "{debug}");

    let debug = format!("{:?}", FileUpload::from_path("clips/video.mp4"));
    assert!(debug.contains("path: \"clips/video.mp4\""), "{debug}");
    assert!(debug.contains("mime_type: None"), "{debug}");
}

#[test]
fn poll_options_fall_back_to_the_given_defaults() {
    let (timeout, interval) = (Duration::from_secs(120), Duration::from_secs(2));
    let unset = PollOptions::new();
    assert_eq!(unset, PollOptions::default());
    assert_eq!(unset.timeout_or(timeout), timeout);
    assert_eq!(unset.poll_interval_or(interval), interval);

    let set = PollOptions::new()
        .with_timeout(Duration::from_secs(5))
        .with_poll_interval(Duration::from_millis(250))
        // `with_*` replaces.
        .with_timeout(Duration::from_secs(9));
    assert_eq!(set.timeout_or(timeout), Duration::from_secs(9));
    assert_eq!(set.poll_interval_or(interval), Duration::from_millis(250));

    // Each value falls back on its own.
    let only_interval = PollOptions::new().with_poll_interval(Duration::from_secs(1));
    assert_eq!(only_interval.timeout_or(timeout), timeout);
}

#[test]
fn file_wait_defaults_are_120_s_and_2_s() {
    assert_eq!(FILE_WAIT_TIMEOUT, Duration::from_secs(120));
    assert_eq!(FILE_POLL_INTERVAL, Duration::from_secs(2));
}

#[test]
fn list_files_setters_replace() {
    let client = Client::new("k".to_string());
    let list = client.files().list();
    assert_eq!(list.page_size, None);
    assert_eq!(list.page_token, None);

    let list = list
        .with_page_size(1)
        .with_page_token("t1")
        .with_page_size(100)
        .with_page_token(String::from("t2"));
    assert_eq!(list.page_size, Some(100));
    assert_eq!(list.page_token.as_deref(), Some("t2"));
}

#[test]
fn files_handle_and_list_debug_redact_the_api_key() {
    let client = Client::new("secret-api-key".to_string());
    for debug in [
        format!("{:?}", client.files()),
        format!("{:?}", client.files().list().with_page_size(5)),
    ] {
        assert!(!debug.contains("secret-api-key"), "{debug}");
        assert!(debug.contains("[REDACTED]"), "{debug}");
    }
}

/// Property-based tests for serialization roundtrips using proptest.
mod proptest_tests {
    use super::*;
    use chrono::TimeZone;
    use proptest::prelude::*;

    /// Strategy for generating DateTime<Utc> values.
    /// Uses second precision to ensure reliable roundtrip.
    fn arb_datetime() -> impl Strategy<Value = DateTime<Utc>> {
        // Generate timestamps between 2020-01-01 and 2030-01-01
        (0i64..315_360_000).prop_map(|offset_secs| {
            Utc.timestamp_opt(1_577_836_800 + offset_secs, 0)
                .single()
                .expect("valid timestamp")
        })
    }

    /// Strategy for generating FileState variants.
    #[cfg(not(feature = "strict-unknown"))]
    fn arb_file_state() -> impl Strategy<Value = FileState> {
        prop_oneof![
            Just(FileState::Processing),
            Just(FileState::Active),
            Just(FileState::Failed),
            // Include Unknown variant for graceful handling
            ("[A-Z_]{4,20}".prop_map(|state_type| FileState::Unknown {
                state_type,
                data: serde_json::Value::Null,
            })),
        ]
    }

    /// Strategy for FileState - no Unknown in strict mode.
    #[cfg(feature = "strict-unknown")]
    fn arb_file_state() -> impl Strategy<Value = FileState> {
        prop_oneof![
            Just(FileState::Processing),
            Just(FileState::Active),
            Just(FileState::Failed),
        ]
    }

    /// Strategy for generating FileError.
    fn arb_file_error() -> impl Strategy<Value = FileError> {
        (
            prop::option::of(any::<i32>()),
            prop::option::of(".{0,100}".prop_map(String::from)),
        )
            .prop_map(|(code, message)| FileError { code, message })
    }

    /// Strategy for generating VideoMetadata.
    fn arb_video_metadata() -> impl Strategy<Value = VideoMetadata> {
        prop::option::of("[0-9]+s".prop_map(String::from))
            .prop_map(|video_duration| VideoMetadata { video_duration })
    }

    /// Strategy for generating FileMetadata.
    fn arb_file_metadata() -> impl Strategy<Value = FileMetadata> {
        (
            "files/[a-zA-Z0-9_]+",              // name
            prop::option::of(".{1,50}"),        // display_name
            "[a-z]+/[a-z0-9+-]+",               // mime_type
            prop::option::of("[0-9]+"),         // size_bytes
            prop::option::of(arb_datetime()),   // create_time
            prop::option::of(arb_datetime()),   // expiration_time
            prop::option::of("[a-f0-9]{64}"),   // sha256_hash (API returns raw hash, no prefix)
            "https?://[a-z]+\\.[a-z]+/[a-z]+",  // uri
            prop::option::of(arb_file_state()), // state is Option<FileState>
            prop::option::of(arb_file_error()),
            prop::option::of(arb_video_metadata()),
        )
            .prop_map(
                |(
                    name,
                    display_name,
                    mime_type,
                    size_bytes,
                    create_time,
                    expiration_time,
                    sha256_hash,
                    uri,
                    state,
                    error,
                    video_metadata,
                )| {
                    FileMetadata {
                        name,
                        display_name,
                        mime_type,
                        size_bytes,
                        create_time,
                        expiration_time,
                        sha256_hash,
                        uri,
                        state,
                        error,
                        video_metadata,
                    }
                },
            )
    }

    proptest! {
        /// Verify FileState roundtrips through JSON serialization.
        #[test]
        fn file_state_roundtrip(state in arb_file_state()) {
            let json = serde_json::to_string(&state).expect("serialize");
            let parsed: FileState = serde_json::from_str(&json).expect("deserialize");
            // For Unknown variants, we can't do exact equality since data may be different
            // Just verify it roundtrips to a valid state
            match (&state, &parsed) {
                (FileState::Processing, FileState::Processing) => {}
                (FileState::Active, FileState::Active) => {}
                (FileState::Failed, FileState::Failed) => {}
                (FileState::Unknown { .. }, FileState::Unknown { .. }) => {}
                _ => panic!("State changed during roundtrip: {:?} -> {:?}", state, parsed),
            }
        }

        /// Verify FileError roundtrips through JSON serialization.
        #[test]
        fn file_error_roundtrip(error in arb_file_error()) {
            let json = serde_json::to_string(&error).expect("serialize");
            let parsed: FileError = serde_json::from_str(&json).expect("deserialize");
            prop_assert_eq!(error.code, parsed.code);
            prop_assert_eq!(error.message, parsed.message);
        }

        /// Verify VideoMetadata roundtrips through JSON serialization.
        #[test]
        fn video_metadata_roundtrip(metadata in arb_video_metadata()) {
            let json = serde_json::to_string(&metadata).expect("serialize");
            let parsed: VideoMetadata = serde_json::from_str(&json).expect("deserialize");
            prop_assert_eq!(metadata.video_duration, parsed.video_duration);
        }

        /// Verify FileMetadata roundtrips through JSON serialization.
        #[test]
        fn file_metadata_roundtrip(metadata in arb_file_metadata()) {
            let json = serde_json::to_string(&metadata).expect("serialize");
            let parsed: FileMetadata = serde_json::from_str(&json).expect("deserialize");

            prop_assert_eq!(&metadata.name, &parsed.name);
            prop_assert_eq!(&metadata.display_name, &parsed.display_name);
            prop_assert_eq!(&metadata.mime_type, &parsed.mime_type);
            prop_assert_eq!(&metadata.size_bytes, &parsed.size_bytes);
            prop_assert_eq!(&metadata.uri, &parsed.uri);
            // Note: state comparison is relaxed for Unknown variants
        }
    }
}
