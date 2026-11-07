# Antigravity Harness Guide

`genai_rs::antigravity` is a native Rust client for the `localharness` agent
runtime that ships with Google's [`google-antigravity`](https://pypi.org/project/google-antigravity/)
Python SDK. The harness binary *is* the agent: model calls, streaming,
history/compaction, built-in tools (shell, file edits, grep, web search,
image generation), MCP, subagents, and trajectory persistence all run inside
it. This module speaks its protocol directly — no Python in the loop — so
your tools, hooks, and policies are ordinary Rust.

> **Note**: The code blocks in this guide compile as doctests under the
> `antigravity` feature (`cargo test --doc --all-features`); hidden setup
> lines supply the `agent` and builder they use. They are `no_run`, since
> running one needs the harness binary and a key. `examples/antigravity/`
> and `tests/antigravity_harness.rs` run the same APIs against a real
> harness.

## Setup

Enable the feature:

```toml
[dependencies]
genai-rs = { version = "0.13", features = ["antigravity"] }
```

Install the harness binary (it ships inside the platform-specific wheel):

```bash
pip install google-antigravity==0.1.23
```

### Version pinning

The harness wire protocol is internal to Google's SDK and changes across
0.1.x releases. Each genai-rs release is verified against exactly one wheel
version, exposed as `antigravity::SUPPORTED_HARNESS_VERSION` (currently
`0.1.23`) — pin that version. **This build drives 0.1.23 only**: it writes
0.1.23's `runWorkflow` toggle, which 0.1.18 rejects at init (`unknown field
"runWorkflow"`, surfaced as `InitFailed`), and older harnesses fail as in
the second worked example below.

**Unknown-value preservation is not the same as forward compatibility.**
Unrecognized events, fields, and enum values are preserved in `Unknown`
variants and `extra` maps rather than erroring (the crate's Evergreen
philosophy), so a newer harness will not crash the bridge. But when a
*renamed* value is one the bridge **matches on**, preservation is exactly
what makes the breakage silent: the match simply stops firing.

The 0.1.5 → 0.1.10 upgrade is the worked example. `STATE_IDLE` became
`STATE_FULLY_IDLE`, and since only that value ends a turn, every turn ran
to its timeout with no error, no failed parse, and a single `warn!` as the
only evidence. `usageMetadata` likewise became `usageUpdate`, silently
zeroing token accounting. Both old spellings are now accepted as aliases,
so the bridge still *reads* either spelling — but the lesson generalizes:
when moving to an unverified harness, run the integration suite
(`--run-ignored all -E 'binary(antigravity_harness)'`) rather than
trusting that a clean parse means a working bridge, and see
[Debugging](#debugging) for the drift diagnostics that surface this class
of mismatch.

The 0.1.10 → 0.1.18 upgrade broke the other direction. The harness dropped
the plain-string `userInput` arm and renamed the multi-part
`complexUserInput` arm to `userInput`, so a user message is now always
`{"userInput": {"parts": [{"text": ...}]}}`. The old
`{"userInput": "hello"}` is a harness-side *parse error* — reported on its
stderr (`failed to unmarshal InputEvent`) and nowhere else — so every turn
failed to start and ran to its timeout. An outbound shape cannot be
aliased: the harness accepts exactly one, which is why a build drives a
single revision (0.1.23 today). The drift guard now checks field *types* as well as names,
since `userInput` kept its name while changing shape.

**The version is checked at spawn.** The binary carries no version of its
own, so `spawn()` reads it from the wheel install the binary sits in
(`google_antigravity-<version>.dist-info` beside
`google/antigravity/bin/localharness`, following a `PATH` symlink into its
venv) and logs a `warn!` when it differs from `SUPPORTED_HARNESS_VERSION`.
`AntigravityAgent::harness_version()` returns it (`None` for a binary that
is not inside a wheel install). A known mismatch is also named in an
`InitFailed` message and in a turn's `Timeout` diagnosis.

It warns rather than refuses (D-019) because the failures a mismatch
causes are already loud (an init rejection with the harness's reason, or a
timeout that now names the version), and because running a newer wheel on
purpose is how a harness bump is tested. To refuse one, compare the two
yourself:

```rust,no_run
use genai_rs::antigravity::SUPPORTED_HARNESS_VERSION;

# async fn run(mut agent: genai_rs::antigravity::AntigravityAgent) -> Result<(), Box<dyn std::error::Error>> {
if agent.harness_version() != Some(SUPPORTED_HARNESS_VERSION) {
    agent.shutdown().await?;
    return Err("unsupported localharness version".into());
}
# Ok(())
# }
```

New releases are watched for you: the `antigravity-release-watch.yml`
workflow compares PyPI's latest `google-antigravity` with the pin daily
and, when they differ, opens an `antigravity-release` issue with a diff of
the two wheels' protobuf descriptors.

### Binary discovery

`spawn()` finds the binary in this order:

1. `AgentBuilder::with_harness_path(...)` — explicit path.
2. `ANTIGRAVITY_HARNESS_PATH` environment variable (the same variable the
   Python SDK honors).
3. `google/antigravity/bin/localharness` inside `python3`'s site-packages.
4. `localharness` on `PATH`.

A miss returns `AntigravityError::HarnessNotFound` listing every location
searched.

## Quick start

```rust,no_run
use genai_rs::antigravity::{AntigravityAgent, policy};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let mut agent = AntigravityAgent::builder()
    .with_api_key(std::env::var("GEMINI_API_KEY")?)
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_system_instructions("You are a code-review assistant.")
    .add_workspace("/path/to/repo")
    .add_policy(policy::deny_all())
    .add_policy(policy::allow("view_file"))
    .spawn()
    .await?;

let response = agent.chat("Summarize the layout of this repo.").await?;
println!("{}", response.text());

agent.shutdown().await?;   // graceful: the harness persists its trajectory
# Ok(())
# }
```

`spawn()` launches the harness, performs the stdio handshake, connects to
its localhost WebSocket, and initializes the conversation. `shutdown()`
closes the WebSocket and stdin (the harness's graceful-exit signal; the
harness then stops the commands it started itself), and escalates to
SIGTERM/SIGKILL only if it lingers. Dropping the agent without `shutdown()`
kills the harness immediately — no zombie, but no trajectory persistence
either.

Every kill reaches the processes the harness started, not just the harness:
on Unix it runs in its own process group, and the forced paths (drop, an
init failure, the shutdown escalation) signal that group *and* the process
group of each of its children. The second part matters because the harness
gives every `run_command` command a process group of its own (0.1.23), so a
kill of the harness or its group alone left those commands running. A
command that detaches itself (`setsid`, a double-forked daemon) is still
out of reach. Because the harness is in its own group, a terminal's Ctrl-C
reaches only your process; when it exits, the harness sees stdin close and
shuts down gracefully.

### Images, documents and slash commands

`chat` and `send_streaming` take a string or a `Prompt`, which holds text,
inline media and slash commands in order (the Python SDK's content
primitives):

```rust,no_run
use genai_rs::antigravity::{Media, Prompt, SlashCommand};

# async fn run(mut agent: genai_rs::antigravity::AntigravityAgent) -> Result<(), Box<dyn std::error::Error>> {
let prompt = Prompt::new()
    .add_slash_command(SlashCommand::Plan)
    .add_text("What does this chart show, and what should we change?")
    .add_media(Media::from_file("q3-revenue.png")?);
