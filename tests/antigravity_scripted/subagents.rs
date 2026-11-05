//! Subagent controls (#493 item 3), each shown changing what the harness
//! offers the root agent or a subagent: its tools, the subagents it may
//! invoke, its skills and its model.

use super::*;
use genai_rs::antigravity::{Subagent, SubagentSkills};

/// The model the `helper` subagent runs on, so its requests are told apart
/// from the root agent's.
const HELPER_MODEL: &str = "scripted-helper-model";

/// A model whose root agent invokes `helper` once, then answers; `helper`
/// answers at once.
async fn delegating_model() -> FakeGemini {
    let main_calls = Arc::new(AtomicUsize::new(0));
    FakeGemini::routed(move |request, _| {
        if request.target.contains(HELPER_MODEL) {
            return ModelTurn::text("helper-done");
        }
        match main_calls.fetch_add(1, Ordering::SeqCst) {
            0 => ModelTurn::call(
                "invoke_subagent",
                json!({"Subagents": [{
                    "TypeName": "helper", "Prompt": "say done", "Role": "Helper"
                }]}),
            ),
            _ => ModelTurn::text("delegated"),
        }
    })
    .await
}

/// The index of the first model request `helper` made.
async fn helper_request(fake: &FakeGemini) -> usize {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(n) = fake
            .requests()
            .iter()
            .position(|r| r.target.contains(HELPER_MODEL))
        {
            return n;
        }
        assert!(
            Instant::now() < deadline,
            "helper never called its model: {:?}",
            fake.requests()
                .iter()
                .map(|r| &r.target)
                .collect::<Vec<_>>()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// An agent whose root may invoke subagents, with `subagents` added.
fn delegating_agent(fake: &FakeGemini, subagents: Vec<Subagent>) -> AgentBuilder {
    subagents.into_iter().fold(
        scripted_agent(fake)
            .with_capabilities(Capabilities::read_only().enable(BuiltinTool::InvokeSubagent))
            .add_policy(policy::allow_all()),
        AgentBuilder::add_subagent,
    )
}

/// The `helper` subagent, on its own model.
fn helper() -> Subagent {
    Subagent::new("helper")
        .with_description("Says done.")
        .with_model(HELPER_MODEL)
}

/// A skills directory holding one skill, `name`, whose description carries
/// `marker`.
fn skill_dir(name: &str, marker: &str) -> tempfile::TempDir {
    let dir = scratch_dir("agy-scripted-subskills");
    std::fs::create_dir_all(dir.path().join(name)).unwrap();
    std::fs::write(
        dir.path().join(format!("{name}/SKILL.md")),
        format!("---\nname: {name}\ndescription: {marker} for probes.\n---\nBody.\n"),
    )
    .unwrap();
    dir
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_subagent_runs_on_its_model_without_define_subagent() {
    let fake = delegating_model().await;
    let mut agent = delegating_agent(&fake, vec![helper()])
        .spawn()
        .await
        .expect("spawn");
    agent.chat("delegate").await.expect("turn");
    // define_subagent is off by default; the rest of the subagent tools
    // are there.
    let tools = fake.tool_names(0);
    assert!(tools.contains(&"invoke_subagent".to_string()), "{tools:?}");
    assert!(!tools.contains(&"define_subagent".to_string()), "{tools:?}");
    // The subagent ran on its own model.
    helper_request(&fake).await;
    agent.shutdown().await.expect("shutdown");

    // Opting in offers define_subagent again.
    let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
    let mut agent = delegating_agent(&fake, Vec::new())
        .with_define_subagent_tool(true)
        .spawn()
        .await
        .expect("spawn");
    agent.chat("hi").await.expect("turn");
    let tools = fake.tool_names(0);
    assert!(tools.contains(&"define_subagent".to_string()), "{tools:?}");
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_allowed_subagents_limit_what_the_root_agent_is_offered() {
    let subagents = || {
        vec![
            Subagent::new("alphaagent").with_description("Alpha."),
            Subagent::new("betaagent").with_description("Beta."),
        ]
    };
    for allowed in [false, true] {
        let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
        let mut builder = delegating_agent(&fake, subagents());
        if allowed {
            builder = builder.with_allowed_subagents(["alphaagent"]);
        }
        let mut agent = builder.spawn().await.expect("spawn");
        agent.chat("hi").await.expect("turn");
        let request = fake.request_json(0).unwrap().to_string();
        assert!(request.contains("alphaagent"), "{request}");
        assert_eq!(
            request.contains("betaagent"),
            !allowed,
            "allowed_subagents={allowed}: {request}"
        );
        agent.shutdown().await.expect("shutdown");
    }
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_subagent_depth_and_allowed_subagents_shape_a_subagents_tools() {
    // (max depth, helper's allowed list) → whether helper can invoke, and
    // whether it is offered `otheragent`.
    for (depth, allow_only_leaf) in [(None, false), (Some(2), false), (Some(2), true)] {
        let fake = delegating_model().await;
        let mut nesting_helper = helper()
            .with_capabilities(Capabilities::read_only().enable(BuiltinTool::InvokeSubagent));
        if allow_only_leaf {
            nesting_helper = nesting_helper.with_allowed_subagents(["leafagent"]);
        }
        let mut builder = delegating_agent(
            &fake,
            vec![
                nesting_helper,
                Subagent::new("leafagent").with_description("Leaf."),
                Subagent::new("otheragent").with_description("Other."),
            ],
        );
        if let Some(depth) = depth {
            builder = builder.with_max_subagent_depth(depth);
        }
        let mut agent = builder.spawn().await.expect("spawn");
        agent.chat("delegate").await.expect("turn");
        let n = helper_request(&fake).await;
        let tools = fake.tool_names(n);
        let request = fake.request_json(n).unwrap().to_string();
        let label = format!("depth {depth:?}, only leaf {allow_only_leaf}");
        // The harness default is one level: a subagent cannot invoke.
        let nests = depth.is_some();
        assert_eq!(
            tools.contains(&"invoke_subagent".to_string()),
            nests,
            "{label}: {tools:?}"
        );
        assert_eq!(
            tools.contains(&"manage_subagents".to_string()),
            nests,
            "{label}: {tools:?}"
        );
        if nests {
            assert!(request.contains("leafagent"), "{label}: {request}");
            assert_eq!(
                request.contains("otheragent"),
                !allow_only_leaf,
                "{label}: {request}"
            );
        }
        agent.shutdown().await.expect("shutdown");
    }
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_subagent_skills_choose_the_skills_a_subagent_sees() {
    let first = skill_dir("first-skill", "FIRST-MARK-5101");
    let second = skill_dir("second-skill", "SECOND-MARK-5102");
    let own = skill_dir("own-skill", "OWN-MARK-5103");
    let own_path = || own.path().to_string_lossy().to_string();
    // (skills, sees first, sees second, sees own)
    let cases = [
        (None, true, true, false),
        (Some(SubagentSkills::inherit()), true, true, false),
        (Some(SubagentSkills::none()), false, false, false),
        (
            Some(SubagentSkills::inherit().add_skill_name("second-skill")),
            false,
            true,
            false,
        ),
        (
            Some(SubagentSkills::inherit().add_extra_path(own_path())),
            true,
            true,
            true,
        ),
        (
            Some(SubagentSkills::from_paths([own_path()])),
            false,
            false,
            true,
        ),
    ];
    for (skills, first_seen, second_seen, own_seen) in cases {
        let label = format!("{skills:?}");
        let fake = delegating_model().await;
        let subagent = match skills {
            Some(skills) => helper().with_skills(skills),
            None => helper(),
        };
        let mut agent = delegating_agent(&fake, vec![subagent])
            .add_skills_path(first.path().to_string_lossy())
            .add_skills_path(second.path().to_string_lossy())
            .spawn()
            .await
            .expect("spawn");
        agent.chat("delegate").await.expect("turn");
        let prompt = fake.system_prompt(helper_request(&fake).await);
        assert_eq!(
            [
                prompt.contains("FIRST-MARK-5101"),
                prompt.contains("SECOND-MARK-5102"),
                prompt.contains("OWN-MARK-5103"),
            ],
            [first_seen, second_seen, own_seen],
            "{label}: {prompt}"
        );
        // The root agent keeps its own skills whatever the subagent sees.
        let root = fake.system_prompt(0);
        assert!(
            root.contains("FIRST-MARK-5101") && root.contains("SECOND-MARK-5102"),
            "{root}"
        );
        agent.shutdown().await.expect("shutdown");
    }
}
