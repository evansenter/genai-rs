//! Property tests: `Content`, its annotations, tool result items and media settings.

use super::*;

/// Strategy for generating arbitrary Resolution values
pub(super) fn arb_resolution() -> impl Strategy<Value = Resolution> {
    prop_oneof![
        Just(Resolution::Low),
        Just(Resolution::Medium),
        Just(Resolution::High),
        Just(Resolution::UltraHigh),
        // Unknown variant with arbitrary string value
        unknown_or(Resolution::Low, |s| Resolution::Unknown {
            resolution_type: s.clone(),
            data: serde_json::Value::String(s),
        }),
    ]
}

/// Strategy for `VideoProcessing`, covering both wire forms (bare mode string
/// and the `{"type": "static", ...}` object) plus the Unknown variant.
pub(super) fn arb_video_processing() -> impl Strategy<Value = VideoProcessing> {
    prop_oneof![
        Just(VideoProcessing::Static),
        Just(VideoProcessing::Agentic),
        (
            proptest::option::of(arb_offset()),
            proptest::option::of(arb_offset()),
            proptest::option::of(arb_fps()),
        )
            .prop_map(
                |(start_offset, end_offset, fps)| VideoProcessing::StaticSegment {
                    start_offset,
                    end_offset,
                    fps,
                    extra: Default::default(),
                }
            ),
        // Unknown variant with arbitrary mode string
        arb_unknown_type().prop_map(|s| VideoProcessing::Unknown {
            processing_type: s.clone(),
            data: serde_json::Value::String(s),
        }),
    ]
}

/// Offsets are decimal seconds with an `s` suffix (e.g. `"10.5s"`).
pub(super) fn arb_offset() -> impl Strategy<Value = String> {
    (0u32..3600u32, 0u32..10u32).prop_map(|(secs, tenths)| format!("{secs}.{tenths}s"))
}

/// Frame rates round-trip through JSON exactly, so restrict to values with an
/// exact f64 representation rather than arbitrary floats.
pub(super) fn arb_fps() -> impl Strategy<Value = f64> {
    (1u32..=120u32).prop_map(f64::from)
}

// =============================================================================
// Annotation Strategies (discriminated union: url/file/place citation)
// =============================================================================

pub(super) fn arb_review_snippet() -> impl Strategy<Value = ReviewSnippet> {
    (
        proptest::option::of(arb_text()),
        proptest::option::of(arb_text()),
        proptest::option::of(arb_identifier()),
    )
        .prop_map(|(title, url, review_id)| ReviewSnippet {
            title,
            url,
            review_id,
            extra: Default::default(),
        })
}

/// Strategy for generating Annotation values (all citation types + Unknown).
pub(super) fn arb_annotation() -> impl Strategy<Value = Annotation> {
    fn span() -> (std::ops::Range<usize>, std::ops::Range<usize>) {
        (0usize..1000, 0usize..1000)
    }
    prop_oneof![
        // UrlCitation
        (
            span(),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text())
        )
            .prop_map(|((start, len), url, title)| Annotation::UrlCitation {
                url,
                title,
                start_index: start,
                end_index: start.saturating_add(len),
                extra: Default::default(),
            }),
        // FileCitation
        (
            span(),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_unknown_object()),
            proptest::option::of(any::<u32>()),
            proptest::option::of(arb_identifier()),
        )
            .prop_map(
                |(
                    (start, len),
                    document_uri,
                    file_name,
                    source,
                    custom_metadata,
                    page_number,
                    media_id,
                )| {
                    Annotation::FileCitation {
                        document_uri,
                        file_name,
                        source,
                        custom_metadata,
                        page_number,
                        media_id,
                        start_index: start,
                        end_index: start.saturating_add(len),
                        extra: Default::default(),
                    }
                }
            ),
        // PlaceCitation
        (
            span(),
            proptest::option::of(arb_identifier()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            prop::collection::vec(arb_review_snippet(), 0..2),
        )
            .prop_map(|((start, len), place_id, name, url, review_snippets)| {
                Annotation::PlaceCitation {
                    place_id,
                    name,
                    url,
                    review_snippets,
                    start_index: start,
                    end_index: start.saturating_add(len),
                    extra: Default::default(),
                }
            }),
        // SpeechMetadata (indices optional)
        (
            proptest::option::of(arb_identifier()),
            proptest::option::of(arb_text()),
            proptest::option::of(0usize..1000),
            proptest::option::of(0usize..1000),
        )
            .prop_map(|(speaker, style, start_index, end_index)| {
                Annotation::SpeechMetadata {
                    speaker,
                    style,
                    start_index,
                    end_index,
                    extra: Default::default(),
                }
            }),
        // WordInfo (indices optional)
        (
            proptest::option::of(arb_text()),
            proptest::option::of(arb_identifier()),
            proptest::option::of(arb_offset()),
            proptest::option::of(arb_offset()),
            proptest::option::of(0usize..1000),
            proptest::option::of(0usize..1000),
        )
            .prop_map(
                |(text, speaker, start_offset, end_offset, start_index, end_index)| {
                    Annotation::WordInfo {
                        text,
                        speaker,
                        start_offset,
                        end_offset,
                        start_index,
                        end_index,
                        extra: Default::default(),
                    }
                }
            ),
        // Unknown annotation type (forward compatibility)
        (arb_unknown_type(), arb_unknown_object()).prop_map(|(annotation_type, data)| {
            Annotation::Unknown {
                annotation_type,
                data,
            }
        }),
    ]
}