let response = agent.chat(prompt).await?;
# let _ = response;
# Ok(())
# }
```

`Media` is one type for images, documents, audio and video: the MIME type
says which (`Media::new(bytes, "application/pdf")`, or `Media::from_file`
from the extension). The harness hands it to the model as inline data after
the text. Harness 0.1.23 drops a media item's `with_description` before the
model sees it, so say what matters in a text part. `SlashCommand::Plan`
reaches the model as `/plan` before the text, the harness's cue to plan
before acting.

## System instructions

The harness writes a system prompt of its own (7 to 10 thousand characters
on 0.1.23): tool guidance, safety rules, the workspace list, the skills
list and the built-in subagents. `with_system_instructions` **adds to** it,
as the Python SDK does: a string becomes a `<user_system_instructions>`
section at the end. `SystemInstructions` covers the other two forms:

```rust,no_run
use genai_rs::antigravity::{AntigravityAgent, SystemInstructions};

// Added to the harness's prompt (the default for a string).
let appended = AntigravityAgent::builder()
    .with_system_instructions("Review for security issues only.");

// The harness's prompt with its identity line replaced and a section added.
let edited = AntigravityAgent::builder().with_system_instructions(
    SystemInstructions::harness_default()
        .with_identity("You are Tern, a release-notes writer.")
        .add_section("style", "Write in the past tense."),
);

// Replaced entirely: you write the whole prompt.
let custom = AntigravityAgent::builder()
    .with_system_instructions(SystemInstructions::custom("You only answer `ok`."));
# let _ = (appended, edited, custom);
```

Replacing the prompt drops the tool guidance and safety rules with it, and
skills from `add_skills_path` become invisible to the model, so prefer the
appended forms. Before 0.14 a string replaced the prompt; pass
`SystemInstructions::custom(text)` for that behavior (D-023).
`Subagent::with_system_instructions` takes the same type.

## Workspaces

`add_workspace(path)` (or `with_workspace(path)` to replace) points the
harness's built-in tools at a directory and, by default, **confines the file
tools to it**: a file read outside the workspaces fails with `Access to path
"…" is denied`. `run_command` is not path-contained (a shell command reads
whatever the process can), so keep it off or gate it when the boundary
matters. The harness still allows the system temp directory and its own
`~/.gemini/antigravity/brain`, so don't treat `$TMPDIR` as private from the
agent. The harness enforces containment only when the client sends a policy
config; before `with_workspace_containment` (on by default), this crate
didn't, and an agent could read any file the process could. Turn it off
only to reach outside the workspaces, behind a pre-tool hook or policy.
Containment does not reach a subagent's tool calls on 0.1.23 (see
[Subagents](#subagents)).

The harness's own prompt lists the workspaces ("Active Workspaces",
verified on 0.1.23 for agents and subagents). [Custom
instructions](#system-instructions) replace that prompt, so the model would
otherwise not know their paths. With custom instructions `genai-rs`
therefore **announces the workspace root(s) to the model**: it adds a
short, clearly delimited note listing the configured root(s) at send time
(your instructions are never mutated). Without this, agents guess paths
(`/workdir`, `/workspace`, …) and wander.

```rust,no_run
# use genai_rs::antigravity::{AntigravityAgent, SystemInstructions};
let builder = AntigravityAgent::builder()
    .add_workspace("/path/to/repo")
    .with_system_instructions(SystemInstructions::custom("Audit this repo."))
    // workspace announcement is ON by default for custom instructions;
    // turn it off to ground the model yourself:
    .with_workspace_announcement(false)
    // ...
    ;
# let _ = builder;
```

The same note goes to each subagent with custom instructions.
The wire protocol has no native workspace-announcement field
(`FilesystemWorkspace` carries only the `directory`; `enforce_workspace_validation`
governs *enforcement*, not disclosure), so this is prompt-level grounding.

## Built-in tools (capabilities)

The harness executes its own tool suite; you choose which tools the agent
sees. The default is the **read-only** set (`list_directory`,
`search_directory`, `find_file`, `view_file`, `finish`):

```rust,no_run
use genai_rs::antigravity::{AntigravityAgent, BuiltinTool, Capabilities};

// Read-only plus shell access:
let caps = Capabilities::read_only().enable(BuiltinTool::RunCommand);

// Everything (requires a policy — see below):
let caps = Capabilities::all();

// Custom tools only:
let caps = Capabilities::none();

let builder = AntigravityAgent::builder().with_capabilities(caps);
# let _ = builder;
```

That set deliberately differs from the Python SDK's 0.1.18 `read_only()`
(`view_file`, `read_url_content`, `schedule`, `finish`). The Python SDK
now calls the three local finders deprecated, but the harness still serves
them and a read-only agent without them can only open files by guessing
paths; `read_url_content` is network egress, so it is write-capable here
(like `search_web`); and `schedule` is not offered at all:

- **`schedule`** lets the agent schedule its own future turns. This client
  drives one turn at a time and cannot observe a turn it did not start, so
  a self-scheduled turn's events would be read as the next `chat`'s
  response. It is always sent disabled.
- **`manage_task`** (list/kill background tasks) is enabled exactly when
  `run_command` is, as in the Python SDK.

Every toggle is written explicitly, disabled ones included. Harness 0.1.18
treats an omitted toggle as off, but 0.1.10 had no toggle for `manage_task`
or `schedule` and exposed both to every agent — including the default
read-only one — so what "omitted" means is not something to rely on.

### Safety gate

Enabling any write-capable builtin (`run_command`, `edit_file`,
`create_file`, `generate_image`, `search_web`, `read_url_content`,
`invoke_subagent`, `ask_question` — everything outside the read-only set)
or any MCP server **without a policy or pre-tool hook** is an error at
`spawn()` time — the same guard the Python SDK enforces. Add
`policy::allow_all()` for autonomous agents, or a deny-by-default rule set.

### Agent behavior

`with_agent_behavior(AgentBehavior::...)` (harness 0.1.18+) selects how the
harness frames the task in its system prompt: `Autonomous` (the harness
default when unset — verified byte-identical), `Interactive` (a human is
in the loop: clarifying questions, artifacts), or `Minimal` (a pruned
prompt for small-context models). `Subagent::with_agent_behavior` sets it
per subagent. The autonomous prompt tells the model *"the user will not
respond to questions"*, which is why [`ask_question`](#answering-agent-questions)
needs `Interactive`.

## Policies

Policies are declarative allow/deny/ask rules over tool names, with
optional conditions on the arguments. They are evaluated **in Rust**, in the
harness's pre-tool callback, which gates the agent's tool calls — builtin,
custom, MCP, `invoke_subagent` — before they run (D-022). A subagent's own
calls bypass it on 0.1.23 (see [Subagents](#subagents)):

```rust,no_run
use genai_rs::antigravity::{AntigravityAgent, policy};

let builder = AntigravityAgent::builder()
    // deny everything, then allow specific tools:
    .add_policy(policy::deny_all())
    .add_policy(policy::allow("view_file"))
    .add_policy(policy::allow("get_weather"))       // a custom tool
    .add_policy(policy::confirm("run_command"))     // defer to on_pre_tool
    .on_pre_tool(|call| {
        // The model's own argument names (see "What a hook is handed").
        // A missing key must fail closed: `unwrap_or("")` here would
        // wave every call through if the harness renamed the argument.
        match call.args["CommandLine"].as_str() {
            Some(cmd) if !cmd.contains("rm ") => genai_rs::antigravity::PreToolDecision::Allow,
            _ => genai_rs::antigravity::PreToolDecision::deny("no deletions"),
        }
    })
    // ...
    ;
