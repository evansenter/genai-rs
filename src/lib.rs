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
//! - **Built-in Tools**: Google Search, Google Maps, Code Execution, URL
//!   Context, File Search, MCP servers and more
//! - **Multimodal**: Images, audio, video, and document inputs
//! - **Thinking Mode**: Access model reasoning with configurable levels
//! - **Resources**: `client.agents()`, `.webhooks()`, `.triggers()`,
//!   `.environments()`, `.credentials()`, `.voices()`, `.files()` and
//!   `.file_search_stores()`, with paged `.items()` streams
//! - **Decisions**: `client.decisions()` answers noul, choice and score
//!   questions about an input with calibrated probabilities
//! - **Background agents and decoding continuation**: Deep Research and
//!   Antigravity agents in the background, and
//!   [`CONTINUATION_MODEL`] answers resumed past `max_output_tokens`
//! - **Local agents** (feature `antigravity`): a native client for the
//!   Antigravity harness, with workspace containment, policies and hooks
//! - **Wire inspection**: `LOUD_WIRE=1` prints every request, response and
//!   SSE frame with no setup; a [`wire::WireInspector`] captures them as
//!   structured events. Library logs go through `tracing`, so `RUST_LOG`
//!   applies once your application installs a subscriber
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
//! For tests that want to *find* unknowns, strict mode
//! (`RUSTFLAGS="--cfg genai_strict_unknown"`, a cfg rather than a Cargo
//! feature) makes unknown `Content`, `Step` and string-enum values errors.
//!
//! ## Module Organization
//!
//! - [`Client`]: Main entry point for API interactions
//! - [`InteractionBuilder`]: Fluent builder for configuring requests
//! - [`Content`] and [`Step`]: Constructors for request content and history
//! - [`function_calling`]: Function registration and execution

// `clippy.toml` exempts a `#[cfg(test)]` module from `unwrap_used`,
// `expect_used` and `panic`; clippy has no such setting for the other
// panicking macros, so the unit-test build allows them here. The library
// build still denies them (D-048).
#![cfg_attr(
    test,
    allow(
        clippy::unreachable,
        clippy::todo,
        clippy::unimplemented,
        reason = "a test reports a failure by panicking"
    )
)]

// =============================================================================
// Internal HTTP Layer (pub(crate))
// =============================================================================
pub(crate) mod http;
pub(crate) mod paging;
pub(crate) mod poll;
pub(crate) mod redact;
pub(crate) mod serde_util;
#[cfg(test)]
pub(crate) mod test_subscriber;
pub(crate) mod wire_enum;
pub(crate) mod wire_fields;

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
/// Capability gap: this model rejects [`ThinkingLevel::Minimal`] with a
/// 400.
pub const DEFAULT_MODEL: &str = "gemini-3.8-flash";

/// The model to use for image generation.
///
/// Image output is a separate model family from [`DEFAULT_MODEL`], and there
/// is no 3.8 image model: `gemini-3.8-flash` answers an image request with
/// text only.
pub const DEFAULT_IMAGE_MODEL: &str = "gemini-3.1-flash-image";

/// The model to use for text-to-speech.
///
/// Returns `audio/wav` (a RIFF container, playable as-is) rather than raw
/// L16 PCM. Multi-speaker requests need a speaker annotation on each text
/// turn; see [`Content::speaker_text`].
pub const DEFAULT_TTS_MODEL: &str = "gemini-3.8-flash-tts";

/// The model to use for speech-to-text transcription.
///
/// It answers audio input with its transcript, and is the model on which
/// [`TranscriptionConfig`] changes the output: a verbatim
/// [`TranscriptionMode`] adds `word_info` annotations
/// ([`Annotation::WordInfo`]) with timings and speakers, and a smart mode's
/// `source_text` steers the wording. It ignores a text prompt beside the
/// audio, answers text-only input with nothing, and rejects function tools.
pub const DEFAULT_TRANSCRIPTION_MODEL: &str = "gemini-3.5-transcribe";

/// The model to use for decoding continuation, the only one that supports
/// it (every other model answers `400 Decoding continuation is not enabled
/// for this model`). See
/// [`InteractionBuilder::with_continuation_token`].
pub const CONTINUATION_MODEL: &str = "gemini-4-argon";

