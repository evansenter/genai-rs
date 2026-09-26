//! Exact serialized form of [`Content`]: key order, skip rules, and
//! `Unknown` merging.

use super::*;
use serde_json::json;

fn to_json(content: &Content) -> String {
    serde_json::to_string(content).expect("Serialization failed")
}

#[test]
fn test_content_serializes_type_first_then_fields_in_order() {
    assert_eq!(
        to_json(&Content::Text {
            text: Some("Hello".to_string()),
            annotations: Some(vec![Annotation::url_citation(
                "https://example.com",
                Some("Example".to_string()),
                0,
                5
            )]),
        }),
        r#"{"type":"text","text":"Hello","annotations":[{"type":"url_citation","url":"https://example.com","title":"Example","start_index":0,"end_index":5}]}"#
    );
    assert_eq!(
        to_json(&Content::Image {
            data: Some("AAAA".to_string()),
            uri: Some("files/img".to_string()),
            mime_type: Some("image/png".to_string()),
            resolution: Some(Resolution::High),
        }),
        r#"{"type":"image","data":"AAAA","uri":"files/img","mime_type":"image/png","resolution":"high"}"#
    );
    assert_eq!(
        to_json(&Content::Audio {
            data: Some("UklG".to_string()),
            uri: Some("files/aud".to_string()),
            mime_type: Some("audio/wav".to_string()),
            sample_rate: Some(24000),
            channels: Some(1),
        }),
        r#"{"type":"audio","data":"UklG","uri":"files/aud","mime_type":"audio/wav","sample_rate":24000,"channels":1}"#
    );
    assert_eq!(
        to_json(&Content::Video {
            data: Some("AAAB".to_string()),
            uri: Some("files/vid".to_string()),
            mime_type: Some("video/mp4".to_string()),
            resolution: Some(Resolution::Low),
            processing: Some(VideoProcessing::StaticSegment {
                start_offset: Some("5s".to_string()),
                end_offset: Some("10s".to_string()),
                fps: Some(1.0),
            }),
            name: Some("clip".to_string()),
        }),
        r#"{"type":"video","data":"AAAB","uri":"files/vid","mime_type":"video/mp4","resolution":"low","processing":{"type":"static","start_offset":"5s","end_offset":"10s","fps":1.0},"name":"clip"}"#
    );
    assert_eq!(
        to_json(&Content::Document {
            data: Some("JVBE".to_string()),
            uri: Some("files/doc".to_string()),
            mime_type: Some("application/pdf".to_string()),
        }),
        r#"{"type":"document","data":"JVBE","uri":"files/doc","mime_type":"application/pdf"}"#
    );
}

#[test]
fn test_content_omits_none_fields() {
    assert_eq!(
        to_json(&Content::Text {
            text: None,
            annotations: None,
        }),
        r#"{"type":"text"}"#
    );
    assert_eq!(
        to_json(&Content::Image {
            data: None,
            uri: None,
            mime_type: None,
            resolution: None,
        }),
        r#"{"type":"image"}"#
    );
    assert_eq!(
        to_json(&Content::Audio {
            data: None,
            uri: None,
            mime_type: None,
            sample_rate: None,
            channels: None,
        }),
        r#"{"type":"audio"}"#
    );
    assert_eq!(
        to_json(&Content::Video {
            data: None,
            uri: None,
            mime_type: None,
            resolution: None,
            processing: None,
            name: None,
        }),
        r#"{"type":"video"}"#
    );
    assert_eq!(
        to_json(&Content::Document {
            data: None,
            uri: None,
            mime_type: None,
        }),
        r#"{"type":"document"}"#
    );
    // Omission is per field: set fields keep their relative order.
    assert_eq!(
        to_json(&Content::Video {
            data: None,
            uri: Some("files/v".to_string()),
            mime_type: None,
            resolution: None,
            processing: Some(VideoProcessing::Agentic),
            name: None,
        }),
        r#"{"type":"video","uri":"files/v","processing":"agentic"}"#
    );
}

