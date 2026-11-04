//! Request side: every key a request value emits exists on its class.
//!
//! Each request type is serialized fully populated, as struct literals with
//! no `..Default::default()`, so a field added to one of these structs fails
//! to compile here until it is populated and checked. Samples from the
//! proptest strategies add the union arms and enum values the literals do not
//! reach. Each value is walked, recursively and path by path, against the
//! google-genai binding class it is sent as (rank 1, D-004; the spec lags
//! the bindings on requests, so it is checked on the response side only).
//!
//! The reverse is checked too: every field of a request class that no value
//! here sends is a known gap, listed below with its reason.

use std::collections::{BTreeSet, HashMap};

use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::TestRunner;
use serde_json::{Value, json};

use super::api_surface::{Finding, Surface, debug_extra_keys};
use super::{Allowed, assert_clean};
use crate::agents::Agent;
use crate::content::{Annotation, CodeExecutionLanguage, Content, Resolution, VideoProcessing};
use crate::credentials::{
    CreateCredentialRequest, CredentialConfig, CredentialType, CredentialUpdate, InjectionLocation,
};
use crate::environments::{
    AllowlistEntry, CreateEnvironmentRequest, EnvVar, EnvironmentSource, EnvironmentSpec,
    NetworkConfig, RemoteEnvironment, SourceType,
};
use crate::file_search_stores::CreateFileSearchStoreRequest;
use crate::proptest_tests as strategies;
use crate::request::{
    AgentConfig, AntigravityConfig, DeepResearchConfig, DynamicConfig, GenerationConfig,
    ImageAspectRatio, ImageConfig, ImageSize, InteractionInput, InteractionRequest, ServiceTier,
    SpeechConfig, ThinkingLevel, ThinkingSummaries, TranscriptionConfig, TranscriptionMode,
    VideoConfig, VideoTask, Visualization,
};
use crate::response_format::{
    ResponseDelivery, ResponseFormat, ResponseFormatSpec, VideoResolution,
};
use crate::safety::{HarmCategory, SafetyMethod, SafetySetting, SafetyThreshold};
use crate::steps::{FunctionResultPayload, Step};
use crate::tools::{
    AllowedTools, ExaAiSearchConfig, FunctionCallingMode, FunctionParameters, HybridSearchConfig,
    ParallelAiSearchConfig, RagFilter, RagRanking, RagResource, RagRetrievalConfig, RagStoreConfig,
    RankService, RetrievalType, SearchType, Tool, ToolChoice, VertexAiSearchConfig,
};
use crate::triggers::{TriggerCreateParams, TriggerStatus, TriggerUpdate};
use crate::voices::{
    CreateVoiceRequest, PromptedVoice, ReplicatedVoice, VoiceAudio, VoiceSpec, VoiceType,
};
use crate::webhooks::{Webhook, WebhookConfig, WebhookEvent, WebhookState, WebhookUpdate};

/// Keys the crate sends that a class lacks, each on purpose.
const ALLOWED: &[Allowed] = &[
    (
        "interactions.FunctionCallStep.signature",
        "Served on function_call steps and required when one is replayed in \
         stateless history (live 2026-07, src/steps/mod.rs); the bindings omit it.",
    ),
    (
        "interactions.FunctionResultStep.signature",
        "Kept so a server-sent function_result (server-run MCP calls arrive as \
         function_call/function_result pairs since 2026-10-01) replays unchanged; \
         not in the bindings, and not seen set live. Follow-up for WP2.",
    ),
    (
        "interactions.FileSearchResultStep.result",
        "Modeled from an older spec; the bindings dropped it and the API never \
         fills it (live 2026-08-16, src/content/results.rs), so it is sent only \
         when a caller fills it. Removal is a follow-up for WP2.",
    ),
    (
        "interactions.GoogleMapsResultPlaces.formatted_address",
        NOT_SERVED_PLACE,
    ),
    ("interactions.GoogleMapsResultPlaces.lat", NOT_SERVED_PLACE),
    ("interactions.GoogleMapsResultPlaces.lng", NOT_SERVED_PLACE),
    (
        "interactions.GoogleMapsResultPlaces.phone_number",
        NOT_SERVED_PLACE,
    ),
    (
        "interactions.GoogleMapsResultPlaces.rating",
        NOT_SERVED_PLACE,
    ),
    (
        "interactions.GoogleMapsResultPlaces.types",
        NOT_SERVED_PLACE,
    ),
    (
        "interactions.GoogleMapsResultPlaces.user_ratings_total",
        NOT_SERVED_PLACE,
    ),
    (
        "interactions.GoogleMapsResultPlaces.website",
        NOT_SERVED_PLACE,
    ),
    (
        "interactions.GoogleSearchResult.rendered_content",
        NOT_SERVED_SEARCH,
    ),
    ("interactions.GoogleSearchResult.title", NOT_SERVED_SEARCH),
    ("interactions.GoogleSearchResult.url", NOT_SERVED_SEARCH),
];

