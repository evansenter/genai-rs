//! Wire format verification tests
//!
//! These tests verify that our serialization/deserialization matches the wire formats
//! documented in `docs/ENUM_WIRE_FORMATS.md` (Interactions API revision 2026-05-20).
//!
//! Each test corresponds to a documented wire format and ensures:
//! 1. Deserialization from wire format works correctly
//! 2. Serialization produces the correct wire format
//! 3. Roundtrip works for all variants
//!
//! When adding new enums or updating wire formats, add corresponding tests here.

use genai_rs::{
    Annotation, CodeExecutionLanguage, Content, FunctionCallingMode, InteractionStatus, Resolution,
    Role, SearchType, ServiceTier, Step, ThinkingLevel, ThinkingSummaries, ToolChoice,
};
use serde_json::json;

// =============================================================================
// ThinkingSummaries Wire Format Tests
// CONTEXT-DEPENDENT SERIALIZATION:
// - GenerationConfig: lowercase ("auto", "none") via Serialize impl
// - AgentConfig: SCREAMING_CASE ("THINKING_SUMMARIES_AUTO") via to_agent_config_value()
// - Deserialization: SCREAMING_CASE always (what API returns)
// =============================================================================

mod thinking_summaries {
    use super::*;

    // Serialization for GenerationConfig uses lowercase
    #[test]
    fn auto_serializes_to_lowercase_for_generation_config() {
        let value = ThinkingSummaries::Auto;
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json, "auto");
    }

    #[test]
    fn none_serializes_to_lowercase_for_generation_config() {
        let value = ThinkingSummaries::None;
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json, "none");
    }

    // AgentConfig uses SCREAMING_CASE via to_agent_config_value()
    #[test]
    fn auto_to_agent_config_uses_screaming_case() {
        let value = ThinkingSummaries::Auto;
        assert_eq!(
            value.to_agent_config_value(),
            json!("THINKING_SUMMARIES_AUTO")
        );
    }

    #[test]
    fn none_to_agent_config_uses_screaming_case() {
        let value = ThinkingSummaries::None;
        assert_eq!(
            value.to_agent_config_value(),
            json!("THINKING_SUMMARIES_NONE")
        );
    }

    // Deserialization accepts SCREAMING_CASE (what API returns)
    #[test]
    fn auto_deserializes_from_screaming_case() {
        let json = json!("THINKING_SUMMARIES_AUTO");
        let value: ThinkingSummaries = serde_json::from_value(json).unwrap();
        assert!(matches!(value, ThinkingSummaries::Auto));
    }

    #[test]
    fn none_deserializes_from_screaming_case() {
        let json = json!("THINKING_SUMMARIES_NONE");
        let value: ThinkingSummaries = serde_json::from_value(json).unwrap();
        assert!(matches!(value, ThinkingSummaries::None));
    }

    // Roundtrip with GenerationConfig format
    #[test]
    fn roundtrip_generation_config_format() {
        for variant in [ThinkingSummaries::Auto, ThinkingSummaries::None] {
            let json = serde_json::to_value(&variant).unwrap();
            let back: ThinkingSummaries = serde_json::from_value(json).unwrap();
            assert_eq!(
                std::mem::discriminant(&variant),
                std::mem::discriminant(&back)
            );
        }
    }
}

// =============================================================================
// ThinkingLevel Wire Format Tests
// Per docs: lowercase - "minimal", "low", "medium", "high"
// =============================================================================

mod thinking_level {
    use super::*;

    #[test]
    fn serializes_to_lowercase() {
        assert_eq!(
            serde_json::to_value(ThinkingLevel::Minimal).unwrap(),
            "minimal"
        );
        assert_eq!(serde_json::to_value(ThinkingLevel::Low).unwrap(), "low");
        assert_eq!(
            serde_json::to_value(ThinkingLevel::Medium).unwrap(),
            "medium"
        );
        assert_eq!(serde_json::to_value(ThinkingLevel::High).unwrap(), "high");
    }

