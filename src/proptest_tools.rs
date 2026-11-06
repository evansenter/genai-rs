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

// =============================================================================
// Recorded regressions
// =============================================================================
//
// A seed in `proptest-regressions/proptest_tools.txt` replays whatever value
// the strategy now draws from it, which drifts as the strategies change (the
// `McpServer` seed now draws a `GoogleMaps` tool). The value each seed shrank
// to when it was recorded is pinned here, so the regression stays covered
// whatever the strategy generates.

/// Seed `74d576b4` (`tool_roundtrip`): an MCP server whose URL and header
/// values are proptest's random Unicode (escapes, BOMs, bidi overrides,
/// NULs, quotes and backslashes), verbatim from the seed's comment.
#[test]
fn recorded_seed_mcp_server_with_random_unicode_roundtrips() {
    let url = "`=Ⱥ\r&\u{4}\\\"\u{202e}<*\u{e9630}\u{3}:c\u{53f8b}={\u{202e}'\u{3}O{\
        \u{74e0f}&\u{5450a}\u{48830}\u{202e}/\u{a7ea2}\u{3e95c}$/j\u{6}\u{5}\0&~\t\t7`]I\
        \u{ac76d}=iQ¥㏓?\u{7f}\u{d1414}<\u{98f55}p.Ⱥ:𢢏𮱉\u{efe97}\u{45465}{\u{49452}v\u{202e}*\
        \"k\u{46869}\\y\u{7}\u{d3200}l\t\u{42d51}\u{feff}*\u{6}\u{90bb8}/\u{cd7fa}<��\r\0Ѩ|\
        \u{7f}�\u{cc50c}W\u{d75c5}Yn\u{b}�V\u{106dc0}\r?\u{202e}\tr\u{202e}𫉷5\u{feff}\u{feff}🕴\
        Ѩ&Ⱥ\t\u{7f}\u{7f}🕴";
    let first_header = "🕴`\u{6374c}¥\u{bcc51}�V\0'?S\u{feff}{%\u{feff}\u{7f}S=\u{b62d8}\
        \u{7f}¯ÜT\u{202e}\u{7f}\u{9b}\u{9c7e2}l.\u{9c643}:\u{7e75e}#$UK\u{b5000}Z¥\u{91}\
        \u{65c8f}\u{75794}Ⱥ ѨV🕴Q&🕴\u{d236e}|'9**=..é\u{feff}'{\u{202e}8Ѩ\u{41997} \u{f561a}^\t*\
        \"\t|\0\u{5dc8a}\u{8ccbe}.\\\u{3c8b3}\\\u{fecba}\0\u{7c8cc}g\rU\u{96a51}&Ⱥ\u{434d5}\
        \u{aa928}\u{3910e}\u{1e2bb}.\u{1b}\u{a4f6c}s*🕴*?*'.$<\u{911a5}\t:1`ȺȺ\u{6}\u{7b6f6}u\0\
        \u{feff}\u{a9cea}\t\u{e2a8c}\t\u{feff}\u{1b}𡣮{P.\t\u{95675}öF}B'\u{2}a¥%ef\u{ffd1a}\
        \u{1b}\u{b}\"\u{1b}\u{84bb7}\\?èb{\u{1fd74}\u{202e}{?6\u{b}[𣮴B&kA¥\0ȺT\u{feff}½`$\
        \u{b63f9}\u{102e4e}\u{1b}//t\u{d673e}\u{1000dc}\u{ddc1b}{Ѩ�<\u{19549}\t\"\u{a5db6}%\
        \u{202e}\u{2}\u{5}\u{b}D\u{c541d}¥\u{109c00}?aJ\t\u{b4b2b}\u{6bae2}\u{94d77}�\u{d491b}f\
        \0🕴\u{81468}'\u{4cc70}v🕴\\¥k[¥Gy\t\u{6055d}$\u{5b24e}*\u{851ab}\"W\\mx\t\r\u{202e}:\
        \u{c4f0f}¥🕴:\u{18cd9}🕴*%\u{ce8ce}\u{202e}I&\u{6}觰&.\u{bdd55}$:𝼋@�3\u{7f}\u{fc55b}\
        \u{84238}\\%�ѨJ\u{64635}=\u{76fe6}.'🕴>l1B\u{e5080}'\u{aa490}G+D🕴𢚌{\u{f5335}\u{c7cce}.\
        \u{97da8}\u{ebfee}ñC.#🕴\u{95e11}¥\\\u{202e}q;?齕H'\u{7f}= ¥\u{a0bb7}ç\u{5}.�'\u{3994b}P\
        {\u{9adcb}\u{ecfeb}�\t<\u{feff}$%\u{5}𬁒G{9\u{202e}\u{9a}\u{4ea1c}</\u{6062b}mBY'\0&$\
        \u{feff}\u{a5b87}\\W\u{b}\u{7}\"\u{b}/`Ѩ`𥵇\u{c8e22}'\u{52744}\u{4c4fb}V?\u{ad52f}C\"/&\
        \"<\u{10346b}/\u{10634b}\u{82090}\u{8a9d5}\u{6f095}J/\"\u{81f6a}%<\t`\u{e6348}\u{63b14}\
        1\u{b}\t*./\u{3d1e4}\u{1b}'*\u{8dd2e}'ç\\*\u{74fb2}\u{feff}<";
    let second_header = "🕴Ⱥ\u{bb3f5}{'\u{7f}\u{202e}K\u{ec5be}Å\u{1c3fb}G\rV\u{7f}\
        \u{c34d9}Ѩl*$?{\u{feff}\\\t'\r\\UC\u{ef898}\u{7f}\u{98ee0}\u{5}\"&\u{b}`\u{1b}~$靀N\0%\
        \tgk>'\u{202e}\"\u{feff}[$/\u{11845}\u{107a05}\0\\\t\u{e54e4}\u{e6ece}\u{cb1b9}v\r:\
        \u{6}rd䜿*''\u{feff}\u{718ea}\u{f6eee}#Ѩ\u{e4445}x:톻\u{104100}&{h`\u{b}\u{d5960}¥E=\r�\
        h�`.\u{50741}h=:\tl:\u{972da}\"/¥\u{5651d}\u{ece53}.🕴D.<m!Y.!&$e?\u{1b}Ѩz�î\u{c31f1}Ⱥ�$\
        \u{5ef73}혹\u{2}ѨO<\u{7}𘄺ꄩ\u{1b}'\u{54d72}=Ѩ㟲\rKÖ[O\u{1c1fb}\u{4}\u{a1412}Ѩ'Î\u{1b}c\
        Ç\u{9b}]\u{c3832}\u{9e220}`?\u{48a47}\u{81cd7}<\u{10e099}ÛUȺ=\u{f4d7f}�㨯*u\u{202e}\
        \u{93301}O\u{4d24f}\u{58a48}\u{1b}k\u{7f}\u{a39d2}??b¥/÷*.�??\u{52159}Ѩ\u{10badc}.8\
        \u{107db4}\u{1b}$\u{a9356}";
    let tool = Tool::McpServer {
        name: "_Zr71Ux_kQ71_v".to_string(),
        url: url.to_string(),
        allowed_tools: None,
        headers: Some(std::collections::HashMap::from([
            ("LtE_99zu_F_4__LF__c".to_string(), first_header.to_string()),
            ("Uj_7mcVau6xL_L_Ot_Z".to_string(), second_header.to_string()),
        ])),
    };
    assert_value_roundtrip(&tool).unwrap();
    let json = serde_json::to_value(&tool).unwrap();
    assert_eq!(json["url"], url);
    assert_eq!(json["headers"]["LtE_99zu_F_4__LF__c"], first_header);
}

/// Seed `75809b10` (`allowed_tools_roundtrip`): an unknown mode with no tools.
#[cfg(not(genai_strict_unknown))]
#[test]
fn recorded_seed_allowed_tools_with_unknown_mode_roundtrips() {
    let allowed = AllowedTools {
        mode: Some(FunctionCallingMode::Unknown {
            mode_type: "zz_0".to_string(),
            data: serde_json::Value::String("zz_0".to_string()),
        }),
        tools: vec![],
    };
    let json = serde_json::to_string(&allowed).expect("Serialization should succeed");
    let restored: AllowedTools =
        serde_json::from_str(&json).expect("Deserialization should succeed");
    assert_eq!(allowed, restored);
}
