//! Fully populated values for the request side: every `Content` variant
//! and annotation, every history `Step` a caller sends back, and every
//! `Tool`. Every `extra` and `arguments_extra` stays empty: they hold the
//! unmodeled fields a response carried, which history replays but the crate
//! does not emit (`a_populated_variant_extra_is_sent_flat_and_not_vouched_for`).

use std::collections::HashMap;

use serde_json::{Map, json};

use crate::content::{Annotation, CodeExecutionLanguage, Content, Resolution, VideoProcessing};
use crate::steps::{FunctionResultPayload, Step};
use crate::tools::{
    AllowedTools, ExaAiSearchConfig, FunctionCallingMode, FunctionParameters, HybridSearchConfig,
    ParallelAiSearchConfig, RagFilter, RagRanking, RagResource, RagRetrievalConfig, RagStoreConfig,
    RankService, RetrievalType, SearchType, Tool, VertexAiSearchConfig,
};

fn annotations() -> Vec<Annotation> {
    vec![
        Annotation::UrlCitation {
            url: Some("https://example.com".into()),
            title: Some("t".into()),
            start_index: 0,
            end_index: 1,
            extra: Map::new(),
        },
        Annotation::FileCitation {
            document_uri: Some("https://example.com/doc".into()),
            file_name: Some("f".into()),
            source: Some("s".into()),
            custom_metadata: Some(json!({"k": "v"})),
            page_number: Some(1),
            media_id: Some("m".into()),
            start_index: 0,
            end_index: 1,
            extra: Map::new(),
        },
        Annotation::PlaceCitation {
            place_id: Some("p".into()),
            name: Some("n".into()),
            url: Some("https://example.com".into()),
            review_snippets: vec![crate::content::ReviewSnippet {
                title: Some("t".into()),
                url: Some("https://example.com".into()),
                review_id: Some("r".into()),
                extra: Map::new(),
            }],
            start_index: 0,
            end_index: 1,
            extra: Map::new(),
        },
        Annotation::SpeechMetadata {
            speaker: Some("Joe".into()),
            style: Some("calm".into()),
            start_index: Some(0),
            end_index: Some(1),
            extra: Map::new(),
        },
        Annotation::WordInfo {
            text: Some("w".into()),
            speaker: Some("Joe".into()),
            start_offset: Some("0s".into()),
            end_offset: Some("1s".into()),
            start_index: Some(0),
            end_index: Some(1),
            extra: Map::new(),
        },
    ]
}

/// Every `Content` variant, every field set. Matching on each one keeps a
/// new variant from going unchecked.
pub(super) fn contents() -> Vec<Content> {
    let all = vec![
        Content::Text {
            text: Some("hello".into()),
            annotations: Some(annotations()),
            extra: Map::new(),
        },
        Content::Image {
            data: Some("AAAA".into()),
            uri: Some("https://example.com/i.png".into()),
            mime_type: Some("image/png".into()),
            resolution: Some(Resolution::High),
            extra: Map::new(),
        },
        Content::Audio {
            data: Some("AAAA".into()),
            uri: Some("https://example.com/a.wav".into()),
            mime_type: Some("audio/wav".into()),
            sample_rate: Some(24_000),
            channels: Some(1),
            extra: Map::new(),
        },
        Content::Video {
            data: Some("AAAA".into()),
            uri: Some("https://example.com/v.mp4".into()),
            mime_type: Some("video/mp4".into()),
            resolution: Some(Resolution::Low),
            processing: Some(
                VideoProcessing::segment()
                    .with_start_offset("1s")
                    .with_end_offset("2s")
                    .with_fps(2.0)
                    .build(),
            ),
            name: Some("clip.mp4".into()),
            extra: Map::new(),
        },
        Content::Document {
            data: Some("AAAA".into()),
            uri: Some("https://example.com/d.pdf".into()),
            mime_type: Some("application/pdf".into()),
            extra: Map::new(),
        },
    ];
    for content in &all {
        match content {
            Content::Text { .. }
            | Content::Image { .. }
            | Content::Audio { .. }
            | Content::Video { .. }
            | Content::Document { .. }
            | Content::Unknown { .. } => {}
        }
    }
    let mut out = all;
    for processing in [VideoProcessing::Static, VideoProcessing::Agentic] {
        out.push(Content::Video {
            data: None,
            uri: Some("https://example.com/v.mp4".into()),
            mime_type: Some("video/mp4".into()),
            resolution: None,
            processing: Some(processing),
            name: None,
            extra: Map::new(),
        });
    }
    out
}