/// `Place` fields beyond the bindings', echoed back only if served.
const NOT_SERVED_PLACE: &str = "On `Place` for a richer places shape; not in the \
     bindings or the spec, and not served (live 2026-11-04: places carry place_id, \
     name and url), so sent only when a caller fills it. Removal is a follow-up for WP2.";

/// `GoogleSearchResultItem` fields beyond the bindings'.
const NOT_SERVED_SEARCH: &str = "On `GoogleSearchResultItem`; not in the bindings, \
     and items carry only search_suggestions (live 2026-07, src/content/results.rs), \
     so sent only when a caller fills it. Removal is a follow-up for WP2.";

/// Request fields the crate never sends.
const NOT_SENT: &[Allowed] = &[
    // Deprecated in the bindings, and either superseded or rejected live.
    (
        "interactions.CreateModelInteraction.response_mime_type",
        "Deprecated; superseded by response_format, and removed from the crate.",
    ),
    (
        "interactions.CreateAgentInteraction.response_mime_type",
        "Deprecated; superseded by response_format, and removed from the crate.",
    ),
    (
        "interactions.CreateModelInteraction.cached_content",
        "Deprecated; rejected live with 400 Unknown parameter (2.25.0 sweep, \
         docs/INTERACTIONS_API_GAP.md, D-005).",
    ),
    (
        "interactions.TranscriptionConfig.language_hints",
        "Deprecated; rejected live with 400 Unknown parameter (2.25.0 sweep, \
         docs/INTERACTIONS_API_GAP.md, D-005).",
    ),
    (
        "interactions.SpeakerConfig.speakers",
        "The crate sends speech_config as the list arm, accepted on every TTS model \
         (live 2026-09-24); the {speakers} object arm is read, never sent.",
    ),
    // Not modeled yet: WP2 (#504).
    ("agents.Agent.agent_config", GAP),
    ("interactions.Environment.environment_id", GAP),
    ("interactions.CodeMenderAgentConfig.*", GAP_CODEMENDER),
    ("interactions.FindRequest.*", GAP_CODEMENDER),
    ("interactions.FixRequest.*", GAP_CODEMENDER),
    ("interactions.FileContent.*", GAP_CODEMENDER),
    ("interactions.SessionConfig.*", GAP_CODEMENDER),
    ("legacy.CreateFileSearchStoreConfig.embeddingModel", GAP),
    ("voices.VoiceInput.accent", GAP_VOICE),
    ("voices.VoiceInput.context", GAP_VOICE),
    ("voices.VoiceInput.description", GAP_VOICE),
    ("voices.VoiceInput.gender", GAP_VOICE),
    ("voices.VoiceInput.language_code", GAP_VOICE),
    ("voices.VoiceInput.persona", GAP_VOICE),
    ("voices.VoiceInput.pitch", GAP_VOICE),
    ("voices.VoiceInput.region_code", GAP_VOICE),
    ("voices.VoiceInput.sample_audio", GAP_VOICE),
];

const GAP: &str = "Not modeled yet; WP2 of #504 (probe live before modeling, D-010).";
const GAP_CODEMENDER: &str = "The CodeMender agent config is not modeled (#501); WP2.";
const GAP_VOICE: &str = "Not on `VoiceSpec` yet; the API may treat some as output only. \
     Probe live before modeling (D-010); WP2.";

// =============================================================================
// Fully populated values
// =============================================================================

fn annotations() -> Vec<Annotation> {
    vec![
        Annotation::UrlCitation {
            url: Some("https://example.com".into()),
            title: Some("t".into()),
            start_index: 0,
            end_index: 1,
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
        },
        Annotation::PlaceCitation {
            place_id: Some("p".into()),
            name: Some("n".into()),
            url: Some("https://example.com".into()),
            review_snippets: vec![crate::content::ReviewSnippet {
                title: Some("t".into()),
                url: Some("https://example.com".into()),
                review_id: Some("r".into()),
            }],
            start_index: 0,
            end_index: 1,
        },
        Annotation::SpeechMetadata {
            speaker: Some("Joe".into()),
            style: Some("calm".into()),
            start_index: Some(0),
            end_index: Some(1),
        },
        Annotation::WordInfo {
            text: Some("w".into()),
            speaker: Some("Joe".into()),
            start_offset: Some("0s".into()),
            end_offset: Some("1s".into()),
            start_index: Some(0),
            end_index: Some(1),
        },
    ]
}

