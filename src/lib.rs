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
//!         .with_model("gemini-3-flash-preview")
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
    GoogleSearchResultItem, Place, Resolution, UrlContextResultItem,
};

// Request types (includes agent configuration)
pub mod request;
pub use request::{
    AgentConfig, DeepResearchConfig, DynamicConfig, GenerationConfig, InteractionInput,
    InteractionRequest, Role, SpeechConfig, ThinkingLevel, ThinkingSummaries, Turn, TurnContent,
};

// Response types
pub mod response;
pub use response::{
    AudioInfo, CodeExecutionCallInfo, CodeExecutionResultInfo, ContentSummary, FunctionCallInfo,
    FunctionResultInfo, GoogleMapsResultInfo, GroundingChunk, GroundingMetadata, ImageInfo,
    InteractionResponse, InteractionStatus, ModalityTokens, OwnedFunctionCallInfo,
    UrlContextMetadata, UrlContextResultInfo, UrlMetadataEntry, UrlRetrievalStatus, UsageMetadata,
    WebSource,
};

// Tool types (function declarations, built-in tools)
pub mod tools;
pub use tools::{
    ComputerUseConfig, FileSearchConfig, FunctionCallingMode, FunctionDeclaration,
    FunctionDeclarationBuilder, FunctionParameters, GoogleMapsConfig, GoogleSearchConfig,
    McpServerConfig, SearchType, Tool,
};

// Wire streaming types (from API)
pub mod wire_streaming;
pub use wire_streaming::{InteractionStreamEvent, StreamChunk, StreamEvent};

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

    // Detailed guides in docs/
    doc_comment!(include_str!("../docs/AGENTS_AND_BACKGROUND.md"));
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
