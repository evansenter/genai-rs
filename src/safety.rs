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
//! Server-side constraint (verified live 2026-08-08): the Gemini API
//! currently rejects `safety_settings` with 400 `invalid_request` — "not
//! available on the Gemini API but it is available on the Gemini
//! Enterprise Agent Platform" (Vertex-only). The types are modeled for
//! spec parity and forward compatibility.

use serde::de::Deserializer;
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Category of harmful content a [`SafetySetting`] applies to.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
///
/// # Wire Format
///
/// Serializes as snake_case strings: `"hate_speech"`, `"dangerous_content"`,
/// `"harassment"`, `"sexually_explicit"`, `"civic_integrity"`,
/// `"image_hate"`, `"image_dangerous_content"`, `"image_harassment"`,
/// `"image_sexually_explicit"`, `"jailbreak"`.
///
/// # Evergreen Pattern
///
/// Unknown values from the API deserialize into the `Unknown` variant,
/// preserving the original data for debugging and roundtrip serialization.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum HarmCategory {
    /// Hateful or discriminatory speech.
    HateSpeech,
    /// Content that facilitates or encourages dangerous acts.
    DangerousContent,
    /// Harassing or bullying content.
    Harassment,
    /// Sexually explicit content.
    SexuallyExplicit,
    /// Content that could undermine civic processes.
    CivicIntegrity,
    /// Hateful imagery.
    ImageHate,
    /// Dangerous imagery.
    ImageDangerousContent,
    /// Harassing imagery.
    ImageHarassment,
    /// Sexually explicit imagery.
    ImageSexuallyExplicit,
    /// Prompt-injection / jailbreak attempts.
    Jailbreak,
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized category type from the API
        category_type: String,
        /// The raw JSON value, preserved for debugging and roundtrip
        data: serde_json::Value,
    },
}

impl HarmCategory {
    /// The wire string for this category — the single source both `Display`
    /// and `Serialize` render, so the two can never disagree.
    fn as_wire(&self) -> &str {
        match self {
            Self::HateSpeech => "hate_speech",
            Self::DangerousContent => "dangerous_content",
            Self::Harassment => "harassment",
            Self::SexuallyExplicit => "sexually_explicit",
            Self::CivicIntegrity => "civic_integrity",
            Self::ImageHate => "image_hate",
            Self::ImageDangerousContent => "image_dangerous_content",
            Self::ImageHarassment => "image_harassment",
            Self::ImageSexuallyExplicit => "image_sexually_explicit",
            Self::Jailbreak => "jailbreak",
            Self::Unknown { category_type, .. } => category_type,
        }
    }

    /// Returns true if this is an unknown harm category.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the category type name if this is an unknown harm category.
    #[must_use]
    pub fn unknown_category_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { category_type, .. } => Some(category_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown harm category.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl fmt::Display for HarmCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_wire())
    }
}

impl Serialize for HarmCategory {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_wire())
    }
}

impl<'de> Deserialize<'de> for HarmCategory {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.as_str() {
            Some("hate_speech") => Ok(Self::HateSpeech),
            Some("dangerous_content") => Ok(Self::DangerousContent),
            Some("harassment") => Ok(Self::Harassment),
            Some("sexually_explicit") => Ok(Self::SexuallyExplicit),
            Some("civic_integrity") => Ok(Self::CivicIntegrity),
            Some("image_hate") => Ok(Self::ImageHate),
            Some("image_dangerous_content") => Ok(Self::ImageDangerousContent),
            Some("image_harassment") => Ok(Self::ImageHarassment),
            Some("image_sexually_explicit") => Ok(Self::ImageSexuallyExplicit),
            Some("jailbreak") => Ok(Self::Jailbreak),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown HarmCategory '{other}' - using Unknown variant (Evergreen)"
                );
                Ok(Self::Unknown {
                    category_type: other.to_string(),
                    data: value.clone(),
                })
            }
            None => {
                tracing::warn!(
                    "HarmCategory received non-string value: {value}. Preserving in Unknown variant."
                );
                Ok(Self::Unknown {
                    category_type: format!("<non-string: {value}>"),
                    data: value,
                })
            }
        }
    }
}

/// Blocking threshold for a [`SafetySetting`].
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
///
/// # Wire Format
///
/// Serializes as snake_case strings: `"block_low_and_above"`,
/// `"block_medium_and_above"`, `"block_only_high"`, `"block_none"`, `"off"`.
///
/// # Evergreen Pattern
///
/// Unknown values from the API deserialize into the `Unknown` variant,
/// preserving the original data for debugging and roundtrip serialization.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum SafetyThreshold {
    /// Block content with low probability of harm and above.
    BlockLowAndAbove,
    /// Block content with medium probability of harm and above.
    BlockMediumAndAbove,
    /// Block only content with high probability of harm.
    BlockOnlyHigh,
    /// Never block for this category, but keep safety scoring on.
    BlockNone,
    /// Disable the safety filter for this category entirely.
    Off,
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized threshold type from the API
        threshold_type: String,
        /// The raw JSON value, preserved for debugging and roundtrip
        data: serde_json::Value,
    },
}

