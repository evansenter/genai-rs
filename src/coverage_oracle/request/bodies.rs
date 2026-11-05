//! Fully populated values for the request side: the configs, the
//! interaction body in its model and agent forms, and every resource's
//! create and update body, plus proptest samples.

use std::collections::HashMap;

use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::TestRunner;
use serde_json::{Value, json};

use super::content::{contents, steps, tools};
use crate::agents::Agent;
use crate::coverage_oracle::api_surface::debug_extra_keys;
use crate::credentials::{
    CreateCredentialRequest, CredentialConfig, CredentialType, CredentialUpdate, InjectionLocation,
};
use crate::environments::{
    AllowlistEntry, CreateEnvironmentRequest, EnvVar, EnvironmentSource, EnvironmentSpec,
    NetworkConfig, RemoteEnvironment, SourceType,
};
use crate::file_search_stores::{
    ChunkingConfig, CreateFileSearchStoreRequest, CustomMetadata, DocumentOptions, WhiteSpaceConfig,
};
use crate::http::file_search_stores::{ImportFileBody, UploadDocumentBody};
use crate::http::webhooks::WebhookInput;
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
use crate::tools::{AllowedTools, FunctionCallingMode, Tool, ToolChoice};
use crate::triggers::{TriggerCreateParams, TriggerStatus, TriggerUpdate};
use crate::voices::{
    CreateVoiceRequest, PromptedVoice, ReplicatedVoice, VoiceAudio, VoicePitch, VoiceSpec,
    VoiceType,
};
use crate::webhooks::{
    SigningSecret, Webhook, WebhookConfig, WebhookEvent, WebhookState, WebhookUpdate,
};

pub(super) fn generation_configs() -> Vec<GenerationConfig> {
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

pub(super) fn safety_settings() -> Vec<SafetySetting> {
    vec![SafetySetting {
        category: HarmCategory::Harassment,
        threshold: SafetyThreshold::BlockOnlyHigh,
        method: Some(SafetyMethod::Probability),
    }]
}

pub(super) fn agent_configs() -> Vec<AgentConfig> {
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

pub(super) fn response_formats() -> Vec<ResponseFormat> {
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
        environment_id: Some("environments/e".into()),
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

pub(super) fn environment_specs() -> Vec<EnvironmentSpec> {
    vec![
        EnvironmentSpec::Id("environments/e".into()),
        EnvironmentSpec::Remote(remote_environment()),
    ]
}

pub(super) fn webhook_config() -> WebhookConfig {
    WebhookConfig {
        uris: Some(vec!["https://example.com/hook".into()]),
        user_metadata: Some(json!({"k": "v"})),
    }
}

pub(super) fn inputs() -> Vec<InteractionInput> {
    vec![
        InteractionInput::Text("hello".into()),
        InteractionInput::Content(contents()),
        InteractionInput::Steps(steps()),
    ]
}

/// The model form: everything but the agent fields.
///
/// `extra` stays empty here, as on every other request type: it holds
/// caller-chosen keys, which the server validates (`400 Unknown parameter`)
/// and the crate never emits on its own. A key in it would also count as
/// sent in the reverse check without the crate modeling it.
/// `a_populated_extra_is_sent_flat_and_not_vouched_for` pins both.
pub(super) fn model_requests() -> Vec<InteractionRequest> {
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
                extra: serde_json::Map::new(),
            });
        }
    }
    out
}

/// The agent form: everything but the model fields.
pub(super) fn agent_requests() -> Vec<InteractionRequest> {
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
            extra: serde_json::Map::new(),
        })
        .collect()
}

/// What `Webhooks::create` sends for a fully populated [`Webhook`]: the
/// output-only fields stay off the wire.
pub(super) fn webhooks() -> Vec<Value> {
    let webhook = Webhook {
        uri: "https://example.com/hook".into(),
        subscribed_events: vec![
            WebhookEvent::BatchSucceeded,
            WebhookEvent::InteractionCompleted,
            WebhookEvent::VideoGenerated,
        ],
        name: Some("hook".into()),
        id: Some("wh-1".into()),
        state: Some(WebhookState::Enabled),
        signing_secrets: Some(vec![SigningSecret::default()]),
        new_signing_secret: Some("secret".into()),
        create_time: Some(chrono::DateTime::UNIX_EPOCH),
        update_time: Some(chrono::DateTime::UNIX_EPOCH),
        extra: serde_json::Map::new(),
    };
    vec![to_value(&WebhookInput::from(&webhook))]
}

