//! Exact serialized form of [`Annotation`]: key order, skip rules, and
//! `Unknown` merging.

use super::*;
use crate::Content;
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
            extra: Default::default(),
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
                extra: Default::default(),
            }],
            start_index: 4,
            end_index: 16,
            extra: Default::default(),
        }),
        r#"{"type":"place_citation","place_id":"p1","name":"Eiffel Tower","url":"https://maps.example/e","review_snippets":[{"title":"Great!","review_id":"r1"}],"start_index":4,"end_index":16}"#
    );
    assert_eq!(
        to_json(&Annotation::SpeechMetadata {
            speaker: Some("Bob".to_string()),
            style: Some("whisper".to_string()),
            start_index: Some(11),
            end_index: Some(37),
            extra: Default::default(),
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
            extra: Default::default(),
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
            extra: Default::default(),
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
            extra: Default::default(),
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
            extra: Default::default(),
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
            extra: Default::default(),
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
            extra: Default::default(),
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
            extra: Default::default(),
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
            extra: Default::default(),
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

// --- Annotation Tests ---
// Annotation is a discriminated union under revision 2026-05-20:
// url_citation / file_citation / place_citation / Unknown.

#[test]
fn test_annotation_url_citation_constructor_and_accessors() {
    let annotation = Annotation::url_citation("https://example.com", None, 0, 10);

    assert_eq!(annotation.start_index(), Some(0));
    assert_eq!(annotation.end_index(), Some(10));
    assert_eq!(annotation.source(), Some("https://example.com"));
    assert!(!annotation.is_unknown());

    let with_title = Annotation::url_citation(
        "https://example.org",
        Some("Example Title".to_string()),
        5,
        15,
    );
    match &with_title {
        Annotation::UrlCitation { url, title, .. } => {
            assert_eq!(url.as_deref(), Some("https://example.org"));
            assert_eq!(title.as_deref(), Some("Example Title"));
        }
        _ => panic!("Expected UrlCitation variant"),
    }
}

#[test]
fn test_annotation_extract_span() {
    let text = "Hello, world!";
    let annotation = Annotation::url_citation("https://example.com", None, 0, 5);
    assert_eq!(annotation.extract_span(text), Some("Hello"));

    let annotation_mid = Annotation::url_citation("https://example.com", None, 7, 12);
    assert_eq!(annotation_mid.extract_span(text), Some("world"));

    // Out of bounds
    let out_of_bounds = Annotation::url_citation("https://example.com", None, 100, 200);
    assert_eq!(out_of_bounds.extract_span(text), None);
}

#[test]
fn test_annotation_extract_span_utf8() {
    // Test with UTF-8 text - annotations use byte indices
    let text = "Héllo, 世界!"; // "Héllo" = 6 bytes (H=1, é=2, l=1, l=1, o=1), ", " = 2 bytes, "世界" = 6 bytes
    let annotation = Annotation::url_citation("https://example.com", None, 0, 6);
    assert_eq!(annotation.extract_span(text), Some("Héllo"));

    // Extract Chinese characters
    let world_annotation = Annotation::url_citation("https://example.com", None, 8, 14);
    assert_eq!(world_annotation.extract_span(text), Some("世界"));
}

#[test]
fn test_annotation_extract_span_inverted_indices() {
    // Edge case: start_index > end_index (malformed annotation)
    // The implementation should gracefully return None
    let inverted = Annotation::url_citation("https://example.com", None, 10, 5);

    let text = "Hello, world!";
    assert_eq!(
        inverted.extract_span(text),
        None,
        "Inverted indices should return None"
    );
}

#[test]
fn test_annotation_extract_span_zero_length() {
    // Edge case: start_index == end_index (zero-length span)
    let zero_len = Annotation::url_citation("https://example.com", None, 5, 5);

    let text = "Hello, world!";
    assert_eq!(
        zero_len.extract_span(text),
        Some(""),
        "Zero-length span should return empty string"
    );
}

#[test]
fn test_annotation_extract_span_mid_utf8_boundary() {
    // Edge case: indices that land in the middle of a multi-byte character
    // "世" is a 3-byte UTF-8 character (E4 B8 96)
    let text = "Hello, 世界!"; // "世" starts at byte 7, "界" starts at byte 10

    // Try to slice starting in the middle of "世" (byte 8)
    let mid_start = Annotation::url_citation("https://example.com", None, 8, 13);
    assert_eq!(
        mid_start.extract_span(text),
        None,
        "Slicing from middle of UTF-8 character should return None"
    );

    // Try to slice ending in the middle of "界" (byte 11)
    let mid_end = Annotation::url_citation("https://example.com", None, 7, 11);
    assert_eq!(
        mid_end.extract_span(text),
        None,
        "Slicing to middle of UTF-8 character should return None"
    );

    // Valid slice of "世界" (bytes 7-13)
    let valid_cjk = Annotation::url_citation("https://example.com", None, 7, 13);
    assert_eq!(
        valid_cjk.extract_span(text),
        Some("世界"),
        "Valid CJK character slice should work"
    );
}

#[test]
fn test_annotation_url_citation_wire_format() {
    // Wire format: {"type":"url_citation","url":...,"title":...,"start_index":N,"end_index":N}
    let annotation =
        Annotation::url_citation("https://example.com", Some("Example".to_string()), 3, 9);

    let json = serde_json::to_string(&annotation).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "url_citation");
    assert_eq!(value["url"], "https://example.com");
    assert_eq!(value["title"], "Example");
    assert_eq!(value["start_index"], 3);
    assert_eq!(value["end_index"], 9);
}

#[test]
fn test_serialize_text_with_annotations() {
    let annotations = vec![
        Annotation::url_citation("https://example.com", None, 0, 5),
        Annotation::UrlCitation {
            url: None,
            title: None,
            start_index: 10,
            end_index: 20,
            extra: Default::default(),
        },
    ];

    let content = Content::Text {
        text: Some("Hello, world! This is grounded text.".to_string()),
        annotations: Some(annotations),
        extra: Default::default(),
    };

    let json = serde_json::to_string(&content).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "text");
    assert_eq!(value["text"], "Hello, world! This is grounded text.");
    assert!(value["annotations"].is_array());
    assert_eq!(value["annotations"].as_array().unwrap().len(), 2);
    assert_eq!(value["annotations"][0]["type"], "url_citation");
    assert_eq!(value["annotations"][0]["start_index"], 0);
    assert_eq!(value["annotations"][0]["end_index"], 5);
    assert_eq!(value["annotations"][0]["url"], "https://example.com");
    assert_eq!(value["annotations"][1]["start_index"], 10);
    assert_eq!(value["annotations"][1]["end_index"], 20);
    // None url is omitted from the wire format
    assert!(value["annotations"][1].get("url").is_none());
}

#[test]
fn test_serialize_text_with_empty_annotations_omitted() {
    // Empty annotations array should not be serialized
    let content = Content::Text {
        text: Some("Plain text".to_string()),
        annotations: Some(vec![]),
        extra: Default::default(),
    };

    let json = serde_json::to_string(&content).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "text");
    assert_eq!(value["text"], "Plain text");
    // Empty annotations should not be serialized
    assert!(value.get("annotations").is_none());
}