# let _ = builder;
```

Rules:

- **Precedence is the harness evaluator's** (verified on 0.1.23): a rule
  naming the tool beats a server-wide MCP rule (`mcp_<server>_*`), which
  beats the wildcard (`"*"`); within a tier, **deny beats ask beats
  allow**, whatever the registration order; then the first registered wins.
  So `[deny_all(), allow("x")]` allows only `x`, and `[allow("x"),
  deny("x")]` denies it.
- No matching rule = allow (default open, like the Python SDK), still
  subject to the `on_pre_tool` hook.
- `confirm(name)` defers to `on_pre_tool`; with no hook configured the call
  is **denied** (fail closed). `ask_user(name, handler)` asks the handler
  instead, handing it the call and the rule's reason. A handler that panics
  denies the call.
- `.when(condition)` applies a rule only to calls the condition accepts;
  others fall through to the next rule. A condition that panics denies.
  `.with_reason(text)` sets the message the model sees on a denial, and
  `.with_name(label)` the label in the default one.
- Targets: builtin wire names (`run_command`, `edit_file`,
  `invoke_subagent`, ...), custom tool names, MCP tools as
  `mcp_<server>_<tool>`, a server's tools as `mcp_<server>_*`, and every MCP
  tool as `mcp_*`. The harness names an MCP tool by its bare name beside a
  separate server field in its hook callbacks, and the bridge maps it onto
  these targets. The server-wide forms match that server field exactly
  (`ToolInvocation::server`), so `mcp_git_*` does not cover a server named
  `git_admin`. A full `mcp_<server>_<tool>` name can still be read two ways
  when one server's name plus `_` starts another's (`git`, `git_admin`);
  `spawn()` warns about such pairs. `start_subagent`, the subagent
  builtin's pre-0.1.18 name, still targets `invoke_subagent`.

```rust,no_run
# use genai_rs::antigravity::{AntigravityAgent, policy};
# let approved: Vec<serde_json::Value> = Vec::new();
# let _builder = AntigravityAgent::builder()
// Deny only the destructive commands; allow the rest.
.add_policy(policy::allow("run_command"))
.add_policy(
    policy::deny("run_command")
        .when(|call| {
            // A missing argument must deny: a renamed key would otherwise
            // wave every call through.
            call.args["CommandLine"].as_str().is_none_or(|cmd| cmd.contains("rm "))
        })
        .with_reason("Deleting files is not allowed here."),
)
// Ask a person about edits, from state the handler already has.
.add_policy(policy::ask_user("edit_file", move |call, _reason| approved.contains(&call.args)))
# ;
```

### The auto policy

`policy::auto()` turns on the harness's own model-judged safety assessment
(the Python SDK's `policy.auto()`): before a risky call (`run_command`,
`read_url_content`, …) runs, the harness has the model vet it, and a call
it flags is **denied** unless the policy's handler approves it. The handler
gets the harness's assessment as its reason:

```rust,no_run
# use genai_rs::antigravity::{AntigravityAgent, policy};
# let _builder = AntigravityAgent::builder()
.add_policy(policy::allow_all())
.add_policy(policy::auto().with_handler(|call, assessment| {
    eprintln!("{} flagged: {}", call.name, assessment.unwrap_or_default());
    false // refuse
}))
# ;
```

It runs harness-side, after this client's rules and hooks allowed the call,
and costs at least one model call per assessed tool call. At most one per
agent. Its `model` setting in the Python SDK is not offered: harness 0.1.23
ignores it.

### What a hook is handed

Since harness 0.1.18 every tool call the agent itself makes — builtin,
custom, MCP, `invoke_subagent` — is gated by the harness's **pre-tool hook
callback**; no observed flow pauses for a tool confirmation any more. (A
subagent's own calls are not gated; see [Subagents](#subagents).) `ToolInvocation.args` on that path is
what the *model* passed, under the harness's own tool-schema names, which
are **not** the `ToolAction::args()` spellings (those are the step's action
record, `filePath`, `commandLine`, ...):

| Builtin | Argument names (0.1.18 and 0.1.23) |
|---------|-------------------------|
| `view_file` | `AbsolutePath`, `StartLine`, `EndLine`, `ContentOffset` |
| `list_directory` | `DirectoryPath` |
| `find_file` | `SearchDirectory`, `Pattern`, `Extensions`, `Excludes`, ... |
| `search_directory` | `SearchPath`, `Query`, `Includes`, `IsRegex`, ... |
| `run_command` | `CommandLine`, `Cwd`, `WaitMsBeforeAsync`, ... |
| `edit_file` | `TargetFile`, `TargetContent`, `ReplacementContent`, ... |
| `create_file` | `TargetFile`, `CodeContent`, `Overwrite`, `Append` (0.1.23), ... |
| `invoke_subagent` | `Subagents` (each with `TypeName`, `Prompt`, `Role`) |

Paths are plain absolute paths here, not `file://` URIs. For MCP tools the
bridge unwraps the harness's `{"Arguments": .., "ServerName": ..,
"ToolName": ..}` envelope, so the hook sees the tool's own arguments. The
names are harness-internal and version-specific (read them off a
`LOUD_WIRE=callHookRequest` trace), so **write gates that fail closed** on a
missing key: a check that indexes a key which is not there sees `Null` and,
written the obvious way, allows everything. Both path-gating examples in
this repository shipped that bug against 0.1.18 before it was caught.

The bridge fails closed on the case it can see: when the harness sends
arguments that are not valid JSON, a call that a pre-tool hook would judge
is **denied before the hook runs**, with a reason saying why (the hook
would otherwise have been handed `{}` and passed a path gate it should have
failed). Without a pre-tool hook, name-based policies decide as usual,
since they never read arguments. A custom tool call whose arguments decode
in neither of the harness's encodings is answered with an error instead of
running the tool on `Null`.

A pre-tool hook can also run the call on other arguments:
`PreToolDecision::modify(json!({"AbsolutePath": "/repo/README.md"}))`. The
harness runs builtins, MCP and custom tools on the replacement (verified on
0.1.23), so write the tool's own argument names.

Hooks are synchronous closures, and each call runs on a thread of its own,
so the turn deadline bounds them on any runtime: a hook still running when
the turn's budget runs out ends the turn with `AntigravityError::Timeout`,
whose `operation` names it. The hook's thread is not stopped; its result is
discarded when it returns. The thread is detached, so a hook still blocked
after a timeout holds up neither runtime shutdown (the end of
`#[tokio::main]`) nor process exit. It does keep running, and keeps what it
captured alive, until it returns, so keep hooks bounded. The thread enters
the turn's runtime, so `tokio::runtime::Handle::current()` works inside a
hook, and a hook with async work can block on it there:
`Handle::current().block_on(check(&call))`. Custom tools are bounded too,
with one limit; see the turn deadline under [Streaming](#streaming).

Arguments can also change meaning. 0.1.23 added `Append` to `create_file`:
`{Overwrite: false, Append: true}` now appends to an existing file, where
0.1.18 refused. A gate that allows `create_file` only when `Overwrite` is
false ("new files only") lets the model modify existing files on 0.1.23;
check `Append` too.

### Unrecognized tool confirmations

Harness-side builtins can pause in a *waiting* step until the client
confirms them (none observed doing so on 0.1.18 — see above); the pending
action's identity comes solely from which action field the step carries
(the confirmation request itself is an empty marker on the wire — verified
against the pinned harness proto). Two edge cases:

- **Pre-request notifications** (a step with *no* action payload at all,
  or only a `customTool` record) announce an upcoming host-side custom tool
  call. They are auto-approved
  regardless of policy, mirroring the reference SDK: the concrete call
  arrives separately and gets its own policy check, so nothing is bypassed.
- **Unknown actions** (a step whose action landed in the Evergreen `extra`
  map — e.g. a builtin newer than this client) **fail closed**: the
  confirmation is that tool's *only* gate, so it is approved only when a
  policy rule matches (wildcard `allow_all()`, or an exact rule naming the
  unknown wire field, e.g. `allow("deleteEverything")`) or the `on_pre_tool`
  hook allows it. A `warn!` records the unknown field names and the decision
  either way. This is stricter than the reference SDK, which auto-approves
  anything it cannot map.

`on_post_tool` observes completed custom tool calls (and harness-side
post-tool hook callbacks) for audit logging. `ToolOutcome.result` is the
**inner** tool result, not the `{"result": ...}` wire envelope the harness
receives: a scalar return `X` arrives as its string form (or `X` serialized
when non-string), and an object return is passed through serialized. Failures
populate `ToolOutcome.error` instead.

### Answering agent questions

The `ask_question` builtin lets the agent pause a turn to ask the user
questions (multiple-choice, optionally multi-select). Three things must line
up for a question to reach you:

1. **The builtin is enabled.** It is off in the default read-only set.
2. **The agent behavior is `Interactive`.** The harness default
   (autonomous) prompt tells the model nobody will answer, so it asks in
   prose instead of calling the tool; `spawn()` warns about this
   combination.
3. **Policy allows `ask_question`.** On harness 0.1.18 the call goes
   through the pre-tool hook like any other tool *before* the question is
   posed, so `deny_all()` alone blocks every question. (On 0.1.10 question
   requests bypassed the policy engine; they no longer do.) `ask_question`
   is also write-capable for the [safety gate](#safety-gate).

Set an `on_questions` hook to answer the questions programmatically — route
them to a CLI prompt, a chat message, or policy code:

```rust,no_run
use genai_rs::antigravity::{
    AgentBehavior, AntigravityAgent, BuiltinTool, Capabilities, QuestionAnswer, QuestionReply,
    policy,
};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let agent = AntigravityAgent::builder()
    // ...
    // read_only() does not include AskQuestion — enable it explicitly.
    .with_capabilities(Capabilities::read_only().enable(BuiltinTool::AskQuestion))
    .with_agent_behavior(AgentBehavior::Interactive)
    // Deny by default, and allow the question tool explicitly: the policy
    // is consulted for it like any other call.
    .add_policy(policy::deny_all())
    .add_policy(policy::allow("ask_question"))
    .on_questions(|questions| {
        // Answer each question: pick the first choice, but never guess on
        // an unmodeled question type or one with no rendered choices.
        QuestionReply::Answers(
            questions
                .iter()
                .map(|q| {
                    if q.is_unknown_type() || q.choices.is_empty() {
                        QuestionAnswer::Unanswered
                    } else {
                        QuestionAnswer::Choices { selected: vec![0], freeform: None }
                    }
                })
                .collect(),
        )
    })
    .spawn()
    .await?;
# let _ = agent;
# Ok(())
# }
```

`QuestionReply::Cancel` sends the cancelled flag with no answers — "stop
asking"; what the harness then does with the in-flight turn is
harness-owned and not live-verified. A short answer list is padded with
`Unanswered`. (`Freeform(text)` is the ergonomic spelling of
a `Choices` answer with no selection and that freeform text — the two
produce identical wire.) Without a hook every question is answered
"unanswered" (with a `warn!`) so the harness never deadlocks — and since
the builtin is off by default, simply not enabling it means the agent
never asks.

A question whose *type* this crate doesn't model arrives with
`is_unknown_type()` true, empty text/choices, and the raw payload in
`extra` — prefer `Cancel` or `Unanswered` there over guessing (the
`is_unknown_type()` check in the snippet above is what routes that case
to `Unanswered` rather than selecting index 0 of an empty list).
`AgentQuestion::unknown(extra)` builds that fixture for unit tests.

The hook is synchronous, and the turn waits for it — don't block in it
waiting for a human. It runs on a thread of its own, so a slow answer no
longer stalls the runtime, but it still spends the turn's budget, and one
still blocked at the deadline ends the turn with `Timeout`. For interactive
flows, collect the answer out-of-band (chat reply, CLI prompt in another
task) into a channel and answer from `try_recv`, returning `Unanswered`
when nothing has arrived; the deadlock-avoidance fallback only covers the
hookless case.

## Lifecycle hooks

Beyond the tool and question hooks, every point the harness calls back at
has a setter. Each is a synchronous closure on a thread of its own, bounded
by the turn deadline like the rest:

| Setter | Fires | Its answer |
|--------|-------|------------|
| `on_session_start` | once, before `spawn()` returns | — |
| `on_session_end` | during `shutdown()` (not on drop) | — |
| `on_pre_turn` | before each turn, with the user's text | `TurnDecision::Deny` fails the turn with `AntigravityError::Turn`; the agent stays usable |
| `on_post_turn` | after each turn, with its response text | — |
| `on_tool_error` | when a tool call fails | `Some(text)` replaces the error the model sees |
| `on_compaction` | when the harness compacts the history | — |
| `on_stop` | when a turn is about to end | `StopDecision::Continue { reason }` adds `reason` as an instruction and calls the model again, in the same turn |

```rust,no_run
# use genai_rs::antigravity::{AntigravityAgent, StopDecision};
# let _builder = AntigravityAgent::builder()
.on_tool_error(|failure| {
    (failure.name == "run_command")
        .then(|| "Commands fail in this sandbox; read the files instead.".to_string())
})
.on_stop(|stop| {
    if stop.continuation_count < 1 && !stop.response_text.contains("DONE") {
        StopDecision::continue_with("Finish the task, then say DONE.")
    } else {
        StopDecision::Stop
    }
})
# ;
```

`on_stop` must stop eventually: check `continuation_count`, or the turn runs
until its deadline. The order within a turn, as 0.1.23 calls them: pre-turn,
then the tool hooks and tool errors as calls happen, then stop (again after
each continue), then post-turn.

A turn nobody consumes (one that hit its `with_turn_timeout`, or a trigger
turn the next `chat` halts) is drained, and its per-turn hooks (pre-turn,
post-turn, tool error, compaction, stop) don't run for it: the drain answers
the harness as if they were not set, so a stop hook can't keep a halted
turn going. Session hooks still run.

## Custom tools — the same `#[tool]` functions as the Interactions API

Tool declarations are the crate's ordinary `FunctionDeclaration`; dispatch
reuses the global `#[tool]` registry and `ToolService`:

```rust,no_run
use genai_rs::CallableFunction;
use genai_rs::antigravity::AntigravityAgent;
use genai_rs_macros::tool;

/// Returns the current weather for a city.
#[tool(city(description = "The city to get weather for"))]
fn get_weather(city: String) -> String {
    format!("Sunny and 22C in {city}")
}

# let my_service: std::sync::Arc<dyn genai_rs::ToolService> = unimplemented!();
let builder = AntigravityAgent::builder()
    .add_tool(GetWeatherCallable.declaration())     // #[tool] machinery
    .with_tool_service(my_service)                  // stateful ToolService
    // ...
    ;
# let _ = builder;
```

When the model calls a custom tool, the crate checks policies, executes your
function, and replies to the harness automatically. Failures become
`{"error": ...}` results the model can react to — the turn is never
deadlocked by a failing tool.

## MCP servers

```rust,no_run
use genai_rs::antigravity::{AntigravityAgent, McpServer, policy};

let builder = AntigravityAgent::builder()
    .add_mcp_server(McpServer::stdio("uvx", ["mcp-server-git"]).with_name("git"))
    .add_mcp_server(McpServer::http("http://localhost:8931/mcp").with_name("tickets"))
    .add_policy(policy::deny_all())
    .add_policy(policy::allow("mcp_git_status"))    // per-tool policy target
    // ...
    ;
# let _ = builder;
```

The harness owns the MCP connections and tool execution; your policies see
the calls as `mcp_<server>_<tool>`.

By default the harness lists a server's tools in its prompt and the model
reaches them through `call_mcp_tool`, which keeps a large server's schemas
out of the context. `McpServer::with_force_all_tools_eager(true)` declares
them by name instead. `with_mcp_max_output_bytes(n)` sets the size past
which the harness writes an MCP tool's output to a file and gives the model
the path (harness default 4096 bytes).

## Subagents

Static subagents run in their own trajectory with their own instructions and
tool set; the parent model delegates to them through the `invoke_subagent`
builtin. That builtin is **off** in the default read-only capability set and
is write-capable, so enabling it requires a policy or pre-tool hook (the
spawn-time safety gate):

```rust,no_run
use genai_rs::CallableFunction;
use genai_rs::antigravity::{AntigravityAgent, BuiltinTool, Capabilities, Subagent, policy};
# use genai_rs_macros::tool;
# /// Classifies a finding's severity.
# #[tool(category(description = "The finding's category"))]
# fn severity_classifier(category: String) -> String { category }

let builder = AntigravityAgent::builder()
    .add_tool(SeverityClassifierCallable.declaration())   // parent registration
    .add_subagent(
        Subagent::new("auditor")
            .with_description("Audits one file for security issues.")
            .with_system_instructions("Focus on injection vectors.")
            .with_capabilities(Capabilities::read_only()) // the default
            .add_tool("severity_classifier"),             // reference by name
    )
    .with_capabilities(Capabilities::read_only().enable(BuiltinTool::InvokeSubagent))
    .add_policy(policy::allow_all())
    // ...
    ;
# let _ = builder;
```

Rules (matching the reference SDK):

- **A subagent's own tool calls are not gated.** On harness 0.1.23 they
  reach neither the pre-tool callback (so no policy or `on_pre_tool` hook
  sees them) nor the harness's own policy evaluator, and workspace
  containment does not confine them: a read-only subagent can read any
  file the process can (pinned by a scripted test; the probe is recorded in
  `docs/ANTIGRAVITY_BRIDGE_DESIGN.md`). The agent's `run_command` sandbox
  is its own; a subagent's commands follow the subagent's capabilities.
  Only the subagent's capabilities limit it, so give a subagent only the
  builtins it may use unchecked. Enabling `invoke_subagent` also offers the
  harness's built-in `research` subagent, even with no subagent added.
- **`spawn()` refuses subagents beside a gate they bypass** (#520), unless
  you opt in with `with_ungated_subagents(true)`. The gates are a pre-tool
  hook, a deny, confirm or `ask_user` policy, `policy::auto()`, workspace
  containment (on by default with a workspace) and the `run_command`
  sandbox; the refusal is an `AntigravityError::Config` naming the ones
  set. It applies when `invoke_subagent` or `with_define_subagent_tool(true)`
  is on. Policies that only allow, like `allow_all()` above, refuse nothing
  and are not a gate:

  ```rust,no_run
  use genai_rs::antigravity::{AntigravityAgent, BuiltinTool, Capabilities, Subagent, policy};

  let builder = AntigravityAgent::builder()
      .add_workspace("/repo")                             // containment: a gate
      .add_policy(policy::deny_all())                     // a gate
      .add_policy(policy::allow("view_file"))
      .add_policy(policy::allow("invoke_subagent"))
      .with_capabilities(Capabilities::read_only().enable(BuiltinTool::InvokeSubagent))
      .add_subagent(Subagent::new("reader").with_description("Reads files."))
      .with_ungated_subagents(true) // the subagents may read outside /repo
      // ...
      ;
  # let _ = builder;
  ```
- **Custom tools are referenced by name** and must also be registered on the
  parent agent (`add_tool` / `with_tool_service`) — subagent custom-tool
  calls dispatch through the parent's registry. `spawn()` validates the
  references (and name uniqueness) and fails with `AntigravityError::Config`
  on a dangling one.
- Subagent `with_system_instructions` follow the parent's rules: a string
  is added to the harness's subagent prompt, `SystemInstructions::custom`
  replaces it.
- Subagent capabilities default to the read-only builtin set.
  `invoke_subagent` inside a subagent takes effect only with
  `with_max_subagent_depth(2)` or more on the agent (the harness default is
  one level); otherwise it is turned off with a `warn!`.
- `Subagent::with_model(name)` runs a subagent on another model, through
  the parent's endpoint. `with_allowed_subagents([..])` on the agent or a
  subagent limits which subagents it may invoke (names are checked at
  `spawn()`), and `Subagent::with_skills(SubagentSkills::none())` (or
  `inherit()`, `from_paths(..)`) sets which skills it sees.
- Subagent activity surfaces in streams as `AgentEvent::ToolAction` with
  `ToolAction::InvokeSubagent`, plus the subagent trajectory's own deltas.
  The `ToolAction` event carries the subagent's `trajectory_id` so its
  actions can be told apart from the parent's (0.1.18 also marks subagent
  steps and trajectories with a `parentTrajectoryId`, which the bridge uses
  to keep a subagent from ever being mistaken for the turn's own
  trajectory). `ToolAction::subagent_name()` exposes the invoked subagent's
  name **if the harness reports it** — harnesses 0.1.5 through 0.1.18 emit
  an empty `invokeSubagent` action (verified via `LOUD_WIRE`), so it is
  `None`; the name is visible only in the pre-tool hook's arguments
  (`Subagents[].TypeName`).
- Enabling the subagent builtin exposes `invoke_subagent`,
  `manage_subagents` and `send_message`. The harness would also offer
  `define_subagent`, with which the model writes subagents of its own at
  runtime; it is **off** unless you call `with_define_subagent_tool(true)`
  (D-023).

## Streaming

```rust,no_run
use futures_util::StreamExt;
use genai_rs::antigravity::{AgentEvent, ErrorSeverity};

# async fn run(mut agent: genai_rs::antigravity::AntigravityAgent) -> Result<(), Box<dyn std::error::Error>> {
let mut stream = agent.send_streaming("Refactor src/lib.rs").await?;
while let Some(event) = stream.next().await {
    match event? {
        AgentEvent::TextDelta(t) => print!("{t}"),
        AgentEvent::ThinkingDelta(_) => {}
        AgentEvent::ToolAction { action, decision, trajectory_id } => {
            // `decision` distinguishes executed from policy/hook-denied
            // actions; `trajectory_id` tells parent and subagent actions apart.
            eprintln!("[harness tool] {action:?} ({decision:?}) traj={trajectory_id:?}");
        }
        AgentEvent::ToolCallDispatched { name, .. } => eprintln!("[custom tool] {name}"),
        AgentEvent::Finished(response) => { println!(); break; }
        // Transient errors are harness-internal noise (the turn continues);
        // only `Severe` ones matter. Turn-ending failures arrive as
        // `AntigravityError::Turn` from the call, not as this event.
        AgentEvent::Error { message, severity } => match severity {
            ErrorSeverity::Severe => eprintln!("[error] {message}"),
            _ => {} // ignore transient noise
        },
        _ => {} // non-exhaustive
    }
}
# Ok(())
# }
```

### Event decisions, trajectory identity, and error severity

- **`ToolAction { action, decision, trajectory_id }`** — `decision` is a
  `ToolDecision` (`Allowed`, or `Denied { reason }`): a policy- or
  hook-blocked harness action is otherwise indistinguishable from an executed
  one. A call blocked at the pre-tool hook never ran, so there is no action
  record for it: the harness reports it as an error step with none, which
  arrives as `ToolAction::Denied { name, args }` (the call as the hook saw
  it) marked `Denied` with the reason this client sent (the bridge matches
  the step against its own verdict). `trajectory_id`
  identifies the (sub)trajectory the action ran in, so parent and subagent
  actions can be told apart in the interleaved stream.
- **`TextDelta` / `ThinkingDelta`** carry the model's own output only.
  Every step streams a `textDelta` — including the echo of your input and
  each tool step's one-line label (`"Weather check"`) — so text deltas are
  taken only from model steps directed at the user (the reference SDK's
  filter), and thinking deltas from any model step. A step in an error
  state streams nothing: 0.1.23 puts a denial's message in such a step's
  delta.
- **`Error { message, severity }`** — `severity` is an `ErrorSeverity`.
  Transient errors are harness-internal noise (retried internally; the turn
  continues) — essentially every error event today. `Severe` marks a
  serious mid-turn error (e.g. a fatal model-backend status that did not abort
  the turn); genuinely turn-*ending* failures surface as
  `AntigravityError::Turn` from `chat`/`send_streaming`, never as this event.

The stream borrows the agent mutably for the turn. To cancel from another
task, take a handle first:

```rust,no_run
# async fn run(agent: genai_rs::antigravity::AntigravityAgent) -> Result<(), Box<dyn std::error::Error>> {
let cancel = agent.cancel_handle();
// ... later, from any task:
cancel.cancel().await?;   // the in-flight turn ends early, keeping partial output
# Ok(())
# }
```

**What a cancelled turn returns**: every harness from 0.1.10 through the
pinned 0.1.23 answers a halt by taking the trajectory to `STATE_FULLY_IDLE`
— the same terminal state as a natural completion, not `STATE_CANCELLED`. The turn therefore resolves *normally*
with whatever partial output it had produced, rather than failing with
`AntigravityError::Turn`. Treat `cancel()` as "stop early and keep what you
have", and record the cancellation on your side if you need to distinguish
a halted turn from a completed one. (`AntigravityError::Turn` is still the
outcome when the harness cancels a turn of its own accord.) On the pinned
harness the key-free `test_scripted_cancel_halts_a_turn_waiting_on_the_model`,
which the required CI check runs, fails if a halted turn does anything but
resolve normally.

`with_turn_timeout(Duration)` bounds each turn's wall-clock time. When the
budget is exceeded, the crate halts the harness's still-running turn and
drains its remaining events before returning `AntigravityError::Timeout`, so
the next turn starts from a clean stream.

The deadline is absolute, stamped when the turn starts, and it covers the
whole turn rather than the gaps between harness events: that includes this
crate's own work inside the turn, so a custom tool or hook that hangs ends
the turn on time, with the `Timeout`'s `operation` naming it
(`agent turn (custom tool 'x' was still running at the deadline)`).

Each custom tool call runs on a tokio task of its own, and a call the
deadline abandons is aborted: an async tool stops at its next `.await`. A
tool body that **blocks its thread** (a synchronous `#[tool]` fn doing
blocking I/O or `std::thread::sleep`) cannot be stopped; it runs to
completion on its worker and its result is discarded. On a multi-thread
runtime with two or more workers the deadline still fires on time, because
a free worker drives the timer. Only workers drive it, though, so a body
blocking the only worker stops the deadline until it returns. That happens
on a current-thread runtime (`#[tokio::main(flavor = "current_thread")]`,
and `#[tokio::test]`'s default), and on a multi-thread runtime with one
worker: `#[tokio::main]` starts one worker per CPU, so a one-CPU host or a
container limited to one CPU gets one. There the turn cannot end before the
tool returns. Make such a tool `async` with non-blocking I/O, or move the
blocking part into `tokio::task::spawn_blocking` inside it. Hooks need
neither: each call always runs on a thread of its own.

An abandoned blocking body keeps its worker until it returns: the runtime
has one worker fewer meanwhile, and dropping the runtime (the end of
`#[tokio::main]`) waits for the body to finish. Bound such a tool yourself.

For
`send_streaming` that includes time **your** code spends between polls of
the stream — so a consumer that renders events interactively, or awaits a
confirmation mid-turn, is spending the same budget the harness is. Raise it,
or call `without_turn_timeout()`, for consumers that pause mid-turn; the
`Timeout` error they would otherwise get carries a stall diagnosis pointing
at the harness, which is the wrong place to look.

**Turns are bounded by default** — `DEFAULT_TURN_TIMEOUT`, 300s — so you get
an error rather than a hang without opting in. An unbounded turn does not
*fail* when the harness stops signalling completion; it hangs, which is
strictly less diagnosable than an error and looks identical to latency from
the outside. That is not hypothetical, and it has happened twice: harness
0.1.10 renamed the terminal trajectory state, and 0.1.18 started rejecting
the user-message shape this crate sent. Both times every turn ran on with
no error, no failed parse, and nothing in this crate's logs.

Raise it for agents that legitimately run long (deep subagent trees, many
tool calls), lower it for interactive use where a stall should surface fast,
or remove it deliberately:

```rust,no_run
# use genai_rs::antigravity::AntigravityAgent;
# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let agent = AntigravityAgent::builder()
    .without_turn_timeout()  // runs until the harness ends the turn
    .spawn().await?;
# let _ = agent;
# Ok(())
# }
```

## Structured output

```rust,no_run
# use genai_rs::antigravity::AntigravityAgent;
# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let mut agent = AntigravityAgent::builder()
    .with_response_schema(serde_json::json!({
        "type": "object",
        "properties": {"severity": {"type": "string"}},
        "required": ["severity"]
    }))
    // ...
    .spawn().await?;

let response = agent.chat("Audit this repo.").await?;
if let Some(value) = response.structured_output() {
    println!("severity = {}", value["severity"]);
}
# Ok(())
# }
```

## Early stops

A turn the harness stops early — the model backend's quota exhausted, or a
harness-side budget exceeded — still *finishes*: it ends in the same
terminal state as a completed turn, so the only symptom would be a short or
empty `text()`. Harness 0.1.18 attaches a reason, surfaced as
`ChatResponse::stop_reason()` (`Some(StopReason::MaxToolCallsExceeded)`,
`Some(StopReason::QuotaExhausted)`, ...; `None` for a normal completion).
Budgets are set with `with_budget` (see below).

## Model and harness options

Every harness option the Python SDK has, and what this crate does with
each, is in the parity table of `docs/ANTIGRAVITY_BRIDGE_DESIGN.md`. The
model options need `with_api_key`, since they travel in the model config:

```rust,no_run
use genai_rs::antigravity::{
    AntigravityAgent, Budget, BuiltinTool, Capabilities, InlineSkill, RetryConfig,
    RunCommandOptions,
};
use genai_rs::{ServiceTier, ThinkingLevel};
use std::time::Duration;

# let key = String::new();
let builder = AntigravityAgent::builder()
    .with_api_key(key)
    .with_thinking_level(ThinkingLevel::High)        // generationConfig.thinkingConfig
    .with_service_tier(ServiceTier::Flex)            // the request's serviceTier
    .with_max_output_tokens(8192)                     // per model call; harness default 65535
    .add_model_header("x-team", "search")             // every model request
    .with_image_model("…")                            // for generate_image
    .with_budget(Budget::new().with_max_tool_calls(50).with_max_total_tokens(500_000))
    .with_retry(RetryConfig::new().with_max_api_retries(5))
    .with_compaction_threshold(200_000)               // tokens of history
    .with_tool_output_truncation(4_000)               // tokens per tool output
    .add_inline_skill(InlineSkill::new("changelog", "How to write an entry.", "…"))
    .with_capabilities(
        Capabilities::read_only()
            .enable(BuiltinTool::RunCommand)
            .with_run_command(RunCommandOptions::new().with_timeout(Duration::from_secs(60))),
    )
    // ...
    ;
# let _ = builder;
```

- **Image model**: with `BuiltinTool::GenerateImage` enabled the bridge
  configures `DEFAULT_IMAGE_MODEL` unless `with_image_model` names another.
  Without an image model the harness fails every `generate_image` call.
- **Budgets** cap model calls, tool calls and tokens for the session
  (`BudgetScope::Lifetime`, the default) or from the session's start
  (`ForwardLooking`). A turn that reaches one ends early with its
  `stop_reason`.
- **Retries**: `RetryConfig::new().with_max_api_retries(0)` fails a turn on
  the first 503; `RetryConfig::benchmark()` is the Python SDK's preset for
  long evaluation runs.
- **Compaction** costs a model call that writes the summary
  (`on_compaction` sees it). The threshold must leave room for the
  harness's own prompt and tools (about 2,700 tokens on 0.1.23 with the
  read-only tools), or the turn fails.
- **Inline skills** and `add_skills_path` cannot be combined: the harness
  takes skills from one source or the other.
- **`run_command`**: `RunCommandOptions::with_timeout` kills a command
  that runs longer; `with_daemons(true)` offers the model `IsDaemon` for
  dev servers and watchers; `with_sandbox(true)` runs commands in the OS
  sandbox (`AntigravityAgent::sandbox_status()` says whether there is one,
  and `spawn()` warns when there is not). Where there is none, as on
  GitHub's Ubuntu 24.04 runners, a sandboxed command fails rather than
  running unsandboxed (harness 0.1.23).
- **`with_session_continuation`**: `SessionContinuation::Resume` makes
  `spawn()` fail when the conversation id is not saved; `CreateOnly` when
  it is.
- **`add_harness_env(key, value)`** sets an environment variable for the
  harness, which `run_command` commands and stdio MCP servers inherit.

## Triggers

Triggers inject a message into the conversation without a user turn, via
the protocol's `automated_trigger` event (mirroring the reference SDK's
`TriggerRunner`). Three kinds: an interval, a file watch, and any async
function of your own:

```rust,no_run
use genai_rs::antigravity::{AntigravityAgent, TriggerConfig, triggers};
use std::time::Duration;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
# let (_jobs, mut queue) = tokio::sync::mpsc::channel::<u32>(8);
let mut agent = AntigravityAgent::builder()
    .add_trigger(TriggerConfig::new(
        "Check the queue for new items and summarize them.",
        Duration::from_secs(300),
    ))
    .add_custom_trigger(triggers::on_file_change(
        "/repo/docs",
        Duration::from_secs(2),
        |changes| Some(format!("{} doc file(s) changed; check the links.", changes.len())),
    ))
    .add_custom_trigger(|ctx| async move {
        while let Some(job) = queue.recv().await {
            ctx.send(format!("Process job {job}.")).await?;
        }
        Ok(())
    })
    .spawn().await?;

// Drive the turns the triggers start, and read their output.
while let Some(response) = agent.next_trigger_turn(Duration::from_secs(60)).await? {
    println!("{}", response.text());
}
# Ok(())
# }
```

The file watch polls every `poll_interval`, comparing each file's size and
modification time (no OS watcher, so no extra dependency), and skips the
directories the Python SDK's `watchfiles` filter skips (`.git`,
`node_modules`, virtualenvs, caches). Changes made while a turn runs are
held until the agent is idle and handed to the callback as one batch, the
net change since the last batch.

