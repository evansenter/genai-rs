use super::InteractionBuilder;
use crate::Step;

impl<'a> InteractionBuilder<'a> {
    /// Starts building a conversation with a fluent API.
    ///
    /// Returns a [`ConversationBuilder`] that allows chaining `.user()` and `.model()`
    /// calls to construct a multi-turn conversation. Call `.done()` to return to
    /// the [`InteractionBuilder`].
    ///
    /// This is an alternative to [`with_history()`] that provides a more readable
    /// syntax for constructing conversations inline.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .conversation()
    ///         .user("What is 2+2?")
    ///         .model("2+2 equals 4.")
    ///         .user("And what's that times 3?")
    ///         .done()
    ///     .create()
    ///     .await?;
    ///
    /// println!("{}", response.as_text().unwrap_or("No response"));
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`with_history()`]: InteractionBuilder::with_history
    #[must_use]
    pub fn conversation(self) -> ConversationBuilder<'a> {
        ConversationBuilder {
            parent: self,
            steps: Vec::new(),
        }
    }
}

// ============================================================================
// ConversationBuilder - Fluent API for building multi-turn conversations
// ============================================================================

/// Builder for constructing multi-turn conversations with a fluent API.
///
/// Created via [`InteractionBuilder::conversation()`]. Allows chaining `.user()` and
/// `.model()` calls to build a conversation history, then `.done()` to return to
/// the parent builder.
///
/// # Example
///
/// ```no_run
/// use genai_rs::Client;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new("api-key".to_string());
///
/// let response = client
///     .interaction()
///     .with_model(genai_rs::DEFAULT_MODEL)
///     .conversation()
///         .user("What is the capital of France?")
///         .model("The capital of France is Paris.")
///         .user("What's the population?")
///         .done()
///     .create()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct ConversationBuilder<'a> {
    parent: InteractionBuilder<'a>,
    steps: Vec<Step>,
}

impl<'a> ConversationBuilder<'a> {
    /// Adds a user message to the conversation.
    ///
    /// Accepts any type that can be converted to [`TurnContent`](crate::TurnContent), including:
    /// - `&str` or `String` for text content
    /// - `Vec<Content>` for multimodal content
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .conversation()
    ///         .user("Hello!")
    ///         .done()
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn user(mut self, content: impl Into<crate::TurnContent>) -> Self {
        self.steps.push(match content.into() {
            crate::TurnContent::Text(text) => Step::user_text(text),
            crate::TurnContent::Parts(parts) => Step::user_input(parts),
        });
        self
    }

    /// Adds a model message to the conversation.
    ///
    /// Use this to include previous model responses in the conversation history.
    /// The model will use this context when generating its next response.
    ///
    /// Accepts any type that can be converted to [`TurnContent`](crate::TurnContent), including:
    /// - `&str` or `String` for text content
    /// - `Vec<Content>` for multimodal content
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .conversation()
    ///         .user("What is 2+2?")
    ///         .model("2+2 equals 4.")
    ///         .user("Multiply that by 3")
    ///         .done()
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn model(mut self, content: impl Into<crate::TurnContent>) -> Self {
        self.steps.push(match content.into() {
            crate::TurnContent::Text(text) => Step::model_text(text),
            crate::TurnContent::Parts(parts) => Step::model_output(parts),
        });
        self
    }

    /// Adds a turn with an explicit role.
    ///
    /// This is useful when you need to dynamically construct conversations
    /// where the role is determined at runtime.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, Role};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let role = Role::User; // Determined at runtime
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .conversation()
    ///         .turn(role, "Dynamic message")
    ///         .done()
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn turn(self, role: crate::Role, content: impl Into<crate::TurnContent>) -> Self {
        match role {
            crate::Role::Model => self.model(content),
            // User and unknown roles are treated as user input; the wire has
            // no role field on steps, only the step type.
            _ => self.user(content),
        }
    }

    /// Finishes building the conversation and returns to the parent [`InteractionBuilder`].
    ///
    /// The accumulated steps are set as the input for the interaction.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .conversation()
    ///         .user("Hello!")
    ///         .done()  // Returns to InteractionBuilder
    ///     .create()    // Now we can call create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn done(self) -> InteractionBuilder<'a> {
        let mut parent = self.parent;
        parent.history = self.steps;
        parent
    }
}
