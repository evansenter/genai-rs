//! Wire format verification tests
//!
//! These tests verify that our serialization/deserialization matches the wire formats
//! documented in `docs/ENUM_WIRE_FORMATS.md`.
//!
//! Each test corresponds to a documented wire format and ensures:
//! 1. Deserialization from wire format works correctly
//! 2. Serialization produces the correct wire format
//! 3. Roundtrip works for all variants
//!
//! When adding new enums or updating wire formats, add corresponding tests here.

use genai_rs::{
    Content, FunctionCallingMode, InteractionStatus, Resolution, Role, ThinkingLevel,
    ThinkingSummaries,
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
// Per docs: SCREAMING_CASE - "AUTO", "ANY", "NONE", "VALIDATED"
// =============================================================================

mod function_calling_mode {
    use super::*;

    #[test]
    fn serializes_to_screaming_case() {
        assert_eq!(
            serde_json::to_value(FunctionCallingMode::Auto).unwrap(),
            "AUTO"
        );
        assert_eq!(
            serde_json::to_value(FunctionCallingMode::Any).unwrap(),
            "ANY"
        );
        assert_eq!(
            serde_json::to_value(FunctionCallingMode::None).unwrap(),
            "NONE"
        );
        assert_eq!(
            serde_json::to_value(FunctionCallingMode::Validated).unwrap(),
            "VALIDATED"
        );
    }

    #[test]
    fn deserializes_from_screaming_case() {
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
// InteractionStatus Wire Format Tests
// Per docs: snake_case - "completed", "in_progress", "requires_action", etc.
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
// Per docs: snake_case type field - "text", "function_call", etc.
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
    fn function_call_uses_snake_case_type() {
        let content = Content::FunctionCall {
            id: Some("call_123".to_string()),
            name: "test".to_string(),
            args: json!({}),
            partial_args: None,
            will_continue: None,
        };
        let json = serde_json::to_value(&content).unwrap();
        assert_eq!(json["type"], "function_call");
    }

    #[test]
    fn function_result_uses_snake_case_type() {
        let content = Content::FunctionResult {
            call_id: "call_123".to_string(),
            name: Some("test".to_string()),
            result: json!({"output": "result"}),
            is_error: None,
        };
        let json = serde_json::to_value(&content).unwrap();
        assert_eq!(json["type"], "function_result");
    }

    #[test]
    fn thought_uses_snake_case_type() {
        let content = Content::Thought {
            signature: Some("Eq0JCqoJ...signature".to_string()),
        };
        let json = serde_json::to_value(&content).unwrap();
        assert_eq!(json["type"], "thought");
        assert_eq!(json["signature"], "Eq0JCqoJ...signature");
    }

    #[test]
    fn code_execution_call_uses_snake_case_type() {
        let content = Content::CodeExecutionCall {
            id: Some("exec_123".to_string()),
            language: genai_rs::CodeExecutionLanguage::Python,
            code: "print('hello')".to_string(),
        };
        let json = serde_json::to_value(&content).unwrap();
        assert_eq!(json["type"], "code_execution_call");
    }

    #[test]
    fn code_execution_result_uses_snake_case_type() {
        let content = Content::CodeExecutionResult {
            call_id: Some("exec_123".to_string()),
            is_error: false,
            result: "hello".to_string(),
        };
        let json = serde_json::to_value(&content).unwrap();
        assert_eq!(json["type"], "code_execution_result");
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
    fn audio_uses_snake_case_type() {
        let content = Content::Audio {
            data: None,
            uri: Some("gs://bucket/audio.mp3".to_string()),
            mime_type: Some("audio/mpeg".to_string()),
        };
        let json = serde_json::to_value(&content).unwrap();
        assert_eq!(json["type"], "audio");
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
