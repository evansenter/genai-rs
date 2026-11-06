//! What a turn sends the agent: text, inline media and slash commands (the
//! Python SDK's `types.Content`).

use std::borrow::Cow;
use std::path::Path;

use base64::Engine;

use super::AntigravityError;
use super::protocol;

/// A message for [`chat`](super::AntigravityAgent::chat) or
/// [`send_streaming`](super::AntigravityAgent::send_streaming): text,
/// [`Media`] and [`SlashCommand`]s, in order (the Python SDK's
/// `types.Content`). A string is a prompt of one text part.
///
/// ```
/// use genai_rs::antigravity::{Media, Prompt, SlashCommand};
///
/// let png = vec![0x89, b'P', b'N', b'G'];
/// let prompt = Prompt::new()
///     .add_slash_command(SlashCommand::Plan)
///     .add_text("What does this chart show?")
///     .add_media(Media::new(png, "image/png").with_description("Q3 revenue"));
/// assert_eq!(prompt.len(), 3);
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Prompt {
    parts: Vec<protocol::UserInputPart>,
}

impl Prompt {
    /// An empty prompt; add parts to it.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a text part.
    #[must_use]
    pub fn add_text(mut self, text: impl Into<String>) -> Self {
        self.parts.push(protocol::UserInputPart::text(text));
        self
    }

    /// Adds inline media: an image, document, audio or video.
    #[must_use]
    pub fn add_media(mut self, media: Media) -> Self {
        self.parts.push(protocol::UserInputPart {
            media: Some(media.to_wire()),
            ..Default::default()
        });
        self
    }

    /// Adds a slash command.
    #[must_use]
    pub fn add_slash_command(mut self, command: SlashCommand) -> Self {
        self.parts.push(protocol::UserInputPart {
            slash_command: Some(protocol::SlashCommand {
                name: Some(command.name().to_string()),
            }),
            ..Default::default()
        });
        self
    }

    /// The number of parts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.parts.len()
    }

    /// Whether the prompt has no parts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    pub(crate) fn to_wire(&self) -> protocol::UserInput {
        protocol::UserInput {
            parts: self.parts.clone(),
        }
    }
}

/// The input event a turn sends for `prompt`. An empty [`Prompt`] is a
/// configuration error: the harness would start a turn with nothing in it.
pub(crate) fn user_input(
    prompt: impl Into<Prompt>,
) -> Result<protocol::InputEvent, AntigravityError> {
    let prompt = prompt.into();
    if prompt.is_empty() {
        return Err(AntigravityError::Config(
            "a Prompt needs at least one part".to_string(),
        ));
    }
    Ok(protocol::InputEvent::UserInput(prompt.to_wire()))
}

impl From<&str> for Prompt {
    fn from(text: &str) -> Self {
        Self::new().add_text(text)
    }
}

impl From<String> for Prompt {
    fn from(text: String) -> Self {
        Self::new().add_text(text)
    }
}

impl From<&String> for Prompt {
    fn from(text: &String) -> Self {
        Self::new().add_text(text.as_str())
    }
}

impl From<Cow<'_, str>> for Prompt {
    fn from(text: Cow<'_, str>) -> Self {
        Self::new().add_text(text)
    }
}

impl From<Box<str>> for Prompt {
    fn from(text: Box<str>) -> Self {
        Self::new().add_text(text)
    }
}

impl From<Media> for Prompt {
    fn from(media: Media) -> Self {
        Self::new().add_media(media)
    }
}

impl From<SlashCommand> for Prompt {
    fn from(command: SlashCommand) -> Self {
        Self::new().add_slash_command(command)
    }
}