pub(super) fn webhook_updates() -> Vec<Value> {
    vec![to_value(&WebhookUpdate {
        name: Some("hook".into()),
        uri: Some("https://example.com/hook".into()),
        subscribed_events: Some(vec![WebhookEvent::InteractionFailed]),
        state: Some(WebhookState::Disabled),
        update_mask: Some("name,uri".into()),
    })]
}

pub(super) fn trigger_params() -> Vec<Value> {
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

pub(super) fn trigger_updates() -> Vec<Value> {
    vec![to_value(&TriggerUpdate {
        display_name: Some("daily".into()),
        status: Some(TriggerStatus::Paused),
        extra: serde_json::Map::new(),
    })]
}

pub(super) fn agents() -> Vec<Value> {
    environment_specs()
        .into_iter()
        .map(|base_environment| {
            to_value(&Agent {
                id: Some("my-agent".into()),
                base_agent: Some(crate::DEFAULT_ANTIGRAVITY_AGENT.into()),
                agent_config: Some(
                    AntigravityConfig::new()
                        .with_model("m")
                        .with_max_total_tokens(1000)
                        .into(),
                ),
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

pub(super) fn environment_requests() -> Vec<Value> {
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

pub(super) fn credential_requests() -> Vec<Value> {
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

pub(super) fn credential_updates() -> Vec<Value> {
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

pub(super) fn voice_requests() -> Vec<Value> {
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
                description: Some("a calm narrator".into()),
                language_code: Some("en-US".into()),
                region_code: Some("US".into()),
                accent: Some("General American".into()),
                persona: Some("Storyteller & Narrator".into()),
                context: Some("Content & Media".into()),
                gender: Some("male".into()),
                pitch: Some(VoicePitch::Low),
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
                description: None,
                language_code: None,
                region_code: None,
                accent: None,
                persona: None,
                context: None,
                gender: None,
                pitch: None,
                extra: serde_json::Map::new(),
            },
            store: Some(false),
        }),
    ]
}

pub(super) fn file_search_store_requests() -> Vec<Value> {
    vec![to_value(&CreateFileSearchStoreRequest {
        display_name: Some("docs".into()),
        embedding_model: Some("models/gemini-embedding-2".into()),
        extra: serde_json::Map::new(),
    })]
}

/// The custom metadata and chunking an upload or import sends, every value
/// kind included.
fn document_options() -> DocumentOptions {
    DocumentOptions {
        custom_metadata: vec![
            CustomMetadata::string("team", "search"),
            CustomMetadata::numeric("year", 2026.0),
            CustomMetadata::string_list("tags", ["a", "b"]),
        ],
        chunking_config: Some(ChunkingConfig {
            white_space_config: Some(WhiteSpaceConfig {
                max_tokens_per_chunk: Some(200),
                max_overlap_tokens: Some(20),
            }),
            extra: serde_json::Map::new(),
        }),
    }
}

/// The JSON that starts a document upload.
pub(super) fn document_uploads() -> Vec<Value> {
    vec![to_value(&UploadDocumentBody {
        display_name: Some("handbook"),
        mime_type: "text/plain",
        options: &document_options(),
    })]
}

/// The body of an import.
pub(super) fn document_imports() -> Vec<Value> {
    vec![to_value(&ImportFileBody {
        file_name: "files/abc",
        options: &document_options(),
    })]
}

pub(super) fn to_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("request types serialize")
}

pub(super) fn values<T: serde::Serialize>(items: Vec<T>) -> Vec<Value> {
    items.iter().map(to_value).collect()
}

/// Proptest samples with no `Unknown` variant and no `extra` key: those carry
/// caller-chosen keys on purpose.
pub(super) fn samples<T, S>(strategy: S, n: usize) -> Vec<Value>
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
