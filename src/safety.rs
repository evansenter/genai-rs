//! Safety settings for interaction requests.
//!
//! A [`SafetySetting`] pairs a [`HarmCategory`] with a blocking
//! [`SafetyThreshold`] (and optionally a [`SafetyMethod`]), and is sent via
//! [`InteractionRequest::safety_settings`](crate::request::InteractionRequest::safety_settings).
//!
//! All enums here follow the Evergreen soft-typing pattern: unrecognized
//! wire values deserialize into `Unknown` variants that preserve the
//! original data for roundtrip serialization.
//!
//! Accepted by the Gemini API (verified live 2026-10-30; the parameter was
//! Vertex-only through 2026-10-08). The wire object is
//! `{"type", "threshold", "method"?}`, and the API validates every value.
//! Some values are still rejected as Vertex-only (400 "...allowed on the
//! Gemini Enterprise Agent Platform"): [`HarmCategory::CivicIntegrity`],
//! the four `Image*` categories and [`SafetyThreshold::BlockNone`].

use crate::wire_enum::wire_enum;
use serde::{Deserialize, Serialize};

wire_enum! {
    /// Category of harmful content a [`SafetySetting`] applies to.
    ///
    /// # Wire Format
    ///
    /// Serializes as snake_case strings: `"hate_speech"`, `"dangerous_content"`,
    /// `"harassment"`, `"sexually_explicit"`, `"civic_integrity"`,
    /// `"image_hate"`, `"image_dangerous_content"`, `"image_harassment"`,
    /// `"image_sexually_explicit"`, `"jailbreak"`.
    pub enum HarmCategory {
        /// Hateful or discriminatory speech.
        HateSpeech = "hate_speech",
        /// Content that facilitates or encourages dangerous acts.
        DangerousContent = "dangerous_content",
        /// Harassing or bullying content.
        Harassment = "harassment",
        /// Sexually explicit content.
        SexuallyExplicit = "sexually_explicit",
        /// Content that could undermine civic processes. Vertex-only on the
        /// Gemini API (verified live 2026-10-30).
        CivicIntegrity = "civic_integrity",
        /// Hateful imagery. Vertex-only on the Gemini API (2026-10-30).
        ImageHate = "image_hate",
        /// Dangerous imagery. Vertex-only on the Gemini API (2026-10-30).
        ImageDangerousContent = "image_dangerous_content",
        /// Harassing imagery. Vertex-only on the Gemini API (2026-10-30).
        ImageHarassment = "image_harassment",
        /// Sexually explicit imagery. Vertex-only on the Gemini API
        /// (2026-10-30).
        ImageSexuallyExplicit = "image_sexually_explicit",
        /// Prompt-injection / jailbreak attempts.
        Jailbreak = "jailbreak",
    }
    unknown(category_type, unknown_category_type)
}

wire_enum! {
    /// Blocking threshold for a [`SafetySetting`].
    ///
    /// # Wire Format
    ///
    /// Serializes as snake_case strings: `"block_low_and_above"`,
    /// `"block_medium_and_above"`, `"block_only_high"`, `"block_none"`, `"off"`.
    pub enum SafetyThreshold {
        /// Block content with low probability of harm and above.
        BlockLowAndAbove = "block_low_and_above",
        /// Block content with medium probability of harm and above.
        BlockMediumAndAbove = "block_medium_and_above",
        /// Block only content with high probability of harm.
        BlockOnlyHigh = "block_only_high",
        /// Never block for this category, but keep safety scoring on.
        /// Vertex-only on the Gemini API (verified live 2026-10-30).
        BlockNone = "block_none",
        /// Disable the safety filter for this category entirely.
        Off = "off",
    }
    unknown(threshold_type, unknown_threshold_type)
}

wire_enum! {
    /// Scoring method a [`SafetySetting`] blocks on.
    ///
    /// # Wire Format
    ///
    /// Serializes as lowercase strings: `"severity"`, `"probability"`.
    /// When unset, the API defaults to the probability score.
    pub enum SafetyMethod {
        /// Block based on the severity score.
        Severity = "severity",
        /// Block based on the probability score (API default).
        Probability = "probability",
    }
    unknown(method_type, unknown_method_type)
}

