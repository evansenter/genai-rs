use super::*;

#[test]
fn document_wait_defaults_are_60_s_and_500_ms() {
    assert_eq!(DOCUMENT_WAIT_TIMEOUT, Duration::from_secs(60));
    assert_eq!(DOCUMENT_POLL_INTERVAL, Duration::from_millis(500));
}

#[test]
fn file_search_handles_and_lists_debug_redact_the_api_key() {
    let client = Client::new("secret-api-key".to_string());
    let stores = client.file_search_stores();
    for debug in [
        format!("{stores:?}"),
        format!("{:?}", stores.documents()),
        format!("{:?}", stores.list().with_page_size(5)),
        format!("{:?}", stores.documents().list("fileSearchStores/abc")),
    ] {
        assert!(!debug.contains("secret-api-key"), "{debug}");
        assert!(debug.contains("[REDACTED]"), "{debug}");
    }
}
