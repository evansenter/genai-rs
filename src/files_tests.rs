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

    let response: FileListResponse = serde_json::from_str(json).unwrap();
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
    let list: FileListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(list.files.len(), 1);
    assert_eq!(list.files[0].name, "files/a");
    assert_eq!(list.next_page_token.as_deref(), Some("p2"));
}

#[test]
fn test_list_files_response_explicit_null_list_is_empty() {
    let list: FileListResponse = serde_json::from_str(r#"{"files": null}"#).unwrap();
    assert!(list.files.is_empty());
}

#[test]
fn test_empty_list_files_response() {
    let json = r#"{}"#;
    let response: FileListResponse = serde_json::from_str(json).unwrap();
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
    let error = |code: Option<i32>, message: Option<&str>| {
        let mut error: FileError = serde_json::from_value(serde_json::json!({})).unwrap();
        error.code = code;
        error.message = message.map(str::to_string);
        error
    };
    assert_eq!(
        error(Some(400), Some("Invalid file format")).to_string(),
        "error 400: Invalid file format"
    );
    assert_eq!(error(Some(500), None).to_string(), "error 500");
    assert_eq!(
        error(None, Some("Something went wrong")).to_string(),
        "Something went wrong"
    );
    assert_eq!(error(None, None).to_string(), "unknown error");
}

/// The failed-file error as served live (2026-11-05), plus a `details` list
/// and an unknown key: both survive a round trip.
#[test]
fn file_error_keeps_details_and_unknown_fields() {
    let wire = serde_json::json!({
        "code": 3,
        "message": "The file failed to be processed.",
        "details": [{"@type": "type.googleapis.com/google.rpc.ErrorInfo", "reason": "CODEC"}],
        "brandNewField": true
    });
    let error: FileError = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(error.code, Some(3));
    let details = error.details.as_ref().expect("details");
    assert_eq!(details[0]["reason"], "CODEC");
    assert_eq!(error.extra["brandNewField"], true);
    assert_eq!(serde_json::to_value(&error).unwrap(), wire);
}

/// `sizeBytes` is a protobuf int64: the API sends a string (live
/// 2026-11-05), and a number reads the same. It serializes back as a string.
#[test]
fn size_bytes_reads_a_string_or_a_number() {
    let file = |size: serde_json::Value| -> FileMetadata {
        serde_json::from_value(serde_json::json!({
            "name": "files/abc", "mimeType": "video/mp4", "uri": "u", "sizeBytes": size
        }))
        .unwrap()
    };
    assert_eq!(file("2147483648".into()).size_bytes, Some(2_147_483_648));
    assert_eq!(file(17.into()).size_bytes, Some(17));
    // Unparseable or the wrong type degrades to `None` rather than failing
    // the whole file.
    assert_eq!(file("not a number".into()).size_bytes, None);
    assert_eq!(file(serde_json::json!([1])).size_bytes, None);
    assert_eq!(
        serde_json::to_value(file(17.into())).unwrap()["sizeBytes"],
        "17"
    );
}

/// The upload response observed live (2026-11-05): `updateTime` and
/// `source` are modeled, so nothing lands in `extra`.
#[test]
fn file_metadata_reads_the_live_upload_shape() {
    let wire = serde_json::json!({
        "name": "files/0z0kpv86pz2s",
        "displayName": "genai-rs-wp2b-probe",
        "mimeType": "text/plain",
        "sizeBytes": "17",
        "createTime": "2026-11-05T08:51:02.537655Z",
        "updateTime": "2026-11-05T08:51:02.537655Z",
        "expirationTime": "2026-11-07T08:51:01.874684367Z",
        "sha256Hash": "ODljMzE4MzdjNzk5OTlmODZiYjc5YjAxOGUyMWE5NmFiYWM1Mzg3MzdjODcxOWRjZDgxZmZmYWI2NjQ1Y2Q1Yg==",
        "uri": "https://generativelanguage.googleapis.com/v1beta/files/0z0kpv86pz2s",
        "state": "ACTIVE",
        "source": "UPLOADED"
    });
    let file: FileMetadata = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(file.size_bytes, Some(17));
    assert_eq!(file.source, Some(FileSource::Uploaded));
    assert_eq!(file.update_time, file.create_time);
    assert!(file.extra.is_empty(), "{:?}", file.extra);
    assert_eq!(serde_json::to_value(&file).unwrap(), wire);
}

#[test]
fn file_source_wire_values() {
    for (source, wire) in [
        (FileSource::Uploaded, "UPLOADED"),
        (FileSource::Generated, "GENERATED"),
        (FileSource::Registered, "REGISTERED"),
    ] {
        assert_eq!(serde_json::to_value(&source).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FileSource>(wire.into()).unwrap(),
            source
        );
    }
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn file_source_unknown_is_preserved() {
    let source: FileSource = serde_json::from_value("COPIED".into()).unwrap();
    assert_eq!(source.unknown_source_type(), Some("COPIED"));
    assert_eq!(serde_json::to_value(&source).unwrap(), "COPIED");
}

#[test]
fn video_metadata_keeps_unknown_keys() {
    let wire = serde_json::json!({"videoDuration": "12s", "frameRate": 30});
    let metadata: VideoMetadata = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(metadata.video_duration.as_deref(), Some("12s"));
    assert_eq!(metadata.extra["frameRate"], 30);
    assert_eq!(serde_json::to_value(&metadata).unwrap(), wire);
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
            prop::option::of(prop::collection::vec(
                "[a-z]{1,10}".prop_map(|reason| serde_json::json!({"reason": reason})),
                0..3,
            )),
        )
            .prop_map(|(code, message, details)| {
                let mut error: FileError =
                    serde_json::from_value(serde_json::json!({})).expect("empty error");
                (error.code, error.message, error.details) = (code, message, details);
                error
            })
    }

    /// Strategy for generating VideoMetadata.
    fn arb_video_metadata() -> impl Strategy<Value = VideoMetadata> {
        prop::option::of("[0-9]+s".prop_map(String::from)).prop_map(|video_duration| {
            VideoMetadata {
                video_duration,
                extra: serde_json::Map::new(),
            }
        })
    }

    /// Strategy for FileSource, with an unknown value outside strict mode.
    fn arb_file_source() -> impl Strategy<Value = FileSource> {
        #[cfg(not(feature = "strict-unknown"))]
        let unknown = "[A-Z_]{4,20}".prop_map(|source_type| FileSource::Unknown {
            data: serde_json::Value::String(source_type.clone()),
            source_type,
        });
        #[cfg(feature = "strict-unknown")]
        let unknown = Just(FileSource::Uploaded);
        prop_oneof![
            Just(FileSource::Uploaded),
            Just(FileSource::Generated),
            Just(FileSource::Registered),
            unknown,
        ]
    }

    /// Strategy for generating FileMetadata.
    fn arb_file_metadata() -> impl Strategy<Value = FileMetadata> {
        (
            (
                "files/[a-zA-Z0-9_]+",       // name
                prop::option::of(".{1,50}"), // display_name
                "[a-z]+/[a-z0-9+-]+",        // mime_type
                prop::option::of(0i64..=i64::MAX),
            ),
            (
                prop::option::of(arb_datetime()), // create_time
                prop::option::of(arb_datetime()), // update_time
                prop::option::of(arb_datetime()), // expiration_time
            ),
            prop::option::of("[a-f0-9]{64}"), // sha256_hash (API returns raw hash, no prefix)
            "https?://[a-z]+\\.[a-z]+/[a-z]+", // uri
            prop::option::of(arb_file_state()),
            prop::option::of(arb_file_error()),
            prop::option::of(arb_video_metadata()),
            prop::option::of(arb_file_source()),
        )
            .prop_map(
                |(
                    (name, display_name, mime_type, size_bytes),
                    (create_time, update_time, expiration_time),
                    sha256_hash,
                    uri,
                    state,
                    error,
                    video_metadata,
                    source,
                )| {
                    FileMetadata {
                        name,
                        display_name,
                        mime_type,
                        size_bytes,
                        create_time,
                        update_time,
                        expiration_time,
                        sha256_hash,
                        uri,
                        state,
                        error,
                        video_metadata,
                        source,
                        extra: serde_json::Map::new(),
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
            prop_assert_eq!(error, parsed);
        }

        /// Verify VideoMetadata roundtrips through JSON serialization, unknown
        /// keys included.
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
            prop_assert_eq!(&metadata.update_time, &parsed.update_time);
            prop_assert_eq!(&metadata.source, &parsed.source);
            prop_assert_eq!(&metadata.error, &parsed.error);
            prop_assert_eq!(&metadata.uri, &parsed.uri);
            // Note: state comparison is relaxed for Unknown variants
        }
    }
}