/// The model for [`Decisions::create`], the one model the decisions
/// endpoint serves: every other model answers it with a `404 Requested
/// model not found`, and this one serves nothing but decisions.
pub const DEFAULT_DECISION_MODEL: &str = "gemini-decision-1";

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
///
/// `antigravity-preview-09-2026` replaced the retired
/// `antigravity-preview-05-2026`. Its built-in tools are renamed and take
/// PascalCase parameters, so code that parses its `function_call` steps by
/// tool name must match the new names.
pub const DEFAULT_ANTIGRAVITY_AGENT: &str = "antigravity-preview-09-2026";

// =============================================================================
// Core Type Modules
// =============================================================================

// Error types
pub mod errors;
pub use errors::GenaiError;

// Content types (Content and related)
pub mod content;
pub use content::{
    Annotation, CodeExecutionLanguage, Content, GoogleMapsResultItem, GoogleSearchResultItem,
    Place, Resolution, ReviewSnippet, UrlContextResultItem, VideoProcessing,
    VideoProcessingBuilder,
};

// Step types (revision 2026-05-20 response model)
pub mod steps;
pub use steps::{CodeExecutionOutput, FunctionResultPayload, Step, StepDelta, StepError};

// Request types (includes agent configuration)
pub mod request;
pub use request::{
    AgentConfig, AntigravityConfig, CodeMenderConfig, CodeMenderFile, CodeMenderFindRequest,
    CodeMenderFixRequest, CodeMenderMode, DeepResearchConfig, DynamicConfig, GenerationConfig,
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
    CreateTriggerRequest, ListTriggerExecutions, ListTriggers, Trigger, TriggerExecution,
    TriggerExecutionListResponse, TriggerExecutionStatus, TriggerListResponse, TriggerStatus,
    TriggerUpdate, Triggers,
};

// Environments: the spec (environment request field, agent
// base_environment), the /v1beta/environments resource, and its files
pub mod environments;
pub use environments::{
    AllowlistEntry, CreateEnvironmentRequest, EnvVar, Environment, EnvironmentFile,
    EnvironmentFileDownload, EnvironmentFileListResponse, EnvironmentFileType,
    EnvironmentFileUpload, EnvironmentFiles, EnvironmentListResponse, EnvironmentSource,
    EnvironmentSpec, EnvironmentStatus, Environments, ListEnvironmentFiles, ListEnvironments,
    NetworkConfig, RemoteEnvironment, SourceType,
};

// File Search Stores resource (/v1beta/fileSearchStores) — the documents
// `Tool::FileSearch` retrieves over
pub mod file_search_stores;
pub use file_search_stores::{
    ChunkingConfig, CreateFileSearchStoreRequest, CustomMetadata, DocumentImport, DocumentState,
    DocumentUpload, FileSearchDocument, FileSearchDocumentListResponse, FileSearchDocuments,
    FileSearchStore, FileSearchStoreListResponse, FileSearchStores, ListFileSearchDocuments,
    ListFileSearchStores, StringList, WhiteSpaceConfig,
};

// Safety settings (request safety_settings field)
pub mod safety;
pub use safety::{HarmCategory, SafetyMethod, SafetySetting, SafetyThreshold};

// Agents resource (/v1beta/agents)
pub mod agents;
pub use agents::{Agent, AgentListResponse, Agents, ListAgents};

pub mod credentials;
pub use credentials::{
    CreateCredentialRequest, Credential, CredentialConfig, CredentialListResponse,
    CredentialStatus, CredentialType, CredentialUpdate, Credentials, InjectionLocation,
    ListCredentials,
};

// Decisions resource (/v1beta/decisions) — calibrated answers to
// structured questions about an input
pub mod decisions;
pub use decisions::{
    Answer, BlockReason, CreateDecisionRequest, CreateDecisionResponse, DecisionInput, Decisions,
    NoulCriteria, PromptFeedback, Question,
};

pub mod voices;
pub use voices::{
    CreateVoiceRequest, ListVoices, PromptedVoice, ReplicatedVoice, Voice, VoiceAudio,
    VoiceListResponse, VoicePitch, VoiceSpec, VoiceType, Voices,
};

