//! Environment types for the `environment` request field and the Agents
//! resource's `base_environment`.
//!
//! An environment describes the sandbox an agent runs in: which sources are
//! mounted (GCS buckets, inline files, repositories, skill registries) and
//! what outbound network access is allowed.
//!
//! The wire union accepts either a string environment ID (an environment
//! created by a previous interaction, echoed as
//! [`InteractionResponse::environment_id`](crate::InteractionResponse)) or a
//! typed remote environment object — modeled here as [`EnvironmentSpec`].

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;

/// The type of a source mounted into an environment.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
///
/// # Wire Format
///
/// Serializes as lowercase snake_case strings: `"gcs"`, `"inline"`,
/// `"repository"`, `"skill_registry"`.
///
/// # Evergreen Pattern
///
/// Unknown values from the API deserialize into the `Unknown` variant,
/// preserving the original data for debugging and roundtrip serialization.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum SourceType {
    /// Google Cloud Storage path.
    Gcs,
    /// Inline content provided in the request.
    Inline,
    /// A source-code repository (e.g., GitHub path).
    Repository,
    /// A skill registry entry.
    SkillRegistry,
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized source type from the API
        source_type: String,
        /// The raw JSON value, preserved for debugging and roundtrip
        data: serde_json::Value,
    },
}

impl SourceType {
    /// Returns true if this is an unknown source type.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the source type name if this is an unknown source type.
    #[must_use]
    pub fn unknown_source_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { source_type, .. } => Some(source_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown source type.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for SourceType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Gcs => serializer.serialize_str("gcs"),
            Self::Inline => serializer.serialize_str("inline"),
            Self::Repository => serializer.serialize_str("repository"),
            Self::SkillRegistry => serializer.serialize_str("skill_registry"),
            Self::Unknown { source_type, .. } => serializer.serialize_str(source_type),
        }
    }
}

impl<'de> Deserialize<'de> for SourceType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.as_str() {
            Some("gcs") => Ok(Self::Gcs),
            Some("inline") => Ok(Self::Inline),
            Some("repository") => Ok(Self::Repository),
            Some("skill_registry") => Ok(Self::SkillRegistry),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown SourceType '{}' - using Unknown variant (Evergreen)",
                    other
                );
                Ok(Self::Unknown {
                    source_type: other.to_string(),
                    data: value,
                })
            }
            None => {
                let source_type = format!("<non-string: {}>", value);
                tracing::warn!(
                    "SourceType received non-string value: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    source_type,
                    data: value,
                })
            }
        }
    }
}

/// A source to be mounted into an environment.
///
/// # Example
///
/// ```
/// use genai_rs::EnvironmentSource;
///
/// // Mount a GCS bucket at /data
/// let gcs = EnvironmentSource::gcs("gs://my-bucket/data", "/data");
///
/// // Provide an inline file
/// let inline = EnvironmentSource::inline("/etc/config.toml", "verbose = true");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EnvironmentSource {
    /// The kind of source (wire: `type`).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub source_type: Option<SourceType>,
    /// The source location. For GCS this is the GCS path; for repositories
    /// this is the repository path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Where the source should appear in the environment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// The inline content when `source_type` is [`SourceType::Inline`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Optional encoding for inline content (e.g., `base64`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
    /// Additional fields not yet modeled (Evergreen forward compatibility)
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl EnvironmentSource {
    /// Creates a GCS source mounted at `target`.
    #[must_use]
    pub fn gcs(source: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            source_type: Some(SourceType::Gcs),
            source: Some(source.into()),
            target: Some(target.into()),
            ..Default::default()
        }
    }

    /// Creates an inline source placed at `target` with the given content.
    #[must_use]
    pub fn inline(target: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            source_type: Some(SourceType::Inline),
            target: Some(target.into()),
            content: Some(content.into()),
            ..Default::default()
        }
    }

    /// Creates a repository source mounted at `target`.
    #[must_use]
    pub fn repository(source: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            source_type: Some(SourceType::Repository),
            source: Some(source.into()),
            target: Some(target.into()),
            ..Default::default()
        }
    }

    /// Creates a skill-registry source.
    #[must_use]
    pub fn skill_registry(source: impl Into<String>) -> Self {
        Self {
            source_type: Some(SourceType::SkillRegistry),
            source: Some(source.into()),
            ..Default::default()
        }
    }

    /// Sets the encoding for inline content (e.g., `base64`).
    #[must_use]
    pub fn with_encoding(mut self, encoding: impl Into<String>) -> Self {
        self.encoding = Some(encoding.into());
        self
    }
}

