//! Exact serialized form of [`Annotation`]: key order, skip rules, and
//! `Unknown` merging.

use super::*;
use serde_json::json;

fn to_json(annotation: &Annotation) -> String {
    serde_json::to_string(annotation).expect("Serialization failed")
}

#[test]
fn test_annotation_serializes_type_first_then_fields_in_order() {
    assert_eq!(
        to_json(&Annotation::url_citation(
            "https://example.com",
            Some("Example".to_string()),
            3,
            9
        )),
        r#"{"type":"url_citation","url":"https://example.com","title":"Example","start_index":3,"end_index":9}"#
    );
    assert_eq!(
        to_json(&Annotation::FileCitation {
            document_uri: Some("files/abc".to_string()),
            file_name: Some("report.pdf".to_string()),
            source: Some("stores/s".to_string()),
            custom_metadata: Some(json!({"k": "v"})),
            page_number: Some(3),
            media_id: Some("m1".to_string()),
            start_index: 0,
            end_index: 12,
        }),
        r#"{"type":"file_citation","document_uri":"files/abc","file_name":"report.pdf","source":"stores/s","custom_metadata":{"k":"v"},"page_number":3,"media_id":"m1","start_index":0,"end_index":12}"#
    );
    assert_eq!(
        to_json(&Annotation::PlaceCitation {
            place_id: Some("p1".to_string()),
            name: Some("Eiffel Tower".to_string()),
            url: Some("https://maps.example/e".to_string()),
            review_snippets: vec![ReviewSnippet {
                title: Some("Great!".to_string()),
                url: None,
                review_id: Some("r1".to_string()),
            }],
            start_index: 4,
            end_index: 16,
        }),
        r#"{"type":"place_citation","place_id":"p1","name":"Eiffel Tower","url":"https://maps.example/e","review_snippets":[{"title":"Great!","review_id":"r1"}],"start_index":4,"end_index":16}"#
    );
    assert_eq!(
        to_json(&Annotation::SpeechMetadata {
            speaker: Some("Bob".to_string()),
            style: Some("whisper".to_string()),
            start_index: Some(11),
            end_index: Some(37),
        }),
        r#"{"type":"speech_metadata","speaker":"Bob","style":"whisper","start_index":11,"end_index":37}"#
    );
    assert_eq!(
        to_json(&Annotation::WordInfo {
            text: Some("Hello".to_string()),
            speaker: Some("1".to_string()),
            start_offset: Some("0.1s".to_string()),
            end_offset: Some("0.4s".to_string()),
            start_index: Some(0),
            end_index: Some(5),
        }),
        r#"{"type":"word_info","text":"Hello","speaker":"1","start_offset":"0.1s","end_offset":"0.4s","start_index":0,"end_index":5}"#
    );
}

#[test]
fn test_annotation_omits_none_fields_and_empty_review_snippets() {
    // Citation indices are plain `usize`s, written even when zero.
    assert_eq!(
        to_json(&Annotation::UrlCitation {
            url: None,
            title: None,
            start_index: 0,
            end_index: 0,
        }),
        r#"{"type":"url_citation","start_index":0,"end_index":0}"#
    );
    assert_eq!(
        to_json(&Annotation::FileCitation {
            document_uri: None,
            file_name: None,
            source: None,
            custom_metadata: None,
            page_number: None,
            media_id: None,
            start_index: 0,
            end_index: 0,
        }),
        r#"{"type":"file_citation","start_index":0,"end_index":0}"#
    );
    // An empty snippet list is omitted, not sent as [].
    assert_eq!(
        to_json(&Annotation::PlaceCitation {
            place_id: None,
            name: None,
            url: None,
            review_snippets: vec![],
            start_index: 0,
            end_index: 0,
        }),
        r#"{"type":"place_citation","start_index":0,"end_index":0}"#
    );
    // Optional indices are omitted when `None` and written when `Some(0)`.
    assert_eq!(
        to_json(&Annotation::speech_metadata(None, None)),
        r#"{"type":"speech_metadata"}"#
    );
    assert_eq!(
        to_json(&Annotation::SpeechMetadata {
            speaker: None,
            style: None,
            start_index: Some(0),
            end_index: None,
        }),
        r#"{"type":"speech_metadata","start_index":0}"#
    );
    assert_eq!(
        to_json(&Annotation::WordInfo {
            text: None,
            speaker: None,
            start_offset: None,
            end_offset: None,
            start_index: None,
            end_index: None,
        }),
        r#"{"type":"word_info"}"#
    );
    assert_eq!(
        to_json(&Annotation::WordInfo {
            text: None,
            speaker: None,
            start_offset: None,
            end_offset: Some("0.4s".to_string()),
            start_index: None,
            end_index: Some(0),
        }),
        r#"{"type":"word_info","end_offset":"0.4s","end_index":0}"#
    );
}