    #[test]
    fn deserializes_from_lowercase() {
        assert!(matches!(
            serde_json::from_value::<ThinkingLevel>(json!("minimal")).unwrap(),
            ThinkingLevel::Minimal
        ));
        assert!(matches!(
            serde_json::from_value::<ThinkingLevel>(json!("low")).unwrap(),
            ThinkingLevel::Low
        ));
        assert!(matches!(
            serde_json::from_value::<ThinkingLevel>(json!("medium")).unwrap(),
            ThinkingLevel::Medium
        ));
        assert!(matches!(
            serde_json::from_value::<ThinkingLevel>(json!("high")).unwrap(),
            ThinkingLevel::High
        ));
    }

    #[test]
    fn roundtrip_all_variants() {
        for variant in [
            ThinkingLevel::Minimal,
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
        ] {
            let json = serde_json::to_value(&variant).unwrap();
            let back: ThinkingLevel = serde_json::from_value(json).unwrap();
            assert_eq!(
                std::mem::discriminant(&variant),
                std::mem::discriminant(&back)
            );
        }
    }
}

// =============================================================================
// FunctionCallingMode Wire Format Tests
// Revision 2026-05-20: lowercase - "auto", "any", "none", "validated".
// Legacy SCREAMING_CASE is still accepted on deserialization.
// =============================================================================

mod function_calling_mode {
    use super::*;

    #[test]
    fn serializes_to_lowercase() {
        assert_eq!(
            serde_json::to_value(FunctionCallingMode::Auto).unwrap(),
            "auto"
        );
        assert_eq!(
            serde_json::to_value(FunctionCallingMode::Any).unwrap(),
            "any"
        );
        assert_eq!(
            serde_json::to_value(FunctionCallingMode::None).unwrap(),
            "none"
        );
        assert_eq!(
            serde_json::to_value(FunctionCallingMode::Validated).unwrap(),
            "validated"
        );
    }

    #[test]
    fn deserializes_from_lowercase() {
        assert!(matches!(
            serde_json::from_value::<FunctionCallingMode>(json!("auto")).unwrap(),
            FunctionCallingMode::Auto
        ));
        assert!(matches!(
            serde_json::from_value::<FunctionCallingMode>(json!("any")).unwrap(),
            FunctionCallingMode::Any
        ));
        assert!(matches!(
            serde_json::from_value::<FunctionCallingMode>(json!("none")).unwrap(),
            FunctionCallingMode::None
        ));
        assert!(matches!(
            serde_json::from_value::<FunctionCallingMode>(json!("validated")).unwrap(),
            FunctionCallingMode::Validated
        ));
    }

    #[test]
    fn deserializes_from_legacy_screaming_case() {
        assert!(matches!(
            serde_json::from_value::<FunctionCallingMode>(json!("AUTO")).unwrap(),
            FunctionCallingMode::Auto
        ));
        assert!(matches!(
            serde_json::from_value::<FunctionCallingMode>(json!("ANY")).unwrap(),
            FunctionCallingMode::Any
        ));
        assert!(matches!(
            serde_json::from_value::<FunctionCallingMode>(json!("NONE")).unwrap(),
            FunctionCallingMode::None
        ));
        assert!(matches!(
            serde_json::from_value::<FunctionCallingMode>(json!("VALIDATED")).unwrap(),
            FunctionCallingMode::Validated
        ));
    }

    #[test]
    fn roundtrip_all_variants() {
        for variant in [
            FunctionCallingMode::Auto,
            FunctionCallingMode::Any,
            FunctionCallingMode::None,
            FunctionCallingMode::Validated,
        ] {
            let json = serde_json::to_value(&variant).unwrap();
            let back: FunctionCallingMode = serde_json::from_value(json).unwrap();
            assert_eq!(
                std::mem::discriminant(&variant),
                std::mem::discriminant(&back)
            );
        }
    }
}

// =============================================================================
// ToolChoice Wire Format Tests
// Union: plain lowercase string (mode) OR {"allowed_tools": {"mode", "tools"}}
// =============================================================================

mod tool_choice {
    use super::*;

