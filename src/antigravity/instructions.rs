//! [`SystemInstructions`]: added to the harness's own system prompt, or
//! replacing it.

use super::protocol;

/// The title a plain-text instruction section gets, as in the Python SDK:
/// the harness renders the text under `<user_system_instructions>`.
pub(crate) const USER_SECTION_TITLE: &str = "user_system_instructions";

/// System instructions for an agent or a [`Subagent`](super::Subagent).
///
/// The harness writes a system prompt of its own (7 to 10 thousand
/// characters on 0.1.23): tool guidance, safety rules, the workspace list,
/// the skills list and the built-in subagents. Instructions either **add
/// to** that prompt ([`appended`](Self::appended), which a plain string
/// converts to) or **replace** it ([`custom`](Self::custom)).
///
/// ```
/// use genai_rs::antigravity::SystemInstructions;
///
/// // Added to the harness's prompt under <user_system_instructions>.
/// let added: SystemInstructions = "Review for security issues only.".into();
/// assert!(!added.is_custom());
///
/// // The harness's prompt with its identity line replaced and a section
/// // of your own after it.
/// let persona = SystemInstructions::harness_default()
///     .with_identity("You are Tern, a release-notes writer.")
///     .add_section("style", "Write in the past tense.");
/// assert!(!persona.is_custom());
///
/// // Everything replaced: you write the whole prompt.
/// let custom = SystemInstructions::custom("You only ever answer `ok`.");
/// assert!(custom.is_custom());
/// ```
///
/// Replacing the prompt drops the harness's tool guidance and safety rules,
/// and with them the workspace list, the skills list (skills from
/// [`add_skills_path`](super::AgentBuilder::add_skills_path) are then
/// invisible to the model) and the built-in subagents. Prefer the appended
/// form unless you mean to write all of that yourself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemInstructions {
    custom: bool,
    identity: Option<String>,
    sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Section {
    title: Option<String>,
    content: String,
}

impl SystemInstructions {
    /// Instructions added to the end of the harness's prompt, under a
    /// `<user_system_instructions>` section (the Python SDK's string form).
    #[must_use]
    pub fn appended(text: impl Into<String>) -> Self {
        Self::harness_default().add_section(USER_SECTION_TITLE, text)
    }

    /// The harness's own prompt, unchanged, to add sections or an identity
    /// to (the Python SDK's `TemplatedSystemInstructions`).
    #[must_use]
    pub fn harness_default() -> Self {
        Self {
            custom: false,
            identity: None,
            sections: Vec::new(),
        }
    }

    /// Instructions that **replace** the harness's prompt entirely (the
    /// Python SDK's `CustomSystemInstructions`). The harness renders the
    /// text under a `<System>` tag; [`with_title`](Self::with_title)
    /// renames it.
    #[must_use]
    pub fn custom(text: impl Into<String>) -> Self {
        Self {
            custom: true,
            identity: None,
            sections: vec![Section {
                title: None,
                content: text.into(),
            }],
        }
    }

    /// Replaces the identity line the harness's prompt opens with ("You are
    /// …"). With [`custom`](Self::custom) instructions it becomes a leading
    /// `<identity>` section, the tag the harness's own prompt uses.
    #[must_use]
    pub fn with_identity(mut self, identity: impl Into<String>) -> Self {
        self.identity = Some(identity.into());
        self
    }

    /// Adds a section, rendered as `<title>content</title>`.
    #[must_use]
    pub fn add_section(mut self, title: impl Into<String>, content: impl Into<String>) -> Self {
        self.sections.push(Section {
            title: Some(title.into()),
            content: content.into(),
        });
        self
    }

    /// Retitles the first section (the one [`appended`](Self::appended) or
    /// [`custom`](Self::custom) created). No-op when there is none.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        if let Some(first) = self.sections.first_mut() {
            first.title = Some(title.into());
        }
        self
    }

    /// Whether these instructions replace the harness's prompt.
    #[must_use]
    pub fn is_custom(&self) -> bool {
        self.custom
    }

    /// The wire form. `workspace_note`, when present, is added as a final
    /// part; callers pass one only for custom instructions, since the
    /// harness's own prompt already lists the workspaces.
    pub(crate) fn to_wire(&self, workspace_note: Option<&str>) -> protocol::SystemInstructions {
        if self.custom {
            let identity = self
                .identity
                .iter()
                .map(|identity| protocol::SystemInstructionPart {
                    text: Some(identity.clone()),
                    title: Some("identity".to_string()),
                });
            let sections = self
                .sections
                .iter()
                .map(|section| protocol::SystemInstructionPart {
                    text: Some(section.content.clone()),
                    title: section.title.clone(),
                });
            let note = workspace_note.map(|note| protocol::SystemInstructionPart {
                text: Some(note.to_string()),
                title: None,
            });
            return protocol::SystemInstructions {
                custom: Some(protocol::CustomSystemInstructions {
                    part: identity.chain(sections).chain(note).collect(),
                }),
                appended: None,
            };
        }
        let sections = self
            .sections
            .iter()
            .map(|section| protocol::InstructionSection {
                title: Some(
                    section
                        .title
                        .clone()
                        .unwrap_or_else(|| USER_SECTION_TITLE.to_string()),
                ),
                content: Some(section.content.clone()),
            })
            .chain(workspace_note.map(|note| protocol::InstructionSection {
                title: Some("Workspace".to_string()),
                content: Some(note.to_string()),
            }))
            .collect();
        protocol::SystemInstructions {
            custom: None,
            appended: Some(protocol::AppendedSystemInstructions {
                custom_identity: self.identity.clone(),
                appended_sections: sections,
            }),
        }
    }
}

impl From<&str> for SystemInstructions {
    fn from(text: &str) -> Self {
        Self::appended(text)
    }
}

impl From<String> for SystemInstructions {
    fn from(text: String) -> Self {
        Self::appended(text)
    }
}

impl From<&String> for SystemInstructions {
    fn from(text: &String) -> Self {
        Self::appended(text.as_str())
    }
}

impl From<std::borrow::Cow<'_, str>> for SystemInstructions {
    fn from(text: std::borrow::Cow<'_, str>) -> Self {
        Self::appended(text)
    }
}

impl From<Box<str>> for SystemInstructions {
    fn from(text: Box<str>) -> Self {
        Self::appended(text)
    }
}

#[cfg(test)]
#[path = "instructions_tests.rs"]
mod tests;