impl SafetyThreshold {
    /// The wire string for this threshold — the single source both `Display`
    /// and `Serialize` render, so the two can never disagree.
    fn as_wire(&self) -> &str {
        match self {
            Self::BlockLowAndAbove => "block_low_and_above",
            Self::BlockMediumAndAbove => "block_medium_and_above",
            Self::BlockOnlyHigh => "block_only_high",
            Self::BlockNone => "block_none",
            Self::Off => "off",
            Self::Unknown { threshold_type, .. } => threshold_type,
        }
    }

    /// Returns true if this is an unknown threshold.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the threshold type name if this is an unknown threshold.
    #[must_use]
    pub fn unknown_threshold_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { threshold_type, .. } => Some(threshold_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown threshold.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl fmt::Display for SafetyThreshold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_wire())
    }
}

impl Serialize for SafetyThreshold {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_wire())
    }
}

impl<'de> Deserialize<'de> for SafetyThreshold {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.as_str() {
            Some("block_low_and_above") => Ok(Self::BlockLowAndAbove),
            Some("block_medium_and_above") => Ok(Self::BlockMediumAndAbove),
            Some("block_only_high") => Ok(Self::BlockOnlyHigh),
            Some("block_none") => Ok(Self::BlockNone),
            Some("off") => Ok(Self::Off),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown SafetyThreshold '{other}' - using Unknown variant (Evergreen)"
                );
                Ok(Self::Unknown {
                    threshold_type: other.to_string(),
                    data: value.clone(),
                })
            }
            None => {
                tracing::warn!(
                    "SafetyThreshold received non-string value: {value}. Preserving in Unknown variant."
                );
                Ok(Self::Unknown {
                    threshold_type: format!("<non-string: {value}>"),
                    data: value,
                })
            }
        }
    }
}

/// Scoring method a [`SafetySetting`] blocks on.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
///
/// # Wire Format
///
/// Serializes as lowercase strings: `"severity"`, `"probability"`.
/// When unset, the API defaults to the probability score.
///
/// # Evergreen Pattern
///
/// Unknown values from the API deserialize into the `Unknown` variant,
/// preserving the original data for debugging and roundtrip serialization.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum SafetyMethod {
    /// Block based on the severity score.
    Severity,
    /// Block based on the probability score (API default).
    Probability,
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized method type from the API
        method_type: String,
        /// The raw JSON value, preserved for debugging and roundtrip
        data: serde_json::Value,
    },
}

impl SafetyMethod {
    /// The wire string for this method — the single source both `Display`
    /// and `Serialize` render, so the two can never disagree.
    fn as_wire(&self) -> &str {
        match self {
            Self::Severity => "severity",
            Self::Probability => "probability",
            Self::Unknown { method_type, .. } => method_type,
        }
    }

    /// Returns true if this is an unknown method.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the method type name if this is an unknown method.
    #[must_use]
    pub fn unknown_method_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { method_type, .. } => Some(method_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown method.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl fmt::Display for SafetyMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_wire())
    }
}

impl Serialize for SafetyMethod {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_wire())
    }
}

impl<'de> Deserialize<'de> for SafetyMethod {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.as_str() {
            Some("severity") => Ok(Self::Severity),
            Some("probability") => Ok(Self::Probability),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown SafetyMethod '{other}' - using Unknown variant (Evergreen)"
                );
                Ok(Self::Unknown {
                    method_type: other.to_string(),
                    data: value.clone(),
                })
            }
            None => {
                tracing::warn!(
                    "SafetyMethod received non-string value: {value}. Preserving in Unknown variant."
                );
                Ok(Self::Unknown {
                    method_type: format!("<non-string: {value}>"),
                    data: value,
                })
            }
        }
    }
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
///     serde_json::json!({"category": "harassment", "threshold": "block_only_high"})
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SafetySetting {
    /// The harm category this setting applies to.
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
                "category": "dangerous_content",
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

    #[test]
    fn unknown_values_roundtrip() {
        let json = serde_json::json!({
            "category": "brand_new_category",
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
        // Exhaustive: with the parameter Vertex-gated, no live probe can
        // ever confirm a category string, so these hand-typed as_wire
        // arms are pinned by nothing else.
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
