use super::*;
use serde_json::json;

// -------------------------------------------------------------------
// Golden proto-JSON fixtures generated with the reference
// implementation (google-antigravity 0.1.5, protobuf json_format;
// user input re-derived against 0.1.18).
// -------------------------------------------------------------------

#[test]
fn test_input_event_user_input_golden() {
    // Harness 0.1.18 carries every user message as a multi-part
    // object. The pre-0.1.18 `{"userInput": "hello"}` string form is a
    // harness-side parse error that surfaces only on its stderr — the
    // turn never starts and every chat ran to its timeout.
    let event = InputEvent::user_text("hello");
    assert_eq!(
        serde_json::to_value(&event).unwrap(),
        json!({"userInput": {"parts": [{"text": "hello"}]}})
    );
    assert_eq!(
        event,
        InputEvent::UserInput(UserInput::text("hello")),
        "user_text is sugar for a single text part"
    );
}

#[test]
fn test_input_event_tool_response_golden() {
    let event = InputEvent::ToolResponse(ToolResponse {
        id: "1".to_string(),
        response_json: Some(r#"{"a":1}"#.to_string()),
        ..Default::default()
    });
    assert_eq!(
        serde_json::to_value(&event).unwrap(),
        json!({"toolResponse": {"id": "1", "responseJson": "{\"a\":1}"}})
    );
}

#[test]
fn test_input_event_halt_request_golden() {
    let event = InputEvent::HaltRequest(true);
    assert_eq!(
        serde_json::to_value(&event).unwrap(),
        json!({"haltRequest": true})
    );
}

#[test]
fn test_input_event_tool_confirmation_golden() {
    let event = InputEvent::ToolConfirmation(ToolConfirmation {
        trajectory_id: "t".to_string(),
        step_index: 2,
        accepted: true,
    });
    assert_eq!(
        serde_json::to_value(&event).unwrap(),
        json!({"toolConfirmation": {"trajectoryId": "t", "stepIndex": 2, "accepted": true}})
    );
}

#[test]
fn test_input_event_call_hook_response_golden() {
    let event = InputEvent::CallHookResponse(CallHookResponse {
        request_id: "r".to_string(),
        pre_tool_result: Some(HookVerdict {
            decision: Some(HookDecision::Allow),
            reason: Some("ok".to_string()),
        }),
        ..Default::default()
    });
    assert_eq!(
        serde_json::to_value(&event).unwrap(),
        json!({"callHookResponse": {"requestId": "r", "preToolResult": {"decision": "ALLOW", "reason": "ok"}}})
    );
}

#[test]
fn test_initialize_conversation_event_golden() {
    // Matches the reference SDK's json_format.MessageToJson output for
    // an equivalent HarnessConfig (field-by-field; ordering differs).
    let event = InitializeConversationEvent {
        config: Some(HarnessConfig {
            cascade_id: Some("cid".to_string()),
            system_instructions: Some(SystemInstructions::custom_text("hi")),
            tools: vec![Tool {
                name: Some("t".to_string()),
                description: Some("d".to_string()),
                parameters_json_schema: Some("{}".to_string()),
                response_json_schema: Some("{}".to_string()),
            }],
            harness_side_tools: Some(HarnessSideTools {
                run_command: Some(ToolToggle::new(false)),
                view_file: Some(ToolToggle::new(true)),
                ..Default::default()
            }),
            workspaces: vec![Workspace::filesystem("/w")],
            mcp_servers: vec![McpServerConfig {
                name: Some("git".to_string()),
                stdio: Some(McpStdioTransport {
                    command: Some("uvx".to_string()),
                    args: vec!["x".to_string()],
                    env: BTreeMap::from([("K".to_string(), "V".to_string())]),
                }),
                ..Default::default()
            }],
            models: vec![ModelConfig {
                name: Some("test-model".to_string()),
                types: vec![ModelType::Text],
                gemini_api_endpoint: Some(GeminiApiEndpoint {
                    api_key: Some("k".to_string()),
                    http_headers: BTreeMap::from([("a".to_string(), "b".to_string())]),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            enabled_hooks: vec![LifecycleHook::PreTool],
            ..Default::default()
        }),
    };
    let expected = json!({"config": {
        "cascadeId": "cid",
        "systemInstructions": {"custom": {"part": [{"text": "hi"}]}},
        "tools": [{"name": "t", "description": "d", "parametersJsonSchema": "{}", "responseJsonSchema": "{}"}],
        "harnessSideTools": {"runCommand": {"enabled": false}, "viewFile": {"enabled": true}},
        "workspaces": [{"filesystemWorkspace": {"directory": "/w"}}],
        "mcpServers": [{"name": "git", "stdio": {"command": "uvx", "args": ["x"], "env": {"K": "V"}}}],
        "models": [{"name": "test-model", "types": ["MODEL_TYPE_TEXT"], "geminiApiEndpoint": {"httpHeaders": {"a": "b"}, "apiKey": "k"}}],
        "enabledHooks": ["LIFECYCLE_HOOK_PRE_TOOL"],
    }});
    assert_eq!(serde_json::to_value(&event).unwrap(), expected);
}

#[test]
fn trajectory_terminal_state_accepts_both_harness_spellings() {
    // Harness 0.1.10 renamed the terminal state. Both spellings must
    // land on `Idle`, because only `Idle` ends a turn: when this
    // regressed, every turn ran to its timeout with no parse error
    // and no failed assertion anywhere — the value was simply
    // absorbed as `Unknown` and never matched.
    for wire in ["STATE_FULLY_IDLE", "STATE_IDLE"] {
        let state: TrajectoryState = serde_json::from_value(json!(wire)).unwrap();
        assert_eq!(
            state,
            TrajectoryState::Idle,
            "{wire} must deserialize to the terminal Idle state"
        );
        assert!(!state.is_unknown(), "{wire} must not degrade to Unknown");
    }
    // The canonical (current-harness) spelling is what we re-emit.
    assert_eq!(TrajectoryState::Idle.as_wire_str(), "STATE_FULLY_IDLE");

    // New in 0.1.10, and deliberately *not* terminal — it means the
    // trajectory is blocked on subtasks and will return to Running.
    let waiting: TrajectoryState =
        serde_json::from_value(json!("STATE_WAITING_FOR_TASKS")).unwrap();
    assert_eq!(waiting, TrajectoryState::WaitingForTasks);
    assert!(!waiting.is_unknown());

    // A genuinely unrecognized state still degrades (Evergreen).
    let bogus: TrajectoryState = serde_json::from_value(json!("STATE_FUTURE")).unwrap();
    assert!(bogus.is_unknown());
    assert_eq!(bogus.unknown_state_type(), Some("STATE_FUTURE"));
}

#[test]
fn output_event_reads_usage_from_both_harness_shapes() {
    // 0.1.5 flat form.
    let flat =
        r#"{"seqNum": "1", "usageMetadata": {"promptTokenCount": "10", "totalTokenCount": "20"}}"#;
    let event: OutputEvent = serde_json::from_str(flat).unwrap();
    let usage = event.usage_metadata.as_ref().expect("flat usage");
    assert_eq!(usage.prompt_token_count, Some(10));
    assert_eq!(usage.total_token_count, Some(20));

    // 0.1.10 nested form: the aggregate lives under `total`, with a
    // per-trajectory breakdown alongside it that we do not model.
    let nested = r#"{"seqNum": "1", "usageUpdate": {"agents": [{"trajectoryId": "t1", "usage": {"totalTokenCount": "5"}}], "total": {"promptTokenCount": "10", "totalTokenCount": "20"}}}"#;
    let event: OutputEvent = serde_json::from_str(nested).unwrap();
    let usage = event.usage_metadata.as_ref().expect("nested usage");
    assert_eq!(usage.prompt_token_count, Some(10));
    assert_eq!(usage.total_token_count, Some(20));
    // Consumed as envelope metadata, not misreported as an unknown
    // oneof payload (the leftover-key arm would otherwise claim it).
    assert!(
        event.payload.is_none(),
        "usageUpdate must not be read as a payload variant, got {:?}",
        event.payload
    );
}

// -------------------------------------------------------------------
// Harness 0.1.18 goldens, verbatim from LOUD_WIRE captures (ids
// shortened). Each exercises a shape new in, or first observed on,
// this revision.
// -------------------------------------------------------------------

#[test]
fn test_0_1_18_custom_tool_step_golden() {
    // The trajectory's record of a client-executed tool. It must parse
    // into the typed field rather than `extra` — an action that lands in
    // `extra` is read as an unknown builtin, which fails closed at a
    // confirmation. Note the echoed response's `id` is the tool *name*.
    let raw = r#"{"stepUpdate":{"cascadeId":"c","customTool":{"toolCall":{"arguments":{"fields":[{"name":"city","value":{"stringValue":"Zurich"}}]},"argumentsJson":"{\"city\":\"Zurich\"}","id":"call_58372","name":"antigravity_test_weather"},"toolResponse":{"id":"antigravity_test_weather","responseJson":"{\"result\":\"ok\"}"}},"source":"SOURCE_MODEL","state":"STATE_DONE","stepIndex":1,"target":"TARGET_ENVIRONMENT","text":"Weather check","textDelta":"","thinking":"","thinkingDelta":"","trajectoryId":"t"},"seqNum":"9"}"#;
    let event: OutputEvent = serde_json::from_str(raw).unwrap();
    let Some(OutputPayload::StepUpdate(step)) = &event.payload else {
        panic!("expected StepUpdate, got {:?}", event.payload);
    };
    assert!(step.extra.is_empty(), "unmodeled: {:?}", step.extra.keys());
    let custom = step.custom_tool.as_ref().expect("customTool");
    let call = custom.tool_call.as_ref().unwrap();
    assert_eq!(call.name.as_deref(), Some("antigravity_test_weather"));
    assert_eq!(call.arguments_json.as_deref(), Some(r#"{"city":"Zurich"}"#));
    let response = custom.tool_response.as_ref().unwrap();
    assert_eq!(
        response.response_json.as_deref(),
        Some(r#"{"result":"ok"}"#)
    );

    // And an in-flight record with no response and no id still parses:
    // `ToolResponse` is lenient because proto3 may omit its `id`.
    let partial: ActionCustomTool =
        serde_json::from_value(json!({"toolResponse": {"responseJson": "{}"}})).unwrap();
    assert_eq!(partial.tool_response.unwrap().id, "");
}

#[test]
fn test_0_1_18_subagent_trajectory_update_golden() {
    let raw = r#"{"trajectoryStateUpdate":{"depth":1,"parentTrajectoryId":"root","state":"STATE_FULLY_IDLE","trajectoryId":"sub"}}"#;
    let event: OutputEvent = serde_json::from_str(raw).unwrap();
    let Some(OutputPayload::TrajectoryStateUpdate(update)) = &event.payload else {
        panic!("expected TrajectoryStateUpdate");
    };
    assert!(
        update.extra.is_empty(),
        "unmodeled: {:?}",
        update.extra.keys()
    );
    assert_eq!(update.parent_trajectory_id.as_deref(), Some("root"));
    assert_eq!(update.depth, Some(1));
    assert_eq!(update.state, Some(TrajectoryState::Idle));

    let stopped: TrajectoryStateUpdate = serde_json::from_value(json!({
        "trajectoryId": "root",
        "state": "STATE_FULLY_IDLE",
        "stopReason": "STOP_REASON_QUOTA_EXHAUSTED"
    }))
    .unwrap();
    assert_eq!(stopped.stop_reason, Some(StopReason::QuotaExhausted));
}

#[test]
fn test_0_1_18_usage_update_with_modality_details_golden() {
    let raw = r#"{"usageUpdate":{"agents":[{"trajectoryId":"t","usage":{"totalTokenCount":"407"}}],"total":{"cachedContentTokenCount":"0","candidatesTokenCount":"39","promptTokenCount":"287","promptTokensDetails":[{"modality":"TEXT","tokenCount":"287"}],"thoughtsTokenCount":"81","totalTokenCount":"407"}}}"#;
    let event: OutputEvent = serde_json::from_str(raw).unwrap();
    let usage = event.usage_metadata.expect("usage");
    assert!(
        usage.extra.is_empty(),
        "unmodeled: {:?}",
        usage.extra.keys()
    );
    assert_eq!(usage.total_token_count, Some(407));
    assert_eq!(
        usage.prompt_tokens_details,
        vec![ModalityTokenCount {
            modality: Some(Modality::Text),
            token_count: Some(287),
        }]
    );
}

#[test]
fn test_0_1_18_init_response_and_hook_args_golden() {
    let raw = r#"{"initializeConversationResponse":{"cascadeId":"c","sandboxStatus":{"available":true,"unavailableReason":""}},"seqNum":"1"}"#;
    let event: OutputEvent = serde_json::from_str(raw).unwrap();
    let Some(OutputPayload::InitializeConversationResponse(init)) = &event.payload else {
        panic!("expected InitializeConversationResponse");
    };
    assert!(init.extra.is_empty(), "unmodeled: {:?}", init.extra.keys());
    assert_eq!(init.sandbox_status.as_ref().unwrap().available, Some(true));

    let raw = r#"{"callHookRequest":{"name":"PreTool","preToolArgs":{"argumentsJson":"{}","callId":"call_85755","serverName":"widgets","stepIndex":2,"toolName":"lookup_widget_code","trajectoryId":"t"},"requestId":"hook_request_0","type":"LIFECYCLE_HOOK_PRE_TOOL"}}"#;
    let event: OutputEvent = serde_json::from_str(raw).unwrap();
    let Some(OutputPayload::CallHookRequest(request)) = &event.payload else {
        panic!("expected CallHookRequest");
    };
    let args = request.pre_tool_args.as_ref().unwrap();
    assert!(args.extra.is_empty(), "unmodeled: {:?}", args.extra.keys());
    assert_eq!(args.server_name.as_deref(), Some("widgets"));
    assert_eq!(args.step_index, Some(2));
    assert_eq!(
        hook_tool_name("lookup_widget_code", args.server_name.as_deref()),
        "mcp_widgets_lookup_widget_code"
    );
    assert_eq!(
        hook_tool_name("invoke_subagent", Some("")),
        "start_subagent"
    );
    assert_eq!(hook_tool_name("view_file", None), "view_file");
}

#[test]
fn test_output_event_step_update_golden() {
    // Verbatim harness-side encoding: int64/uint64 as strings.
    let raw = r#"{"seqNum": "12345678901234", "timestampMicros": "2", "stepUpdate": {"trajectoryId": "traj", "stepIndex": 3, "state": "STATE_DONE", "source": "SOURCE_MODEL", "target": "TARGET_USER", "text": "hello", "runCommand": {"commandLine": "ls", "exitCode": 0, "combinedOutput": "a\n"}}, "usageMetadata": {"promptTokenCount": "10", "totalTokenCount": "20"}}"#;
    let event: OutputEvent = serde_json::from_str(raw).unwrap();
    assert_eq!(event.seq_num, Some(12_345_678_901_234));
    assert_eq!(event.timestamp_micros, Some(2));
    let usage = event.usage_metadata.as_ref().unwrap();
    assert_eq!(usage.prompt_token_count, Some(10));
    assert_eq!(usage.total_token_count, Some(20));
    let Some(OutputPayload::StepUpdate(step)) = &event.payload else {
        panic!("expected StepUpdate, got {:?}", event.payload);
    };
    assert_eq!(step.trajectory_id.as_deref(), Some("traj"));
    assert_eq!(step.step_index, Some(3));
    assert_eq!(step.state, Some(StepState::Done));
    assert_eq!(step.source, Some(StepSource::Model));
    assert_eq!(step.target, Some(StepTarget::User));
    assert_eq!(step.text.as_deref(), Some("hello"));
    let run = step.run_command.as_ref().unwrap();
    assert_eq!(run.command_line.as_deref(), Some("ls"));
    assert_eq!(run.exit_code, Some(0));
    assert_eq!(run.combined_output.as_deref(), Some("a\n"));
}

#[test]
fn test_output_event_tool_call_golden() {
    let raw = r#"{"seqNum": "1", "toolCall": {"id": "abc", "name": "get_weather", "argumentsJson": "{\"city\":\"SF\"}"}}"#;
    let event: OutputEvent = serde_json::from_str(raw).unwrap();
    let Some(OutputPayload::ToolCall(call)) = &event.payload else {
        panic!("expected ToolCall");
    };
    assert_eq!(call.id.as_deref(), Some("abc"));
    assert_eq!(call.name.as_deref(), Some("get_weather"));
    assert_eq!(call.arguments_json.as_deref(), Some(r#"{"city":"SF"}"#));
}

#[test]
fn test_output_event_init_response_golden() {
    let raw = r#"{"initializeConversationResponse": {"cascadeId": "cid"}}"#;
    let event: OutputEvent = serde_json::from_str(raw).unwrap();
    let Some(OutputPayload::InitializeConversationResponse(resp)) = &event.payload else {
        panic!("expected InitializeConversationResponse");
    };
    assert_eq!(resp.cascade_id.as_deref(), Some("cid"));
    assert!(resp.history.is_empty());
}

// -------------------------------------------------------------------
// Roundtrips and Evergreen preservation
// -------------------------------------------------------------------

fn roundtrip_output(event: &OutputEvent) -> OutputEvent {
    let json = serde_json::to_string(event).unwrap();
    serde_json::from_str(&json).unwrap()
}

fn roundtrip_input(event: &InputEvent) -> InputEvent {
    let json = serde_json::to_string(event).unwrap();
    serde_json::from_str(&json).unwrap()
}

#[test]
fn test_input_event_roundtrip_all_variants() {
    let events = vec![
        InputEvent::user_text("hi"),
        InputEvent::UserInput(UserInput {
            parts: vec![
                UserInputPart::text("a"),
                UserInputPart {
                    media: Some(Media {
                        mime_type: Some("image/png".to_string()),
                        description: Some("d".to_string()),
                        data: Some("aGVsbG8=".to_string()),
                    }),
                    ..Default::default()
                },
                UserInputPart {
                    slash_command: Some(SlashCommand {
                        name: Some("compact".to_string()),
                    }),
                    ..Default::default()
                },
            ],
        }),
        InputEvent::ToolConfirmation(ToolConfirmation {
            trajectory_id: "t".to_string(),
            step_index: 7,
            accepted: false,
        }),
        InputEvent::ToolResponse(ToolResponse {
            id: "id".to_string(),
            response_json: Some("{}".to_string()),
            supplemental_media: vec![Media::default()],
            ..Default::default()
        }),
        InputEvent::QuestionResponse(UserQuestionsResponse {
            trajectory_id: "t".to_string(),
            step_index: 1,
            cancelled: None,
            response: Some(QuestionsResponse {
                answers: vec![
                    UserQuestionAnswer::unanswered(),
                    UserQuestionAnswer {
                        multiple_choice_answer: Some(MultipleChoiceAnswer {
                            selected_choice_indices: vec![0, 2],
                            freeform_response: Some("f".to_string()),
                        }),
                        ..Default::default()
                    },
                ],
            }),
        }),
        InputEvent::HaltRequest(true),
        InputEvent::AutomatedTrigger("tick".to_string()),
        InputEvent::CallHookResponse(CallHookResponse {
            request_id: "r".to_string(),
            empty_result: Some(EmptyResult {}),
            ..Default::default()
        }),
        InputEvent::SessionEndRequest(true),
        InputEvent::Unknown {
            event_type: "futureEvent".to_string(),
            data: json!({"x": 1}),
        },
    ];
    for event in events {
        assert_eq!(roundtrip_input(&event), event);
    }
}

#[test]
fn test_input_event_unknown_variant_preserves_wire_format() {
    let raw = r#"{"futureEvent": {"payload": 42}}"#;
    let event: InputEvent = serde_json::from_str(raw).unwrap();
    assert!(event.is_unknown());
    assert_eq!(event.unknown_event_type(), Some("futureEvent"));
    assert_eq!(event.unknown_data(), Some(&json!({"payload": 42})));
    // Roundtrips back to the original single-key object.
    assert_eq!(
        serde_json::to_value(&event).unwrap(),
        json!({"futureEvent": {"payload": 42}})
    );
}

#[test]
fn test_output_event_roundtrip_all_variants() {
    let events = vec![
        OutputEvent {
            seq_num: Some(1),
            timestamp_micros: Some(2),
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: Some(1),
                total_token_count: Some(2),
                ..Default::default()
            }),
            payload: Some(OutputPayload::StepUpdate(Box::new(StepUpdate {
                trajectory_id: Some("t".to_string()),
                step_index: Some(1),
                state: Some(StepState::Active),
                text_delta: Some("d".to_string()),
                ..Default::default()
            }))),
        },
        OutputEvent {
            payload: Some(OutputPayload::TrajectoryStateUpdate(
                TrajectoryStateUpdate {
                    trajectory_id: Some("t".to_string()),
                    state: Some(TrajectoryState::Idle),
                    stop_reason: Some(StopReason::QuotaExhausted),
                    ..Default::default()
                },
            )),
            ..Default::default()
        },
        OutputEvent {
            payload: Some(OutputPayload::ToolCall(ToolCall {
                id: Some("i".to_string()),
                name: Some("n".to_string()),
                arguments_json: Some("{}".to_string()),
                ..Default::default()
            })),
            ..Default::default()
        },
        OutputEvent {
            payload: Some(OutputPayload::InitializeConversationResponse(
                InitializeConversationResponse {
                    cascade_id: Some("c".to_string()),
                    history: vec![StepUpdate::default()],
                    ..Default::default()
                },
            )),
            ..Default::default()
        },
        OutputEvent {
            payload: Some(OutputPayload::CallHookRequest(CallHookRequest {
                request_id: Some("r".to_string()),
                hook_type: Some(LifecycleHook::PreTool),
                pre_tool_args: Some(PreToolArgs {
                    tool_name: Some("run_command".to_string()),
                    arguments_json: Some("{}".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            })),
            ..Default::default()
        },
        OutputEvent {
            payload: Some(OutputPayload::SessionEndResponse(true)),
            ..Default::default()
        },
        OutputEvent {
            payload: Some(OutputPayload::Unknown {
                event_type: "newThing".to_string(),
                data: json!({"a": [1, 2]}),
            }),
            ..Default::default()
        },
        OutputEvent::default(),
    ];
    for event in events {
        assert_eq!(roundtrip_output(&event), event);
    }
}

#[test]
fn test_output_event_unknown_variant_preserved() {
    let raw = r#"{"seqNum": "5", "brandNewEvent": {"k": "v"}}"#;
    let event: OutputEvent = serde_json::from_str(raw).unwrap();
    assert_eq!(event.seq_num, Some(5));
    let payload = event.payload.as_ref().unwrap();
    assert!(payload.is_unknown());
    assert_eq!(payload.unknown_event_type(), Some("brandNewEvent"));
    assert_eq!(payload.unknown_data(), Some(&json!({"k": "v"})));
    // Reserialization preserves the unknown payload.
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["brandNewEvent"], json!({"k": "v"}));
    assert_eq!(json["seqNum"], json!(5));
}

#[test]
fn test_step_update_unknown_fields_preserved() {
    let raw = r#"{"trajectoryId": "t", "futureField": {"nested": true}}"#;
    let step: StepUpdate = serde_json::from_str(raw).unwrap();
    assert_eq!(
        step.extra.get("futureField"),
        Some(&json!({"nested": true}))
    );
    let json = serde_json::to_value(&step).unwrap();
    assert_eq!(json["futureField"], json!({"nested": true}));
}

#[test]
fn test_wire_enum_unknown_value_preserved() {
    let state: StepState = serde_json::from_value(json!("STATE_HIBERNATING")).unwrap();
    assert!(state.is_unknown());
    assert_eq!(state.unknown_state_type(), Some("STATE_HIBERNATING"));
    assert_eq!(state.unknown_data(), Some(&json!("STATE_HIBERNATING")));
    assert_eq!(
        serde_json::to_value(&state).unwrap(),
        json!("STATE_HIBERNATING")
    );
}

#[test]
fn test_wire_enum_known_values_roundtrip() {
    for (value, wire) in [
        (StepState::Unspecified, "STATE_UNSPECIFIED"),
        (StepState::Active, "STATE_ACTIVE"),
        (StepState::Done, "STATE_DONE"),
        (StepState::WaitingForUser, "STATE_WAITING_FOR_USER"),
        (StepState::Error, "STATE_ERROR"),
    ] {
        assert_eq!(serde_json::to_value(&value).unwrap(), json!(wire));
        let parsed: StepState = serde_json::from_value(json!(wire)).unwrap();
        assert_eq!(parsed, value);
        assert!(!parsed.is_unknown());
        assert_eq!(parsed.as_wire_str(), wire);
    }
}

#[test]
fn test_usage_metadata_accepts_numbers_and_strings() {
    let from_strings: UsageMetadata =
        serde_json::from_value(json!({"promptTokenCount": "7", "totalTokenCount": "9"})).unwrap();
    let from_numbers: UsageMetadata =
        serde_json::from_value(json!({"promptTokenCount": 7, "totalTokenCount": 9})).unwrap();
    assert_eq!(from_strings.prompt_token_count, Some(7));
    assert_eq!(from_strings, from_numbers);
}

#[test]
fn test_questions_request_deserializes() {
    let raw = r#"{"questions": [{"multipleChoice": {"question": "q?", "choices": ["a", "b"], "isMultiSelect": true}}, {"holographicQuestion": {"q": 1}}]}"#;
    let req: UserQuestionsRequest = serde_json::from_str(raw).unwrap();
    assert_eq!(req.questions.len(), 2);
    let mc = req.questions[0].multiple_choice.as_ref().unwrap();
    assert_eq!(mc.question.as_deref(), Some("q?"));
    assert_eq!(mc.choices, vec!["a", "b"]);
    assert_eq!(mc.is_multi_select, Some(true));
    // Unknown question type preserved via extra.
    assert!(req.questions[1].multiple_choice.is_none());
    assert!(req.questions[1].extra.contains_key("holographicQuestion"));
}

#[test]
fn test_edit_file_diff_structure() {
    let raw = r#"{"filePath": "/f", "diffBlock": [{"startLine": 1, "endLine": 2, "lines": [{"text": "x", "action": "LINE_ACTION_INSERT"}]}]}"#;
    let action: ActionEditFile = serde_json::from_str(raw).unwrap();
    assert_eq!(action.file_path.as_deref(), Some("/f"));
    assert_eq!(
        action.diff_block[0].lines[0].action,
        Some(LineAction::Insert)
    );
}

#[test]
fn test_decode_genai_struct() {
    // Verbatim `ToolCall.arguments` from a 0.1.18 capture.
    let wire = json!({"fields": [{"name": "city", "value": {"stringValue": "Zurich"}}]});
    assert_eq!(decode_genai_struct(&wire), Some(json!({"city": "Zurich"})));

    let nested = json!({"fields": [
        {"name": "n", "value": {"numberValue": 2.5}},
        {"name": "b", "value": {"boolValue": true}},
        {"name": "z", "value": {"nullValue": "NULL_VALUE"}},
        {"name": "unset", "value": {}},
        {"name": "o", "value": {"structValue": {"fields": [
            {"name": "k", "value": {"stringValue": "v"}}
        ]}}},
        {"name": "l", "value": {"listValue": {"values": [
            {"numberValue": 1}, {"stringValue": "two"}
        ]}}},
        {"name": "empty", "value": {"structValue": {}}},
    ]});
    assert_eq!(
        decode_genai_struct(&nested),
        Some(json!({
            "n": 2.5, "b": true, "z": null, "unset": null,
            "o": {"k": "v"}, "l": [1, "two"], "empty": {}
        }))
    );
    // Not the struct shape: refuse rather than guess.
    assert_eq!(decode_genai_struct(&json!({"city": "Zurich"})), None);
    assert_eq!(decode_genai_struct(&json!("x")), None);
}

#[test]
fn test_tool_call_arguments_struct_preserved() {
    let raw = r#"{"id": "1", "name": "n", "arguments": {"fields": [{"name": "city", "value": {"stringValue": "SF"}}]}}"#;
    let call: ToolCall = serde_json::from_str(raw).unwrap();
    assert!(call.arguments.is_some());
    let json = serde_json::to_value(&call).unwrap();
    assert_eq!(json["arguments"]["fields"][0]["name"], "city");
}

#[test]
fn test_flex_num_rejects_garbage() {
    let result: Result<UsageMetadata, _> =
        serde_json::from_value(json!({"promptTokenCount": "not-a-number"}));
    assert!(result.is_err());
    let result: Result<UsageMetadata, _> =
        serde_json::from_value(json!({"promptTokenCount": true}));
    assert!(result.is_err());
}

#[test]
fn test_flex_num_null_is_none() {
    let usage: UsageMetadata = serde_json::from_value(json!({"promptTokenCount": null})).unwrap();
    assert_eq!(usage.prompt_token_count, None);
}

#[test]
fn test_output_event_seq_num_string_and_number() {
    let a: OutputEvent = serde_json::from_str(r#"{"seqNum": "3"}"#).unwrap();
    let b: OutputEvent = serde_json::from_str(r#"{"seqNum": 3}"#).unwrap();
    assert_eq!(a.seq_num, Some(3));
    assert_eq!(a, b);
}
