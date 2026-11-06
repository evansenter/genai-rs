//! Property tests: tools, tool choice, function declarations and retrieval configs.

use super::*;

/// Strategy for FunctionCallingMode (wire format is lowercase).
pub(super) fn arb_function_calling_mode() -> impl Strategy<Value = FunctionCallingMode> {
    prop_oneof![
        Just(FunctionCallingMode::Auto),
        Just(FunctionCallingMode::Any),
        Just(FunctionCallingMode::None),
        Just(FunctionCallingMode::Validated),
        // Unknown variant with preserved data
        unknown_or(FunctionCallingMode::Auto, |mode_type| {
            FunctionCallingMode::Unknown {
                mode_type: mode_type.clone(),
                data: serde_json::Value::String(mode_type),
            }
        }),
    ]
}

/// Strategy for SearchType (includes EnterpriseWebSearch).
pub(super) fn arb_search_type() -> impl Strategy<Value = SearchType> {
    prop_oneof![
        Just(SearchType::WebSearch),
        Just(SearchType::ImageSearch),
        Just(SearchType::EnterpriseWebSearch),
        // Unknown variant with preserved data
        unknown_or(SearchType::WebSearch, |search_type| SearchType::Unknown {
            search_type: search_type.clone(),
            data: serde_json::Value::String(search_type),
        }),
    ]
}

// =============================================================================
// ToolChoice / AllowedTools Strategies
// =============================================================================

pub(super) fn arb_allowed_tools() -> impl Strategy<Value = AllowedTools> {
    (
        proptest::option::of(arb_function_calling_mode()),
        prop::collection::vec(arb_identifier(), 0..3),
    )
        .prop_map(|(mode, tools)| AllowedTools { mode, tools })
}

pub(crate) fn arb_tool_choice() -> impl Strategy<Value = ToolChoice> {
    prop_oneof![
        arb_function_calling_mode().prop_map(ToolChoice::Mode),
        arb_allowed_tools().prop_map(ToolChoice::AllowedTools),
        // Unknown shape: an object without the "allowed_tools" key
        arb_unknown_type().prop_map(|key| {
            let data = serde_json::json!({ key.clone(): "future_config" });
            ToolChoice::Unknown {
                choice_type: key,
                data,
            }
        }),
    ]
}

// =============================================================================
// Tool Strategy
// =============================================================================

pub(super) fn arb_function_parameters() -> impl Strategy<Value = FunctionParameters> {
    (
        Just("object".to_string()),
        arb_json_value(),
        prop::collection::vec(arb_identifier(), 0..3),
    )
        .prop_map(|(type_, properties, required)| {
            FunctionParameters::new(type_, properties, required)
        })
}

