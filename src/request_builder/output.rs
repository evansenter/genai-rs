use super::InteractionBuilder;
use crate::{
    GenerationConfig, ImageConfig, ResponseFormat, ResponseFormatSpec, SpeechConfig,
    TranscriptionConfig, VideoConfig,
};

impl<'a> InteractionBuilder<'a> {
    /// Sets response modalities (e.g., `["image"]`).
    ///
    /// The API is case-sensitive and only accepts lowercase modality names
    /// (`text`, `image`, `audio`, `video`, `document` — verified live), so
    /// each provided value is lowercased before being sent. The list stays
    /// `Vec<String>` (open enum) so new modalities pass through unchanged.
    ///
    /// Deprecation signal: the official SDK marks `response_modalities` as
    /// deprecated in favor of the typed `response_format` union — prefer
    /// [`with_response_format`](Self::with_response_format) for new code.
    #[must_use]
    pub fn with_response_modalities(mut self, modalities: Vec<String>) -> Self {
        self.response_modalities = Some(modalities.into_iter().map(|m| m.to_lowercase()).collect());
        self
    }

    /// Configures the request to return image output.
    ///
    /// This is a convenience method equivalent to:
    /// ```ignore
    /// .with_response_modalities(vec!["image".to_string()])
    /// ```
    ///
    /// Use this when you want the model to generate images. Requires a model
    /// that supports image generation (e.g. [`DEFAULT_IMAGE_MODEL`](crate::DEFAULT_IMAGE_MODEL)).
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
    ///     .with_model(genai_rs::DEFAULT_IMAGE_MODEL)
    ///     .with_text("A cute cat playing with yarn")
    ///     .with_image_output()
    ///     .create()
    ///     .await?;
    ///
    /// // Extract generated image
    /// if let Some(bytes) = response.first_image_bytes()? {
    ///     std::fs::write("cat.png", &bytes)?;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_image_output(self) -> Self {
        self.with_response_modalities(vec!["image".to_string()])
    }

    /// Configures the request to return audio output.
    ///
    /// This is a convenience method equivalent to:
    /// ```ignore
    /// .with_response_modalities(vec!["audio".to_string()])
    /// ```
    ///
    /// Use this when you want the model to generate speech audio. Requires a model
    /// that supports text-to-speech (e.g. [`DEFAULT_TTS_MODEL`](crate::DEFAULT_TTS_MODEL)).
    ///
    /// For voice customization, chain with [`with_speech_config`](Self::with_speech_config)
    /// or [`with_voice`](Self::with_voice).
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
    ///     .with_model(genai_rs::DEFAULT_TTS_MODEL)
    ///     .with_text("Hello, world! Welcome to text-to-speech.")
    ///     .with_audio_output()
    ///     .with_voice("Kore")
    ///     .create()
    ///     .await?;
    ///
    /// // Extract generated audio using the helper methods
    /// if let Some(audio) = response.first_audio() {
    ///     let bytes = audio.bytes()?;
    ///     std::fs::write("speech.wav", &bytes)?;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_audio_output(self) -> Self {
        self.with_response_modalities(vec!["audio".to_string()])
    }

    /// Sets a single speech configuration for text-to-speech output,
    /// replacing any previously set speaker configs.
    ///
    /// Use this to customize voice, language, and speaker settings when
    /// generating audio output. On the wire, `speech_config` is a list; this
    /// method sends a single-entry list. For multi-speaker TTS use
    /// [`with_speech_configs()`](Self::with_speech_configs) or
    /// [`add_speech_config()`](Self::add_speech_config).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, SpeechConfig};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let config = SpeechConfig {
    ///     voice: Some("Puck".to_string()),
    ///     language: Some("en-US".to_string()),
    ///     speaker: None,
    /// };
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_TTS_MODEL)
    ///     .with_text("Hello from Puck!")
    ///     .with_audio_output()
    ///     .with_speech_config(config)
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_speech_config(mut self, config: SpeechConfig) -> Self {
        self.speech_configs = Some(vec![config]);
        self
    }

    /// Sets the full list of speaker configurations for multi-speaker
    /// text-to-speech, replacing any previously set configs.
    ///
    /// Each entry's `speaker` names a speaker the input's turns refer to.
    /// On [`DEFAULT_TTS_MODEL`](crate::DEFAULT_TTS_MODEL) every text turn
    /// must carry that name via [`Content::speaker_text`](crate::Content::speaker_text); older TTS models
    /// instead read an `Alice: ...` transcript and reject the annotation
    /// (verified live 2026-09-24). Either way one combined audio stream is
    /// returned.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, Content, InteractionInput, SpeechConfig};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_TTS_MODEL)
    ///     .with_input(InteractionInput::Content(vec![
    ///         Content::speaker_text("Alice", "Hi Bob!"),
    ///         Content::speaker_text("Bob", "Hey Alice, how are you?"),
    ///     ]))
    ///     .with_audio_output()
    ///     .with_speech_configs(vec![
    ///         SpeechConfig { voice: Some("Kore".into()), language: Some("en-US".into()), speaker: Some("Alice".into()) },
    ///         SpeechConfig { voice: Some("Puck".into()), language: Some("en-US".into()), speaker: Some("Bob".into()) },
    ///     ])
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_speech_configs(mut self, configs: Vec<SpeechConfig>) -> Self {
        self.speech_configs = Some(configs);
        self
    }

    /// Adds one speaker configuration, accumulating for multi-speaker
    /// text-to-speech.
    ///
    /// See [`with_speech_configs()`](Self::with_speech_configs) for the
    /// replace-all form.
    #[must_use]
    pub fn add_speech_config(mut self, config: SpeechConfig) -> Self {
        self.speech_configs
            .get_or_insert_with(Vec::new)
            .push(config);
        self
    }

    /// Sets the image generation configuration.
    ///
    /// Controls aspect ratio and size for image generation output.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, ImageConfig, ImageAspectRatio, ImageSize};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let config = ImageConfig {
    ///     aspect_ratio: Some(ImageAspectRatio::Widescreen16x9),
    ///     image_size: Some(ImageSize::Hd2k),
    /// };
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_IMAGE_MODEL)
    ///     .with_text("Generate a landscape photo")
    ///     .with_image_config(config)
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_image_config(mut self, config: ImageConfig) -> Self {
        let gen_config = self
            .generation_config
            .get_or_insert_with(GenerationConfig::default);
        gen_config.image_config = Some(config);
        self
    }

    /// Sets the video generation configuration
    /// (`generation_config.video_config`).
    ///
    /// Controls the video generation task mode. Combine with
    /// [`with_video_output()`](Self::with_video_output) and optionally a
    /// video [`ResponseFormat`] for delivery options.
    ///
    /// Live availability note (2026-07): Veo models (e.g.
    /// `veo-3.1-generate-preview`) are listed by `/v1beta/models` with only
    /// the legacy `predictLongRunning` method and return
    /// `404 "Model ... not found"` from the Interactions API; no
    /// Interactions-served model supported the `video` response modality at
    /// verification time. The `video_config` field itself is schema-valid
    /// (its `task` enum is validated server-side).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, VideoConfig, VideoTask};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model("veo-3.1-generate-preview")
    ///     .with_text("A hummingbird hovering over a flower, slow motion")
    ///     .with_video_output()
    ///     .with_video_config(VideoConfig::new().with_task(VideoTask::TextToVideo))
    ///     .with_background(true)
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_video_config(mut self, config: VideoConfig) -> Self {
        let gen_config = self
            .generation_config
            .get_or_insert_with(GenerationConfig::default);
        gen_config.video_config = Some(config);
        self
    }

    /// Sets the audio transcription configuration
    /// (`generation_config.transcription_config`).
    ///
    /// Controls language hints, diarization, custom vocabulary and
    /// timestamp granularity when transcribing audio input.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, TranscriptionConfig};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Transcribe the attached audio")
    ///     .with_transcription_config(
    ///         TranscriptionConfig::new().with_language_codes(["en-US"]),
    ///     )
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_transcription_config(mut self, config: TranscriptionConfig) -> Self {
        let gen_config = self
            .generation_config
            .get_or_insert_with(GenerationConfig::default);
        gen_config.transcription_config = Some(config);
        self
    }

    /// Configures the request to return video output.
    ///
    /// This is a convenience method equivalent to:
    /// ```ignore
    /// .with_response_modalities(vec!["video".to_string()])
    /// ```
    ///
    /// Requires a model that supports video generation. Video generation
    /// typically runs in the background — pair with `with_background(true)`
    /// and poll or use webhooks (`video.generated` event) for completion.
    /// See `docs/OUTPUT_MODALITIES.md`.
    #[must_use]
    pub fn with_video_output(self) -> Self {
        self.with_response_modalities(vec!["video".to_string()])
    }

    /// Sets the voice for text-to-speech output (defaults to en-US language).
    ///
    /// This is a convenience method that sets the voice with a default language of "en-US".
    /// For other languages, use [`with_speech_config`](Self::with_speech_config).
    ///
    /// # Available Voices
    ///
    /// Common voices include: Aoede, Charon, Fenrir, Kore, Puck, and others.
    /// See [Google's TTS documentation](https://ai.google.dev/gemini-api/docs/text-generation)
    /// for the full list.
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
    ///     .with_model(genai_rs::DEFAULT_TTS_MODEL)
    ///     .with_text("Hello, world!")
    ///     .with_audio_output()
    ///     .with_voice("Kore")
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_voice(self, voice: impl Into<String>) -> Self {
        // Language is required by the API, default to en-US
        self.with_speech_config(SpeechConfig::with_voice_and_language(voice, "en-US"))
    }

    /// Sets a JSON schema to enforce structured output from the model.
    ///
    /// When you provide a JSON schema, the model will return responses that
    /// conform exactly to your schema structure. This is useful for:
    /// - Extracting structured data from text
    /// - Building reliable data pipelines
    /// - Ensuring consistent API responses
    ///
    /// The schema should be a standard JSON Schema object with `type`, `properties`,
    /// and optionally `required` fields.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    /// use serde_json::json;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let schema = json!({
    ///     "type": "object",
    ///     "properties": {
    ///         "name": {"type": "string"},
    ///         "age": {"type": "integer"},
    ///         "hobbies": {
    ///             "type": "array",
    ///             "items": {"type": "string"}
    ///         }
    ///     },
    ///     "required": ["name", "age"]
    /// });
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Generate info for someone named Alice who is 30 and likes hiking")
    ///     .with_response_format(schema)
    ///     .create()
    ///     .await?;
    ///
    /// // Response is guaranteed to be valid JSON matching the schema
    /// let text = response.as_text().unwrap();
    /// let data: serde_json::Value = serde_json::from_str(text)?;
    /// println!("Name: {}", data["name"]);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Combining with Tools
    ///
    /// Structured output can be combined with built-in tools like Google Search
    /// or URL Context to get structured data from real-time sources:
    ///
    /// ```no_run
    /// # use genai_rs::Client;
    /// # use serde_json::json;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("api-key".to_string());
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("What is the current weather in Tokyo?")
    ///     .with_google_search()
    ///     .with_response_format(json!({
    ///         "type": "object",
    ///         "properties": {
    ///             "temperature": {"type": "string"},
    ///             "conditions": {"type": "string"}
    ///         },
    ///         "required": ["temperature", "conditions"]
    ///     }))
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Typed Formats
    ///
    /// Beyond raw JSON schemas, this accepts any
    /// [`ResponseFormat`] — audio, image, and video
    /// output formats included. A raw `serde_json::Value` schema converts to
    /// `ResponseFormat::Text { mime_type: "application/json", schema }`
    /// (the pre-0.8 wire behavior wrapped in the typed union). For the list
    /// form use [`with_response_formats()`](Self::with_response_formats).
    ///
    /// ```no_run
    /// # use genai_rs::{Client, ResponseDelivery, ResponseFormat};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("api-key".to_string());
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_TTS_MODEL)
    ///     .with_text("Read this aloud")
    ///     .with_audio_output()
    ///     .with_response_format(ResponseFormat::Audio {
    ///         mime_type: Some("audio/mp3".to_string()),
    ///         delivery: Some(ResponseDelivery::Inline),
    ///         sample_rate: Some(24000),
    ///         bit_rate: None,
    ///     })
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_response_format(mut self, format: impl Into<ResponseFormat>) -> Self {
        self.response_format = Some(ResponseFormatSpec::Single(format.into()));
        self
    }

    /// Sets a list of response formats (one per requested output modality).
    ///
    /// Use with [`with_response_modalities()`](Self::with_response_modalities)
    /// when requesting multiple output modalities.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::{Client, ResponseFormat};
    /// # use serde_json::json;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("api-key".to_string());
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_IMAGE_MODEL)
    ///     .with_text("A labeled diagram of a volcano")
    ///     .with_response_formats(vec![
    ///         ResponseFormat::text_plain(),
    ///         ResponseFormat::Image {
    ///             mime_type: Some("image/jpeg".to_string()),
    ///             delivery: None,
    ///             aspect_ratio: None,
    ///             image_size: None,
    ///         },
    ///     ])
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_response_formats(mut self, formats: Vec<ResponseFormat>) -> Self {
        self.response_format = Some(ResponseFormatSpec::List(formats));
        self
    }
}