/// Every `Content` variant, every field set. Matching on each one keeps a
/// new variant from going unchecked.
fn contents() -> Vec<Content> {
    let all = vec![
        Content::Text {
            text: Some("hello".into()),
            annotations: Some(annotations()),
        },
        Content::Image {
            data: Some("AAAA".into()),
            uri: Some("https://example.com/i.png".into()),
            mime_type: Some("image/png".into()),
            resolution: Some(Resolution::High),
        },
        Content::Audio {
            data: Some("AAAA".into()),
            uri: Some("https://example.com/a.wav".into()),
            mime_type: Some("audio/wav".into()),
            sample_rate: Some(24_000),
            channels: Some(1),
        },
        Content::Video {
            data: Some("AAAA".into()),
            uri: Some("https://example.com/v.mp4".into()),
            mime_type: Some("video/mp4".into()),
            resolution: Some(Resolution::Low),
            processing: Some(
                VideoProcessing::segment()
                    .start_offset("1s")
                    .end_offset("2s")
                    .fps(2.0)
                    .build(),
            ),
            name: Some("clip.mp4".into()),
        },
        Content::Document {
            data: Some("AAAA".into()),
            uri: Some("https://example.com/d.pdf".into()),
            mime_type: Some("application/pdf".into()),
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
        });
    }
    out
}

/// History steps a caller sends back as input: every variant a response can
/// carry, since `InteractionInput::Steps` echoes them.
fn steps() -> Vec<Step> {
    let payloads = [
        FunctionResultPayload::Text("ok".into()),
        FunctionResultPayload::Json(json!({"k": "v"})),
        FunctionResultPayload::Contents(vec![Content::text("ok")]),
    ];
    let mut all = vec![
        Step::UserInput {
            content: contents(),
        },
        Step::ModelOutput {
            content: contents(),
            error: Some(crate::steps::StepError {
                code: Some(3),
                message: Some("bad".into()),
                details: Some(vec![
                    json!({"@type": "type.googleapis.com/google.rpc.ErrorInfo"}),
                ]),
            }),
        },
        Step::Thought {
            signature: Some("c2ln".into()),
            summary: vec![Content::text("thinking")],
        },
        Step::FunctionCall {
            id: "c1".into(),
            name: "f".into(),
            arguments: json!({"a": 1}),
            signature: Some("c2ln".into()),
        },
        Step::CodeExecutionCall {
            id: "c2".into(),
            language: CodeExecutionLanguage::Python,
            code: "print(1)".into(),
            signature: Some("c2ln".into()),
        },
        Step::CodeExecutionResult {
            call_id: "c2".into(),
            result: "1".into(),
            is_error: true,
            signature: Some("c2ln".into()),
        },
        Step::UrlContextCall {
            id: "c3".into(),
            urls: vec!["https://example.com".into()],
            signature: Some("c2ln".into()),
        },
        Step::UrlContextResult {
            call_id: "c3".into(),
            result: vec![crate::content::UrlContextResultItem {
                url: "https://example.com".into(),
                status: "success".into(),
            }],
            is_error: Some(false),
            signature: Some("c2ln".into()),
        },
        Step::GoogleSearchCall {
            id: "c4".into(),
            queries: vec!["q".into()],
            search_type: Some(SearchType::WebSearch),
            signature: Some("c2ln".into()),
        },
        Step::GoogleSearchResult {
            call_id: "c4".into(),
            result: vec![crate::content::GoogleSearchResultItem {
                title: "t".into(),
                url: "https://example.com".into(),
                rendered_content: Some("<div/>".into()),
                search_suggestions: Some("<div/>".into()),
            }],
            is_error: Some(false),
            signature: Some("c2ln".into()),
        },
        Step::McpServerToolCall {
            id: "c5".into(),
            name: "tool".into(),
            server_name: "server".into(),
            arguments: json!({"a": 1}),
        },
        Step::FileSearchCall {
            id: "c6".into(),
            signature: Some("c2ln".into()),
        },
        Step::FileSearchResult {
            call_id: "c6".into(),
            result: vec![crate::content::FileSearchResultItem {
                title: "t".into(),
                text: "x".into(),
                store: "fileSearchStores/s".into(),
            }],
            signature: Some("c2ln".into()),
        },
        Step::GoogleMapsCall {
            id: "c7".into(),
            queries: vec!["q".into()],
            signature: Some("c2ln".into()),
        },
        Step::GoogleMapsResult {
            call_id: "c7".into(),
            result: vec![crate::content::GoogleMapsResultItem {
                places: Some(vec![crate::content::Place {
                    name: Some("n".into()),
                    formatted_address: Some("1 Main St".into()),
                    place_id: Some("p".into()),
                    url: Some("https://example.com".into()),
                    lat: Some(37.0),
                    lng: Some(-122.0),
                    types: Some(vec!["cafe".into()]),
                    rating: Some(4.5),
                    user_ratings_total: Some(10),
                    website: Some("https://example.com".into()),
                    phone_number: Some("555".into()),
                    review_snippets: Some(vec![crate::content::ReviewSnippet {
                        title: Some("t".into()),
                        url: Some("https://example.com".into()),
                        review_id: Some("r".into()),
                    }]),
                    extra: serde_json::Map::new(),
                }]),
                widget_context_token: Some("w".into()),
            }],
            signature: Some("c2ln".into()),
        },
        Step::ProcessingCall {
            id: "c8".into(),
            signature: Some("c2ln".into()),
        },
        Step::ProcessingResult {
            call_id: "c8".into(),
            signature: Some("c2ln".into()),
        },
        Step::RetrievalCall {
            id: "c9".into(),
            queries: vec!["q".into()],
            retrieval_type: Some(RetrievalType::RagStore),
            signature: Some("c2ln".into()),
        },
        Step::RetrievalResult {
            call_id: "c9".into(),
            is_error: Some(false),
            signature: Some("c2ln".into()),
        },
    ];
    for result in payloads {
        all.push(Step::FunctionResult {
            call_id: "c1".into(),
            name: Some("f".into()),
            result: result.clone(),
            is_error: Some(false),
            signature: Some("c2ln".into()),
        });
        all.push(Step::McpServerToolResult {
            call_id: "c5".into(),
            name: Some("tool".into()),
            server_name: Some("server".into()),
            result,
        });
    }
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
                }),
            }),
        }),
    }
}