// =============================================================================
// Tool Result Item Strategies
// =============================================================================

/// Strategy for generating GoogleSearchResultItem objects.
pub(super) fn arb_google_search_result_item() -> impl Strategy<Value = GoogleSearchResultItem> {
    proptest::option::of(arb_text()).prop_map(|search_suggestions| GoogleSearchResultItem {
        search_suggestions,
        extra: Default::default(),
    })
}

/// Strategy for generating Place objects.
pub(super) fn arb_place() -> impl Strategy<Value = Place> {
    (
        proptest::option::of(arb_text()),
        proptest::option::of(arb_text()),
        proptest::option::of(arb_text()),
        proptest::option::of(prop::collection::vec(arb_review_snippet(), 0..2)),
    )
        .prop_map(|(name, place_id, url, review_snippets)| Place {
            name,
            place_id,
            url,
            review_snippets,
            extra: serde_json::Map::new(),
        })
}

/// Strategy for generating GoogleMapsResultItem objects.
pub(super) fn arb_google_maps_result_item() -> impl Strategy<Value = GoogleMapsResultItem> {
    (
        proptest::option::of(proptest::collection::vec(arb_place(), 0..3)),
        proptest::option::of(arb_text()),
    )
        .prop_map(|(places, widget_context_token)| GoogleMapsResultItem {
            places,
            widget_context_token,
            extra: Default::default(),
        })
}

/// Strategy for generating UrlContextResultItem objects.
pub(super) fn arb_url_context_result_item() -> impl Strategy<Value = UrlContextResultItem> {
    (
        arb_text(),
        prop_oneof![
            Just("success"),
            Just("error"),
            Just("paywall"),
            Just("unsafe")
        ],
    )
        .prop_map(|(url, status)| UrlContextResultItem::new(url, status))
}

/// Strategy for CodeExecutionLanguage (wire format is lowercase).
pub(super) fn arb_code_execution_language() -> impl Strategy<Value = CodeExecutionLanguage> {
    prop_oneof![
        Just(CodeExecutionLanguage::Python),
        Just(CodeExecutionLanguage::Bash),
        // Unknown variant with preserved data
        unknown_or(CodeExecutionLanguage::Python, |language_type| {
            CodeExecutionLanguage::Unknown {
                language_type: language_type.clone(),
                data: serde_json::Value::String(language_type),
            }
        }),
    ]
}

// =============================================================================
// Content Strategy (slimmed media union: text/image/audio/video/document)
// =============================================================================

/// Helper to create the known Content variants (used by both strict and non-strict modes).
pub(super) fn arb_known_content() -> impl Strategy<Value = Content> {
    prop_oneof![
        // Text content (with optional annotations)
        (
            proptest::option::of(arb_text()),
            proptest::option::of(proptest::collection::vec(arb_annotation(), 0..3))
        )
            .prop_map(|(text, annotations)| Content::Text {
                text,
                annotations,
                extra: Default::default()
            }),
        // Image content
        (
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_resolution())
        )
            .prop_map(|(data, uri, mime_type, resolution)| Content::Image {
                data,
                uri,
                mime_type,
                resolution,
                extra: Default::default()
            }),
        // Audio content (with sample_rate/channels)
        (
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(any::<u32>()),
            proptest::option::of(1u32..8),
        )
            .prop_map(
                |(data, uri, mime_type, sample_rate, channels)| Content::Audio {
                    data,
                    uri,
                    mime_type,
                    sample_rate,
                    channels,
                    extra: Default::default(),
                }
            ),
        // Video content
        (
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_resolution()),
            proptest::option::of(arb_video_processing())
        )
            .prop_map(
                |(data, uri, mime_type, resolution, processing)| Content::Video {
                    data,
                    uri,
                    mime_type,
                    resolution,
                    processing,
                    name: None,
                    extra: Default::default(),
                }
            ),
        // Document content
        (
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text())
        )
            .prop_map(|(data, uri, mime_type)| Content::Document {
                data,
                uri,
                mime_type,
                extra: Default::default()
            }),
    ]
}