pub(super) fn arb_tool() -> impl Strategy<Value = Tool> {
    prop_oneof![
        // Function tool
        (
            arb_identifier(),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_function_parameters()),
        )
            .prop_map(|(name, description, parameters)| Tool::Function {
                name,
                description,
                parameters
            }),
        // Google Search (non-empty search_types when present)
        proptest::option::of(proptest::collection::vec(arb_search_type(), 1..3))
            .prop_map(|search_types| Tool::GoogleSearch { search_types }),
        // Google Maps (with optional location bias)
        (
            proptest::option::of(any::<bool>()),
            proptest::option::of(arb_clean_f64()),
            proptest::option::of(arb_clean_f64()),
        )
            .prop_map(|(enable_widget, latitude, longitude)| Tool::GoogleMaps {
                enable_widget,
                latitude,
                longitude,
            }),
        Just(Tool::CodeExecution),
        Just(Tool::UrlContext),
        // FileSearch tool
        (
            proptest::collection::vec(arb_identifier(), 1..4),
            proptest::option::of(any::<i32>()),
            proptest::option::of(arb_text())
        )
            .prop_map(|(store_names, top_k, metadata_filter)| {
                Tool::FileSearch {
                    store_names,
                    top_k,
                    metadata_filter,
                }
            }),
        // ComputerUse tool
        (
            prop_oneof![
                Just("browser".to_string()),
                Just("mobile".to_string()),
                Just("desktop".to_string()),
                arb_identifier(),
            ],
            proptest::collection::vec(arb_identifier(), 0..3),
            proptest::option::of(any::<bool>()),
            proptest::collection::vec(arb_identifier(), 0..2),
        )
            .prop_map(
                |(environment, excluded, detect, disabled)| Tool::ComputerUse {
                    environment,
                    excluded_predefined_functions: excluded,
                    enable_prompt_injection_detection: detect,
                    disabled_safety_policies: disabled,
                }
            ),
        // MCP Server (use BTreeMap for deterministic JSON key ordering in roundtrip tests;
        // allowed_tools is non-empty when present since empty is skipped on serialize)
        (
            arb_identifier(),
            arb_text(),
            proptest::option::of(proptest::collection::vec(arb_allowed_tools(), 1..3)),
            proptest::option::of(proptest::collection::btree_map(
                arb_identifier(),
                arb_text(),
                1..3
            )),
        )
            .prop_map(|(name, url, allowed_tools, headers)| Tool::McpServer {
                name,
                url,
                allowed_tools,
                headers: headers.map(|m| m.into_iter().collect()),
            }),
        // Retrieval tool (types non-empty when present since empty is skipped)
        (
            proptest::option::of(proptest::collection::vec(arb_retrieval_type(), 1..3)),
            proptest::option::of(arb_vertex_ai_search_config()),
            proptest::option::of(arb_exa_ai_search_config()),
            proptest::option::of(arb_parallel_ai_search_config()),
            proptest::option::of(arb_rag_store_config()),
        )
            .prop_map(
                |(
                    retrieval_types,
                    vertex_ai_search_config,
                    exa_ai_search_config,
                    parallel_ai_search_config,
                    rag_store_config,
                )| Tool::Retrieval {
                    retrieval_types,
                    vertex_ai_search_config,
                    exa_ai_search_config,
                    parallel_ai_search_config,
                    rag_store_config: rag_store_config.map(Box::new),
                },
            ),
        // Unknown tool (Tool preserves unknowns even in strict mode)
        (arb_unknown_type(), arb_unknown_object())
            .prop_map(|(tool_type, data)| Tool::Unknown { tool_type, data }),
    ]
}

// =============================================================================
// Retrieval Tool Strategies
// =============================================================================

pub(super) fn arb_retrieval_type() -> impl Strategy<Value = RetrievalType> {
    prop_oneof![
        Just(RetrievalType::VertexAiSearch),
        Just(RetrievalType::RagStore),
        Just(RetrievalType::ExaAiSearch),
        Just(RetrievalType::ParallelAiSearch),
        unknown_or(RetrievalType::VertexAiSearch, |retrieval_type| {
            RetrievalType::Unknown {
                data: serde_json::Value::String(retrieval_type.clone()),
                retrieval_type,
            }
        }),
    ]
}

pub(super) fn arb_vertex_ai_search_config() -> impl Strategy<Value = VertexAiSearchConfig> {
    (
        proptest::option::of(arb_identifier()),
        proptest::option::of(proptest::collection::vec(arb_identifier(), 0..3)),
    )
        .prop_map(|(engine, datastores)| VertexAiSearchConfig { engine, datastores })
}

pub(super) fn arb_exa_ai_search_config() -> impl Strategy<Value = ExaAiSearchConfig> {
    (arb_identifier(), proptest::option::of(arb_unknown_object())).prop_map(
        |(api_key, custom_config)| ExaAiSearchConfig {
            api_key,
            custom_config,
        },
    )
}

pub(super) fn arb_parallel_ai_search_config() -> impl Strategy<Value = ParallelAiSearchConfig> {
    (
        proptest::option::of(arb_identifier()),
        proptest::option::of(arb_unknown_object()),
    )
        .prop_map(|(api_key, custom_config)| ParallelAiSearchConfig {
            api_key,
            custom_config,
        })
}