    #[test]
    fn mode_serializes_as_plain_string() {
        let choice = ToolChoice::Mode(FunctionCallingMode::Auto);
        assert_eq!(serde_json::to_value(&choice).unwrap(), json!("auto"));

        let choice = ToolChoice::Mode(FunctionCallingMode::Validated);
        assert_eq!(serde_json::to_value(&choice).unwrap(), json!("validated"));
    }

    #[test]
    fn allowed_tools_serializes_as_object() {
        let choice = ToolChoice::allowed_tools(
            Some(FunctionCallingMode::Any),
            vec!["get_weather".to_string(), "get_time".to_string()],
        );
        let json = serde_json::to_value(&choice).unwrap();
        assert_eq!(
            json,
            json!({
                "allowed_tools": {
                    "mode": "any",
                    "tools": ["get_weather", "get_time"]
                }
            })
        );
    }

    #[test]
    fn allowed_tools_without_mode_omits_mode_key() {
        let choice = ToolChoice::allowed_tools(None, vec!["get_weather".to_string()]);
        let json = serde_json::to_value(&choice).unwrap();
        assert_eq!(json, json!({"allowed_tools": {"tools": ["get_weather"]}}));
    }

    #[test]
    fn deserializes_string_as_mode() {
        let choice: ToolChoice = serde_json::from_value(json!("any")).unwrap();
        assert!(matches!(choice, ToolChoice::Mode(FunctionCallingMode::Any)));
    }

    #[test]
    fn deserializes_object_as_allowed_tools() {
        let choice: ToolChoice = serde_json::from_value(json!({
            "allowed_tools": {"mode": "auto", "tools": ["a", "b"]}
        }))
        .unwrap();
        match choice {
            ToolChoice::AllowedTools(allowed) => {
                assert!(matches!(allowed.mode, Some(FunctionCallingMode::Auto)));
                assert_eq!(allowed.tools, vec!["a".to_string(), "b".to_string()]);
            }
            other => panic!("Expected AllowedTools, got {:?}", other),
        }
    }

    #[test]
    fn roundtrip_both_shapes() {
        for choice in [
            ToolChoice::Mode(FunctionCallingMode::None),
            ToolChoice::allowed_tools(
                Some(FunctionCallingMode::Any),
                vec!["get_weather".to_string()],
            ),
        ] {
            let json = serde_json::to_value(&choice).unwrap();
            let back: ToolChoice = serde_json::from_value(json).unwrap();
            assert_eq!(choice, back);
        }
    }
}

// =============================================================================
// ServiceTier Wire Format Tests
// Per docs: lowercase - "flex", "standard", "priority"
// =============================================================================

mod service_tier {
    use super::*;

    #[test]
    fn serializes_to_lowercase() {
        assert_eq!(serde_json::to_value(ServiceTier::Flex).unwrap(), "flex");
        assert_eq!(
            serde_json::to_value(ServiceTier::Standard).unwrap(),
            "standard"
        );
        assert_eq!(
            serde_json::to_value(ServiceTier::Priority).unwrap(),
            "priority"
        );
    }

    #[test]
    fn deserializes_from_lowercase() {
        assert!(matches!(
            serde_json::from_value::<ServiceTier>(json!("flex")).unwrap(),
            ServiceTier::Flex
        ));
        assert!(matches!(
            serde_json::from_value::<ServiceTier>(json!("standard")).unwrap(),
            ServiceTier::Standard
        ));
        assert!(matches!(
            serde_json::from_value::<ServiceTier>(json!("priority")).unwrap(),
            ServiceTier::Priority
        ));
    }

    #[test]
    fn roundtrip_all_variants() {
        for variant in [
            ServiceTier::Flex,
            ServiceTier::Standard,
            ServiceTier::Priority,
        ] {
            let json = serde_json::to_value(&variant).unwrap();
            let back: ServiceTier = serde_json::from_value(json).unwrap();
            assert_eq!(variant, back);
        }
    }
}

// =============================================================================
// SearchType Wire Format Tests
// Per docs: snake_case - "web_search", "image_search", "enterprise_web_search"
// =============================================================================

mod search_type {
    use super::*;