Delivery semantics (see `antigravity::triggers` for details):

- The first firing happens after the first interval elapses, not
  immediately. Intervals must be non-zero (`spawn()` validates).
- A trigger can fire before the first turn on harness 0.1.23, which runs it
  as a normal turn. Harnesses 0.1.10 and 0.1.18 aborted the agent run
  instead (`earliest step index is out of bounds: 0 vs 0`) and closed the
  WebSocket, so the session died; on those, give the conversation one real
  turn first, as `examples/antigravity/proactive_agent` still does.
- A firing is delivered **only while the agent is idle** (no
  `chat`/`send_streaming` turn in flight). If it comes due mid-turn, it is
  deferred until the turn ends, and missed intervals collapse into a single
  delivery (no backlog after a long turn).
- Trigger tasks stop cleanly on `shutdown()` and on drop — no zombie
  timers. A failed delivery (session closed) ends that trigger's task; other
  triggers and the session are unaffected.
- A trigger delivered while idle starts a harness-side turn.
  `next_trigger_turn(wait)` drives it and returns its `ChatResponse` (or
  `None` when no turn starts within `wait`). Not consumed, it runs
  unobserved: the next `chat`/`send_streaming` call halts it if it is still
  running and discards its events before sending your input, so a trigger
  turn can never surface as (or desync) your turn's response. The trigger's
  effects on conversation history (and any tool calls completed before the
  halt) persist.