/// Every `Tool` variant, every field set.
fn tools() -> Vec<Tool> {
    let all = vec![
        Tool::Function {
            name: "get_weather".into(),
            description: "Gets the weather".into(),
            parameters: FunctionParameters::new(
                "object".into(),
                json!({"city": {"type": "string"}}),
                vec!["city".into()],
            ),
        },
        Tool::GoogleSearch {
            search_types: Some(vec![
                SearchType::WebSearch,
                SearchType::ImageSearch,
                SearchType::EnterpriseWebSearch,
            ]),
        },
        Tool::GoogleMaps {
            enable_widget: Some(true),
            latitude: Some(37.0),
            longitude: Some(-122.0),
        },
        Tool::CodeExecution,
        Tool::UrlContext,
        Tool::ComputerUse {
            environment: "browser".into(),
            excluded_predefined_functions: vec!["drag_and_drop".into()],
            enable_prompt_injection_detection: Some(true),
            disabled_safety_policies: vec!["financial_transactions".into()],
        },
        Tool::McpServer {
            name: "server".into(),
            url: "https://example.com/mcp".into(),
            allowed_tools: Some(vec![AllowedTools {
                mode: Some(FunctionCallingMode::Validated),
                tools: vec!["t".into()],
            }]),
            headers: Some(HashMap::from([("Authorization".into(), "Bearer x".into())])),
        },
        Tool::FileSearch {
            store_names: vec!["fileSearchStores/s".into()],
            top_k: Some(3),
            metadata_filter: Some("a = 1".into()),
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
        },
    ];
    for tool in &all {
        match tool {
            Tool::Function { .. }
            | Tool::GoogleSearch { .. }
            | Tool::GoogleMaps { .. }
            | Tool::CodeExecution
            | Tool::UrlContext
            | Tool::ComputerUse { .. }
            | Tool::McpServer { .. }
            | Tool::FileSearch { .. }
            | Tool::Retrieval { .. }
            | Tool::Unknown { .. } => {}
        }
    }
    all
}

fn generation_configs() -> Vec<GenerationConfig> {
    let full = GenerationConfig {
        temperature: Some(0.5),
        max_output_tokens: Some(100),
        top_p: Some(0.9),
        thinking_level: Some(ThinkingLevel::High),
        seed: Some(7),
        stop_sequences: Some(vec!["STOP".into()]),
        thinking_summaries: Some(ThinkingSummaries::Auto),
        tool_choice: Some(ToolChoice::AllowedTools(AllowedTools {
            mode: Some(FunctionCallingMode::Any),
            tools: vec!["get_weather".into()],
        })),
        speech_config: Some(vec![SpeechConfig {
            voice: Some("Kore".into()),
            language: Some("en-US".into()),
            speaker: Some("Joe".into()),
        }]),
        image_config: Some(ImageConfig {
            aspect_ratio: Some(ImageAspectRatio::Square),
            image_size: Some(ImageSize::Hd1k),
        }),
        video_config: Some(VideoConfig {
            task: Some(VideoTask::TextToVideo),
        }),
        transcription_config: Some(TranscriptionConfig {
            adaptation_phrases: Some(vec!["genai".into()]),
            custom_vocabulary: Some(vec!["genai".into()]),
            diarization_mode: Some("speaker".into()),
            language_codes: Some(vec!["en-US".into()]),
            timestamp_granularities: Some(vec!["word".into()]),
            mode: Some(TranscriptionMode::Verbatim {
                diarization_mode: Some("speaker".into()),
                timestamp_granularities: Some(vec!["word".into()]),
            }),
        }),
    };
    let mut modes = full.clone();
    modes.tool_choice = Some(ToolChoice::Mode(FunctionCallingMode::Auto));
    modes.transcription_config = Some(TranscriptionConfig {
        mode: Some(TranscriptionMode::Smart),
        ..Default::default()
    });
    vec![full, modes]
}

fn safety_settings() -> Vec<SafetySetting> {
    vec![SafetySetting {
        category: HarmCategory::Harassment,
        threshold: SafetyThreshold::BlockOnlyHigh,
        method: Some(SafetyMethod::Probability),
    }]
}

