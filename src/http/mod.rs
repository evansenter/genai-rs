//! Internal HTTP layer for Gemini API communication.
//!
//! This module is `pub(crate)` - it contains implementation details
//! not exposed to library users.

pub(crate) mod common;
pub(crate) mod context;
pub(crate) mod error_helpers;
pub(crate) mod files;
pub(crate) mod interactions;
pub(crate) mod sse_parser;
