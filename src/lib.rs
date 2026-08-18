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
//! - [`interactions_api`]: Helper functions for constructing content
//! - [`function_calling`]: Function registration and execution

// =============================================================================
// Internal HTTP Layer (pub(crate))
// =============================================================================
pub(crate) mod http;
pub(crate) mod serde_util;
#[cfg(test)]
pub(crate) mod test_subscriber;

// =============================================================================
// Model defaults
// =============================================================================

/// The model this crate is developed and verified against.
///
/// Exposed so that examples, tests and callers name one constant instead of
/// a string literal. That is not cosmetic: before this existed, a model bump
/// meant editing ~600 occurrences across the repo, and the sweep is exactly
/// the kind of mechanical change that misses a few and leaves them silently
/// pinned to a retired model.
///
/// A *default*, not a constraint — [`with_model`](InteractionBuilder::with_model)
/// accepts any model id, and picking one deliberately is normal.
///
/// Capability note: this model rejects **inline (base64) video** with
/// `400 invalid_request` while accepting video by URI (verified live on both
/// `gemini-3.6-flash` and `gemini-3.7-flash`). Image, audio and PDF inline
/// data are unaffected. See [`INLINE_VIDEO_MODEL`].
///
/// It also rejects [`ThinkingLevel::Minimal`] — see
/// [`MINIMAL_THINKING_MODEL`]. Both gaps are model capability, not library
/// limits, and both were found by running the live suite against the model
/// before adopting it.
pub const DEFAULT_MODEL: &str = "gemini-3.7-flash";

/// A model that accepts **inline (base64) video bytes**, which
/// [`DEFAULT_MODEL`] does not.
///
/// Only needed for the inline form; video by URI works on the default.
///
/// Re-pin independently of [`DEFAULT_MODEL`]: this tracks whichever model
/// currently has the capability, so it goes stale when *that* model is
/// retired rather than when the default moves. The literal guard cannot
/// help here — its whole job is to keep ids in this file, so an id that is
/// stale *in* this file is invisible to it by construction.
pub const INLINE_VIDEO_MODEL: &str = "gemini-3-flash-preview";

/// A model that supports [`ThinkingLevel::Minimal`], which
/// [`DEFAULT_MODEL`] does not.
///
/// `gemini-3.7-flash` rejects `minimal` with
/// `400 'minimal' is not a supported thinking level for this model.
/// Allowed values are: high, low, medium.` (verified live 2026-08-15;
/// `gemini-3.6-flash` and `gemini-3.5-flash` still accept it). The
/// [`ThinkingLevel::Minimal`] variant remains valid — model support for it
/// is what varies.
///
/// Re-pin independently of [`DEFAULT_MODEL`], as with [`INLINE_VIDEO_MODEL`]
/// — and sooner: this is pinned to the model the crate just migrated *off*,
/// so it goes stale when 3.6 is retired. `gemini-3.5-flash` also accepts
/// `minimal`, so the fix at that point is another re-pin here, not a
/// redesign. Left unfixed it surfaces as a 404 on an unrelated-looking
/// model inside a test whose subject is a thinking level.
pub const MINIMAL_THINKING_MODEL: &str = "gemini-3.6-flash";

/// The model to use for image generation.
///
/// Image output is a separate model family from [`DEFAULT_MODEL`]; passing
/// the default to an image-generation request will not produce images.
pub const DEFAULT_IMAGE_MODEL: &str = "gemini-3.1-flash-image";

/// The model to use for text-to-speech.
pub const DEFAULT_TTS_MODEL: &str = "gemini-2.5-pro-preview-tts";

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
};

// Step types (revision 2026-05-20 response model)
pub mod steps;
pub use steps::{FunctionResultPayload, Step, StepDelta, StepError};

// Request types (includes agent configuration)
pub mod request;
pub use request::{
    AgentConfig, AntigravityConfig, DeepResearchConfig, DynamicConfig, GenerationConfig,
    ImageAspectRatio, ImageConfig, ImageSize, InteractionInput, InteractionRequest, Role,
    ServiceTier, SpeechConfig, ThinkingLevel, ThinkingSummaries, TranscriptionConfig, TurnContent,
    VideoConfig, VideoTask, Visualization,
};