fn agent_configs() -> Vec<AgentConfig> {
    vec![
        DeepResearchConfig::new()
            .with_thinking_summaries(ThinkingSummaries::Auto)
            .with_visualization(Visualization::Auto)
            .with_collaborative_planning(true)
            .with_bigquery_tool(true)
            .into(),
        AntigravityConfig::new()
            .with_model("m")
            .with_max_total_tokens(1000)
            .into(),
        DynamicConfig::new().into(),
    ]
}

fn response_formats() -> Vec<ResponseFormat> {
    let all = vec![
        ResponseFormat::Text {
            mime_type: Some("application/json".into()),
            schema: Some(json!({"type": "object"})),
        },
        ResponseFormat::Audio {
            mime_type: Some("audio/wav".into()),
            delivery: Some(ResponseDelivery::Inline),
            sample_rate: Some(24_000),
            bit_rate: Some(64_000),
        },
        ResponseFormat::Image {
            mime_type: Some("image/jpeg".into()),
            delivery: Some(ResponseDelivery::Uri),
            aspect_ratio: Some(ImageAspectRatio::Widescreen16x9),
            image_size: Some(ImageSize::Hd2k),
        },
        ResponseFormat::Video {
            delivery: Some(ResponseDelivery::Uri),
            gcs_uri: Some("gs://bucket/out".into()),
            aspect_ratio: Some(ImageAspectRatio::Widescreen16x9),
            duration: Some("8s".into()),
            resolution: Some(VideoResolution::Hd720p),
        },
    ];
    for format in &all {
        match format {
            ResponseFormat::Text { .. }
            | ResponseFormat::Audio { .. }
            | ResponseFormat::Image { .. }
            | ResponseFormat::Video { .. }
            | ResponseFormat::Unknown { .. } => {}
        }
    }
    all
}

fn remote_environment() -> RemoteEnvironment {
    RemoteEnvironment {
        sources: environment_sources(),
        network: Some(network()),
        env: [
            ("PLAIN".to_string(), EnvVar::value("v")),
            ("SECRET".to_string(), EnvVar::credential("credentials/c")),
        ]
        .into_iter()
        .collect(),
        extra: serde_json::Map::new(),
    }
}

fn environment_sources() -> Vec<EnvironmentSource> {
    [
        SourceType::Gcs,
        SourceType::Inline,
        SourceType::Repository,
        SourceType::SkillRegistry,
    ]
    .into_iter()
    .map(|source_type| EnvironmentSource {
        source_type: Some(source_type),
        source: Some("gs://bucket/dir".into()),
        target: Some("/workspace".into()),
        content: Some("print(1)".into()),
        encoding: Some("base64".into()),
        extra: serde_json::Map::new(),
    })
    .collect()
}

fn network() -> NetworkConfig {
    NetworkConfig::Allowlist {
        entries: vec![AllowlistEntry {
            domain: "example.com".into(),
            transform: Some(vec![HashMap::from([(
                "Authorization".into(),
                "Bearer x".into(),
            )])]),
            credential: Some("credentials/c".into()),
            extra: serde_json::Map::new(),
        }],
        extra: serde_json::Map::new(),
    }
}

fn environment_specs() -> Vec<EnvironmentSpec> {
    vec![
        EnvironmentSpec::Id("environments/e".into()),
        EnvironmentSpec::Remote(remote_environment()),
    ]
}

fn webhook_config() -> WebhookConfig {
    WebhookConfig {
        uris: Some(vec!["https://example.com/hook".into()]),
        user_metadata: Some(json!({"k": "v"})),
    }
}

fn inputs() -> Vec<InteractionInput> {
    vec![
        InteractionInput::Text("hello".into()),
        InteractionInput::Content(contents()),
        InteractionInput::Steps(steps()),
    ]
}

/// The model form: everything but the agent fields.
fn model_requests() -> Vec<InteractionRequest> {
    let mut out = Vec::new();
    for (i, input) in inputs().into_iter().enumerate() {
        for format in [
            ResponseFormatSpec::Single(response_formats().remove(0)),
            ResponseFormatSpec::List(response_formats()),
        ] {
            out.push(InteractionRequest {
                model: Some("test-model".into()),
                agent: None,
                agent_config: None,
                input: input.clone(),
                previous_interaction_id: Some("prev".into()),
                tools: Some(tools()),
                response_modalities: Some(vec!["text".into(), "image".into()]),
                response_format: Some(format),
                generation_config: Some(generation_configs()[i % 2].clone()),
                stream: Some(true),
                background: Some(true),
                store: Some(true),
                system_instruction: Some("be brief".into()),
                service_tier: Some(ServiceTier::Flex),
                webhook_config: Some(webhook_config()),
                environment: Some(environment_specs()[i % 2].clone()),
                safety_settings: Some(safety_settings()),
                labels: Some([("team".to_string(), "x".to_string())].into()),
                continuation_token: Some("token".into()),
            });
        }
    }
    out
}

