//! [`CodeExecutionOutput`], the `result` of a `code_execution_result` step.

use std::borrow::Cow;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::content::Content;

/// The `result` of a [`Step::CodeExecutionResult`](super::Step::CodeExecutionResult).
///
/// The wire format is a union (google-genai 3.0.0): the program's output as
/// a plain string, or a list of content blocks. Both forms serialize back
/// exactly as they arrived, so replayed history keeps the form the API sent.
/// The Gemini API's current form is recorded under Code execution in
/// `docs/ENUM_WIRE_FORMATS.md`.
///
/// [`text`](Self::text) reads either form as text, and `Display` prints it.
///
/// # Example
///
/// ```
/// use genai_rs::{CodeExecutionOutput, Content};
///
/// let printed = CodeExecutionOutput::from("42\n");
/// assert_eq!(printed.as_text(), Some("42\n"));
///
/// let blocks = CodeExecutionOutput::from(vec![Content::text("42\n")]);
/// assert_eq!(blocks.text(), "42\n");
/// assert_eq!(
///     serde_json::to_value(&blocks).unwrap(),
///     serde_json::json!([{"type": "text", "text": "42\n"}])
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum CodeExecutionOutput {
    /// The output as a string: stdout on success, the error on failure.
    Text(String),
    /// The output as content blocks. An unrecognized block is kept as
    /// [`Content::Unknown`].
    Contents(Vec<Content>),
}

impl Default for CodeExecutionOutput {
    /// An empty string, which is what a `step.start` announces before the
    /// output arrives in deltas.
    fn default() -> Self {
        Self::Text(String::new())
    }
}

impl CodeExecutionOutput {
    /// Returns the string, if this is the string form.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Contents(_) => None,
        }
    }

    /// Returns the content blocks, if this is the list form.
    #[must_use]
    pub fn as_contents(&self) -> Option<&[Content]> {
        match self {
            Self::Contents(contents) => Some(contents),
            Self::Text(_) => None,
        }
    }

    /// The output as text: the string form as it is, or the list form's
    /// text blocks joined (its other blocks, such as images, are left out;
    /// [`as_contents`](Self::as_contents) has them).
    #[must_use]
    pub fn text(&self) -> Cow<'_, str> {
        match self {
            Self::Text(text) => Cow::Borrowed(text),
            Self::Contents(contents) => {
                let mut texts = contents.iter().filter_map(Content::as_text);
                match (texts.next(), texts.next()) {
                    (None, _) => Cow::Borrowed(""),
                    (Some(only), None) => Cow::Borrowed(only),
                    (Some(first), Some(second)) => {
                        let mut joined = format!("{first}{second}");
                        joined.extend(texts);
                        Cow::Owned(joined)
                    }
                }
            }
        }
    }

    /// Appends a streamed fragment: to the string, or to the list's last
    /// block when that is text, else as a new text block.
    pub(crate) fn push_str(&mut self, fragment: &str) {
        match self {
            Self::Text(text) => text.push_str(fragment),
            Self::Contents(contents) => match contents.last_mut() {
                Some(Content::Text {
                    text: Some(text), ..
                }) => text.push_str(fragment),
                _ => contents.push(Content::text(fragment)),
            },
        }
    }
}

impl fmt::Display for CodeExecutionOutput {
    /// Writes [`text`](Self::text).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text())
    }
}

impl From<String> for CodeExecutionOutput {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for CodeExecutionOutput {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

impl From<Vec<Content>> for CodeExecutionOutput {
    fn from(contents: Vec<Content>) -> Self {
        Self::Contents(contents)
    }
}

impl<'de> Deserialize<'de> for CodeExecutionOutput {
    /// A string or a list of content blocks; anything else is an error, which
    /// sends the whole step to [`Step::Unknown`](super::Step::Unknown).
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match serde_json::Value::deserialize(deserializer)? {
            serde_json::Value::String(text) => Ok(Self::Text(text)),
            list @ serde_json::Value::Array(_) => serde_json::from_value(list)
                .map(Self::Contents)
                .map_err(serde::de::Error::custom),
            other => Err(serde::de::Error::custom(format!(
                "expected a string or a list of content blocks, got {other}"
            ))),
        }
    }
}

#[cfg(test)]
#[path = "code_execution_tests.rs"]
mod tests;