// Typed response_format union (text/audio/image/video + list form)
pub mod response_format;
pub use response_format::{ResponseDelivery, ResponseFormat, ResponseFormatSpec};

// Environment types (environment request field, agent base_environment)
pub mod environment;
pub use environment::{
    AllowlistEntry, EnvironmentSource, EnvironmentSpec, NetworkConfig, RemoteEnvironment,
    SourceType,
};

// Triggers resource (/v1beta/triggers) — server-side scheduled interactions
pub mod triggers;
pub use triggers::{
    Trigger, TriggerCreateParams, TriggerExecution, TriggerExecutionListResponse,
    TriggerExecutionStatus, TriggerListResponse, TriggerStatus, TriggerUpdate,
};

// Environments resource (/v1beta/environments)
pub mod environments;
pub use environments::{
    CreateEnvironmentRequest, Environment, EnvironmentListResponse, EnvironmentStatus,
};

// Safety settings (request safety_settings field)
pub mod safety;
pub use safety::{HarmCategory, SafetyMethod, SafetySetting, SafetyThreshold};

// Agents resource (/v1beta/agents)
pub mod agents;
pub use agents::{Agent, AgentListResponse};

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
    InteractionStatus, ModalityTokens, OwnedFunctionCallInfo, StepSummary, UrlContextResultInfo,
    UsageMetadata,
};

// Tool types (function declarations, built-in tools)
pub mod tools;
pub use tools::{
    AllowedTools, ComputerUseConfig, ExaAiSearchConfig, FileSearchConfig, FunctionCallingMode,
    FunctionDeclaration, FunctionDeclarationBuilder, FunctionParameters, GoogleMapsConfig,
    GoogleSearchConfig, HybridSearchConfig, McpServerConfig, ParallelAiSearchConfig, RagFilter,
    RagRanking, RagResource, RagRetrievalConfig, RagStoreConfig, RetrievalConfig, RetrievalType,
    SearchType, Tool, ToolChoice, VertexAiSearchConfig,
};

// Wire streaming types (from API)
pub mod wire_streaming;
pub use wire_streaming::{InteractionStreamEvent, StreamChunk, StreamEvent, StreamMetadata};

// Wire-level inspection (WireEvent, WireInspector, built-in inspectors)
pub mod wire;

// Native client for Google's Antigravity localharness agent runtime
// (feature = "antigravity"). See docs/ANTIGRAVITY.md.
#[cfg(feature = "antigravity")]
pub mod antigravity;

// Files API types
pub use http::files::{
    DEFAULT_CHUNK_SIZE, FileError, FileMetadata, FileState, ListFilesResponse, ResumableUpload,
    VideoMetadata,
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

// =============================================================================
// Streaming Types for Auto Function Calling
// =============================================================================

pub mod streaming;
pub use streaming::{
    AutoFunctionResult, AutoFunctionResultAccumulator, AutoFunctionStreamChunk,
    AutoFunctionStreamEvent, FunctionExecutionResult, PendingFunctionCall,
};

// =============================================================================
// Content Constructor Functions
// =============================================================================
//
// ## Export Strategy
//
// Model output constructors for testing and response simulation.
// Use `Content::*()` constructors for user input content.
pub mod interactions_api;

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

    // Detailed guides in docs/
    doc_comment!(include_str!("../docs/AGENTS_AND_BACKGROUND.md"));
    // ANTIGRAVITY.md uses `rust,ignore` code blocks: its snippets are
    // fragments (undefined `agent`/`client` bindings) compile-checked via the
    // feature-gated example and tests. The all-features doctest job could
    // compile them if promoted to `no_run` and made self-contained — a
    // possible future improvement.
    doc_comment!(include_str!("../docs/ANTIGRAVITY.md"));
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
    doc_comment!(include_str!("../docs/RELIABILITY_PATTERNS.md"));
    doc_comment!(include_str!("../docs/STREAMING_API.md"));
    doc_comment!(include_str!("../docs/TESTING.md"));
    doc_comment!(include_str!("../docs/THINKING_MODE.md"));
}