/// History steps a caller sends back as input: every variant a response can
/// carry, since `InteractionInput::Steps` echoes them.
pub(super) fn steps() -> Vec<Step> {
    let payloads = [
        FunctionResultPayload::Text("ok".into()),
        FunctionResultPayload::Json(json!({"k": "v"})),
        FunctionResultPayload::Contents(vec![Content::text("ok")]),
    ];
    let mut all = vec![
        Step::UserInput {
            content: contents(),
            extra: Map::new(),
        },
        Step::ModelOutput {
            content: contents(),
            error: Some(crate::steps::StepError {
                code: Some(3),
                message: Some("bad".into()),
                details: Some(vec![
                    json!({"@type": "type.googleapis.com/google.rpc.ErrorInfo"}),
                ]),
                extra: Map::new(),
            }),
            extra: Map::new(),
        },
        Step::Thought {
            signature: Some("c2ln".into()),
            summary: vec![Content::text("thinking")],
            extra: Map::new(),
        },
        Step::FunctionCall {
            id: "c1".into(),
            name: "f".into(),
            arguments: json!({"a": 1}),
            signature: Some("c2ln".into()),
            extra: Map::new(),
        },
        Step::CodeExecutionCall {
            id: "c2".into(),
            language: CodeExecutionLanguage::Python,
            code: "print(1)".into(),
            signature: Some("c2ln".into()),
            arguments_extra: Map::new(),
            extra: Map::new(),
        },
        Step::CodeExecutionResult {
            call_id: "c2".into(),
            result: "1".into(),
            is_error: true,
            signature: Some("c2ln".into()),
            extra: Map::new(),
        },
        Step::UrlContextCall {
            id: "c3".into(),
            urls: vec!["https://example.com".into()],
            signature: Some("c2ln".into()),
            arguments_extra: Map::new(),
            extra: Map::new(),
        },
        Step::UrlContextResult {
            call_id: "c3".into(),
            result: vec![crate::content::UrlContextResultItem {
                url: "https://example.com".into(),
                status: "success".into(),
                extra: Map::new(),
            }],
            is_error: Some(false),
            signature: Some("c2ln".into()),
            extra: Map::new(),
        },
        Step::GoogleSearchCall {
            id: "c4".into(),
            queries: vec!["q".into()],
            search_type: Some(SearchType::WebSearch),
            signature: Some("c2ln".into()),
            arguments_extra: Map::new(),
            extra: Map::new(),
        },
        Step::GoogleSearchResult {
            call_id: "c4".into(),
            result: vec![crate::content::GoogleSearchResultItem {
                search_suggestions: Some("<div/>".into()),
                extra: Map::new(),
            }],
            is_error: Some(false),
            signature: Some("c2ln".into()),
            extra: Map::new(),
        },
        Step::McpServerToolCall {
            id: "c5".into(),
            name: "tool".into(),
            server_name: "server".into(),
            arguments: json!({"a": 1}),
            extra: Map::new(),
        },
        Step::FileSearchCall {
            id: "c6".into(),
            signature: Some("c2ln".into()),
            extra: Map::new(),
        },
        Step::FileSearchResult {
            call_id: "c6".into(),
            signature: Some("c2ln".into()),
            extra: Map::new(),
        },
        Step::GoogleMapsCall {
            id: "c7".into(),
            queries: vec!["q".into()],
            signature: Some("c2ln".into()),
            arguments_extra: Map::new(),
            extra: Map::new(),
        },
        Step::GoogleMapsResult {
            call_id: "c7".into(),
            result: vec![crate::content::GoogleMapsResultItem {
                places: Some(vec![crate::content::Place {
                    name: Some("n".into()),
                    place_id: Some("p".into()),
                    url: Some("https://example.com".into()),
                    review_snippets: Some(vec![crate::content::ReviewSnippet {
                        title: Some("t".into()),
                        url: Some("https://example.com".into()),
                        review_id: Some("r".into()),
                        extra: Map::new(),
                    }]),
                    extra: Map::new(),
                }]),
                widget_context_token: Some("w".into()),
                extra: Map::new(),
            }],
            signature: Some("c2ln".into()),
            extra: Map::new(),
        },
        Step::ProcessingCall {
            id: "c8".into(),
            signature: Some("c2ln".into()),
            extra: Map::new(),
        },
        Step::ProcessingResult {
            call_id: "c8".into(),
            signature: Some("c2ln".into()),
            extra: Map::new(),
        },
        Step::RetrievalCall {
            id: "c9".into(),
            queries: vec!["q".into()],
            retrieval_type: Some(RetrievalType::RagStore),
            signature: Some("c2ln".into()),
            arguments_extra: Map::new(),
            extra: Map::new(),
        },
        Step::RetrievalResult {
            call_id: "c9".into(),
            is_error: Some(false),
            signature: Some("c2ln".into()),
            extra: Map::new(),
        },
    ];
    for result in payloads {
        all.push(Step::FunctionResult {
            call_id: "c1".into(),
            name: Some("f".into()),
            result: result.clone(),
            is_error: Some(false),
            signature: Some("c2ln".into()),
            extra: Map::new(),
        });
        all.push(Step::McpServerToolResult {
            call_id: "c5".into(),
            name: Some("tool".into()),
            server_name: Some("server".into()),
            result,
            extra: Map::new(),
        });
    }
    // The list arm of the code execution result (google-genai 3.0.0).
    all.push(Step::CodeExecutionResult {
        call_id: "c2".into(),
        result: crate::CodeExecutionOutput::Contents(contents()),
        is_error: false,
        signature: Some("c2ln".into()),
        extra: Map::new(),
    });
    for step in &all {
        match step {
            Step::UserInput { .. }
            | Step::ModelOutput { .. }
            | Step::Thought { .. }
            | Step::FunctionCall { .. }
            | Step::FunctionResult { .. }
            | Step::CodeExecutionCall { .. }
            | Step::CodeExecutionResult { .. }
            | Step::UrlContextCall { .. }
            | Step::UrlContextResult { .. }
            | Step::GoogleSearchCall { .. }
            | Step::GoogleSearchResult { .. }
            | Step::ToolCall { .. }
            | Step::McpServerToolCall { .. }
            | Step::McpServerToolResult { .. }
            | Step::FileSearchCall { .. }
            | Step::FileSearchResult { .. }
            | Step::GoogleMapsCall { .. }
            | Step::GoogleMapsResult { .. }
            | Step::ProcessingCall { .. }
            | Step::ProcessingResult { .. }
            | Step::RetrievalCall { .. }
            | Step::RetrievalResult { .. }
            | Step::Unknown { .. } => {}
        }
    }
    all
}

