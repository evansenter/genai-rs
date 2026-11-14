//! Fully populated values for the request side: the configs, the
//! interaction body in its model and agent forms, and every resource's
//! create and update body, plus proptest samples.

use std::collections::{BTreeMap, HashMap};

use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::TestRunner;
use serde_json::{Value, json};

use super::content::{contents, steps, tools};
use crate::agents::Agent;
use crate::coverage_oracle::api_surface::debug_extra_keys;
use crate::credentials::{
    CreateCredentialRequest, CredentialConfig, CredentialType, CredentialUpdate, InjectionLocation,
};
use crate::decisions::{CreateDecisionRequest, DecisionInput, NoulCriteria, Question};
use crate::environments::{
    AllowlistEntry, CreateEnvironmentRequest, EnvVar, EnvironmentSource, EnvironmentSpec,
    NetworkConfig, RemoteEnvironment, SourceType,
};
use crate::file_search_stores::{
    ChunkingConfig, CreateFileSearchStoreRequest, CustomMetadata, DocumentOptions, StringList,
    WhiteSpaceConfig,
};
use crate::http::file_search_stores::{ImportFileBody, UploadDocumentBody};
use crate::http::webhooks::WebhookInput;
use crate::request::{
    AgentConfig, AntigravityConfig, CodeMenderConfig, DeepResearchConfig, DynamicConfig,
    GenerationConfig, ImageAspectRatio, ImageConfig, ImageSize, InteractionInput,
    InteractionRequest, ServiceTier, SpeechConfig, ThinkingLevel, ThinkingSummaries,
    TranscriptionConfig, TranscriptionMode, VideoConfig, VideoTask,
};
use crate::response_format::{
    ResponseDelivery, ResponseFormat, ResponseFormatSpec, VideoResolution,
};
use crate::safety::{HarmCategory, SafetyMethod, SafetySetting, SafetyThreshold};
use crate::tools::{AllowedTools, FunctionCallingMode, Tool, ToolChoice};
use crate::triggers::{CreateTriggerRequest, TriggerStatus, TriggerUpdate};
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
        tool_choice: Some(ToolChoice::AllowedTools {
            allowed_tools: AllowedTools {
                mode: Some(FunctionCallingMode::Any),
                tools: vec!["get_weather".into()],
                extra: Default::default(),
            },
            extra: Default::default(),
        }),
        speech_config: Some(vec![SpeechConfig {
            voice: Some("Kore".into()),
            language: Some("en-US".into()),
            speaker: Some("Joe".into()),
            extra: Default::default(),
        }]),
        // The keys beside a `{"speakers": [...]}` form read back, which no
        // binding class names.
        speech_config_extra: Default::default(),
        // The caller's keys, not the crate's (see `cases`).
        extra: serde_json::Map::new(),
        image_config: Some(ImageConfig {
            aspect_ratio: Some(ImageAspectRatio::Square),
            image_size: Some(ImageSize::Hd1k),
            extra: Default::default(),
        }),
        video_config: Some(VideoConfig {
            task: Some(VideoTask::TextToVideo),
            extra: Default::default(),
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
                extra: Default::default(),
            }),
            extra: Default::default(),
        }),
    };
    let mut modes = full.clone();
    modes.tool_choice = Some(ToolChoice::Mode(FunctionCallingMode::Auto));
    modes.transcription_config = Some(TranscriptionConfig {
        adaptation_phrases: None,
        custom_vocabulary: None,
        diarization_mode: None,
        language_codes: None,
        timestamp_granularities: None,
        mode: Some(TranscriptionMode::Smart {
            source_text: Some("Hello there.".into()),
            extra: Default::default(),
        }),
        extra: Default::default(),
    });
    vec![full, modes]
}

pub(super) fn safety_settings() -> Vec<SafetySetting> {
    vec![SafetySetting {
        category: HarmCategory::Harassment,
        threshold: SafetyThreshold::BlockOnlyHigh,
        method: Some(SafetyMethod::Probability),
        extra: serde_json::Map::new(),
    }]
}