## Session persistence and resume

```rust,no_run
# use genai_rs::antigravity::AntigravityAgent;
# async fn run() -> Result<(), Box<dyn std::error::Error>> {
// First run:
let agent = AntigravityAgent::builder()
    .with_save_dir("/var/lib/myapp/agent-sessions")
    .spawn().await?;
let id = agent.conversation_id().unwrap().to_string();
agent.shutdown().await?;   // shutdown() persists the trajectory

// Later:
let agent = AntigravityAgent::builder()
    .with_save_dir("/var/lib/myapp/agent-sessions")
    .with_conversation_id(id)
    .spawn().await?;
println!("restored {} steps", agent.initial_history().len());
# Ok(())
# }
```

`with_save_dir` is where the harness keeps conversations;
`with_app_data_dir(dir)` sets the harness's own application data
directory. Unset, the field is left out of the handshake and the harness
uses its default.

## Debugging

### Diagnosing protocol drift

Four signals exist specifically for the failure mode described under
[Version pinning](#version-pinning) — a harness that speaks a dialect this
build only partly understands, which otherwise presents as "the agent just
didn't do anything":

| Signal | What it tells you |
|--------|-------------------|
| `protocol::drift_report()` | Every unrecognized wire value seen, as `"EnumName=WIRE_VALUE" -> count`. Empty is healthy. Process-wide and cumulative; `clear_drift_report()` resets it. |
| The `warn!` on `shutdown()` | The same aggregate, logged once at the natural end of a session, so it does not scroll past like the per-value warns do. |
| `AntigravityError::Timeout` | When a turn times out having seen unrecognized *main-trajectory* states, or after the harness logged that it could not unmarshal something this session sent, the `operation` field names the cause and points at `SUPPORTED_HARNESS_VERSION` instead of reporting an undifferentiated stall. It also names a known version mismatch, and a custom tool or hook of yours that was still running. |
| `AntigravityAgent::harness_version()` and the spawn-time `warn!` | The installed wheel's version, when it differs from `SUPPORTED_HARNESS_VERSION`. |

Programmatic check, for anything long-running:

```rust,no_run
let drift = genai_rs::antigravity::protocol::drift_report();
if !drift.is_empty() {
    eprintln!("harness sent values this build does not model: {drift:?}");
}
```

CI runs a stronger version of this: `test_antigravity_protocol_enums_have_not_drifted`
diffs the installed wheel's protobuf descriptor against the crate's wire
enums and fails naming any value the harness can send that the crate does
not model. It also checks that every field the crate reads or writes by
hand still exists, and still has the *type* the crate encodes — the check
that catches a field keeping its name while changing shape, as
`InputEvent.userInput` did in 0.1.18. That is what turns a renamed enum or
a reshaped message from a silent hang into a red test.

A turn that times out also names the harness-side cause when there is
one: unrecognized main-trajectory states, or a line on the harness's
stderr saying it failed to unmarshal a message this session sent.

### Another model host, or a scripted one

`with_gemini_base_url(url)` points the harness's model calls at another
Gemini API host — a gateway or recording proxy — instead of
`generativelanguage.googleapis.com`. The harness calls
`{url}/v1beta/models/<model>:streamGenerateContent?alt=sse` with the key
in `x-goog-api-key`. It needs a non-empty `with_api_key`: without a
configured key, or with an empty one, the harness falls back to its own
environment's `GEMINI_API_KEY` and sends that to the custom host (harness
0.1.23), so `spawn()` refuses the combination. As a second
guard, while a base URL is set the harness starts with `GEMINI_API_KEY` and
`GOOGLE_API_KEY` removed from its environment, so there is no key to fall
back on; commands the agent runs (`run_command`) inherit that environment
and don't see them either.