    #[test]
    fn serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_value(SearchType::WebSearch).unwrap(),
            "web_search"
        );
        assert_eq!(
            serde_json::to_value(SearchType::ImageSearch).unwrap(),
            "image_search"
        );
        assert_eq!(
            serde_json::to_value(SearchType::EnterpriseWebSearch).unwrap(),
            "enterprise_web_search"
        );
    }

    #[test]
    fn deserializes_from_snake_case() {
        assert!(matches!(
            serde_json::from_value::<SearchType>(json!("web_search")).unwrap(),
            SearchType::WebSearch
        ));
        assert!(matches!(
            serde_json::from_value::<SearchType>(json!("image_search")).unwrap(),
            SearchType::ImageSearch
        ));
        assert!(matches!(
            serde_json::from_value::<SearchType>(json!("enterprise_web_search")).unwrap(),
            SearchType::EnterpriseWebSearch
        ));
    }

    #[test]
    fn roundtrip_all_variants() {
        for variant in [
            SearchType::WebSearch,
            SearchType::ImageSearch,
            SearchType::EnterpriseWebSearch,
        ] {
            let json = serde_json::to_value(&variant).unwrap();
            let back: SearchType = serde_json::from_value(json).unwrap();
            assert_eq!(variant, back);
        }
    }
}

// =============================================================================
// CodeExecutionLanguage Wire Format Tests
// Revision 2026-05-20: lowercase "python"; legacy "PYTHON" accepted on read.
// =============================================================================

mod code_execution_language {
    use super::*;

    #[test]
    fn serializes_to_lowercase() {
        assert_eq!(
            serde_json::to_value(CodeExecutionLanguage::Python).unwrap(),
            "python"
        );
    }

    #[test]
    fn deserializes_from_lowercase() {
        assert!(matches!(
            serde_json::from_value::<CodeExecutionLanguage>(json!("python")).unwrap(),
            CodeExecutionLanguage::Python
        ));
    }

    #[test]
    fn deserializes_from_legacy_uppercase() {
        assert!(matches!(
            serde_json::from_value::<CodeExecutionLanguage>(json!("PYTHON")).unwrap(),
            CodeExecutionLanguage::Python
        ));
    }

    #[test]
    fn display_prints_lowercase() {
        assert_eq!(CodeExecutionLanguage::Python.to_string(), "python");
    }
}

// =============================================================================
// InteractionStatus Wire Format Tests
// Per docs: snake_case - "completed", "in_progress", "requires_action",
// "failed", "cancelled", "budget_exceeded"
// =============================================================================

mod interaction_status {
    use super::*;

    #[test]
    fn deserializes_from_snake_case() {
        assert!(matches!(
            serde_json::from_value::<InteractionStatus>(json!("completed")).unwrap(),
            InteractionStatus::Completed
        ));
        assert!(matches!(
            serde_json::from_value::<InteractionStatus>(json!("in_progress")).unwrap(),
            InteractionStatus::InProgress
        ));
        assert!(matches!(
            serde_json::from_value::<InteractionStatus>(json!("requires_action")).unwrap(),
            InteractionStatus::RequiresAction
        ));
        assert!(matches!(
            serde_json::from_value::<InteractionStatus>(json!("failed")).unwrap(),
            InteractionStatus::Failed
        ));
        assert!(matches!(
            serde_json::from_value::<InteractionStatus>(json!("cancelled")).unwrap(),
            InteractionStatus::Cancelled
        ));
        assert!(matches!(
            serde_json::from_value::<InteractionStatus>(json!("budget_exceeded")).unwrap(),
            InteractionStatus::BudgetExceeded
        ));
    }

    #[test]
    fn serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_value(InteractionStatus::Completed).unwrap(),
            "completed"
        );
        assert_eq!(
            serde_json::to_value(InteractionStatus::InProgress).unwrap(),
            "in_progress"
        );
        assert_eq!(
            serde_json::to_value(InteractionStatus::RequiresAction).unwrap(),
            "requires_action"
        );
        assert_eq!(
            serde_json::to_value(InteractionStatus::Failed).unwrap(),
            "failed"
        );
        assert_eq!(
            serde_json::to_value(InteractionStatus::Cancelled).unwrap(),
            "cancelled"
        );
        assert_eq!(
            serde_json::to_value(InteractionStatus::BudgetExceeded).unwrap(),
            "budget_exceeded"
        );
    }

    #[test]
    fn default_is_in_progress() {
        assert_eq!(InteractionStatus::default(), InteractionStatus::InProgress);
    }
}