/// A single domain allowlist rule with optional header injection.
///
/// `domain` supports wildcards (e.g. `*.googleapis.com`); use `*` to allow
/// all domains while still injecting headers on specific ones. Each
/// `transform` entry is a flat `{header_name: header_value}` map injected on
/// matching outbound requests by the egress proxy.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AllowlistEntry {
    /// Domain to allow outbound requests to.
    pub domain: String,
    /// Headers to inject on outbound requests matching this domain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<Vec<HashMap<String, String>>>,
    /// Additional fields not yet modeled (Evergreen forward compatibility)
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl AllowlistEntry {
    /// Creates an allowlist rule for the given domain.
    #[must_use]
    pub fn new(domain: impl Into<String>) -> Self {
        Self {
            domain: domain.into(),
            ..Default::default()
        }
    }

    /// Sets header-injection transforms for this domain.
    #[must_use]
    pub fn with_transform(mut self, transform: Vec<HashMap<String, String>>) -> Self {
        self.transform = Some(transform);
        self
    }
}

/// Outbound network configuration for an environment sandbox.
///
/// The wire union accepts the string `"disabled"` (all network off) or an
/// object `{"allowlist": [...]}` restricting outbound domains. Omit the
/// field entirely to allow all outbound traffic.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
///
/// # Evergreen Pattern
///
/// Unknown shapes from the API deserialize into the `Unknown` variant,
/// preserving the original data for debugging and roundtrip serialization.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum NetworkConfig {
    /// Turns all network access off (wire: `"disabled"`).
    Disabled,
    /// Restricts outbound traffic to the listed domains
    /// (wire: `{"allowlist": [{domain, transform}]}`).
    Allowlist(Vec<AllowlistEntry>),
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// A short descriptor of the unrecognized shape
        network_type: String,
        /// The raw JSON value, preserved for debugging and roundtrip
        data: serde_json::Value,
    },
}

impl NetworkConfig {
    /// Returns true if this is an unknown network config.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the descriptor if this is an unknown network config.
    #[must_use]
    pub fn unknown_network_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { network_type, .. } => Some(network_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown network config.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for NetworkConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;
        match self {
            Self::Disabled => serializer.serialize_str("disabled"),
            Self::Allowlist(entries) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("allowlist", entries)?;
                map.end()
            }
            Self::Unknown { data, .. } => data.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for NetworkConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match &value {
            serde_json::Value::String(s) if s == "disabled" => Ok(Self::Disabled),
            serde_json::Value::Object(obj) if obj.contains_key("allowlist") => {
                match serde_json::from_value::<Vec<AllowlistEntry>>(obj["allowlist"].clone()) {
                    Ok(entries) => Ok(Self::Allowlist(entries)),
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse network allowlist: {}. \
                             Preserving in Unknown variant.",
                            e
                        );
                        Ok(Self::Unknown {
                            network_type: "allowlist".to_string(),
                            data: value,
                        })
                    }
                }
            }
            _ => {
                tracing::warn!(
                    "Encountered unknown NetworkConfig shape: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    network_type: format!("<unrecognized: {}>", value),
                    data: value,
                })
            }
        }
    }
}