/// A safety setting that affects the safety-blocking behavior for one
/// [`HarmCategory`].
///
/// # Example
///
/// ```
/// use genai_rs::{HarmCategory, SafetySetting, SafetyThreshold};
///
/// let setting = SafetySetting::new(HarmCategory::Harassment, SafetyThreshold::BlockOnlyHigh);
/// assert_eq!(
///     serde_json::to_value(&setting).unwrap(),
///     serde_json::json!({"type": "harassment", "threshold": "block_only_high"})
/// );
/// ```
///
/// # Unknown variants on the send side
///
/// All three enums here are open (Evergreen): an unrecognized *string*
/// wire value deserializes into `Unknown` and re-serializes faithfully.
/// A **non-string** wire value (e.g. a numeric `type` in a config
/// file feeding [`TriggerCreateParams`](crate::TriggerCreateParams))
/// also parses into `Unknown`, but its stored type is the
/// `<non-string: ...>` debug marker — sending that back produces a
/// category string that means nothing to the server. Don't echo back a
/// setting whose `is_unknown()` variant came from a non-string value
/// (same caveat as
/// [`TriggerUpdate::with_status`](crate::TriggerUpdate::with_status)).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SafetySetting {
    /// The harm category this setting applies to (wire key `type`).
    #[serde(rename = "type")]
    pub category: HarmCategory,
    /// The threshold for blocking content in this category.
    pub threshold: SafetyThreshold,
    /// The scoring method to block on. When `None`, the API defaults to
    /// the probability score.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<SafetyMethod>,
}

impl SafetySetting {
    /// Creates a safety setting for `category` at `threshold`.
    #[must_use]
    pub const fn new(category: HarmCategory, threshold: SafetyThreshold) -> Self {
        Self {
            category,
            threshold,
            method: None,
        }
    }

    /// Sets the scoring method to block on.
    #[must_use]
    pub fn with_method(mut self, method: SafetyMethod) -> Self {
        self.method = Some(method);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safety_setting_serializes_snake_case() {
        let setting = SafetySetting::new(
            HarmCategory::DangerousContent,
            SafetyThreshold::BlockMediumAndAbove,
        )
        .with_method(SafetyMethod::Severity);
        let json = serde_json::to_value(&setting).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "type": "dangerous_content",
                "threshold": "block_medium_and_above",
                "method": "severity"
            })
        );
    }

    #[test]
    fn method_omitted_when_none() {
        let setting = SafetySetting::new(HarmCategory::Jailbreak, SafetyThreshold::Off);
        let json = serde_json::to_value(&setting).unwrap();
        assert!(json.get("method").is_none());
    }

    #[cfg(not(feature = "strict-unknown"))]
    #[test]
    fn unknown_values_roundtrip() {
        let json = serde_json::json!({
            "type": "brand_new_category",
            "threshold": "block_everything"
        });
        let setting: SafetySetting = serde_json::from_value(json.clone()).unwrap();
        assert!(setting.category.is_unknown());
        assert_eq!(
            setting.category.unknown_category_type(),
            Some("brand_new_category")
        );
        assert!(setting.threshold.is_unknown());
        assert_eq!(serde_json::to_value(&setting).unwrap(), json);
    }

    #[test]
    fn known_values_roundtrip() {
        // Exhaustive: the live tests confirm only the Gemini-accepted
        // values (2026-10-30); `civic_integrity`, `image_*` and `block_none`
        // are Vertex-only, so these hand-typed as_wire arms pin the rest.
        for (category, wire) in [
            (HarmCategory::HateSpeech, "hate_speech"),
            (HarmCategory::DangerousContent, "dangerous_content"),
            (HarmCategory::Harassment, "harassment"),
            (HarmCategory::SexuallyExplicit, "sexually_explicit"),
            (HarmCategory::CivicIntegrity, "civic_integrity"),
            (HarmCategory::ImageHate, "image_hate"),
            (
                HarmCategory::ImageDangerousContent,
                "image_dangerous_content",
            ),
            (HarmCategory::ImageHarassment, "image_harassment"),
            (
                HarmCategory::ImageSexuallyExplicit,
                "image_sexually_explicit",
            ),
            (HarmCategory::Jailbreak, "jailbreak"),
        ] {
            let json = serde_json::to_value(&category).unwrap();
            assert_eq!(json, serde_json::json!(wire));
            let back: HarmCategory = serde_json::from_value(json).unwrap();
            assert_eq!(back, category);
            // Display is public API and must agree with the wire value.
            assert_eq!(category.to_string(), wire);
        }
    }

    #[test]
    fn display_agrees_with_wire_value() {
        for (threshold, wire) in [
            (SafetyThreshold::BlockLowAndAbove, "block_low_and_above"),
            (
                SafetyThreshold::BlockMediumAndAbove,
                "block_medium_and_above",
            ),
            (SafetyThreshold::BlockOnlyHigh, "block_only_high"),
            (SafetyThreshold::BlockNone, "block_none"),
            (SafetyThreshold::Off, "off"),
        ] {
            assert_eq!(serde_json::to_value(&threshold).unwrap(), wire);
            assert_eq!(threshold.to_string(), wire);
        }
        for (method, wire) in [
            (SafetyMethod::Severity, "severity"),
            (SafetyMethod::Probability, "probability"),
        ] {
            assert_eq!(serde_json::to_value(&method).unwrap(), wire);
            assert_eq!(method.to_string(), wire);
        }
    }
}