// =============================================================================
// Resolution Wire Format Tests
// Per docs: snake_case - "low", "medium", "high", "ultra_high"
// =============================================================================

mod resolution {
    use super::*;

    #[test]
    fn serializes_to_snake_case() {
        assert_eq!(serde_json::to_value(Resolution::Low).unwrap(), "low");
        assert_eq!(serde_json::to_value(Resolution::Medium).unwrap(), "medium");
        assert_eq!(serde_json::to_value(Resolution::High).unwrap(), "high");
        assert_eq!(
            serde_json::to_value(Resolution::UltraHigh).unwrap(),
            "ultra_high"
        );
    }

    #[test]
    fn deserializes_from_snake_case() {
        assert!(matches!(
            serde_json::from_value::<Resolution>(json!("low")).unwrap(),
            Resolution::Low
        ));
        assert!(matches!(
            serde_json::from_value::<Resolution>(json!("medium")).unwrap(),
            Resolution::Medium
        ));
        assert!(matches!(
            serde_json::from_value::<Resolution>(json!("high")).unwrap(),
            Resolution::High
        ));
        assert!(matches!(
            serde_json::from_value::<Resolution>(json!("ultra_high")).unwrap(),
            Resolution::UltraHigh
        ));
    }

    #[test]
    fn roundtrip_all_variants() {
        for variant in [
            Resolution::Low,
            Resolution::Medium,
            Resolution::High,
            Resolution::UltraHigh,
        ] {
            let json = serde_json::to_value(&variant).unwrap();
            let back: Resolution = serde_json::from_value(json).unwrap();
            assert_eq!(
                std::mem::discriminant(&variant),
                std::mem::discriminant(&back)
            );
        }
    }
}

// =============================================================================
// Role Wire Format Tests
// Per docs: lowercase - "user", "model"
// =============================================================================

mod role {
    use super::*;

    #[test]
    fn serializes_to_lowercase() {
        assert_eq!(serde_json::to_value(Role::User).unwrap(), "user");
        assert_eq!(serde_json::to_value(Role::Model).unwrap(), "model");
    }

    #[test]
    fn deserializes_from_lowercase() {
        assert!(matches!(
            serde_json::from_value::<Role>(json!("user")).unwrap(),
            Role::User
        ));
        assert!(matches!(
            serde_json::from_value::<Role>(json!("model")).unwrap(),
            Role::Model
        ));
    }

    #[test]
    fn roundtrip_all_variants() {
        for variant in [Role::User, Role::Model] {
            let json = serde_json::to_value(&variant).unwrap();
            let back: Role = serde_json::from_value(json).unwrap();
            assert_eq!(
                std::mem::discriminant(&variant),
                std::mem::discriminant(&back)
            );
        }
    }
}

// =============================================================================
// Content Wire Format Tests
// Per docs: snake_case type field - "text", "image", "audio", "video",
// "document". Tool calls/results are Steps in revision 2026-05-20.
// =============================================================================

mod interaction_content {
    use super::*;

    #[test]
    fn text_uses_snake_case_type() {
        let content = Content::Text {
            text: Some("hello".to_string()),
            annotations: None,
        };
        let json = serde_json::to_value(&content).unwrap();
        assert_eq!(json["type"], "text");
    }

    #[test]
    fn image_uses_snake_case_type() {
        let content = Content::Image {
            data: None,
            uri: Some("gs://bucket/image.png".to_string()),
            mime_type: Some("image/png".to_string()),
            resolution: None,
        };
        let json = serde_json::to_value(&content).unwrap();
        assert_eq!(json["type"], "image");
    }

    #[test]
    fn audio_uses_snake_case_type_with_sample_rate_and_channels() {
        let content = Content::Audio {
            data: None,
            uri: Some("gs://bucket/audio.mp3".to_string()),
            mime_type: Some("audio/mpeg".to_string()),
            sample_rate: Some(24000),
            channels: Some(1),
        };
        let json = serde_json::to_value(&content).unwrap();
        assert_eq!(json["type"], "audio");
        assert_eq!(json["sample_rate"], 24000);
        assert_eq!(json["channels"], 1);
    }