/// Strategy for known Content variants only.
/// Used when strict mode is enabled (Unknown variants fail to deserialize in strict mode).
#[cfg(genai_strict_unknown)]
pub(super) fn arb_content() -> impl Strategy<Value = Content> {
    arb_known_content()
}

/// Strategy for all Content variants including Unknown.
/// Used in normal mode (Unknown variants are gracefully handled).
#[cfg(not(genai_strict_unknown))]
pub(super) fn arb_content() -> impl Strategy<Value = Content> {
    prop_oneof![
        arb_known_content(),
        // Unknown content (for forward compatibility testing)
        (arb_unknown_type(), arb_unknown_object())
            .prop_map(|(content_type, data)| Content::Unknown { content_type, data }),
    ]
}

proptest! {
    /// Test that CodeExecutionLanguage roundtrips correctly through JSON
    /// (wire format is lowercase "python").
    #[test]
    fn code_execution_language_roundtrip(lang in arb_code_execution_language()) {
        let json = serde_json::to_string(&lang).expect("Serialization should succeed");
        let restored: CodeExecutionLanguage = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(lang, restored);
    }

    /// Test that the known CodeExecutionLanguage wire format is lowercase.
    #[test]
    fn code_execution_language_wire_is_lowercase(_unused in Just(())) {
        let json = serde_json::to_string(&CodeExecutionLanguage::Python).unwrap();
        prop_assert_eq!(json, "\"python\"");
    }

    /// Test that Annotation roundtrips stably through JSON.
    ///
    /// Uses JSON comparison since the Unknown variant normalizes its data
    /// payload (the "type" tag is merged into the preserved JSON).
    #[test]
    fn annotation_roundtrip(annotation in arb_annotation()) {
        assert_value_roundtrip(&annotation)?;
    }

    /// Test that Content roundtrips stably through JSON.
    #[test]
    fn content_roundtrip(content in arb_content()) {
        assert_value_roundtrip(&content)?;
    }

    /// Test that unknown Annotation types are preserved through a roundtrip.
    #[test]
    fn annotation_unknown_preservation(annotation_type in arb_unknown_type(), data in arb_unknown_object()) {
        let annotation = Annotation::Unknown {
            annotation_type: annotation_type.clone(),
            data,
        };

        let json = serde_json::to_value(&annotation).expect("Serialization should succeed");
        let restored: Annotation = serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        prop_assert!(restored.is_unknown());
        prop_assert_eq!(restored.unknown_annotation_type(), Some(annotation_type.as_str()));

        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test empty strings are handled correctly.
    #[test]
    fn empty_text_content_roundtrip(_unused in Just(())) {
        let content = Content::Text { text: Some(String::new()), annotations: None, extra: Default::default() };
        assert_value_roundtrip(&content)?;
    }

    /// Test None text content is handled correctly.
    #[test]
    fn none_text_content_roundtrip(_unused in Just(())) {
        let content = Content::Text { text: None, annotations: None, extra: Default::default() };
        assert_value_roundtrip(&content)?;
    }

    /// Test special characters in strings are handled correctly.
    #[test]
    fn special_chars_in_text(text in ".*[\n\r\t\"\\\\].*") {
        let content = Content::Text { text: Some(text), annotations: None, extra: Default::default() };
        assert_value_roundtrip(&content)?;
    }

    /// Test Unicode in strings is handled correctly.
    #[test]
    fn unicode_in_text(text in ".*[\\u{1F600}-\\u{1F64F}].*") {
        let content = Content::Text { text: Some(text), annotations: None, extra: Default::default() };
        assert_value_roundtrip(&content)?;
    }
}

#[cfg(not(genai_strict_unknown))]
proptest! {
    /// Test that unknown Content types are preserved through a roundtrip.
    #[test]
    fn content_unknown_preservation(content_type in arb_unknown_type(), data in arb_unknown_object()) {
        let content = Content::Unknown {
            content_type: content_type.clone(),
            data,
        };

        let json = serde_json::to_value(&content).expect("Serialization should succeed");
        let restored: Content = serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        prop_assert!(restored.is_unknown());
        prop_assert_eq!(restored.unknown_content_type(), Some(content_type.as_str()));

        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }
}
