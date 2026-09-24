//! # genai-rs
//!
//! A Rust client library for Google's Generative AI (Gemini) API using the Interactions API.
//!
//! ## Quick Start
//!
//! ```no_run
//! use genai_rs::Client;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), genai_rs::GenaiError> {
//!     let client = Client::new(
//!         std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not set")
//!     );
//!
//!     let response = client
//!         .interaction()
//!         .with_model(genai_rs::DEFAULT_MODEL)
//!         .with_text("Hello, Gemini!")
//!         .create()
//!         .await?;
//!
//!     println!("{}", response.as_text().unwrap_or("No response"));
//!     Ok(())
//! }
//! ```
//!
//! ## Features
//!
//! - **Fluent Builder API**: Chain methods for readable request construction
//! - **Streaming**: Real-time response streaming with `create_stream()`
//! - **Function Calling**: Automatic function discovery and execution via macros
//! - **Built-in Tools**: Google Search, Code Execution, URL Context
//! - **Multimodal**: Images, audio, video, and document inputs
//! - **Thinking Mode**: Access model reasoning with configurable levels
//!
//! ## API Stability & Forward Compatibility
//!
//! This library is designed for forward compatibility with evolving APIs:
//!
//! - **`#[non_exhaustive]` enums**: Match statements require wildcard arms (`_ => ...`)
//! - **`Unknown` variants**: Unrecognized API types are captured, not rejected
//! - **Graceful degradation**: New API features won't break existing code
//!
//! When Google adds new features, your code continues to work. Unknown content types
//! and tools are preserved for inspection via helper methods like `has_unknown()`.
//!
//! ## Module Organization
//!
//! - [`Client`]: Main entry point for API interactions
//! - [`InteractionBuilder`]: Fluent builder for configuring requests
//! - [`Content`] and [`Step`]: Constructors for request content and history
//! - [`function_calling`]: Function registration and execution

// =============================================================================
// Internal HTTP Layer (pub(crate))
// =============================================================================
pub(crate) mod http;
pub(crate) mod serde_util;
#[cfg(test)]
pub(crate) mod test_subscriber;
pub(crate) mod wire_enum;

// =============================================================================
// Model defaults
// =============================================================================

/// The model this crate is developed and verified against.
///
/// Examples, tests and callers name this constant rather than a string
/// literal, so a model bump is a one-line change (`tests/model_literals.rs`
/// enforces it). It is a default, not a constraint:
/// [`with_model`](InteractionBuilder::with_model) accepts any model id.
///
/// Capability gap: this model rejects [`ThinkingLevel::Minimal`] — see
/// [`MINIMAL_THINKING_MODEL`] (verified live 2026-09-24).
pub const DEFAULT_MODEL: &str = "gemini-3.8-flash";

/// A model that supports [`ThinkingLevel::Minimal`], which
/// [`DEFAULT_MODEL`] rejects (`'minimal' is not a supported thinking level
/// for this model. Allowed values are: high, low, medium.`).
///
/// `gemini-3.7-flash` and `gemini-3.8-flash` both reject it; `gemini-3.6-flash`
/// and `gemini-3.5-flash` accept it (verified live 2026-09-24). Pinned
/// independently of [`DEFAULT_MODEL`]: it tracks whichever model has the
/// capability, so it goes stale when *that* model is retired.
pub const MINIMAL_THINKING_MODEL: &str = "gemini-3.6-flash";

/// The model to use for image generation.
///
/// Image output is a separate model family from [`DEFAULT_MODEL`]; passing
/// the default to an image-generation request will not produce images.
pub const DEFAULT_IMAGE_MODEL: &str = "gemini-3.1-flash-image";

/// The model to use for text-to-speech.
///
/// Returns `audio/wav` (a RIFF container, playable as-is) rather than raw
/// L16 PCM. Multi-speaker requests need a speaker annotation on each text
/// turn; see [`Content::speaker_text`].
pub const DEFAULT_TTS_MODEL: &str = "gemini-3.8-flash-tts";

/// The Deep Research agent id, for [`with_agent`](InteractionBuilder::with_agent).
///
/// Agent interactions require `background = true`.
pub const DEFAULT_DEEP_RESEARCH_AGENT: &str = "deep-research-preview-04-2026";

/// The managed Antigravity agent id, for
/// [`with_agent`](InteractionBuilder::with_agent).
///
/// Requires an `environment` (see [`EnvironmentSpec`]) and `background = true`.
/// Unrelated to the `antigravity` cargo feature, which drives a *local*
/// harness process instead.
pub const DEFAULT_ANTIGRAVITY_AGENT: &str = "antigravity-preview-05-2026";

// =============================================================================
// Core Type Modules
// =============================================================================

// Error types
pub mod errors;
pub use errors::GenaiError;