fn rag_store_config() -> RagStoreConfig {
    RagStoreConfig {
        rag_resources: Some(vec![RagResource {
            rag_corpus: Some("projects/p/locations/l/ragCorpora/c".into()),
            rag_file_ids: Some(vec!["f".into()]),
        }]),
        similarity_top_k: Some(3),
        vector_distance_threshold: Some(0.5),
        rag_retrieval_config: Some(RagRetrievalConfig {
            top_k: Some(3),
            hybrid_search: Some(HybridSearchConfig { alpha: Some(0.5) }),
            filter: Some(RagFilter {
                vector_distance_threshold: Some(0.5),
                vector_similarity_threshold: Some(0.5),
                metadata_filter: Some("a = 1".into()),
            }),
            ranking: Some(RagRanking {
                ranking_config: "rank_service".into(),
                model_name: Some("m".into()),
                rank_service: Some(RankService {
                    model_name: Some("m".into()),
                    extra: serde_json::Map::new(),
                }),
            }),
        }),
    }
}

/// Every `Tool` variant, every field set.
pub(super) fn tools() -> Vec<Tool> {
    let all = vec![
        Tool::Function {
            name: "get_weather".into(),
            description: Some("Gets the weather".into()),
            parameters: Some(FunctionParameters::new(
                "object".into(),
                json!({"city": {"type": "string"}}),
                vec!["city".into()],
            )),
            extra: Default::default(),
        },
        Tool::GoogleSearch {
            search_types: Some(vec![
                SearchType::WebSearch,
                SearchType::ImageSearch,
                SearchType::EnterpriseWebSearch,
            ]),
            extra: Default::default(),
        },
        Tool::GoogleMaps {
            enable_widget: Some(true),
            latitude: Some(37.0),
            longitude: Some(-122.0),
            extra: Default::default(),
        },
        Tool::code_execution(),
        Tool::url_context(),
        Tool::ComputerUse {
            environment: "browser".into(),
            excluded_predefined_functions: vec!["drag_and_drop".into()],
            enable_prompt_injection_detection: Some(true),
            disabled_safety_policies: vec!["financial_transactions".into()],
            extra: Default::default(),
        },
        Tool::McpServer {
            name: "server".into(),
            url: "https://example.com/mcp".into(),
            allowed_tools: Some(vec![AllowedTools {
                mode: Some(FunctionCallingMode::Validated),
                tools: vec!["t".into()],
                ..Default::default()
            }]),
            headers: Some(HashMap::from([("Authorization".into(), "Bearer x".into())])),
            extra: Default::default(),
        },
        Tool::FileSearch {
            store_names: vec!["fileSearchStores/s".into()],
            top_k: Some(3),
            metadata_filter: Some("a = 1".into()),
            extra: Default::default(),
        },
        Tool::Retrieval {
            retrieval_types: Some(vec![
                RetrievalType::VertexAiSearch,
                RetrievalType::RagStore,
                RetrievalType::ExaAiSearch,
                RetrievalType::ParallelAiSearch,
            ]),
            vertex_ai_search_config: Some(VertexAiSearchConfig {
                engine: Some("projects/p/locations/l/collections/c/engines/e".into()),
                datastores: Some(vec!["d".into()]),
            }),
            exa_ai_search_config: Some(ExaAiSearchConfig {
                api_key: "k".into(),
                custom_config: Some(json!({"k": "v"})),
            }),
            parallel_ai_search_config: Some(ParallelAiSearchConfig {
                api_key: Some("k".into()),
                custom_config: Some(json!({"k": "v"})),
            }),
            rag_store_config: Some(Box::new(rag_store_config())),
            extra: Default::default(),
        },
    ];
    for tool in &all {
        match tool {
            Tool::Function { .. }
            | Tool::GoogleSearch { .. }
            | Tool::GoogleMaps { .. }
            | Tool::CodeExecution { .. }
            | Tool::UrlContext { .. }
            | Tool::ComputerUse { .. }
            | Tool::McpServer { .. }
            | Tool::FileSearch { .. }
            | Tool::Retrieval { .. }
            | Tool::Unknown { .. } => {}
        }
    }
    all
}