/// A typed remote environment definition (wire: `{"type": "remote", ...}`).
///
/// # Example
///
/// ```
/// use genai_rs::{AllowlistEntry, EnvironmentSource, NetworkConfig, RemoteEnvironment};
///
/// let env = RemoteEnvironment::new()
///     .add_source(EnvironmentSource::gcs("gs://my-bucket/data", "/data"))
///     .add_source(EnvironmentSource::inline("/etc/motd", "hello"))
///     .with_network(NetworkConfig::Allowlist(vec![
///         AllowlistEntry::new("*.googleapis.com"),
///     ]));
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RemoteEnvironment {
    /// Sources mounted into the environment.
    pub sources: Vec<EnvironmentSource>,
    /// Outbound network configuration. `None` allows all outbound traffic.
    pub network: Option<NetworkConfig>,
    /// Additional fields not yet modeled (Evergreen forward compatibility)
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl RemoteEnvironment {
    /// Creates an empty remote environment.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a source to mount into the environment.
    #[must_use]
    pub fn add_source(mut self, source: EnvironmentSource) -> Self {
        self.sources.push(source);
        self
    }

    /// Sets the outbound network configuration.
    #[must_use]
    pub fn with_network(mut self, network: NetworkConfig) -> Self {
        self.network = Some(network);
        self
    }
}

impl Serialize for RemoteEnvironment {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("type", "remote")?;
        if !self.sources.is_empty() {
            map.serialize_entry("sources", &self.sources)?;
        }
        if let Some(network) = &self.network {
            map.serialize_entry("network", network)?;
        }
        for (key, value) in &self.extra {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for RemoteEnvironment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            sources: Vec<EnvironmentSource>,
            #[serde(default)]
            network: Option<NetworkConfig>,
            /// Unknown fields, preserved for Evergreen roundtrip.
            #[serde(flatten)]
            extra: serde_json::Map<String, serde_json::Value>,
        }
        let mut raw = Raw::deserialize(deserializer)?;
        // The `type` discriminant is re-emitted by `Serialize`; keeping it in
        // `extra` would duplicate the key on roundtrip.
        raw.extra.remove("type");
        Ok(Self {
            sources: raw.sources,
            network: raw.network,
            extra: raw.extra,
        })
    }
}

/// The `environment` union on an interaction request (and the Agents
/// resource's `base_environment`).
///
/// Either a string environment ID referencing an existing environment, or a
/// typed [`RemoteEnvironment`] object.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
///
/// # Example
///
/// ```
/// use genai_rs::{EnvironmentSpec, EnvironmentSource, RemoteEnvironment};
///
/// // Reference an existing environment by ID
/// let by_id: EnvironmentSpec = "environments/env-123".into();
///
/// // Or define one inline
/// let inline: EnvironmentSpec = RemoteEnvironment::new()
///     .add_source(EnvironmentSource::gcs("gs://bucket", "/data"))
///     .into();
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum EnvironmentSpec {
    /// A string ID referencing an existing environment.
    Id(String),
    /// A typed remote environment definition.
    Remote(RemoteEnvironment),
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized environment type from the API
        environment_type: String,
        /// The raw JSON value, preserved for debugging and roundtrip
        data: serde_json::Value,
    },
}

