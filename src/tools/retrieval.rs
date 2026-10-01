use super::Tool;
use crate::wire_enum::wire_enum;
use serde::{Deserialize, Serialize};

wire_enum! {
    /// Retrieval backends for the built-in `retrieval` tool.
    ///
    /// # Wire Format
    ///
    /// Serializes as snake_case strings: `"vertex_ai_search"`, `"rag_store"`,
    /// `"exa_ai_search"`, `"parallel_ai_search"`.
    pub enum RetrievalType {
        /// Vertex AI Search engines and datastores.
        VertexAiSearch = "vertex_ai_search",
        /// Vertex RAG Store corpora.
        RagStore = "rag_store",
        /// Exa.ai search.
        ExaAiSearch = "exa_ai_search",
        /// Parallel.ai search.
        ParallelAiSearch = "parallel_ai_search",
    }
    unknown(retrieval_type, unknown_retrieval_type)
}

/// Configuration for the Vertex AI Search retrieval backend.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VertexAiSearchConfig {
    /// The Vertex AI Search engine to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    /// The Vertex AI Search datastores to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datastores: Option<Vec<String>>,
}

impl VertexAiSearchConfig {
    /// Creates an empty Vertex AI Search config.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Vertex AI Search engine.
    #[must_use]
    pub fn with_engine(mut self, engine: impl Into<String>) -> Self {
        self.engine = Some(engine.into());
        self
    }

    /// Sets the Vertex AI Search datastores.
    #[must_use]
    pub fn with_datastores(mut self, datastores: Vec<String>) -> Self {
        self.datastores = Some(datastores);
        self
    }
}

/// Configuration for the Exa.ai search retrieval backend.
///
/// **Note**: `api_key` is your Exa.ai API key and is sent on the wire; treat
/// request logs accordingly.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExaAiSearchConfig {
    /// The Exa.ai API key (required by the API).
    pub api_key: String,
    /// Extra parameters passed through to the Exa.ai Search API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_config: Option<serde_json::Value>,
}

impl ExaAiSearchConfig {
    /// Creates a config with the given Exa.ai API key.
    #[must_use]
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            custom_config: None,
        }
    }

    /// Sets extra parameters passed through to the Exa.ai Search API.
    #[must_use]
    pub fn with_custom_config(mut self, custom_config: serde_json::Value) -> Self {
        self.custom_config = Some(custom_config);
        self
    }
}

/// Configuration for the Parallel.ai search retrieval backend.
///
/// **Note**: `api_key` is your Parallel.ai API key and is sent on the wire;
/// treat request logs accordingly.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParallelAiSearchConfig {
    /// The Parallel.ai API key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Extra parameters for Parallel.ai search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_config: Option<serde_json::Value>,
}

impl ParallelAiSearchConfig {
    /// Creates an empty Parallel.ai search config.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Parallel.ai API key.
    #[must_use]
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Sets extra parameters for Parallel.ai search.
    #[must_use]
    pub fn with_custom_config(mut self, custom_config: serde_json::Value) -> Self {
        self.custom_config = Some(custom_config);
        self
    }
}

/// A RAG resource reference (corpus + optional file restriction).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RagResource {
    /// `RagCorpora` resource name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rag_corpus: Option<String>,
    /// RAG file IDs; the files must belong to `rag_corpus`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rag_file_ids: Option<Vec<String>>,
}

impl RagResource {
    /// Creates a RAG resource for the given corpus.
    #[must_use]
    pub fn new(rag_corpus: impl Into<String>) -> Self {
        Self {
            rag_corpus: Some(rag_corpus.into()),
            rag_file_ids: None,
        }
    }

    /// Restricts retrieval to the given file IDs within the corpus.
    #[must_use]
    pub fn with_rag_file_ids(mut self, rag_file_ids: Vec<String>) -> Self {
        self.rag_file_ids = Some(rag_file_ids);
        self
    }
}

/// Hybrid-search configuration for RAG retrieval.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HybridSearchConfig {
    /// Alpha value controlling the weight between dense and sparse vector
    /// search results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha: Option<f32>,
}