#[test]
fn test_annotation_file_citation_writes_explicit_null_metadata() {
    // Only `None` is skipped: a hand-built `Some(Null)` goes out as null.
    assert_eq!(
        to_json(&Annotation::FileCitation {
            document_uri: None,
            file_name: None,
            source: None,
            custom_metadata: Some(serde_json::Value::Null),
            page_number: None,
            media_id: None,
            start_index: 1,
            end_index: 2,
        }),
        r#"{"type":"file_citation","custom_metadata":null,"start_index":1,"end_index":2}"#
    );
    // A wire null deserializes to `None`, so it is not sent back.
    let parsed: Annotation =
        serde_json::from_str(r#"{"type":"file_citation","custom_metadata":null}"#)
            .expect("Deserialization failed");
    assert_eq!(
        to_json(&parsed),
        r#"{"type":"file_citation","start_index":0,"end_index":0}"#
    );
}

#[test]
fn test_annotation_unknown_serializes_type_merged_into_data() {
    let unknown = |data| Annotation::Unknown {
        annotation_type: "future_citation".to_string(),
        data,
    };
    // `annotation_type` goes first and wins over a stale "type" inside
    // `data`; the other entries follow in the data map's order.
    assert_eq!(
        to_json(&unknown(json!({
            "alpha": {"on": true},
            "type": "stale",
            "zeta": 1
        }))),
        r#"{"type":"future_citation","alpha":{"on":true},"zeta":1}"#
    );
    // A hand-built unknown annotation without "type" in its data still
    // sends one.
    assert_eq!(
        to_json(&unknown(json!({"end_index": 6, "start_index": 2}))),
        r#"{"type":"future_citation","end_index":6,"start_index":2}"#
    );
    assert_eq!(
        to_json(&unknown(json!({}))),
        r#"{"type":"future_citation"}"#
    );
    // Non-object data nests under "data"; null data leaves just the type.
    assert_eq!(
        to_json(&unknown(json!([1, "a"]))),
        r#"{"type":"future_citation","data":[1,"a"]}"#
    );
    assert_eq!(
        to_json(&unknown(json!("raw"))),
        r#"{"type":"future_citation","data":"raw"}"#
    );
    assert_eq!(
        to_json(&unknown(serde_json::Value::Null)),
        r#"{"type":"future_citation"}"#
    );

    // A type-less object parses to Unknown with the "<missing type>"
    // sentinel, which the merge then writes back as the type.
    let parsed: Annotation =
        serde_json::from_str(r#"{"start_index":1}"#).expect("Deserialization failed");
    assert_eq!(parsed.unknown_annotation_type(), Some("<missing type>"));
    assert_eq!(
        to_json(&parsed),
        r#"{"type":"<missing type>","start_index":1}"#
    );

    // A known type with a mistyped field also falls back to Unknown, and is
    // sent back unchanged under its original type.
    let parsed: Annotation = serde_json::from_str(r#"{"type":"url_citation","start_index":"3"}"#)
        .expect("Deserialization failed");
    assert_eq!(parsed.unknown_annotation_type(), Some("url_citation"));
    assert_eq!(
        to_json(&parsed),
        r#"{"type":"url_citation","start_index":"3"}"#
    );
}