    #[test]
    fn video_uses_snake_case_type() {
        let content = Content::Video {
            data: None,
            uri: Some("gs://bucket/video.mp4".to_string()),
            mime_type: Some("video/mp4".to_string()),
            resolution: None,
        };
        let json = serde_json::to_value(&content).unwrap();
        assert_eq!(json["type"], "video");
    }

    #[test]
    fn document_uses_snake_case_type() {
        let content = Content::Document {
            data: None,
            uri: Some("gs://bucket/doc.pdf".to_string()),
            mime_type: Some("application/pdf".to_string()),
        };
        let json = serde_json::to_value(&content).unwrap();
        assert_eq!(json["type"], "document");
    }
}

// =============================================================================
// Step Wire Format Tests
// Per docs: snake_case type tag; call steps nest string-array arguments under
// an "arguments" object; function_call keeps id/name/arguments at top level.
// =============================================================================

mod step {
    use super::*;

    #[test]
    fn user_input_wire_format() {
        let step = Step::user_text("hi");
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["type"], "user_input");
        assert_eq!(json["content"][0]["type"], "text");
        assert_eq!(json["content"][0]["text"], "hi");
    }

    #[test]
    fn model_output_wire_format() {
        let step = Step::model_text("hello");
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["type"], "model_output");
        assert_eq!(json["content"][0]["text"], "hello");
    }

    #[test]
    fn thought_wire_format() {
        let step = Step::Thought {
            signature: Some("sig123".to_string()),
            summary: vec![],
        };
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["type"], "thought");
        assert_eq!(json["signature"], "sig123");
    }

    #[test]
    fn function_call_keeps_arguments_at_top_level() {
        let step = Step::function_call("call_1", "get_weather", json!({"city": "Paris"}));
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["type"], "function_call");
        assert_eq!(json["id"], "call_1");
        assert_eq!(json["name"], "get_weather");
        assert_eq!(json["arguments"]["city"], "Paris");
    }

    #[test]
    fn function_result_wire_format() {
        let step = Step::function_result("get_weather", "call_1", json!({"temp": 22}));
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["type"], "function_result");
        assert_eq!(json["call_id"], "call_1");
        assert_eq!(json["name"], "get_weather");
        assert_eq!(json["result"]["temp"], 22);
    }

    #[test]
    fn code_execution_call_nests_language_and_code_in_arguments() {
        let step = Step::CodeExecutionCall {
            id: "exec_1".to_string(),
            language: CodeExecutionLanguage::Python,
            code: "print(1)".to_string(),
            signature: None,
        };
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["type"], "code_execution_call");
        assert_eq!(json["id"], "exec_1");
        assert_eq!(json["arguments"]["language"], "python");
        assert_eq!(json["arguments"]["code"], "print(1)");
    }

    #[test]
    fn url_context_call_nests_urls_in_arguments() {
        let step = Step::UrlContextCall {
            id: "ctx_1".to_string(),
            urls: vec!["https://example.com".to_string()],
            signature: None,
        };
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["type"], "url_context_call");
        assert_eq!(json["arguments"]["urls"][0], "https://example.com");
    }

    #[test]
    fn google_search_call_nests_queries_in_arguments() {
        let step = Step::GoogleSearchCall {
            id: "search_1".to_string(),
            queries: vec!["rust serde".to_string()],
            search_type: Some(SearchType::WebSearch),
            signature: None,
        };
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["type"], "google_search_call");
        assert_eq!(json["arguments"]["queries"][0], "rust serde");
        assert_eq!(json["search_type"], "web_search");
    }

    #[test]
    fn google_maps_call_nests_queries_in_arguments() {
        let step = Step::GoogleMapsCall {
            id: "maps_1".to_string(),
            queries: vec!["coffee".to_string()],
            signature: Some("sig".to_string()),
        };
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["type"], "google_maps_call");
        assert_eq!(json["arguments"]["queries"][0], "coffee");
        assert_eq!(json["signature"], "sig");
    }

    #[test]
    fn roundtrip_representative_steps() {
        let steps = vec![
            Step::user_text("hi"),
            Step::model_text("hello"),
            Step::Thought {
                signature: Some("sig".to_string()),
                summary: vec![],
            },
            Step::function_call("call_1", "fn", json!({"a": 1})),
            Step::function_result("fn", "call_1", json!({"b": 2})),
            Step::CodeExecutionCall {
                id: "exec_1".to_string(),
                language: CodeExecutionLanguage::Python,
                code: "print(1)".to_string(),
                signature: None,
            },
            Step::CodeExecutionResult {
                call_id: "exec_1".to_string(),
                result: "1".to_string(),
                is_error: false,
                signature: None,
            },
            Step::UrlContextCall {
                id: "ctx_1".to_string(),
                urls: vec!["https://example.com".to_string()],
                signature: None,
            },
            Step::GoogleSearchCall {
                id: "search_1".to_string(),
                queries: vec!["q".to_string()],
                search_type: Some(SearchType::WebSearch),
                signature: None,
            },
        ];

        for step in steps {
            let json = serde_json::to_value(&step).unwrap();
            let back: Step = serde_json::from_value(json).unwrap();
            assert_eq!(step, back, "Step should roundtrip losslessly");
        }
    }
}

