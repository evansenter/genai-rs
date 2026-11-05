use super::*;

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