#[test]
fn test_deserialize_text_with_annotations() {
    let json = r#"{
        "type": "text",
        "text": "This is grounded text.",
        "annotations": [
            {"type": "url_citation", "url": "https://example.com", "title": "Example", "start_index": 0, "end_index": 4},
            {"type": "url_citation", "start_index": 8, "end_index": 16}
        ]
    }"#;

    let content: Content = serde_json::from_str(json).expect("Deserialization failed");

    match content {
        Content::Text {
            text, annotations, ..
        } => {
            assert_eq!(text, Some("This is grounded text.".to_string()));
            let annots = annotations.expect("Should have annotations");
            assert_eq!(annots.len(), 2);
            assert_eq!(annots[0].start_index(), Some(0));
            assert_eq!(annots[0].end_index(), Some(4));
            assert_eq!(annots[0].source(), Some("https://example.com"));
            assert_eq!(annots[1].start_index(), Some(8));
            assert_eq!(annots[1].end_index(), Some(16));
            assert_eq!(annots[1].source(), None);
        }
        _ => panic!("Expected Text variant"),
    }
}

#[test]
fn test_deserialize_text_without_annotations() {
    let json = r#"{"type": "text", "text": "Plain text"}"#;

    let content: Content = serde_json::from_str(json).expect("Deserialization failed");

    match content {
        Content::Text {
            text, annotations, ..
        } => {
            assert_eq!(text, Some("Plain text".to_string()));
            assert!(annotations.is_none());
        }
        _ => panic!("Expected Text variant"),
    }
}

#[test]
fn test_text_with_annotations_roundtrip() {
    let annotations = vec![Annotation::url_citation(
        "https://source.example.com",
        None,
        5,
        15,
    )];

    let original = Content::Text {
        text: Some("Some grounded content here.".to_string()),
        annotations: Some(annotations),
        extra: Default::default(),
    };

    let json = serde_json::to_string(&original).expect("Serialization failed");
    let deserialized: Content = serde_json::from_str(&json).expect("Deserialization failed");

    // Compare by re-serializing and checking the JSON
    let roundtrip_json = serde_json::to_string(&deserialized).expect("Serialization failed");
    assert_eq!(json, roundtrip_json);

    // Also verify content matches
    match deserialized {
        Content::Text {
            text, annotations, ..
        } => {
            assert_eq!(text, Some("Some grounded content here.".to_string()));
            let annots = annotations.expect("Should have annotations");
            assert_eq!(annots.len(), 1);
            assert_eq!(annots[0].start_index(), Some(5));
            assert_eq!(annots[0].end_index(), Some(15));
            assert_eq!(annots[0].source(), Some("https://source.example.com"));
        }
        _ => panic!("Expected Text variant"),
    }
}