// Webhooks resource (/v1beta/webhooks) and per-request webhook_config
pub mod webhooks;
pub use webhooks::{
    ListWebhooks, RevocationBehavior, RotateSigningSecretResponse, SigningSecret, Webhook,
    WebhookConfig, WebhookEvent, WebhookListResponse, WebhookState, WebhookUpdate, Webhooks,
};

// Response types
pub mod response;
pub use response::{
    AudioInfo, CodeExecutionCallInfo, CodeExecutionResultInfo, FunctionCallInfo,
    FunctionResultInfo, GoogleMapsResultInfo, GroundingToolCount, ImageInfo, InteractionError,
    InteractionResponse, InteractionStatus, McpCallInfo, ModalityTokens, OwnedFunctionCallInfo,
    StepSummary, ToolCallInfo, UrlContextResultInfo, UsageMetadata,
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
    FileError, FileListResponse, FileMetadata, FileSource, FileState, FileUpload,
    FileUploadResponse, Files, ListFiles, PollOptions, VideoMetadata,
};

// =============================================================================
// Client and Builder
// =============================================================================

pub mod client;
pub use client::{Client, ClientBuilder, DEFAULT_CONNECT_TIMEOUT};

mod interactions;
pub use interactions::Interactions;

pub mod request_builder;
pub use request_builder::{ConversationBuilder, InteractionBuilder};

// =============================================================================
// Function Calling
// =============================================================================

pub mod function_calling;
pub use function_calling::{CallableFunction, FunctionError, FunctionFuture, ToolService};

/// The plumbing `#[tool]` expands to. **Not a public API** — no semver
/// guarantee, and nothing outside the generated code should name it (#490).
///
/// The generated code registers each function with `inventory` through
/// `submit!` and `CallableFunctionFactory`, builds its declaration with
/// `function_declaration`, and builds JSON with `serde_json`. Resolving
/// `::serde_json` would look it up in the *consumer's* dependency graph,
/// which forced every downstream crate to add it as a direct dependency just
/// to use the macro (#402). Routing through here resolves both in this
/// crate's graph instead, so `genai-rs` + `genai-rs-macros` is the whole
/// dependency list.
#[doc(hidden)]
pub mod __private {
    pub use inventory::submit;
    pub use serde_json;

    /// A `#[tool]` function's registration, collected by `inventory` into
    /// the registry `create_with_auto_functions` consults.
    #[derive(Debug)]
    pub struct CallableFunctionFactory {
        pub(crate) factory_fn: fn() -> Box<dyn crate::CallableFunction>,
    }

    impl CallableFunctionFactory {
        /// Registers the function `factory_fn` builds.
        pub const fn new(factory_fn: fn() -> Box<dyn crate::CallableFunction>) -> Self {
            Self { factory_fn }
        }
    }

    /// A `#[tool]` function's declaration: its name and description, and an
    /// object schema of `properties` with the `required` names. The
    /// constructors it calls are crate-private, so the generated code
    /// reaches them only through here.
    #[must_use]
    pub fn function_declaration(
        name: &str,
        description: &str,
        properties: serde_json::Value,
        required: Vec<String>,
    ) -> crate::FunctionDeclaration {
        crate::FunctionDeclaration::new(
            name,
            description,
            crate::FunctionParameters::new("object", properties, required),
        )
    }
}

/// Entry points for the `cargo fuzz` targets in `fuzz/`. **Not a public
/// API**: it exists only under `cfg(fuzzing)` (set by `cargo fuzz`) and in
/// this crate's tests.
#[cfg(any(fuzzing, test))]
#[doc(hidden)]
#[path = "fuzzing.rs"]
pub mod __fuzz;

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
mod coverage_oracle;
#[cfg(test)]
mod proptest_tests;
#[cfg(test)]
mod streaming_tests;