/// The MIME types the Python SDK's `Image`, `Document`, `Audio` and `Video`
/// accept (0.1.23). Others are sent with a warning, not refused: the model
/// decides.
const SUPPORTED_MIME_TYPES: &[&str] = &[
    // Image
    "image/bmp",
    "image/jpeg",
    "image/png",
    "image/webp",
    // Document
    "application/pdf",
    "application/json",
    "text/css",
    "text/csv",
    "text/html",
    "text/javascript",
    "text/plain",
    "text/rtf",
    "text/xml",
    // Audio
    "audio/wav",
    "audio/x-wav",
    "audio/wave",
    "audio/vnd.wave",
    "audio/mp3",
    "audio/mp4",
    "audio/webm",
    "audio/aac",
    "audio/ogg",
    "audio/flac",
    "audio/opus",
    "audio/mpeg",
    "audio/m4a",
    "audio/l16",
    // Video
    "video/3gpp",
    "video/avi",
    "video/mp4",
    "video/mpeg",
    "video/mpg",
    "video/quicktime",
    "video/webm",
    "video/wmv",
    "video/x-flv",
];

/// Inline media for a [`Prompt`]: an image, document, audio or video clip
/// as bytes and a MIME type (the Python SDK's `Image`, `Document`, `Audio`
/// and `Video`, which differ only in the MIME types they accept). The
/// harness hands it to the model as inline data, after the prompt's text.
#[derive(Clone, PartialEq, Eq)]
pub struct Media {
    data: Vec<u8>,
    mime_type: String,
    description: Option<String>,
}

impl std::fmt::Debug for Media {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Media")
            .field("data", &format_args!("<{} bytes>", self.data.len()))
            .field("mime_type", &self.mime_type)
            .field("description", &self.description)
            .finish()
    }
}

impl Media {
    /// Media from bytes and a MIME type (`image/png`, `application/pdf`,
    /// `audio/wav`, `video/mp4`, ...). The MIME type is trimmed and
    /// lowercased. One the Python SDK does not list is sent with a
    /// `warn!`, not refused.
    #[must_use]
    pub fn new(data: impl Into<Vec<u8>>, mime_type: impl Into<String>) -> Self {
        let mime_type = mime_type.into().trim().to_ascii_lowercase();
        if !SUPPORTED_MIME_TYPES.contains(&mime_type.as_str()) {
            tracing::warn!(
                "Media MIME type '{mime_type}' is not one the Antigravity Python SDK accepts; \
                 sending it anyway."
            );
        }
        Self {
            data: data.into(),
            mime_type,
            description: None,
        }
    }

    /// Reads a file, taking the MIME type from its extension (the Python
    /// SDK's `from_file`).
    ///
    /// # Errors
    ///
    /// [`AntigravityError::Config`] when the extension names no known type,
    /// and [`AntigravityError::Io`] when the file cannot be read.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, AntigravityError> {
        let path = path.as_ref();
        let mime_type = crate::detect_mime_type(path).ok_or_else(|| {
            AntigravityError::Config(format!(
                "cannot infer a MIME type from the extension of {}; use Media::new(bytes, \
                 mime_type)",
                path.display()
            ))
        })?;
        let data = std::fs::read(path)?;
        Ok(Self::new(data, mime_type))
    }

    /// A description of the media, sent to the harness beside it (the
    /// Python SDK's `description`). Harness 0.1.23 does not pass it on to
    /// the model (verified 2026-11-05), so put what the model must know in
    /// a text part.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// The raw bytes.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// The MIME type, normalized.
    #[must_use]
    pub fn mime_type(&self) -> &str {
        &self.mime_type
    }

    /// The description, when set.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    fn to_wire(&self) -> protocol::Media {
        protocol::Media {
            mime_type: Some(self.mime_type.clone()),
            description: self.description.clone(),
            data: Some(base64::engine::general_purpose::STANDARD.encode(&self.data)),
        }
    }
}

/// A harness slash command (the Python SDK's `BuiltinSlashCommandName`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SlashCommand {
    /// `/plan`: plan before acting (the Python SDK describes it as writing
    /// an implementation plan and waiting for approval). Harness 0.1.23
    /// passes it to the model as `/plan` before the prompt's text.
    Plan,
}

impl SlashCommand {
    /// The command's wire name, without the slash.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Plan => "plan",
        }
    }
}

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod tests;