Pointed at a local server that answers with scripted `streamGenerateContent`
events, the real harness runs real builtins, hooks, policies, containment,
triggers and cancellation against a "model" that does exactly what a test
says, with no API key. That is how `tests/antigravity_scripted.rs` works
(see `docs/TESTING.md`):

```rust,no_run
# use genai_rs::antigravity::AntigravityAgent;
# struct FakeGemini;
# impl FakeGemini { fn base_url(&self) -> String { String::new() } }
# async fn run() -> Result<(), Box<dyn std::error::Error>> {
# let (fake, dir) = (FakeGemini, std::env::temp_dir().display().to_string());
let agent = AntigravityAgent::builder()
    .with_api_key("placeholder")             // never a real key here
    .with_gemini_base_url(fake.base_url())   // tests/common/fake_gemini.rs
    .add_workspace(dir)
    .spawn()
    .await?;
# let _ = agent;
# Ok(())
# }
```

### Wire inspection

The Antigravity client feeds the crate's canonical wire-inspection layer
(`genai_rs::wire`). `LOUD_WIRE=1` pretty-prints everything to stderr:

- `HARNESS <path> (pid N)` — process spawn,
- `WS Send` / `WS Receive` — every proto-JSON message,
- `STDERR:` — every harness diagnostic line.