/// Filter configuration for RAG retrieval.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RagFilter {
    /// Only return contexts with vector distance smaller than the threshold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_distance_threshold: Option<f64>,
    /// Only return contexts with vector similarity larger than the threshold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_similarity_threshold: Option<f64>,
    /// String for metadata filtering.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_filter: Option<String>,
}

/// Ranking configuration for RAG retrieval (Rank Service).
///
/// # Wire Format
///
/// `{"ranking_config": "rank_service", "model_name": "..."}` — the
/// `ranking_config` discriminator is always `"rank_service"`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RagRanking {
    /// The ranking config discriminator (always `"rank_service"`).
    #[serde(default = "RagRanking::default_ranking_config")]
    pub ranking_config: String,
    /// The model name of the rank service.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model_name: Option<String>,
    /// Rank Service settings in the nested form added in google-genai 2.24.
    /// Like the whole retrieval tool, Vertex-only on the Gemini API.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub rank_service: Option<RankService>,
}

/// Rank Service settings (`rank_service` inside [`RagRanking`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct RankService {
    /// The rank service model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
}

impl RankService {
    /// Rank Service settings using `model_name`.
    #[must_use]
    pub fn new(model_name: impl Into<String>) -> Self {
        Self {
            model_name: Some(model_name.into()),
        }
    }
}

impl RagRanking {
    fn default_ranking_config() -> String {
        "rank_service".to_string()
    }

    /// Creates a rank-service ranking config.
    #[must_use]
    pub fn rank_service() -> Self {
        Self::default()
    }

    /// Sets the rank service model name.
    #[must_use]
    pub fn with_model_name(mut self, model_name: impl Into<String>) -> Self {
        self.model_name = Some(model_name.into());
        self
    }

    /// Sets the nested `rank_service` settings.
    #[must_use]
    pub fn with_rank_service(mut self, rank_service: RankService) -> Self {
        self.rank_service = Some(rank_service);
        self
    }
}

impl Default for RagRanking {
    fn default() -> Self {
        Self {
            ranking_config: Self::default_ranking_config(),
            model_name: None,
            rank_service: None,
        }
    }
}

/// Context-retrieval configuration for the RAG store backend
/// (wire: `rag_retrieval_config`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RagRetrievalConfig {
    /// The number of contexts to retrieve.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,
    /// Hybrid search configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hybrid_search: Option<HybridSearchConfig>,
    /// Filter configuration (wire: `filter`).
    #[serde(rename = "filter", skip_serializing_if = "Option::is_none")]
    pub filter: Option<RagFilter>,
    /// Rank Service configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranking: Option<RagRanking>,
}

impl RagRetrievalConfig {
    /// Creates an empty RAG retrieval config.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the number of contexts to retrieve.
    #[must_use]
    pub fn with_top_k(mut self, top_k: i32) -> Self {
        self.top_k = Some(top_k);
        self
    }

    /// Sets the hybrid-search alpha (dense vs. sparse weighting).
    #[must_use]
    pub fn with_hybrid_search_alpha(mut self, alpha: f32) -> Self {
        self.hybrid_search = Some(HybridSearchConfig { alpha: Some(alpha) });
        self
    }

    /// Sets the filter configuration.
    #[must_use]
    pub fn with_filter(mut self, filter: RagFilter) -> Self {
        self.filter = Some(filter);
        self
    }

    /// Sets the ranking configuration.
    #[must_use]
    pub fn with_ranking(mut self, ranking: RagRanking) -> Self {
        self.ranking = Some(ranking);
        self
    }
}

/// Configuration for the RAG Store retrieval backend.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RagStoreConfig {
    /// The RAG sources to retrieve from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rag_resources: Option<Vec<RagResource>>,
    /// Number of top-k results to return from the selected corpora.
    ///
    /// Deprecated by the API in favor of
    /// `rag_retrieval_config.top_k`; still sent when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity_top_k: Option<i32>,
    /// Only return results with vector distance smaller than the threshold.
    ///
    /// Deprecated by the API in favor of
    /// `rag_retrieval_config.filter.vector_distance_threshold`; still sent
    /// when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_distance_threshold: Option<f64>,
    /// Context-retrieval configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rag_retrieval_config: Option<RagRetrievalConfig>,
}