// =============================================================================
// Documentation Tests
// =============================================================================
//
// These include markdown documentation files for doctest verification.
// Code blocks in markdown use annotations:
// - `rust` - Compiled and run (needs no key or network)
// - `rust,no_run` - Compiled but not executed (needs an API key, or the
//   Antigravity harness)
// - `rust,ignore` - Not compiled; each one carries a one-line
//   `<!-- ignore: reason -->` comment above its fence
// - `rust,compile_fail` - Should fail compilation
//
// Run with: cargo test --workspace --doc --all-features (CI also runs the
// default feature set and strict mode). Not included: CLAUDE.md files,
// CHANGELOG.md, and the maintainer notes docs/INTERACTIONS_API_GAP.md and
// docs/ANTIGRAVITY_BRIDGE_DESIGN.md, which Cargo.toml's `include` leaves out
// of the published crate. `tests/doc_guards/` fails on any other Markdown
// file under docs/, or top-level one with a Rust block, left out of this
// list (`NOT_DOCTESTED` has each exclusion's reason), and
// check_dated_facts.sh on a Rust block in a file left out that is not
// `ignore`d with a reason. Both read only an include in code (not a
// comment) under no `cfg` the doctest run leaves out.

#[cfg(doctest)]
mod doc_tests {
    // Root-level documentation
    #[doc = include_str!("../README.md")]
    mod readme {}
    #[doc = include_str!("../TROUBLESHOOTING.md")]
    mod troubleshooting {}
    #[doc = include_str!("../CONTRIBUTING.md")]
    mod contributing {}
    #[doc = include_str!("../DECISIONS.md")]
    mod decisions {}
    #[doc = include_str!("../SECURITY.md")]
    mod security {}

    // Detailed guides in docs/
    #[doc = include_str!("../docs/AGENTS_AND_BACKGROUND.md")]
    mod agents_and_background {}
    // ANTIGRAVITY.md's blocks use `genai_rs::antigravity`, so they compile
    // only with the feature: the all-features doctest job runs them.
    #[cfg(feature = "antigravity")]
    #[doc = include_str!("../docs/ANTIGRAVITY.md")]
    mod antigravity {}
    #[doc = include_str!("../docs/BUILDER_API.md")]
    mod builder_api {}
    #[doc = include_str!("../docs/BUILT_IN_TOOLS.md")]
    mod built_in_tools {}
    #[doc = include_str!("../docs/CONFIGURATION.md")]
    mod configuration {}
    #[doc = include_str!("../docs/CONVERSATIONS.md")]
    mod conversations {}
    #[doc = include_str!("../docs/ENUM_WIRE_FORMATS.md")]
    mod enum_wire_formats {}
    #[doc = include_str!("../docs/ERROR_HANDLING.md")]
    mod error_handling {}
    #[doc = include_str!("../docs/EXAMPLES_INDEX.md")]
    mod examples_index {}
    #[doc = include_str!("../docs/FUNCTION_CALLING.md")]
    mod function_calling {}
    #[doc = include_str!("../docs/LOGGING_STRATEGY.md")]
    mod logging_strategy {}
    #[doc = include_str!("../docs/MULTIMODAL.md")]
    mod multimodal {}
    #[doc = include_str!("../docs/OUTPUT_MODALITIES.md")]
    mod output_modalities {}
    #[doc = include_str!("../docs/RELIABILITY.md")]
    mod reliability {}
    #[doc = include_str!("../docs/RESOURCES.md")]
    mod resources {}
    #[doc = include_str!("../docs/STREAMING_API.md")]
    mod streaming_api {}
    #[doc = include_str!("../docs/TESTING.md")]
    mod testing {}
    #[doc = include_str!("../docs/THINKING_MODE.md")]
    mod thinking_mode {}

    /// `FunctionDeclaration::new` and `FunctionParameters::new` are
    /// crate-private: `#[tool]` reaches them through `__private`, and a
    /// caller builds a declaration with `FunctionDeclaration::builder`.
    /// A doctest rather than a trybuild fixture, which would pin rustc's
    /// text for a note that points into this crate's source and a help
    /// line that names a trait from the dependency graph.
    ///
    /// ```compile_fail,E0624
    /// let _ = genai_rs::FunctionParameters::new("object", Default::default(), Vec::new());
    /// ```
    ///
    /// ```compile_fail,E0624
    /// let built = genai_rs::FunctionDeclaration::builder("name").build();
    /// let _ = genai_rs::FunctionDeclaration::new("name", "", built.parameters().clone());
    /// ```
    mod private_function_constructors {}
}