/// The agent form: everything but the model fields.
fn agent_requests() -> Vec<InteractionRequest> {
    agent_configs()
        .into_iter()
        .enumerate()
        .map(|(i, config)| InteractionRequest {
            model: None,
            agent: Some("test-agent".into()),
            agent_config: Some(config),
            input: inputs().remove(i % 3),
            previous_interaction_id: Some("prev".into()),
            tools: Some(tools()),
            response_modalities: Some(vec!["text".into()]),
            response_format: Some(ResponseFormatSpec::List(response_formats())),
            generation_config: None,
            stream: Some(true),
            background: Some(true),
            store: Some(true),
            system_instruction: Some("be brief".into()),
            service_tier: Some(ServiceTier::Standard),
            webhook_config: Some(webhook_config()),
            environment: Some(environment_specs()[i % 2].clone()),
            safety_settings: Some(safety_settings()),
            labels: Some([("team".to_string(), "x".to_string())].into()),
            continuation_token: Some("token".into()),
        })
        .collect()
}

fn webhooks() -> Vec<Value> {
    let webhook = Webhook {
        uri: "https://example.com/hook".into(),
        subscribed_events: vec![
            WebhookEvent::BatchSucceeded,
            WebhookEvent::InteractionCompleted,
            WebhookEvent::VideoGenerated,
        ],
        name: Some("hook".into()),
        // Output-only fields: not sent on create (`Webhooks::create` serializes
        // the value as given, so a caller could set them; the API ignores them).
        id: None,
        state: None,
        signing_secrets: None,
        new_signing_secret: None,
        create_time: None,
        update_time: None,
        extra: serde_json::Map::new(),
    };
    vec![to_value(&webhook)]
}

fn webhook_updates() -> Vec<Value> {
    vec![to_value(&WebhookUpdate {
        name: Some("hook".into()),
        uri: Some("https://example.com/hook".into()),
        subscribed_events: Some(vec![WebhookEvent::InteractionFailed]),
        state: Some(WebhookState::Disabled),
        update_mask: Some("name,uri".into()),
    })]
}

fn trigger_params() -> Vec<Value> {
    agent_requests()
        .into_iter()
        .map(|interaction| {
            to_value(&TriggerCreateParams {
                schedule: "0 9 * * *".into(),
                time_zone: "America/Los_Angeles".into(),
                interaction,
                display_name: Some("daily".into()),
                environment_id: Some("environments/e".into()),
                max_consecutive_failures: Some(3),
                execution_timeout_seconds: Some(600),
                extra: serde_json::Map::new(),
            })
        })
        .collect()
}

fn trigger_updates() -> Vec<Value> {
    vec![to_value(&TriggerUpdate {
        display_name: Some("daily".into()),
        status: Some(TriggerStatus::Paused),
        extra: serde_json::Map::new(),
    })]
}

fn agents() -> Vec<Value> {
    environment_specs()
        .into_iter()
        .map(|base_environment| {
            to_value(&Agent {
                id: Some("my-agent".into()),
                base_agent: Some("antigravity-preview-09-2026".into()),
                system_instruction: Some("be brief".into()),
                description: Some("d".into()),
                tools: Some(vec![
                    tools().remove(0),
                    Tool::CodeExecution,
                    Tool::UrlContext,
                    tools().remove(1),
                    tools().remove(6),
                ]),
                base_environment: Some(base_environment),
                extra: serde_json::Map::new(),
            })
        })
        .collect()
}

fn environment_requests() -> Vec<Value> {
    vec![
        to_value(&CreateEnvironmentRequest {
            from_environment: Some("environments/e".into()),
            sources: Some(environment_sources()),
            network: Some(network()),
            extra: serde_json::Map::new(),
        }),
        to_value(&CreateEnvironmentRequest {
            from_environment: None,
            sources: None,
            network: Some(NetworkConfig::Disabled),
            extra: serde_json::Map::new(),
        }),
    ]
}

fn credential_requests() -> Vec<Value> {
    let configs = vec![
        CredentialConfig::BearerToken {
            token: "t".into(),
            header_name: Some("X-Token".into()),
            prefix: Some("Bearer".into()),
        },
        CredentialConfig::EnvironmentVariable {
            value: "v".into(),
            injection_location: vec![
                InjectionLocation::Header,
                InjectionLocation::Query,
                InjectionLocation::Body,
            ],
            trusted_domains: vec!["example.com".into()],
        },
        CredentialConfig::OAuth2 {
            client_id: "c".into(),
            client_secret: "s".into(),
            refresh_token: "r".into(),
            token_url: "https://example.com/token".into(),
            scopes: Some(vec!["read".into()]),
        },
    ];
    for config in &configs {
        match config {
            CredentialConfig::BearerToken { .. }
            | CredentialConfig::EnvironmentVariable { .. }
            | CredentialConfig::OAuth2 { .. } => {}
        }
    }
    configs
        .into_iter()
        .map(|config| {
            to_value(&CreateCredentialRequest {
                id: Some("my-credential".into()),
                config,
            })
        })
        .collect()
}