impl RagStoreConfig {
    /// Creates a RAG store config over the given resources.
    #[must_use]
    pub fn new(rag_resources: Vec<RagResource>) -> Self {
        Self {
            rag_resources: Some(rag_resources),
            ..Default::default()
        }
    }

    /// Sets the context-retrieval configuration.
    #[must_use]
    pub fn with_rag_retrieval_config(mut self, config: RagRetrievalConfig) -> Self {
        self.rag_retrieval_config = Some(config);
        self
    }
}

/// Configuration for the built-in Retrieval tool.
///
/// Each `with_*` backend method enables the corresponding
/// [`RetrievalType`] and attaches its config, keeping `retrieval_types`
/// consistent with the per-backend configuration.
///
/// # Example
///
/// ```no_run
/// use genai_rs::{RagResource, RagStoreConfig, RetrievalConfig, VertexAiSearchConfig};
///
/// // Vertex AI Search grounding
/// let config = RetrievalConfig::new().with_vertex_ai_search(
///     VertexAiSearchConfig::new().with_engine("projects/p/locations/global/engines/e"),
/// );
///
/// // RAG store grounding
/// let config = RetrievalConfig::new().with_rag_store(RagStoreConfig::new(vec![
///     RagResource::new("projects/p/locations/us/ragCorpora/c"),
/// ]));
/// ```
#[derive(Clone, Debug, Default)]
pub struct RetrievalConfig {
    retrieval_types: Vec<RetrievalType>,
    vertex_ai_search_config: Option<VertexAiSearchConfig>,
    exa_ai_search_config: Option<ExaAiSearchConfig>,
    parallel_ai_search_config: Option<ParallelAiSearchConfig>,
    rag_store_config: Option<Box<RagStoreConfig>>,
}

impl RetrievalConfig {
    /// Creates an empty retrieval config.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn enable(&mut self, retrieval_type: RetrievalType) {
        if !self.retrieval_types.contains(&retrieval_type) {
            self.retrieval_types.push(retrieval_type);
        }
    }

    /// Enables Vertex AI Search retrieval with the given config.
    #[must_use]
    pub fn with_vertex_ai_search(mut self, config: VertexAiSearchConfig) -> Self {
        self.enable(RetrievalType::VertexAiSearch);
        self.vertex_ai_search_config = Some(config);
        self
    }

    /// Enables Exa.ai search retrieval with the given config.
    #[must_use]
    pub fn with_exa_ai_search(mut self, config: ExaAiSearchConfig) -> Self {
        self.enable(RetrievalType::ExaAiSearch);
        self.exa_ai_search_config = Some(config);
        self
    }

    /// Enables Parallel.ai search retrieval with the given config.
    #[must_use]
    pub fn with_parallel_ai_search(mut self, config: ParallelAiSearchConfig) -> Self {
        self.enable(RetrievalType::ParallelAiSearch);
        self.parallel_ai_search_config = Some(config);
        self
    }

    /// Enables RAG Store retrieval with the given config.
    #[must_use]
    pub fn with_rag_store(mut self, config: RagStoreConfig) -> Self {
        self.enable(RetrievalType::RagStore);
        self.rag_store_config = Some(Box::new(config));
        self
    }

    /// Sets the enabled retrieval types explicitly (escape hatch).
    ///
    /// Replaces the types accumulated by the `with_*` backend methods.
    #[must_use]
    pub fn with_retrieval_types(mut self, retrieval_types: Vec<RetrievalType>) -> Self {
        self.retrieval_types = retrieval_types;
        self
    }
}

impl From<RetrievalConfig> for Tool {
    fn from(config: RetrievalConfig) -> Self {
        Tool::Retrieval {
            retrieval_types: if config.retrieval_types.is_empty() {
                None
            } else {
                Some(config.retrieval_types)
            },
            vertex_ai_search_config: config.vertex_ai_search_config,
            exa_ai_search_config: config.exa_ai_search_config,
            parallel_ai_search_config: config.parallel_ai_search_config,
            rag_store_config: config.rag_store_config,
        }
    }
}

#[cfg(test)]
#[path = "retrieval_tests.rs"]
mod tests;