#[test]
fn test_content_keeps_empty_and_zero_values() {
    // Only `None` is skipped: an empty string and a zero are sent.
    assert_eq!(
        to_json(&Content::Text {
            text: Some(String::new()),
            annotations: None,
        }),
        r#"{"type":"text","text":""}"#
    );
    assert_eq!(
        to_json(&Content::Audio {
            data: None,
            uri: None,
            mime_type: None,
            sample_rate: Some(0),
            channels: Some(0),
        }),
        r#"{"type":"audio","sample_rate":0,"channels":0}"#
    );
    assert_eq!(
        to_json(&Content::Video {
            data: None,
            uri: None,
            mime_type: None,
            resolution: None,
            processing: Some(VideoProcessing::StaticSegment {
                start_offset: None,
                end_offset: None,
                fps: None,
            }),
            name: Some(String::new()),
        }),
        r#"{"type":"video","processing":{"type":"static"},"name":""}"#
    );
}

#[test]
fn test_text_omits_empty_annotations() {
    // `Some(vec![])` is omitted like `None`, not sent as `[]`.
    assert_eq!(
        to_json(&Content::Text {
            text: Some("hi".to_string()),
            annotations: Some(vec![]),
        }),
        r#"{"type":"text","text":"hi"}"#
    );
    assert_eq!(
        to_json(&Content::Text {
            text: None,
            annotations: Some(vec![]),
        }),
        r#"{"type":"text"}"#
    );
}

#[test]
fn test_unknown_content_merges_type_into_data() {
    let unknown = |data: serde_json::Value| Content::Unknown {
        content_type: "zz_new".to_string(),
        data,
    };

    // Data without a "type" key: the type goes first, then the entries.
    assert_eq!(
        to_json(&unknown(json!({"a": "x", "b": 1}))),
        r#"{"type":"zz_new","a":"x","b":1}"#
    );
    // A matching "type" in data is written once, first.
    assert_eq!(
        to_json(&unknown(json!({"a": 1, "type": "zz_new", "z": 2}))),
        r#"{"type":"zz_new","a":1,"z":2}"#
    );
    // A stale or non-string "type" in data is replaced by content_type.
    assert_eq!(
        to_json(&unknown(json!({"type": "old", "a": 1}))),
        r#"{"type":"zz_new","a":1}"#
    );
    assert_eq!(
        to_json(&unknown(json!({"type": 7, "a": null}))),
        r#"{"type":"zz_new","a":null}"#
    );
    // Empty object and null data leave just the type.
    assert_eq!(to_json(&unknown(json!({}))), r#"{"type":"zz_new"}"#);
    assert_eq!(
        to_json(&unknown(serde_json::Value::Null)),
        r#"{"type":"zz_new"}"#
    );
    // Non-object data goes under "data".
    assert_eq!(
        to_json(&unknown(json!([1, 2]))),
        r#"{"type":"zz_new","data":[1,2]}"#
    );
    assert_eq!(
        to_json(&unknown(json!("s"))),
        r#"{"type":"zz_new","data":"s"}"#
    );
    assert_eq!(to_json(&unknown(json!(0))), r#"{"type":"zz_new","data":0}"#);
    assert_eq!(
        to_json(&unknown(json!(false))),
        r#"{"type":"zz_new","data":false}"#
    );
    // Nested objects are written as-is, including an inner "type".
    assert_eq!(
        to_json(&unknown(json!({"data": {"type": "inner"}}))),
        r#"{"type":"zz_new","data":{"type":"inner"}}"#
    );
}

#[test]
fn test_unknown_content_sentinel_type_is_sent_verbatim() {
    // What deserializing a type-less object produces, re-sent as-is.
    assert_eq!(
        to_json(&Content::Unknown {
            content_type: "<missing type>".to_string(),
            data: json!({"text": "hi"}),
        }),
        r#"{"type":"<missing type>","text":"hi"}"#
    );
    assert_eq!(
        to_json(&Content::Unknown {
            content_type: String::new(),
            data: json!({"field": "value"}),
        }),
        r#"{"type":"","field":"value"}"#
    );
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_malformed_known_content_resends_under_its_own_type() {
    // A known tag with a mistyped field falls back to Unknown, and goes
    // back out unchanged under the same tag.
    let content: Content =
        serde_json::from_str(r#"{"type":"audio","sample_rate":"x","uri":"files/a"}"#).unwrap();
    assert_eq!(content.unknown_content_type(), Some("audio"));
    assert_eq!(
        to_json(&content),
        r#"{"type":"audio","sample_rate":"x","uri":"files/a"}"#
    );
}