#[test]
fn test_annotations_helper_method() {
    let content_with_annotations = Content::Text {
        text: Some("Hello".to_string()),
        annotations: Some(vec![Annotation::url_citation(
            "https://example.com",
            None,
            0,
            5,
        )]),
        extra: Default::default(),
    };

    assert!(content_with_annotations.annotations().is_some());
    assert_eq!(content_with_annotations.annotations().unwrap().len(), 1);

    let content_without_annotations = Content::Text {
        text: Some("Hello".to_string()),
        annotations: None,
        extra: Default::default(),
    };

    assert!(content_without_annotations.annotations().is_none());

    // Non-text content returns None
    let image = Content::image_data("base64", "image/png");
    assert!(image.annotations().is_none());
}

#[test]
fn test_annotation_file_citation_deserialize() {
    let json = r#"{
        "type": "file_citation",
        "document_uri": "files/abc123",
        "file_name": "report.pdf",
        "source": "stores/my-store",
        "page_number": 3,
        "start_index": 0,
        "end_index": 12
    }"#;

    let annotation: Annotation = serde_json::from_str(json).expect("Should deserialize");

    match &annotation {
        Annotation::FileCitation {
            document_uri,
            file_name,
            source,
            page_number,
            start_index,
            end_index,
            ..
        } => {
            assert_eq!(document_uri.as_deref(), Some("files/abc123"));
            assert_eq!(file_name.as_deref(), Some("report.pdf"));
            assert_eq!(source.as_deref(), Some("stores/my-store"));
            assert_eq!(*page_number, Some(3));
            assert_eq!(*start_index, 0);
            assert_eq!(*end_index, 12);
        }
        _ => panic!("Expected FileCitation variant, got {:?}", annotation),
    }

    // source() prefers document_uri for file citations
    assert_eq!(annotation.source(), Some("files/abc123"));
    assert!(!annotation.is_unknown());
}

#[test]
fn test_annotation_place_citation_deserialize() {
    let json = r#"{
        "type": "place_citation",
        "place_id": "ChIJLU7jZClu5kcR",
        "name": "Eiffel Tower",
        "url": "https://maps.example.com/eiffel",
        "review_snippets": [{"title": "Great!", "url": "https://r.example.com", "review_id": "r1"}],
        "start_index": 4,
        "end_index": 16
    }"#;

    let annotation: Annotation = serde_json::from_str(json).expect("Should deserialize");

    match &annotation {
        Annotation::PlaceCitation {
            place_id,
            name,
            url,
            review_snippets,
            start_index,
            end_index,
            ..
        } => {
            assert_eq!(place_id.as_deref(), Some("ChIJLU7jZClu5kcR"));
            assert_eq!(name.as_deref(), Some("Eiffel Tower"));
            assert_eq!(url.as_deref(), Some("https://maps.example.com/eiffel"));
            assert_eq!(review_snippets.len(), 1);
            assert_eq!(review_snippets[0].title.as_deref(), Some("Great!"));
            assert_eq!(review_snippets[0].review_id.as_deref(), Some("r1"));
            assert_eq!(*start_index, 4);
            assert_eq!(*end_index, 16);
        }
        _ => panic!("Expected PlaceCitation variant, got {:?}", annotation),
    }

    // source() prefers the place URL
    assert_eq!(annotation.source(), Some("https://maps.example.com/eiffel"));
}

#[test]
fn test_annotation_unknown_variant() {
    // Unrecognized annotation types are preserved in Unknown
    let json =
        r#"{"type": "future_citation", "custom_field": "value", "start_index": 2, "end_index": 6}"#;
    let annotation: Annotation = serde_json::from_str(json).expect("Should deserialize");

    assert!(annotation.is_unknown());
    assert_eq!(
        annotation.unknown_annotation_type(),
        Some("future_citation")
    );
    let data = annotation.unknown_data().expect("Should have data");
    assert_eq!(data["custom_field"], "value");

    // Indices are still readable from the preserved data
    assert_eq!(annotation.start_index(), Some(2));
    assert_eq!(annotation.end_index(), Some(6));
    assert_eq!(annotation.extract_span("Hello, world!"), Some("llo,"));
    assert_eq!(annotation.source(), None);

    // Roundtrip preserves the original fields
    let reserialized = serde_json::to_string(&annotation).expect("Should serialize");
    let value: serde_json::Value = serde_json::from_str(&reserialized).unwrap();
    assert_eq!(value["type"], "future_citation");
    assert_eq!(value["custom_field"], "value");
    assert_eq!(value["start_index"], 2);
    assert_eq!(value["end_index"], 6);
}