fn credential_updates() -> Vec<Value> {
    let full = |credential_type| CredentialUpdate {
        credential_type,
        token: Some("t".into()),
        header_name: Some("X-Token".into()),
        prefix: Some("Bearer".into()),
        value: Some("v".into()),
        injection_location: Some(vec![InjectionLocation::Header]),
        trusted_domains: Some(vec!["example.com".into()]),
        client_id: Some("c".into()),
        client_secret: Some("s".into()),
        refresh_token: Some("r".into()),
        token_url: Some("https://example.com/token".into()),
        scopes: Some(vec!["read".into()]),
        update_mask: Some("token".into()),
    };
    // Each type with only its own fields, as the builders make it: a bearer
    // update carrying `client_id` is a caller error the server rejects.
    let mut bearer = full(CredentialType::BearerToken);
    (
        bearer.value,
        bearer.injection_location,
        bearer.trusted_domains,
    ) = (None, None, None);
    (bearer.client_id, bearer.client_secret, bearer.refresh_token) = (None, None, None);
    (bearer.token_url, bearer.scopes) = (None, None);
    let mut env = full(CredentialType::EnvironmentVariable);
    (env.token, env.header_name, env.prefix) = (None, None, None);
    (env.client_id, env.client_secret, env.refresh_token) = (None, None, None);
    (env.token_url, env.scopes) = (None, None);
    let mut oauth = full(CredentialType::OAuth2);
    (oauth.token, oauth.header_name, oauth.prefix) = (None, None, None);
    (oauth.value, oauth.injection_location, oauth.trusted_domains) = (None, None, None);
    vec![to_value(&bearer), to_value(&env), to_value(&oauth)]
}

fn voice_requests() -> Vec<Value> {
    let audio = || VoiceAudio {
        data: "AAAA".into(),
        mime_type: "audio/wav".into(),
    };
    vec![
        to_value(&CreateVoiceRequest {
            voice: VoiceSpec {
                voice_type: Some(VoiceType::Prompted),
                prompted: Some(PromptedVoice {
                    input: "a calm narrator".into(),
                    base_voice: Some("Kore".into()),
                    extra: serde_json::Map::new(),
                }),
                replicated: None,
                display_name: Some("narrator".into()),
                model: Some("m".into()),
                extra: serde_json::Map::new(),
            },
            store: Some(true),
        }),
        to_value(&CreateVoiceRequest {
            voice: VoiceSpec {
                voice_type: Some(VoiceType::Replicated),
                prompted: None,
                replicated: Some(ReplicatedVoice {
                    source_audio: audio(),
                    consent_audio: audio(),
                }),
                display_name: Some("me".into()),
                model: Some("m".into()),
                extra: serde_json::Map::new(),
            },
            store: Some(false),
        }),
    ]
}

fn file_search_store_requests() -> Vec<Value> {
    vec![to_value(&CreateFileSearchStoreRequest {
        display_name: Some("docs".into()),
        extra: serde_json::Map::new(),
    })]
}

fn to_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("request types serialize")
}

fn values<T: serde::Serialize>(items: Vec<T>) -> Vec<Value> {
    items.iter().map(to_value).collect()
}

/// Proptest samples with no `Unknown` variant and no `extra` key: those carry
/// caller-chosen keys on purpose.
fn samples<T, S>(strategy: S, n: usize) -> Vec<Value>
where
    S: Strategy<Value = T>,
    T: serde::Serialize + std::fmt::Debug,
{
    let mut runner = TestRunner::deterministic();
    (0..n)
        .map(|_| strategy.new_tree(&mut runner).expect("strategy").current())
        .filter(|value| {
            let debug = format!("{value:?}");
            !debug.contains("Unknown {") && debug_extra_keys(&debug).is_empty()
        })
        .map(|value| to_value(&value))
        .collect()
}

// =============================================================================
// The table
// =============================================================================

/// A request type, the binding class it is sent as, and the values to check.
struct Case {
    name: &'static str,
    binding: &'static str,
    values: Vec<Value>,
}