pub(super) fn arb_rag_store_config() -> impl Strategy<Value = RagStoreConfig> {
    (
        proptest::option::of(proptest::collection::vec(
            (
                proptest::option::of(arb_identifier()),
                proptest::option::of(proptest::collection::vec(arb_identifier(), 0..2)),
            )
                .prop_map(|(rag_corpus, rag_file_ids)| RagResource {
                    rag_corpus,
                    rag_file_ids,
                }),
            0..3,
        )),
        proptest::option::of(1..100i32),
        proptest::option::of(arb_clean_f64()),
        proptest::option::of(arb_rag_retrieval_config()),
    )
        .prop_map(
            |(rag_resources, similarity_top_k, vector_distance_threshold, rag_retrieval_config)| {
                RagStoreConfig {
                    rag_resources,
                    similarity_top_k,
                    vector_distance_threshold,
                    rag_retrieval_config,
                }
            },
        )
}

pub(super) fn arb_rag_retrieval_config() -> impl Strategy<Value = RagRetrievalConfig> {
    (
        proptest::option::of(1..100i32),
        proptest::option::of(
            proptest::option::of(arb_clean_float().prop_map(|v| v.as_f64().unwrap() as f32))
                .prop_map(|alpha| HybridSearchConfig { alpha }),
        ),
        proptest::option::of(
            (
                proptest::option::of(arb_clean_f64()),
                proptest::option::of(arb_clean_f64()),
                proptest::option::of(arb_text()),
            )
                .prop_map(
                    |(vector_distance_threshold, vector_similarity_threshold, metadata_filter)| {
                        RagFilter {
                            vector_distance_threshold,
                            vector_similarity_threshold,
                            metadata_filter,
                        }
                    },
                ),
        ),
        proptest::option::of(
            proptest::option::of(arb_identifier()).prop_map(|model_name| RagRanking {
                ranking_config: "rank_service".to_string(),
                model_name,
                rank_service: None,
            }),
        ),
    )
        .prop_map(
            |(top_k, hybrid_search, filter, ranking)| RagRetrievalConfig {
                top_k,
                hybrid_search,
                filter,
                ranking,
            },
        )
}

proptest! {
    /// Test that FunctionCallingMode roundtrips correctly through JSON
    /// (wire format is lowercase).
    #[test]
    fn function_calling_mode_roundtrip(mode in arb_function_calling_mode()) {
        let json = serde_json::to_string(&mode).expect("Serialization should succeed");
        let restored: FunctionCallingMode = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(mode, restored);
    }

    /// Test that SearchType roundtrips correctly through JSON.
    #[test]
    fn search_type_roundtrip(search_type in arb_search_type()) {
        let json = serde_json::to_string(&search_type).expect("Serialization should succeed");
        let restored: SearchType = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(search_type, restored);
    }

    /// Test that AllowedTools roundtrips correctly through JSON.
    #[test]
    fn allowed_tools_roundtrip(allowed in arb_allowed_tools()) {
        let json = serde_json::to_string(&allowed).expect("Serialization should succeed");
        let restored: AllowedTools = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(allowed, restored);
    }

    /// Test that ToolChoice roundtrips stably through JSON.
    ///
    /// Uses JSON comparison since the Unknown variant regenerates its
    /// choice_type descriptor on deserialization.
    #[test]
    fn tool_choice_roundtrip(choice in arb_tool_choice()) {
        assert_value_roundtrip(&choice)?;
    }

    /// Test that RetrievalType roundtrips correctly through JSON.
    #[test]
    fn retrieval_type_roundtrip(retrieval_type in arb_retrieval_type()) {
        let json = serde_json::to_string(&retrieval_type).expect("Serialization should succeed");
        let restored: RetrievalType = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(retrieval_type, restored);
    }

    /// Test that Tool roundtrips stably through JSON.
    #[test]
    fn tool_roundtrip(tool in arb_tool()) {
        assert_value_roundtrip(&tool)?;
    }
}