```bash
LOUD_WIRE=1 cargo run --example antigravity_agent --features antigravity
```

`LOUD_WIRE=1` is the right default for a single HTTP request and the wrong
one for a harness session: a few turns produce thousands of pretty-printed
lines, and finding the message that matters means grepping raw JSON out of
the scrollback. So `LOUD_WIRE` also takes a comma-separated filter.

| Value | Keeps |
|-------|-------|
| `1`, `true`, `yes`, `on`, `all`, or empty | Everything (unchanged) |
| `harness` | Spawn and stderr lines |
| `ws` | Every WebSocket message |
| `request`, `response`, `sse`, `upload` | The HTTP-side categories |
| *any other token* | WebSocket messages whose top-level key matches |
| `summary` | Modifier: one line per event instead of full bodies |

The last two are what make a session readable. Selectors name the proto
oneof arm — `stepUpdate`, `toolCall`, `userInput`, `trajectoryStateUpdate`
— matched case-insensitively, and envelope bookkeeping (`seqNum`,
`timestampMicros`) is ignored so it cannot match everything.

```bash
LOUD_WIRE=summary                  # the whole session, one line per message
LOUD_WIRE=toolCall,summary         # just the tool traffic, one line each
LOUD_WIRE=trajectoryStateUpdate    # why a turn will not finish
LOUD_WIRE=callHookRequest          # what every tool gate is handed
LOUD_WIRE=harness                  # spawn + stderr only, no protocol noise
```