fn cases() -> Vec<Case> {
    const N: usize = 64;
    let case = |name, binding, values| Case {
        name,
        binding,
        values,
    };
    vec![
        case(
            "InteractionRequest (model)",
            "interactions.CreateModelInteraction",
            values(model_requests()),
        ),
        case(
            "InteractionRequest (agent)",
            "interactions.CreateAgentInteraction",
            values(agent_requests()),
        ),
        case(
            "GenerationConfig",
            "interactions.GenerationConfig",
            [
                values(generation_configs()),
                samples(strategies::arb_generation_config(), N),
            ]
            .concat(),
        ),
        case(
            "ToolChoice",
            "interactions.ToolChoice",
            samples(strategies::arb_tool_choice(), N),
        ),
        case(
            "SpeechConfig",
            "interactions.SpeechConfig",
            samples(strategies::arb_speech_config(), N),
        ),
        case(
            "TranscriptionConfig",
            "interactions.TranscriptionConfig",
            samples(strategies::arb_transcription_config(), N),
        ),
        case("Tool", "interactions.Tool", values(tools())),
        case(
            "SafetySetting",
            "interactions.SafetySetting",
            [
                values(safety_settings()),
                samples(strategies::arb_safety_setting(), N),
            ]
            .concat(),
        ),
        case(
            "AgentConfig",
            "interactions.CreateAgentInteractionAgentConfig",
            values(agent_configs()),
        ),
        case(
            "ResponseFormat",
            "interactions.ResponseFormat",
            [
                values(response_formats()),
                samples(strategies::arb_response_format(), N),
            ]
            .concat(),
        ),
        case(
            "ResponseFormatSpec",
            "interactions.CreateModelInteractionResponseFormat",
            samples(strategies::arb_response_format_spec(), N),
        ),
        case(
            "EnvironmentSpec",
            "interactions.CreateModelInteractionEnvironment",
            [
                values(environment_specs()),
                samples(strategies::arb_environment_spec(), N),
            ]
            .concat(),
        ),
        case(
            "WebhookConfig",
            "interactions.WebhookConfig",
            [
                vec![to_value(&webhook_config())],
                samples(strategies::arb_webhook_config(), N),
            ]
            .concat(),
        ),
        case(
            "InteractionInput",
            "interactions.InteractionsInput",
            values(inputs())
                .into_iter()
                .map(wrap_content_input)
                .collect(),
        ),
        case("Step (history)", "interactions.Step", values(steps())),
        case("Content", "interactions.Content", values(contents())),
        case("Webhook (create)", "webhooks.WebhookInput", webhooks()),
        case("WebhookUpdate", "webhooks.WebhookUpdate", webhook_updates()),
        case(
            "TriggerCreateParams",
            "triggers.TriggerCreateParams",
            trigger_params(),
        ),
        case("TriggerUpdate", "triggers.TriggerUpdate", trigger_updates()),
        case("Agent (create)", "agents.Agent", agents()),
        case(
            "CreateEnvironmentRequest",
            "environments.CreateEnvironmentRequest",
            environment_requests(),
        ),
        case(
            "CreateCredentialRequest",
            "credentials.CredentialCreateParams",
            credential_requests(),
        ),
        case(
            "CredentialUpdate",
            "credentials.CredentialUpdate",
            credential_updates(),
        ),
        case(
            "CreateVoiceRequest",
            "voices.CreateVoiceRequest",
            voice_requests(),
        ),
        case(
            "CreateFileSearchStoreRequest",
            "legacy.CreateFileSearchStoreConfig",
            file_search_store_requests(),
        ),
    ]
}

/// `InteractionInput` serializes faithfully (a bare content array); only the
/// request field wraps content in a `user_input` step. Check the wrapped
/// shape, which is what goes on the wire.
fn wrap_content_input(value: Value) -> Value {
    match &value {
        Value::Array(items)
            if !items.is_empty()
                && items.iter().all(|i| {
                    matches!(
                        i["type"].as_str(),
                        Some("text" | "image" | "audio" | "video" | "document")
                    )
                }) =>
        {
            json!([{"type": "user_input", "content": value}])
        }
        _ => value,
    }
}

fn findings(surface: &Surface) -> Vec<Finding> {
    let mut out = Vec::new();
    for case in cases() {
        let class = case.binding;
        assert!(!case.values.is_empty(), "{}: no values", case.name);
        let ty = surface.ty(class);
        for value in &case.values {
            for mut finding in surface.check(&ty, value) {
                finding.path = format!("{} {}", case.name, finding.path);
                out.push(finding);
            }
        }
    }
    out
}

#[test]
fn request_keys_exist_on_their_binding_classes() {
    let surface = Surface::bindings();
    assert_clean(
        &format!("Request keys vs google-genai {} bindings", surface.version),
        &surface,
        findings(&surface),
        &[("ALLOWED", ALLOWED)],
    );
}

/// The other direction: every field of every class a request reaches is
/// sent by some value above, or listed in [`NOT_SENT`] with its reason.
#[test]
fn request_classes_have_no_unsent_fields() {
    let surface = Surface::bindings();
    let mut sent: BTreeSet<String> = BTreeSet::new();
    let mut reached: BTreeSet<String> = BTreeSet::new();
    for case in cases() {
        let ty = surface.ty(case.binding);
        reached.extend(surface.reachable_classes(&ty));
        for value in &case.values {
            sent.extend(surface.sites(&ty, value).into_values());
        }
    }
    let mut findings = Vec::new();
    for class in &reached {
        let Some(fields) = surface.classes.get(class) else {
            continue;
        };
        for (wire, field) in &fields.fields {
            let site = format!("{class}.{wire}");
            if !sent.contains(&site) {
                findings.push(Finding {
                    path: class.clone(),
                    site,
                    message: format!(
                        "never sent{}",
                        if field.deprecated {
                            " (deprecated)"
                        } else {
                            ""
                        }
                    ),
                });
            }
        }
    }
    assert_clean(
        &format!(
            "Request-class fields no value sends (google-genai {})",
            surface.version
        ),
        &surface,
        findings,
        &[("NOT_SENT", NOT_SENT)],
    );
}
