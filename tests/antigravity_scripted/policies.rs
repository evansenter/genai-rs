//! Policies (#493 item 6): conditions, ask-user handlers and the auto
//! policy, against the real harness; and the stream naming a denied call
//! (a WP5 follow-up).

use super::*;

/// Drives one streamed turn, keeping each tool action's name, arguments and
/// decision.
async fn turn_actions(
    agent: &mut AntigravityAgent,
    prompt: &str,
) -> (Vec<(String, Value, ToolDecision)>, ChatResponse) {
    let mut actions = Vec::new();
    let mut stream = agent.send_streaming(prompt).await.expect("stream starts");
    while let Some(event) = stream.next().await {
        match event.expect("stream event") {
            AgentEvent::ToolAction {
                action, decision, ..
            } => actions.push((action.tool_name(), action.args(), decision)),
            AgentEvent::Finished(response) => return (actions, *response),
            _ => {}
        }
    }
    panic!("the stream ended without a Finished event");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_when_condition_denies_only_the_calls_it_matches() {
    let workspace = scratch_dir("agy-scripted-when");
    let secret = write_file(&workspace, "secret.txt", "secret-content-7701");
    let public = write_file(&workspace, "public.txt", "public-content-7702");
    let fake = FakeGemini::start(vec![
        view_file(&secret),
        view_file(&public),
        ModelTurn::text("done"),
    ])
    .await;
    let mut agent = scripted_agent(&fake)
        .add_workspace(workspace.path().to_string_lossy())
        .add_policy(policy::allow_all())
        .add_policy(
            policy::deny("view_file")
                .when(|call| {
                    call.args["AbsolutePath"]
                        .as_str()
                        .is_none_or(|path| path.contains("secret"))
                })
                .with_reason("WHEN-DENIED-7703"),
        )
        .spawn()
        .await
        .expect("spawn");
    let (actions, _) = turn_actions(&mut agent, "read both").await;
    let after_first = function_response_text(&fake, 1);
    assert!(after_first.contains("WHEN-DENIED-7703"), "{after_first}");
    assert!(
        !after_first.contains("secret-content-7701"),
        "{after_first}"
    );
    assert!(
        function_response_text(&fake, 2).contains("public-content-7702"),
        "the condition let the other call through"
    );
    // The denial is named on the stream, with the arguments it was judged
    // on (it used to read "error").
    let denied: Vec<_> = actions
        .iter()
        .filter(|(_, _, decision)| decision.is_denied())
        .collect();
    assert_eq!(denied.len(), 1, "{actions:?}");
    assert_eq!(denied[0].0, "view_file");
    assert_eq!(denied[0].1["AbsolutePath"], secret.as_str());
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_ask_user_handler_approves_and_refuses() {
    let workspace = scratch_dir("agy-scripted-ask");
    let ok = write_file(&workspace, "ok.txt", "approved-content-7711");
    let no = write_file(&workspace, "no.txt", "refused-content-7712");
    let fake = FakeGemini::start(vec![
        view_file(&ok),
        view_file(&no),
        ModelTurn::text("done"),
    ])
    .await;
    let asked: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = Arc::clone(&asked);
    let mut agent = scripted_agent(&fake)
        .add_workspace(workspace.path().to_string_lossy())
        .add_policy(
            policy::ask_user("view_file", move |call, reason| {
                let path = call.args["AbsolutePath"].as_str().unwrap_or_default();
                log.lock()
                    .unwrap()
                    .push(format!("{path}|{}", reason.unwrap_or_default()));
                path.ends_with("ok.txt")
            })
            .with_reason("ASK-REASON-7713"),
        )
        .spawn()
        .await
        .expect("spawn");
    agent.chat("read both").await.expect("turn");
    assert!(function_response_text(&fake, 1).contains("approved-content-7711"));
    let refused = function_response_text(&fake, 2);
    assert!(!refused.contains("refused-content-7712"), "{refused}");
    assert!(refused.contains("ASK-REASON-7713"), "{refused}");
    let asked = asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert!(
        asked.iter().all(|a| a.ends_with("|ASK-REASON-7713")),
        "{asked:?}"
    );
    agent.shutdown().await.expect("shutdown");
}

/// `ask_question` is gated like any other call, before the question is
/// posed (docs/ANTIGRAVITY.md, "Answering agent questions"): `deny_all()`
/// alone keeps every question from `on_questions`, an `allow` rule lets one
/// through, and a pre-tool hook sees the call first.
#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_a_question_is_gated_like_any_tool_call() {
    use genai_rs::antigravity::{AgentBehavior, QuestionReply};

    let ask = ModelTurn::call(
        "ask_question",
        json!({
            "questions": [{"question": "Which color?", "options": ["red", "blue"]}],
            "toolSummary": "Color question",
            "toolAction": "Asking question",
        }),
    );
    for (rules, pre_tool_hook, posed) in [
        (vec![policy::deny_all()], false, false),
        (
            vec![policy::deny_all(), policy::allow("ask_question")],
            false,
            true,
        ),
        (Vec::new(), true, true),
    ] {
        let fake = FakeGemini::start(vec![ask.clone(), ModelTurn::text("done")]).await;
        let log: Arc<Mutex<Vec<String>>> = Arc::default();
        let (on_question, on_tool) = (Arc::clone(&log), Arc::clone(&log));
        let mut builder = scripted_agent(&fake)
            .with_capabilities(Capabilities::read_only().enable(BuiltinTool::AskQuestion))
            .with_agent_behavior(AgentBehavior::Interactive)
            .on_questions(move |questions| {
                on_question
                    .lock()
                    .unwrap()
                    .push(format!("asked {}", questions.len()));
                QuestionReply::Answers(Vec::new())
            });
        for rule in rules {
            builder = builder.add_policy(rule);
        }
        if pre_tool_hook {
            builder = builder.on_pre_tool(move |call| {
                on_tool
                    .lock()
                    .unwrap()
                    .push(format!("pre-tool {}", call.name));
                PreToolDecision::Allow
            });
        }
        let mut agent = builder.spawn().await.expect("spawn");
        agent.chat("ask me").await.expect("turn");
        let log = log.lock().unwrap().clone();
        let answer = function_response_text(&fake, 1);
        if posed {
            let mut want = vec!["asked 1".to_string()];
            if pre_tool_hook {
                want.insert(0, "pre-tool ask_question".to_string());
            }
            assert_eq!(log, want, "{answer}");
        } else {
            assert!(log.is_empty(), "{log:?}");
            assert!(answer.contains("Denied by policy"), "{answer}");
        }
        agent.shutdown().await.expect("shutdown");
    }
}

/// A scripted model for the auto policy: the agent asks for a destructive
/// command; the harness's assessor (the request declaring `vet_*` tools)
/// vets it, and its follow-up analysis gets an answer it cannot parse, so
/// the harness asks this client (verified on 0.1.23, 2026-11-05). Unix only,
/// as the test that uses it is.
#[cfg(unix)]
async fn auto_policy_model(command: String, cwd: String) -> FakeGemini {
    let vetted = Arc::new(AtomicUsize::new(0));
    FakeGemini::routed(move |request, _| {
        let json = request.json();
        let declared: Vec<&str> = json["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|tool| {
                tool["functionDeclarations"]
                    .as_array()
                    .into_iter()
                    .flatten()
            })
            .filter_map(|declaration| declaration["name"].as_str())
            .collect();
        let body = json.to_string();
        if declared.contains(&"vet_destruction") {
            if vetted.fetch_add(1, Ordering::SeqCst) == 0 {
                return ModelTurn::call(
                    "vet_destruction",
                    json!({
                        "action_type": "delete", "category": "filesystem",
                        "command_or_action": command, "force": true, "recursive": true,
                        "target": command
                    }),
                );
            }
            return ModelTurn::text("assessed");
        }
        if declared.contains(&"run_command") {
            if body.contains("functionResponse") {
                return ModelTurn::text("auto-done");
            }
            return ModelTurn::call(
                "run_command",
                json!({"CommandLine": command, "Cwd": cwd, "WaitMsBeforeAsync": 3000}),
            );
        }
        ModelTurn::text("unparseable assessment")
    })
    .await
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_auto_policy_asks_the_handler_about_a_flagged_call() {
    let workspace = scratch_dir("agy-scripted-auto");
    let ws = workspace.path().canonicalize().unwrap();
    let doomed = ws.join("doomed");
    std::fs::create_dir_all(&doomed).unwrap();
    let command = format!("rm -rf {}", doomed.display());

    // Refused: the flagged command never runs, and the stream names it.
    let fake = auto_policy_model(command.clone(), ws.to_string_lossy().into_owned()).await;
    let flagged: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = Arc::clone(&flagged);
    let mut agent = scripted_agent(&fake)
        .add_workspace(ws.to_string_lossy())
        .with_capabilities(Capabilities::read_only().enable(BuiltinTool::RunCommand))
        .add_policy(policy::allow_all())
        .add_policy(policy::auto().with_handler(move |call, reason| {
            log.lock()
                .unwrap()
                .push(format!("{}|{}", call.name, reason.unwrap_or_default()));
            false
        }))
        .spawn()
        .await
        .expect("spawn");
    let (actions, _) = turn_actions(&mut agent, "clean up").await;
    let flagged = flagged.lock().unwrap().clone();
    assert_eq!(
        flagged.len(),
        1,
        "{flagged:?}; model calls: {:?}",
        fake.requests()
            .iter()
            .map(|r| &r.target)
            .collect::<Vec<_>>()
    );
    assert!(flagged[0].starts_with("run_command|"), "{flagged:?}");
    assert!(doomed.exists(), "a refused command must not run");
    assert!(
        actions
            .iter()
            .any(|(name, _, decision)| name == "run_command" && decision.is_denied()),
        "{actions:?}"
    );
    agent.shutdown().await.expect("shutdown");

    // Approved: the same command runs.
    let fake = auto_policy_model(command, ws.to_string_lossy().into_owned()).await;
    let mut agent = scripted_agent(&fake)
        .add_workspace(ws.to_string_lossy())
        .with_capabilities(Capabilities::read_only().enable(BuiltinTool::RunCommand))
        .add_policy(policy::allow_all())
        .add_policy(policy::auto().with_handler(|_, _| true))
        .spawn()
        .await
        .expect("spawn");
    agent.chat("clean up").await.expect("turn");
    assert!(!doomed.exists(), "an approved command runs");
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_policy_precedence_matches_the_harness() {
    // Within a tier deny beats allow whatever the order, and a named tool
    // beats the wildcard: the harness evaluator's order.
    let workspace = scratch_dir("agy-scripted-order");
    let path = write_file(&workspace, "f.txt", "order-content-7731");
    let fake = FakeGemini::start(vec![view_file(&path), ModelTurn::text("done")]).await;
    let mut agent = scripted_agent(&fake)
        .add_workspace(workspace.path().to_string_lossy())
        .add_policy(policy::deny_all())
        .add_policy(policy::allow("view_file"))
        .add_policy(policy::deny("view_file").with_name("second-deny"))
        .spawn()
        .await
        .expect("spawn");
    let (actions, _) = turn_actions(&mut agent, "read").await;
    let told = function_response_text(&fake, 1);
    assert!(told.contains("second-deny"), "{told}");
    assert!(matches!(
        actions.first(),
        Some((name, _, ToolDecision::Denied { .. })) if name == "view_file"
    ));
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_server_wide_rule_covers_its_server_only() {
    // Server `widgets_admin`'s tools are named `mcp_widgets_admin_…`, which
    // starts with `mcp_widgets_`: `allow("mcp_widgets_*")` must still not
    // reach them.
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp_echo_server.py");
    let server = |name: &str| {
        genai_rs::antigravity::McpServer::stdio("python3", [fixture.to_string_lossy().to_string()])
            .with_name(name)
            .with_force_all_tools_eager(true)
    };
    let call = |tool: &str| ModelTurn::call(tool, json!({"widget": "flange"}));
    let fake = FakeGemini::start(vec![
        call("mcp_widgets_admin_lookup_widget_code"),
        call("mcp_widgets_lookup_widget_code"),
        ModelTurn::text("done"),
    ])
    .await;
    let mut agent = scripted_agent(&fake)
        .add_mcp_server(server("widgets"))
        .add_mcp_server(server("widgets_admin"))
        .add_policy(policy::deny_all())
        .add_policy(policy::allow("mcp_widgets_*"))
        .spawn()
        .await
        .expect("spawn");
    let (actions, _) = turn_actions(&mut agent, "look it up twice").await;
    let decisions: Vec<_> = actions
        .iter()
        .map(|(name, _, decision)| {
            (
                name.as_str(),
                matches!(decision, ToolDecision::Denied { .. }),
            )
        })
        .collect();
    assert!(
        decisions.contains(&("mcp_widgets_admin_lookup_widget_code", true)),
        "{decisions:?}"
    );
    assert!(
        !function_response_text(&fake, 1).contains("wibble-3317-quux"),
        "the other server's tool ran: {}",
        function_response_text(&fake, 1)
    );
    // The rule's own server still runs.
    assert!(
        function_response_text(&fake, 2).contains("wibble-3317-quux"),
        "{}",
        function_response_text(&fake, 2)
    );
    agent.shutdown().await.expect("shutdown");
}