`LOUD_WIRE=summary` renders each message as its payload keys, which is
usually enough to see the shape and order of a turn; drop to a selector
once you know which message you want in full. The same filtering is
available programmatically via `wire::WireFilter` and
`LoudWirePrinter::with_filter`.

For programmatic capture, register inspectors on the builder — the
`WireEvent` variants are `HarnessSpawn`, `WsSend`, `WsReceive`, and
`HarnessStderr`, sharing one correlation id per harness session:

```rust,no_run
use genai_rs::antigravity::AntigravityAgent;
use genai_rs::wire::TracingForwarder;
use std::sync::Arc;

// Visible with RUST_LOG=genai_rs::wire=debug once your program installs a
// tracing subscriber (docs/LOGGING_STRATEGY.md).
let builder = AntigravityAgent::builder()
    .add_wire_inspector(Arc::new(TracingForwarder::new()))
    // ...
    ;
# let _ = builder;
```

Spawn- and init-time errors (`HandshakeFailed`, `InitFailed`,
`ConnectionClosed`) carry the tail of the harness's stderr — that is where
the harness explains itself (e.g. `no text model configuration provided`).

## Errors

`AntigravityError` is structural (`#[non_exhaustive]`, thiserror): match on
variants, never on message text. Key variants: `HarnessNotFound{searched}`,
`HandshakeFailed`/`InitFailed`/`ConnectionClosed` (with `stderr`),
`HarnessExited` (a failed exit on `shutdown()`, with `stderr`),
`Config` (spawn-time validation, including the safety gate), `Turn`
(cancellation, pre-turn denial, fatal model-backend errors), `Timeout`,
`ToolDispatch`, `WebSocket`, `Protocol`, `Io`, `Json`.

## Current limitations (follow-ups)

The [parity table](ANTIGRAVITY_BRIDGE_DESIGN.md#python-sdk-parity-google-antigravity-0123)
lists every Python SDK option this crate does not model, with the reason.
The ones a user is likeliest to look for:

- **User questions**: the `ask_question` builtin is answered "unanswered"
  automatically (never deadlocks) unless an `on_questions` hook is set —
  see [Answering agent questions](#answering-agent-questions).
- **Hooks are synchronous closures**, one per kind: each call runs on a
  thread of its own, bounded by the turn deadline; async work blocks on
  the runtime there.
- **A blocking tool body can't be preempted on a current-thread or
  single-worker runtime**: the turn deadline abandons a blocked synchronous
  tool only on a multi-thread runtime with two or more workers (see the turn
  deadline under [Streaming](#streaming)).
- **Subagents are not gated**: on 0.1.23 a subagent's own tool calls skip
  policies, the pre-tool hook and workspace containment, so `spawn()`
  refuses subagents beside those gates without `with_ungated_subagents(true)`
  (see [Subagents](#subagents)).
- **Only the bespoke `localharness` protocol**: the harness's
  Interactions-mode transport (`use_interactions_api`) is not used. On
  0.1.23 it rejects every `interaction.create` that carries an agent config,
  the Python SDK's own included (see the #494 decision in
  `docs/ANTIGRAVITY_BRIDGE_DESIGN.md`).
- **Trigger turns are surfaced on request**: `next_trigger_turn` drives
  them; nothing reads them in the background, so call it between user
  turns, and a trigger turn nobody consumes is halted by the next `chat`.
  A background consumer (#493 item 7) would read the harness while the
  agent is idle, which means moving turn processing (hooks, policies,
  custom tools) out of `&mut self` into a task of its own: a redesign of
  `AntigravityAgent`, left for its own change. The `schedule` builtin
  waits on it.
- **No `schedule` builtin**: a turn the agent schedules for itself could
  start while a `chat` turn does, and its events would land in that turn.
- **Not modeled (product decisions)**: Vertex AI endpoints (wire types
  exist; the supported path is a Gemini API key), the OpenAI-compatible and
  LiteRT model backends, and the beta `run_workflow` builtin (written
  disabled), whose workflows are Python scripts.