// Content types (Content and related)
pub mod content;
pub use content::{
    Annotation, CodeExecutionLanguage, Content, FileSearchResultItem, GoogleMapsResultItem,
    GoogleSearchResultItem, Place, Resolution, ReviewSnippet, UrlContextResultItem,
    VideoProcessing, VideoProcessingBuilder,
};

// Step types (revision 2026-05-20 response model)
pub mod steps;
pub use steps::{FunctionResultPayload, Step, StepDelta, StepError};

// Request types (includes agent configuration)
pub mod request;
pub use request::{
    AgentConfig, AntigravityConfig, DeepResearchConfig, DynamicConfig, GenerationConfig,
    ImageAspectRatio, ImageConfig, ImageSize, InteractionInput, InteractionRequest, Role,
    ServiceTier, SpeechConfig, ThinkingLevel, ThinkingSummaries, TranscriptionConfig,
    TranscriptionMode, TurnContent, VideoConfig, VideoTask, Visualization,
};

// Typed response_format union (text/audio/image/video + list form)
pub mod response_format;
pub use response_format::{ResponseDelivery, ResponseFormat, ResponseFormatSpec, VideoResolution};

// Triggers resource (/v1beta/triggers) — server-side scheduled interactions
pub mod triggers;
pub use triggers::{
    Trigger, TriggerCreateParams, TriggerExecution, TriggerExecutionListResponse,
    TriggerExecutionStatus, TriggerListResponse, TriggerStatus, TriggerUpdate,
};

// Environments: the spec (environment request field, agent
// base_environment), the /v1beta/environments resource, and its files
pub mod environments;
pub use environments::{
    AllowlistEntry, CreateEnvironmentRequest, EnvVar, Environment, EnvironmentFile,
    EnvironmentFileList, EnvironmentFileType, EnvironmentFileUpload, EnvironmentListResponse,
    EnvironmentSource, EnvironmentSpec, EnvironmentStatus, NetworkConfig, RemoteEnvironment,
    SourceType,
};

// File Search Stores resource (/v1beta/fileSearchStores) — the documents
// `Tool::FileSearch` retrieves over
pub mod file_search_stores;
pub use file_search_stores::{
    CreateFileSearchStoreRequest, DocumentListResponse, DocumentState, FileSearchDocument,
    FileSearchStore, FileSearchStoreListResponse,
};

// Safety settings (request safety_settings field)
pub mod safety;
pub use safety::{HarmCategory, SafetyMethod, SafetySetting, SafetyThreshold};

// Agents resource (/v1beta/agents)
pub mod agents;
pub use agents::{Agent, AgentListResponse};

pub mod credentials;
pub use credentials::{
    CreateCredentialRequest, Credential, CredentialConfig, CredentialListResponse,
    CredentialStatus, CredentialType, CredentialUpdate, InjectionLocation,
};

pub mod voices;
pub use voices::{
    CreateVoiceRequest, ListVoicesParams, PromptedVoice, ReplicatedVoice, Voice, VoiceAudio,
    VoiceListResponse, VoicePitch, VoiceSpec, VoiceType,
};

// Webhooks resource (/v1beta/webhooks) and per-request webhook_config
pub mod webhooks;
pub use webhooks::{
    RevocationBehavior, RotateSigningSecretResponse, SigningSecret, Webhook, WebhookConfig,
    WebhookEvent, WebhookListResponse, WebhookState, WebhookUpdate,
};

// Response types
pub mod response;
pub use response::{
    AudioInfo, CodeExecutionCallInfo, CodeExecutionResultInfo, FunctionCallInfo,
    FunctionResultInfo, GoogleMapsResultInfo, GroundingToolCount, ImageInfo, InteractionResponse,
    InteractionStatus, ModalityTokens, OwnedFunctionCallInfo, StepSummary, ToolCallInfo,
    UrlContextResultInfo, UsageMetadata,
};

// Tool types (function declarations, built-in tools)
pub mod tools;
pub use tools::{
    AllowedTools, ComputerUseConfig, ExaAiSearchConfig, FileSearchConfig, FunctionCallingMode,
    FunctionDeclaration, FunctionDeclarationBuilder, FunctionParameters, GoogleMapsConfig,
    GoogleSearchConfig, HybridSearchConfig, McpServerConfig, ParallelAiSearchConfig, RagFilter,
    RagRanking, RagResource, RagRetrievalConfig, RagStoreConfig, RankService, RetrievalConfig,
    RetrievalType, SearchType, Tool, ToolChoice, VertexAiSearchConfig,
};

// Wire streaming types (from API)
pub mod wire_streaming;
pub use wire_streaming::{StreamChunk, StreamEvent};

// Wire-level inspection (WireEvent, WireInspector, built-in inspectors)
pub mod wire;

// Native client for Google's Antigravity localharness agent runtime
// (feature = "antigravity"). See docs/ANTIGRAVITY.md.
#[cfg(feature = "antigravity")]
pub mod antigravity;