/// Every agent config, each built by its type's `fully_populated()`: a
/// struct literal with every field set, which a new field fails to compile
/// (the fields are private, so the literals live beside the types, in
/// `src/request/agent_config.rs`). Paired with the `{:?}` of each, so
/// `agent_configs_set_every_field` can see a field that sends no key.
pub(super) fn agent_config_types() -> Vec<(String, AgentConfig)> {
    let deep_research = DeepResearchConfig::fully_populated();
    let antigravity = AntigravityConfig::fully_populated();
    let dynamic = DynamicConfig::fully_populated();
    let code_mender = CodeMenderConfig::fully_populated();
    vec![
        (format!("{deep_research:?}"), deep_research.into()),
        (format!("{antigravity:?}"), antigravity.into()),
        (format!("{dynamic:?}"), dynamic.into()),
        (format!("{code_mender:?}"), code_mender.into()),
    ]
}

pub(super) fn agent_configs() -> Vec<AgentConfig> {
    agent_config_types()
        .into_iter()
        .map(|(_, config)| config)
        .collect()
}

pub(super) fn response_formats() -> Vec<ResponseFormat> {
    let all = vec![
        ResponseFormat::Text {
            mime_type: Some("application/json".into()),
            schema: Some(json!({"type": "object"})),
            extra: Default::default(),
        },
        ResponseFormat::Audio {
            mime_type: Some("audio/wav".into()),
            delivery: Some(ResponseDelivery::Inline),
            sample_rate: Some(24_000),
            bit_rate: Some(64_000),
            extra: Default::default(),
        },
        ResponseFormat::Image {
            mime_type: Some("image/jpeg".into()),
            delivery: Some(ResponseDelivery::Uri),
            aspect_ratio: Some(ImageAspectRatio::Widescreen16x9),
            image_size: Some(ImageSize::Hd2k),
            extra: Default::default(),
        },
        ResponseFormat::Video {
            delivery: Some(ResponseDelivery::Uri),
            gcs_uri: Some("gs://bucket/out".into()),
            aspect_ratio: Some(ImageAspectRatio::Widescreen16x9),
            duration: Some("8s".into()),
            resolution: Some(VideoResolution::Hd720p),
            extra: Default::default(),
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
            (
                "PLAIN".to_string(),
                EnvVar {
                    value: Some("v".into()),
                    credential: None,
                    extra: serde_json::Map::new(),
                },
            ),
            (
                "SECRET".to_string(),
                EnvVar {
                    value: None,
                    credential: Some("credentials/c".into()),
                    extra: serde_json::Map::new(),
                },
            ),
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
        // The allowlist's `Disabled` arm (#489).
        EnvironmentSpec::Remote(RemoteEnvironment {
            network: Some(NetworkConfig::AllowlistDisabled),
            ..remote_environment()
        }),
    ]
}

pub(super) fn webhook_config() -> WebhookConfig {
    WebhookConfig {
        uris: Some(vec!["https://example.com/hook".into()]),
        user_metadata: Some(json!({"k": "v"})),
        extra: Default::default(),
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
                input: Some(input.clone()),
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
            input: Some(inputs().remove(i % 3)),
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
        signing_secrets: Some(vec![SigningSecret {
            truncated_secret: Some("abc".into()),
            expire_time: Some(chrono::DateTime::UNIX_EPOCH),
            extra: serde_json::Map::new(),
        }]),
        new_signing_secret: Some("secret".into()),
        create_time: Some(chrono::DateTime::UNIX_EPOCH),
        update_time: Some(chrono::DateTime::UNIX_EPOCH),
        extra: serde_json::Map::new(),
    };
    vec![to_value(&WebhookInput::from(&webhook))]
}

/// One update per state, every event subscribed: a state or event only an
/// update takes is a request-only enumeration, which `enum_values` holds to
/// being sent.
pub(super) fn webhook_updates() -> Vec<Value> {
    [
        WebhookState::Enabled,
        WebhookState::Disabled,
        WebhookState::DisabledDueToFailedDeliveries,
    ]
    .into_iter()
    .map(|state| {
        to_value(&WebhookUpdate {
            name: Some("hook".into()),
            uri: Some("https://example.com/hook".into()),
            subscribed_events: Some(vec![
                WebhookEvent::BatchSucceeded,
                WebhookEvent::BatchExpired,
                WebhookEvent::BatchFailed,
                WebhookEvent::InteractionRequiresAction,
                WebhookEvent::InteractionCompleted,
                WebhookEvent::InteractionFailed,
                WebhookEvent::VideoGenerated,
            ]),
            state: Some(state),
            update_mask: Some("name,uri".into()),
        })
    })
    .collect()
}

pub(super) fn trigger_params() -> Vec<Value> {
    agent_requests()
        .into_iter()
        .map(|interaction| {
            to_value(&CreateTriggerRequest {
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

/// One update per status, each a request-only value (see
/// [`webhook_updates`]).
pub(super) fn trigger_updates() -> Vec<Value> {
    [
        TriggerStatus::Active,
        TriggerStatus::Paused,
        TriggerStatus::Error,
    ]
    .into_iter()
    .map(|status| {
        to_value(&TriggerUpdate {
            display_name: Some("daily".into()),
            status: Some(status),
            extra: serde_json::Map::new(),
        })
    })
    .collect()
}

pub(super) fn agents() -> Vec<Value> {
    environment_specs()
        .into_iter()
        .map(|base_environment| {
            to_value(&Agent {
                id: Some("my-agent".into()),
                base_agent: Some(crate::DEFAULT_ANTIGRAVITY_AGENT.into()),
                agent_config: Some(AntigravityConfig::fully_populated().into()),
                system_instruction: Some("be brief".into()),
                description: Some("d".into()),
                tools: Some(vec![
                    tools().remove(0),
                    Tool::CodeExecution {
                        extra: serde_json::Map::new(),
                    },
                    Tool::UrlContext {
                        extra: serde_json::Map::new(),
                    },
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
        to_value(&CreateEnvironmentRequest {
            from_environment: None,
            sources: None,
            network: Some(NetworkConfig::AllowlistDisabled),
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
        extra: serde_json::Map::new(),
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

/// A decision over each input form, with one question of each type (every
/// field set) under the content input.
pub(super) fn decision_requests() -> Vec<Value> {
    let questions = BTreeMap::from([
        (
            "positive".to_string(),
            Question::Noul {
                instructions: "Is it positive?".into(),
                criteria: Some(NoulCriteria {
                    true_description: Some("liked".into()),
                    false_description: Some("disliked".into()),
                }),
            },
        ),
        (
            "topic".to_string(),
            Question::Choice {
                instructions: "What is it about?".into(),
                criteria: BTreeMap::from([
                    ("food".to_string(), "The food".to_string()),
                    ("service".to_string(), String::new()),
                ]),
            },
        ),
        (
            "stars".to_string(),
            Question::Score {
                instructions: "How many stars?".into(),
                criteria: vec!["zero".into(), "one".into()],
            },
        ),
    ]);
    [
        DecisionInput::Text("The soup was cold.".into()),
        DecisionInput::Content(contents()),
    ]
    .into_iter()
    .map(|input| {
        to_value(&CreateDecisionRequest {
            model: "models/m".into(),
            input,
            questions: questions.clone(),
        })
    })
    .collect()
}

pub(super) fn file_search_store_requests() -> Vec<Value> {
    vec![to_value(&CreateFileSearchStoreRequest {
        display_name: Some("docs".into()),
        embedding_model: Some("models/gemini-embedding-2".into()),
        extra: serde_json::Map::new(),
    })]
}

/// The custom metadata and chunking an upload or import sends, every value
/// kind included, one per entry, as the API takes them.
fn document_options() -> DocumentOptions {
    DocumentOptions {
        custom_metadata: vec![
            CustomMetadata {
                key: "team".into(),
                string_value: Some("search".into()),
                numeric_value: None,
                string_list_value: None,
                extra: serde_json::Map::new(),
            },
            CustomMetadata {
                key: "year".into(),
                string_value: None,
                numeric_value: Some(2026.0),
                string_list_value: None,
                extra: serde_json::Map::new(),
            },
            CustomMetadata {
                key: "tags".into(),
                string_value: None,
                numeric_value: None,
                string_list_value: Some(StringList {
                    values: vec!["a".into(), "b".into()],
                    extra: serde_json::Map::new(),
                }),
                extra: serde_json::Map::new(),
            },
        ],
        chunking_config: Some(ChunkingConfig {
            white_space_config: Some(WhiteSpaceConfig {
                max_tokens_per_chunk: Some(200),
                max_overlap_tokens: Some(20),
                extra: serde_json::Map::new(),
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