impl EnvironmentSpec {
    /// Returns true if this is an unknown environment spec.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the environment type name if this is an unknown environment spec.
    #[must_use]
    pub fn unknown_environment_type(&self) -> Option<&str> {
        match self {
            Self::Unknown {
                environment_type, ..
            } => Some(environment_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown environment spec.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl From<String> for EnvironmentSpec {
    fn from(id: String) -> Self {
        Self::Id(id)
    }
}

impl From<&str> for EnvironmentSpec {
    fn from(id: &str) -> Self {
        Self::Id(id.to_string())
    }
}

impl From<RemoteEnvironment> for EnvironmentSpec {
    fn from(env: RemoteEnvironment) -> Self {
        Self::Remote(env)
    }
}

impl Serialize for EnvironmentSpec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Id(id) => serializer.serialize_str(id),
            Self::Remote(env) => env.serialize(serializer),
            Self::Unknown { data, .. } => data.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for EnvironmentSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match &value {
            serde_json::Value::String(id) => Ok(Self::Id(id.clone())),
            serde_json::Value::Object(obj) => {
                let env_type = obj.get("type").and_then(|t| t.as_str()).unwrap_or("remote");
                if env_type == "remote" {
                    match serde_json::from_value::<RemoteEnvironment>(value.clone()) {
                        Ok(env) => Ok(Self::Remote(env)),
                        Err(e) => {
                            tracing::warn!(
                                "Failed to parse remote environment: {}. \
                                 Preserving in Unknown variant.",
                                e
                            );
                            Ok(Self::Unknown {
                                environment_type: "remote".to_string(),
                                data: value,
                            })
                        }
                    }
                } else {
                    tracing::warn!(
                        "Encountered unknown EnvironmentSpec type '{}' - using Unknown variant (Evergreen)",
                        env_type
                    );
                    Ok(Self::Unknown {
                        environment_type: env_type.to_string(),
                        data: value,
                    })
                }
            }
            _ => {
                tracing::warn!(
                    "Encountered unknown EnvironmentSpec shape: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    environment_type: format!("<unrecognized: {}>", value),
                    data: value,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_source_type_wire_roundtrip() {
        for (source_type, wire) in [
            (SourceType::Gcs, "\"gcs\""),
            (SourceType::Inline, "\"inline\""),
            (SourceType::Repository, "\"repository\""),
            (SourceType::SkillRegistry, "\"skill_registry\""),
        ] {
            assert_eq!(serde_json::to_string(&source_type).unwrap(), wire);
            let parsed: SourceType = serde_json::from_str(wire).unwrap();
            assert_eq!(parsed, source_type);
        }
    }

    #[test]
    fn test_source_type_unknown_roundtrip() {
        let unknown: SourceType = serde_json::from_str("\"s3\"").unwrap();
        assert!(unknown.is_unknown());
        assert_eq!(unknown.unknown_source_type(), Some("s3"));
        assert!(unknown.unknown_data().is_some());
        assert_eq!(serde_json::to_string(&unknown).unwrap(), "\"s3\"");
    }

    #[test]
    fn test_environment_source_constructors() {
        let gcs = EnvironmentSource::gcs("gs://bucket/path", "/data");
        let value = serde_json::to_value(&gcs).unwrap();
        assert_eq!(
            value,
            json!({"type": "gcs", "source": "gs://bucket/path", "target": "/data"})
        );

        let inline = EnvironmentSource::inline("/etc/motd", "aGVsbG8=").with_encoding("base64");
        let value = serde_json::to_value(&inline).unwrap();
        assert_eq!(
            value,
            json!({
                "type": "inline",
                "target": "/etc/motd",
                "content": "aGVsbG8=",
                "encoding": "base64"
            })
        );

        let repo = EnvironmentSource::repository("github.com/org/repo", "/workspace");
        assert_eq!(repo.source_type, Some(SourceType::Repository));

        let skill = EnvironmentSource::skill_registry("skills/my-skill");
        assert_eq!(skill.source_type, Some(SourceType::SkillRegistry));
        assert_eq!(skill.target, None);
    }

    #[test]
    fn test_network_config_disabled_roundtrip() {
        let network = NetworkConfig::Disabled;
        let json = serde_json::to_string(&network).unwrap();
        assert_eq!(json, "\"disabled\"");
        let parsed: NetworkConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, network);
    }

    #[test]
    fn test_network_config_allowlist_roundtrip() {
        let network = NetworkConfig::Allowlist(vec![
            AllowlistEntry::new("*.googleapis.com"),
            AllowlistEntry::new("api.example.com").with_transform(vec![HashMap::from([(
                "Authorization".to_string(),
                "Bearer token".to_string(),
            )])]),
        ]);

        let value = serde_json::to_value(&network).unwrap();
        assert_eq!(value["allowlist"][0]["domain"], "*.googleapis.com");
        assert!(value["allowlist"][0].get("transform").is_none());
        assert_eq!(
            value["allowlist"][1]["transform"][0]["Authorization"],
            "Bearer token"
        );

        let parsed: NetworkConfig = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, network);
    }

    #[test]
    fn test_network_config_unknown_roundtrip() {
        let raw = json!({"proxy": "socks5://localhost"});
        let parsed: NetworkConfig = serde_json::from_value(raw.clone()).unwrap();
        assert!(parsed.is_unknown());
        assert!(parsed.unknown_network_type().is_some());
        assert_eq!(parsed.unknown_data(), Some(&raw));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), raw);
    }

    #[test]
    fn test_remote_environment_serialization() {
        let env = RemoteEnvironment::new()
            .add_source(EnvironmentSource::gcs("gs://bucket", "/data"))
            .with_network(NetworkConfig::Disabled);

        let value = serde_json::to_value(&env).unwrap();
        assert_eq!(
            value,
            json!({
                "type": "remote",
                "sources": [{"type": "gcs", "source": "gs://bucket", "target": "/data"}],
                "network": "disabled"
            })
        );
    }

    #[test]
    fn test_remote_environment_empty_serializes_type_only() {
        let env = RemoteEnvironment::new();
        let value = serde_json::to_value(&env).unwrap();
        assert_eq!(value, json!({"type": "remote"}));
    }

    #[test]
    fn test_remote_environment_preserves_unknown_fields() {
        // Unknown top-level fields must survive a deserialize/serialize
        // roundtrip (Evergreen), without duplicating the `type` discriminant.
        let raw = json!({
            "type": "remote",
            "sources": [{"type": "gcs", "source": "gs://bucket", "target": "/data"}],
            "timeout_seconds": 300,
            "region": "us-central1"
        });
        let env: RemoteEnvironment = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(env.sources.len(), 1);
        assert_eq!(env.extra["timeout_seconds"], json!(300));
        assert_eq!(env.extra["region"], json!("us-central1"));
        assert!(!env.extra.contains_key("type"));
        assert_eq!(serde_json::to_value(&env).unwrap(), raw);
    }

    #[test]
    fn test_environment_source_preserves_unknown_fields() {
        let raw = json!({
            "type": "repository",
            "source": "owner/repo",
            "target": "/workspace",
            "branch": "main"
        });
        let source: EnvironmentSource = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(source.extra["branch"], json!("main"));
        assert_eq!(serde_json::to_value(&source).unwrap(), raw);
    }

    #[test]
    fn test_allowlist_entry_preserves_unknown_fields() {
        let raw = json!({
            "domain": "*.googleapis.com",
            "max_requests": 10
        });
        let entry: AllowlistEntry = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(entry.domain, "*.googleapis.com");
        assert_eq!(entry.extra["max_requests"], json!(10));
        assert_eq!(serde_json::to_value(&entry).unwrap(), raw);
    }

    #[test]
    fn test_environment_spec_id_roundtrip() {
        let spec: EnvironmentSpec = "environments/env-123".into();
        let json = serde_json::to_string(&spec).unwrap();
        assert_eq!(json, "\"environments/env-123\"");
        let parsed: EnvironmentSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, spec);
    }

    #[test]
    fn test_environment_spec_remote_roundtrip() {
        let spec: EnvironmentSpec = RemoteEnvironment::new()
            .add_source(EnvironmentSource::inline("/f.txt", "content"))
            .into();
        let json = serde_json::to_string(&spec).unwrap();
        let parsed: EnvironmentSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, spec);
    }

    #[test]
    fn test_environment_spec_unknown_type_roundtrip() {
        let raw = json!({"type": "local", "path": "/tmp"});
        let parsed: EnvironmentSpec = serde_json::from_value(raw.clone()).unwrap();
        assert!(parsed.is_unknown());
        assert_eq!(parsed.unknown_environment_type(), Some("local"));
        assert_eq!(parsed.unknown_data(), Some(&raw));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), raw);
    }

    #[test]
    fn test_environment_spec_known_not_unknown() {
        let spec: EnvironmentSpec = "env-1".into();
        assert!(!spec.is_unknown());
        assert_eq!(spec.unknown_environment_type(), None);
        assert_eq!(spec.unknown_data(), None);
    }
}