// =============================================================================
// Annotation Wire Format Tests
// Tagged union: "url_citation", "file_citation", "place_citation"
// =============================================================================

mod annotation {
    use super::*;

    #[test]
    fn url_citation_wire_format() {
        let annotation =
            Annotation::url_citation("https://example.com", Some("Example".to_string()), 0, 5);
        let json = serde_json::to_value(&annotation).unwrap();
        assert_eq!(json["type"], "url_citation");
        assert_eq!(json["url"], "https://example.com");
        assert_eq!(json["title"], "Example");
        assert_eq!(json["start_index"], 0);
        assert_eq!(json["end_index"], 5);
    }

    #[test]
    fn url_citation_deserializes() {
        let annotation: Annotation = serde_json::from_value(json!({
            "type": "url_citation",
            "url": "https://example.com",
            "title": "Example",
            "start_index": 3,
            "end_index": 9
        }))
        .unwrap();
        assert!(matches!(annotation, Annotation::UrlCitation { .. }));
        assert_eq!(annotation.start_index(), Some(3));
        assert_eq!(annotation.end_index(), Some(9));
        assert_eq!(annotation.source(), Some("https://example.com"));
    }

    #[test]
    fn file_citation_deserializes() {
        let annotation: Annotation = serde_json::from_value(json!({
            "type": "file_citation",
            "document_uri": "files/doc123",
            "file_name": "report.pdf",
            "page_number": 4,
            "start_index": 0,
            "end_index": 10
        }))
        .unwrap();
        assert!(matches!(annotation, Annotation::FileCitation { .. }));
        assert_eq!(annotation.source(), Some("files/doc123"));
    }

    #[test]
    fn place_citation_deserializes() {
        let annotation: Annotation = serde_json::from_value(json!({
            "type": "place_citation",
            "place_id": "abc123",
            "name": "Central Park",
            "url": "https://maps.example.com/abc123",
            "review_snippets": [],
            "start_index": 0,
            "end_index": 12
        }))
        .unwrap();
        assert!(matches!(annotation, Annotation::PlaceCitation { .. }));
        assert_eq!(annotation.source(), Some("https://maps.example.com/abc123"));
    }

    #[test]
    fn annotation_roundtrips() {
        let annotation =
            Annotation::url_citation("https://example.com", Some("T".to_string()), 1, 4);
        let json = serde_json::to_value(&annotation).unwrap();
        let back: Annotation = serde_json::from_value(json).unwrap();
        assert_eq!(annotation, back);
    }

    #[test]
    fn extract_span_uses_indices() {
        let annotation = Annotation::url_citation("https://example.com", None, 0, 5);
        assert_eq!(annotation.extract_span("Hello, world!"), Some("Hello"));
    }
}
