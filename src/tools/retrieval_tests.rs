use super::*;
use serde_json;

#[test]
fn test_retrieval_type_wire_roundtrip() {
    for (retrieval_type, wire) in [
        (RetrievalType::VertexAiSearch, "\"vertex_ai_search\""),
        (RetrievalType::RagStore, "\"rag_store\""),
        (RetrievalType::ExaAiSearch, "\"exa_ai_search\""),
        (RetrievalType::ParallelAiSearch, "\"parallel_ai_search\""),
    ] {
        assert_eq!(serde_json::to_string(&retrieval_type).unwrap(), wire);
        let parsed: RetrievalType = serde_json::from_str(wire).unwrap();
        assert_eq!(parsed, retrieval_type);
    }
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_retrieval_type_unknown_roundtrip() {
    let unknown: RetrievalType = serde_json::from_str("\"bing_search\"").unwrap();
    assert!(unknown.is_unknown());
    assert_eq!(unknown.unknown_retrieval_type(), Some("bing_search"));
    assert!(unknown.unknown_data().is_some());
    assert_eq!(serde_json::to_string(&unknown).unwrap(), "\"bing_search\"");

    // Known types are not unknown
    assert!(!RetrievalType::RagStore.is_unknown());
    assert_eq!(RetrievalType::RagStore.unknown_retrieval_type(), None);
    assert_eq!(RetrievalType::RagStore.unknown_data(), None);
}

#[test]
fn test_tool_retrieval_vertex_ai_search_wire_shape() {
    let tool: Tool = RetrievalConfig::new()
        .with_vertex_ai_search(
            VertexAiSearchConfig::new()
                .with_engine("projects/p/locations/global/engines/e")
                .with_datastores(vec!["ds-1".to_string()]),
        )
        .into();

    let value = serde_json::to_value(&tool).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "type": "retrieval",
            "retrieval_types": ["vertex_ai_search"],
            "vertex_ai_search_config": {
                "engine": "projects/p/locations/global/engines/e",
                "datastores": ["ds-1"]
            }
        })
    );
}

#[test]
fn test_tool_retrieval_rag_store_wire_shape() {
    let tool: Tool = RetrievalConfig::new()
        .with_rag_store(
            RagStoreConfig::new(vec![
                RagResource::new("projects/p/locations/us/ragCorpora/c")
                    .with_rag_file_ids(vec!["f1".to_string()]),
            ])
            .with_rag_retrieval_config(
                RagRetrievalConfig::new()
                    .with_top_k(8)
                    .with_hybrid_search_alpha(0.5)
                    .with_filter(RagFilter {
                        vector_distance_threshold: Some(0.7),
                        vector_similarity_threshold: None,
                        metadata_filter: Some("category = \"tech\"".to_string()),
                    })
                    .with_ranking(RagRanking::rank_service().with_model_name("ranker-v2")),
            ),
        )
        .into();

    let value = serde_json::to_value(&tool).unwrap();
    assert_eq!(value["type"], "retrieval");
    assert_eq!(value["retrieval_types"], serde_json::json!(["rag_store"]));
    let rag = &value["rag_store_config"];
    assert_eq!(
        rag["rag_resources"][0]["rag_corpus"],
        "projects/p/locations/us/ragCorpora/c"
    );
    assert_eq!(rag["rag_resources"][0]["rag_file_ids"][0], "f1");
    let retrieval = &rag["rag_retrieval_config"];
    assert_eq!(retrieval["top_k"], 8);
    assert_eq!(retrieval["hybrid_search"]["alpha"], 0.5);
    // Wire field is `filter` (Rust field `filter`, spec alias `filter_`)
    assert_eq!(retrieval["filter"]["vector_distance_threshold"], 0.7);
    assert_eq!(
        retrieval["filter"]["metadata_filter"],
        "category = \"tech\""
    );
    assert_eq!(retrieval["ranking"]["ranking_config"], "rank_service");
    assert_eq!(retrieval["ranking"]["model_name"], "ranker-v2");
}

#[test]
fn test_tool_retrieval_exa_and_parallel_wire_shape() {
    let tool: Tool = RetrievalConfig::new()
        .with_exa_ai_search(
            ExaAiSearchConfig::new("exa-key")
                .with_custom_config(serde_json::json!({"num_results": 5})),
        )
        .with_parallel_ai_search(ParallelAiSearchConfig::new().with_api_key("par-key"))
        .into();

    let value = serde_json::to_value(&tool).unwrap();
    assert_eq!(
        value["retrieval_types"],
        serde_json::json!(["exa_ai_search", "parallel_ai_search"])
    );
    assert_eq!(value["exa_ai_search_config"]["api_key"], "exa-key");
    assert_eq!(
        value["exa_ai_search_config"]["custom_config"]["num_results"],
        5
    );
    assert_eq!(value["parallel_ai_search_config"]["api_key"], "par-key");
}

#[test]
fn test_tool_retrieval_roundtrip() {
    let tool: Tool = RetrievalConfig::new()
        .with_rag_store(RagStoreConfig::new(vec![RagResource::new("corpora/c")]))
        .with_vertex_ai_search(VertexAiSearchConfig::new().with_engine("engines/e"))
        .into();

    let json = serde_json::to_string(&tool).expect("Serialization failed");
    let parsed: Tool = serde_json::from_str(&json).expect("Deserialization failed");
    match parsed {
        Tool::Retrieval {
            retrieval_types,
            vertex_ai_search_config,
            rag_store_config,
            exa_ai_search_config,
            parallel_ai_search_config,
        } => {
            assert_eq!(
                retrieval_types,
                Some(vec![RetrievalType::RagStore, RetrievalType::VertexAiSearch])
            );
            assert_eq!(
                vertex_ai_search_config.unwrap().engine.as_deref(),
                Some("engines/e")
            );
            assert!(rag_store_config.is_some());
            assert_eq!(exa_ai_search_config, None);
            assert_eq!(parallel_ai_search_config, None);
        }
        other => panic!("Expected Retrieval variant, got {:?}", other),
    }
}

#[test]
fn test_retrieval_config_unknown_types_escape_hatch() {
    let tool: Tool = RetrievalConfig::new()
        .with_retrieval_types(vec![RetrievalType::Unknown {
            retrieval_type: "future_backend".to_string(),
            data: serde_json::json!("future_backend"),
        }])
        .into();
    let value = serde_json::to_value(&tool).unwrap();
    assert_eq!(value["retrieval_types"][0], "future_backend");
}

#[test]
fn test_rag_ranking_nested_rank_service() {
    let ranking = RagRanking::rank_service().with_rank_service(RankService::new("ranker-v3"));
    let wire = serde_json::json!({
        "ranking_config": "rank_service",
        "rank_service": {"model_name": "ranker-v3"}
    });
    assert_eq!(serde_json::to_value(&ranking).unwrap(), wire);
    assert_eq!(serde_json::from_value::<RagRanking>(wire).unwrap(), ranking);
}