// Files API (/v1beta/files)
pub mod files;
pub use files::{
    FileError, FileMetadata, FileState, FileUploadResponse, ListFilesResponse, VideoMetadata,
};

// =============================================================================
// Client and Builder
// =============================================================================

pub mod client;
pub use client::{Client, ClientBuilder};

pub mod request_builder;
pub use request_builder::{ConversationBuilder, InteractionBuilder};

// =============================================================================
// Function Calling
// =============================================================================

pub mod function_calling;
pub use function_calling::{CallableFunction, FunctionError, ToolService};

/// Re-exports for `genai-rs-macros`. **Not a public API** — no semver
/// guarantee, and nothing outside the generated code should name it.
///
/// `#[tool]` expands to code referencing `async_trait` and `serde_json`.
/// Resolving those as `::async_trait` / `::serde_json` looks them up in the
/// *consumer's* dependency graph, which forced every downstream crate to add
/// both as direct dependencies just to use the macro (#402). Routing through
/// here resolves them in this crate's graph instead, so `genai-rs` +
/// `genai-rs-macros` is the whole dependency list.
#[doc(hidden)]
pub mod __private {
    pub use async_trait;
    pub use serde_json;
}

// =============================================================================
// Streaming Types for Auto Function Calling
// =============================================================================

pub mod streaming;
pub use streaming::{
    AutoFunctionResult, AutoFunctionResultAccumulator, AutoFunctionStreamChunk,
    AutoFunctionStreamEvent, FunctionExecutionResult,
};

// =============================================================================
// Multimodal File Loading Utilities
// =============================================================================

pub mod multimodal;
pub use multimodal::{
    audio_from_file, audio_from_file_with_mime, detect_mime_type, document_from_file,
    document_from_file_with_mime, image_from_file, image_from_file_with_mime, video_from_file,
    video_from_file_with_mime,
};

// =============================================================================
// Test Modules
// =============================================================================

#[cfg(test)]
mod content_tests;
#[cfg(test)]
mod proptest_tests;
#[cfg(test)]
mod request_tests;
#[cfg(test)]
mod response_tests;
#[cfg(test)]
mod streaming_tests;

// =============================================================================
// Documentation Tests
// =============================================================================
//
// These include markdown documentation files for doctest verification.
// Code blocks in markdown use annotations:
// - `rust,ignore` - Not compiled (incomplete snippets)
// - `rust,no_run` - Compiled but not executed (needs API key)
// - `rust,compile_fail` - Should fail compilation
//
// Run with: cargo test --doc

#[cfg(doctest)]
mod doc_tests {
    use doc_comment::doc_comment;

    // Root-level documentation
    doc_comment!(include_str!("../README.md"));
    doc_comment!(include_str!("../TROUBLESHOOTING.md"));
    doc_comment!(include_str!("../CONTRIBUTING.md"));
    doc_comment!(include_str!("../DECISIONS.md"));
    doc_comment!(include_str!("../SECURITY.md"));

    // Detailed guides in docs/
    doc_comment!(include_str!("../docs/AGENTS_AND_BACKGROUND.md"));
    // ANTIGRAVITY.md uses `rust,ignore` code blocks: its snippets are
    // fragments (undefined `agent`/`client` bindings) compile-checked via the
    // feature-gated example and tests. The all-features doctest job could
    // compile them if promoted to `no_run` and made self-contained — a
    // possible future improvement.
    doc_comment!(include_str!("../docs/ANTIGRAVITY.md"));
    doc_comment!(include_str!("../docs/BUILDER_API.md"));
    doc_comment!(include_str!("../docs/BUILT_IN_TOOLS.md"));
    doc_comment!(include_str!("../docs/CONFIGURATION.md"));
    doc_comment!(include_str!("../docs/CONVERSATION_PATTERNS.md"));
    doc_comment!(include_str!("../docs/ENUM_WIRE_FORMATS.md"));
    doc_comment!(include_str!("../docs/ERROR_HANDLING.md"));
    doc_comment!(include_str!("../docs/EXAMPLES_INDEX.md"));
    doc_comment!(include_str!("../docs/FUNCTION_CALLING.md"));
    doc_comment!(include_str!("../docs/LOGGING_STRATEGY.md"));
    doc_comment!(include_str!("../docs/MULTI_TURN_FUNCTION_CALLING.md"));
    doc_comment!(include_str!("../docs/MULTIMODAL.md"));
    doc_comment!(include_str!("../docs/OUTPUT_MODALITIES.md"));
    doc_comment!(include_str!("../docs/RELIABILITY.md"));
    doc_comment!(include_str!("../docs/STREAMING_API.md"));
    doc_comment!(include_str!("../docs/TESTING.md"));
    doc_comment!(include_str!("../docs/THINKING_MODE.md"));
}
