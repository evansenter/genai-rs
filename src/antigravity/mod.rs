//! Native client for Google's Antigravity `localharness` agent runtime.
//!
//! The [`google-antigravity` Python SDK](https://pypi.org/project/google-antigravity/)
//! ships a Go binary (`localharness`) that *is* the agent runtime: model
//! calls, streaming, history/compaction, built-in tool execution (shell,
//! file edits, web search), MCP, and trajectory persistence all live inside
//! it. This module speaks the harness's protocol directly — a stdio
//! handshake plus proto-JSON over a localhost WebSocket — so Rust
//! applications get the full agent runtime with **Rust-native tools, hooks,
//! and policies** and no Python in the loop.
//!
//! Enable with the `antigravity` cargo feature. See `docs/ANTIGRAVITY.md`
//! for the full guide, and [`SUPPORTED_HARNESS_VERSION`] for the pinned
//! harness version.
//!
//! # Quick start
//!
//! ```rust,ignore
//! use genai_rs::antigravity::{AntigravityAgent, policy};
//!
//! let mut agent = AntigravityAgent::builder()
//!     .with_api_key(std::env::var("GEMINI_API_KEY")?)
//!     .with_model(genai_rs::DEFAULT_MODEL)
//!     .with_system_instructions("You are a code-review assistant.")
//!     .add_workspace("/path/to/repo")
//!     .add_policy(policy::deny_all())
//!     .add_policy(policy::allow("view_file"))
//!     .spawn()
//!     .await?;
//!
//! let response = agent.chat("Summarize src/lib.rs").await?;
//! println!("{}", response.text());
//! agent.shutdown().await?;
//! ```

mod agent;
mod builder;
mod config;
mod error;
mod handshake;
mod hook_mapping;
mod hooks;
mod process;
pub mod protocol;
mod session;
mod streaming;
mod tools;
pub mod triggers;
mod turn;

pub use agent::{AntigravityAgent, CancelHandle};
pub use builder::{AgentBuilder, DEFAULT_TURN_TIMEOUT};
pub use config::{
    AgentBehavior, BuiltinTool, Capabilities, McpServer, SUPPORTED_HARNESS_VERSION, Subagent,
};
pub use error::AntigravityError;
pub use hooks::{
    AgentQuestion, Policy, PolicyDecision, PostToolHook, PreToolDecision, PreToolHook,
    QuestionAnswer, QuestionHook, QuestionReply, ToolInvocation, ToolOutcome, policy,
};
pub use streaming::{AgentEvent, AgentEventStream, ErrorSeverity, ToolAction, ToolDecision};
pub use triggers::TriggerConfig;
pub use turn::ChatResponse;
