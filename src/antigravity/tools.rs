//! Custom-tool dispatch: bridges harness `ToolCall`s to the crate's
//! existing function-calling machinery (`#[tool]` macro / global
//! [`FunctionRegistry`](crate::function_calling), [`ToolService`]).

use crate::function_calling::{CallableFunction, FunctionError, get_global_function_registry};
use crate::{FunctionDeclaration, ToolService};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

use super::protocol;

/// Owns the agent's custom tools and executes harness tool calls.
///
/// Tools come from two places:
///
/// - `add_tool(FunctionDeclaration)` — declarations from the `#[tool]`
///   macro; execution resolves through the crate's global function registry
///   by name (exactly like the Interactions-API auto-function path).
/// - `add_tool_service(Arc<dyn ToolService>)` — stateful tools carrying
///   their own [`CallableFunction`] implementations.
///
/// Only tools registered here are declared to the harness, and only
/// declared names are executable (an unknown name returns an error result
/// to the model instead of probing the global registry — defense in depth).
pub(crate) struct ToolDispatcher {
    declarations: Vec<FunctionDeclaration>,
    service_tools: HashMap<String, Arc<dyn CallableFunction>>,
}

impl std::fmt::Debug for ToolDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolDispatcher")
            .field(
                "declarations",
                &self
                    .declarations
                    .iter()
                    .map(FunctionDeclaration::name)
                    .collect::<Vec<_>>(),
            )
            .field(
                "service_tools",
                &self.service_tools.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl ToolDispatcher {
    pub(crate) fn new(
        declarations: Vec<FunctionDeclaration>,
        services: &[Arc<dyn ToolService>],
    ) -> Self {
        let mut service_tools = HashMap::new();
        for service in services {
            for tool in service.tools() {
                let name = tool.declaration().name().to_string();
                if service_tools.insert(name.clone(), tool).is_some() {
                    tracing::warn!(
                        "Duplicate tool name from tool services: '{name}'. \
                         Last registration will be used."
                    );
                }
            }
        }
        Self {
            declarations,
            service_tools,
        }
    }

    /// Whether any custom tools are registered.
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.declarations.is_empty() && self.service_tools.is_empty()
    }

    /// Builds the harness `Tool` declarations for the init config.
    ///
    /// The parameter schema is the declaration's `FunctionParameters`
    /// serialized as a JSON *string* (the harness's `parameters_json_schema`
    /// wire format).
    pub(crate) fn harness_declarations(&self) -> Vec<protocol::Tool> {
        let mut tools = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let service_declarations = self
            .service_tools
            .values()
            .map(|t| t.declaration())
            .collect::<Vec<_>>();
        for declaration in self.declarations.iter().chain(service_declarations.iter()) {
            if !seen.insert(declaration.name().to_string()) {
                tracing::warn!(
                    "Duplicate tool declaration '{}' skipped.",
                    declaration.name()
                );
                continue;
            }
            let schema = serde_json::to_string(declaration.parameters())
                .unwrap_or_else(|_| r#"{"type":"object"}"#.to_string());
            tools.push(protocol::Tool {
                name: Some(declaration.name().to_string()),
                description: Some(declaration.description().to_string()),
                parameters_json_schema: Some(schema),
                response_json_schema: None,
            });
        }
        tools
    }

    fn resolve(&self, name: &str) -> Option<ResolvedTool> {
        if let Some(tool) = self.service_tools.get(name) {
            return Some(ResolvedTool::Service(Arc::clone(tool)));
        }
        if self.declarations.iter().any(|d| d.name() == name)
            && let Some(function) = get_global_function_registry().get(name)
        {
            return Some(ResolvedTool::Registry(function));
        }
        None
    }

    /// Executes a tool call and shapes the result the way the harness
    /// expects (`{"result": ...}` / verbatim object on success,
    /// `{"error": ...}` on failure). Never fails: errors become error
    /// results so the model can react — mirroring the reference SDK.
    ///
    /// The tool runs on a task of its own (see [`run_tool`]), so the turn
    /// deadline can abandon it.
    pub(crate) async fn execute(&self, name: &str, args: Value) -> Value {
        let Some(tool) = self.resolve(name) else {
            tracing::warn!("Harness called unregistered tool '{name}'.");
            return json!({
                "error": format!(
                    "Tool '{name}' is not registered with this agent."
                )
            });
        };
        match run_tool(tool, args).await {
            Ok(Value::Object(map)) => Value::Object(map),
            Ok(other) => json!({ "result": other }),
            Err(err) => {
                let error = crate::function_calling::error_for_model(&err);
                tracing::warn!("Tool '{name}' failed: {error}");
                json!({ "error": error })
            }
        }
    }
}

/// A tool a dispatch task can own: service tools are shared `Arc`s, and
/// registry tools live in the `'static` global registry.
enum ResolvedTool {
    Service(Arc<dyn CallableFunction>),
    Registry(&'static dyn CallableFunction),
}

impl ResolvedTool {
    async fn call(&self, args: Value) -> Result<Value, FunctionError> {
        match self {
            Self::Service(tool) => tool.call(args).await,
            Self::Registry(tool) => tool.call(args).await,
        }
    }
}

/// Runs a tool call on a task of its own.
///
/// Called inline, a tool whose body blocks (a synchronous `#[tool]` fn runs
/// its body directly inside `call`) holds the task that polls the turn
/// deadline, so the deadline cannot fire until the tool returns. On its own
/// task, the turn waits on a join handle the deadline can abandon, and on a
/// multi-thread runtime with another worker free, that worker drives the
/// timer. Only workers drive a multi-thread runtime's timer, so with a
/// single worker, as on a current-thread runtime, a blocking body holds the
/// only thread that could; nothing in this crate can preempt it there.
///
/// Abandoning the call aborts the task: an async tool is cancelled at its
/// next `.await`, as it was when the call ran inline, and a blocking body
/// runs to completion with its result discarded. A panicking tool re-raises
/// its panic here, as an inline call would.
async fn run_tool(tool: ResolvedTool, args: Value) -> Result<Value, FunctionError> {
    let mut task = AbortOnDrop(tokio::spawn(async move { tool.call(args).await }));
    match (&mut task.0).await {
        Ok(result) => result,
        Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
        Err(e) => Err(FunctionError::ExecutionError(Box::new(e))),
    }
}

/// Aborts the task when the waiter goes away (the turn deadline fired, or
/// the turn was dropped).
struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FunctionParameters;

    struct EchoTool;

    impl CallableFunction for EchoTool {
        fn declaration(&self) -> FunctionDeclaration {
            FunctionDeclaration::new(
                "antigravity_echo".to_string(),
                "Echoes its input".to_string(),
                FunctionParameters::new(
                    "object".to_string(),
                    json!({"value": {"type": "string"}}),
                    vec!["value".to_string()],
                ),
            )
        }

        fn call(&self, args: Value) -> crate::FunctionFuture<'_> {
            Box::pin(async move { Ok(json!({"echo": args["value"]})) })
        }
    }

    struct ScalarTool;

    impl CallableFunction for ScalarTool {
        fn declaration(&self) -> FunctionDeclaration {
            FunctionDeclaration::new(
                "antigravity_scalar".to_string(),
                "Returns a scalar".to_string(),
                FunctionParameters::new("object".to_string(), json!({}), vec![]),
            )
        }

        fn call(&self, _args: Value) -> crate::FunctionFuture<'_> {
            Box::pin(async move { Ok(json!(42)) })
        }
    }

    struct FailingTool;

    impl CallableFunction for FailingTool {
        fn declaration(&self) -> FunctionDeclaration {
            FunctionDeclaration::new(
                "antigravity_fails".to_string(),
                "Always fails".to_string(),
                FunctionParameters::new("object".to_string(), json!({}), vec![]),
            )
        }

        fn call(&self, _args: Value) -> crate::FunctionFuture<'_> {
            Box::pin(async move { Err(FunctionError::ArgumentMismatch("bad input".to_string())) })
        }
    }

    struct TestService;

    impl ToolService for TestService {
        fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
            vec![
                Arc::new(EchoTool),
                Arc::new(ScalarTool),
                Arc::new(FailingTool),
            ]
        }
    }

    fn dispatcher() -> ToolDispatcher {
        let services: Vec<Arc<dyn ToolService>> = vec![Arc::new(TestService)];
        ToolDispatcher::new(vec![], &services)
    }

    #[test]
    fn test_harness_declarations_schema_is_json_string() {
        let d = dispatcher();
        let declarations = d.harness_declarations();
        assert_eq!(declarations.len(), 3);
        let echo = declarations
            .iter()
            .find(|t| t.name.as_deref() == Some("antigravity_echo"))
            .unwrap();
        assert_eq!(echo.description.as_deref(), Some("Echoes its input"));
        // The schema must be a JSON string that parses back to the object.
        let schema: Value =
            serde_json::from_str(echo.parameters_json_schema.as_ref().unwrap()).unwrap();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["value"]["type"], "string");
        assert_eq!(schema["required"][0], "value");
    }

    #[tokio::test]
    async fn test_execute_object_result_passes_through() {
        let d = dispatcher();
        let result = d.execute("antigravity_echo", json!({"value": "hi"})).await;
        assert_eq!(result, json!({"echo": "hi"}));
    }

    #[tokio::test]
    async fn test_execute_scalar_result_is_wrapped() {
        let d = dispatcher();
        let result = d.execute("antigravity_scalar", json!({})).await;
        assert_eq!(result, json!({"result": 42}));
    }

    #[tokio::test]
    async fn test_execute_error_becomes_error_result() {
        let d = dispatcher();
        let result = d.execute("antigravity_fails", json!({})).await;
        assert!(result["error"].as_str().unwrap().contains("bad input"));
    }

    #[tokio::test]
    async fn test_execute_unknown_tool_returns_error_result() {
        let d = dispatcher();
        let result = d.execute("never_registered", json!({})).await;
        assert!(result["error"].as_str().unwrap().contains("not registered"));
    }

    #[tokio::test]
    async fn test_declared_tools_resolve_through_global_registry() {
        // "test_function_global" is registered in function_calling's tests
        // via inventory; declaring it here should make it dispatchable.
        let declaration = FunctionDeclaration::new(
            "test_function_global".to_string(),
            "A global test function".to_string(),
            FunctionParameters::new(
                "object".to_string(),
                json!({"param": {"type": "string"}}),
                vec!["param".to_string()],
            ),
        );
        let d = ToolDispatcher::new(vec![declaration], &[]);
        let result = d
            .execute("test_function_global", json!({"param": "World"}))
            .await;
        assert_eq!(result["result"], "Global says: Hello, World");
    }

    /// Reports that its body started, then blocks its thread until released
    /// (or 30s pass).
    #[derive(Clone)]
    struct BlockingTool {
        started: std::sync::mpsc::Sender<()>,
        release: Arc<std::sync::Mutex<std::sync::mpsc::Receiver<()>>>,
    }

    impl CallableFunction for BlockingTool {
        fn declaration(&self) -> FunctionDeclaration {
            FunctionDeclaration::new(
                "antigravity_blocks".to_string(),
                "Blocks its thread".to_string(),
                FunctionParameters::new("object".to_string(), json!({}), vec![]),
            )
        }

        fn call(&self, _args: Value) -> crate::FunctionFuture<'_> {
            Box::pin(async move {
                let _ = self.started.send(());
                let _ = self
                    .release
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(30));
                Ok(json!("released"))
            })
        }
    }

    struct BlockingService(BlockingTool);

    impl ToolService for BlockingService {
        fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
            vec![Arc::new(self.0.clone())]
        }
    }

    /// A dispatcher whose one tool, `antigravity_blocks`, blocks its thread.
    /// Returns it with the sender that releases the tool and the receiver
    /// that hears when the tool's body starts.
    fn blocking_dispatcher() -> (
        ToolDispatcher,
        std::sync::mpsc::Sender<()>,
        std::sync::mpsc::Receiver<()>,
    ) {
        let (started_tx, started) = std::sync::mpsc::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        let tool = BlockingTool {
            started: started_tx,
            release: Arc::new(std::sync::Mutex::new(release_rx)),
        };
        let services: Vec<Arc<dyn ToolService>> = vec![Arc::new(BlockingService(tool))];
        (ToolDispatcher::new(vec![], &services), release, started)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_a_blocking_tool_does_not_hold_the_callers_timer() {
        // Regression: the tool ran inline on the turn's task, so a body that
        // blocked held the poll the turn deadline needed, and a 2s turn ran
        // as long as the tool.
        let (d, release, _started) = blocking_dispatcher();
        let start = std::time::Instant::now();
        let outcome = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            d.execute("antigravity_blocks", json!({})),
        )
        .await;
        let elapsed = start.elapsed();
        release.send(()).unwrap();
        assert!(outcome.is_err(), "the timeout must win: {outcome:?}");
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "the timer fired late: {elapsed:?}"
        );
    }

    #[test]
    fn test_dropping_the_runtime_waits_for_an_abandoned_blocking_tool() {
        // Pins the doc note on with_turn_timeout and in docs/ANTIGRAVITY.md:
        // an abandoned blocking body keeps its worker until it returns, and
        // dropping the runtime (the end of `#[tokio::main]`) waits for it.
        const HOLD: std::time::Duration = std::time::Duration::from_millis(500);
        let (d, release, started) = blocking_dispatcher();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let call = d.execute("antigravity_blocks", json!({}));
            // Abandon the call once its body is running, as the deadline does.
            let _ = tokio::time::timeout(std::time::Duration::from_millis(100), call).await;
        });
        started
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the tool's body never started");
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(HOLD);
            release.send(()).unwrap();
        });
        let start = std::time::Instant::now();
        drop(runtime);
        let elapsed = start.elapsed();
        releaser.join().unwrap();
        assert!(
            elapsed >= HOLD - std::time::Duration::from_millis(50),
            "the runtime dropped in {elapsed:?} while an abandoned body still held a \
             worker; the runtime-drop note in the docs no longer holds"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_a_blocking_tool_holds_a_single_worker_runtimes_timer() {
        // Pins what with_turn_timeout, docs/ANTIGRAVITY.md and the CHANGELOG
        // tell users: only a multi-thread runtime's workers drive its timer,
        // so with one worker (`#[tokio::main]` on a one-CPU host) a blocking
        // tool body stops the deadline until the body returns, as on a
        // current-thread runtime. If the deadline starts firing on time
        // here, those docs can drop the single-worker caveat.
        const HOLD: std::time::Duration = std::time::Duration::from_millis(500);
        let (d, release, started) = blocking_dispatcher();
        let mut call = Box::pin(d.execute("antigravity_blocks", json!({})));
        // Start the call, then wait on this (non-worker) thread until the
        // tool's body holds the only worker, so the timer below cannot have
        // fired before the body blocked.
        let first = std::future::poll_fn(|cx| std::task::Poll::Ready(call.as_mut().poll(cx))).await;
        assert!(first.is_pending(), "the call must still be running");
        started
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the tool's body never started");
        let start = std::time::Instant::now();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(HOLD);
            release.send(()).unwrap();
        });
        let _ = tokio::time::timeout(std::time::Duration::from_millis(100), &mut call).await;
        let elapsed = start.elapsed();
        releaser.join().unwrap();
        assert!(
            elapsed >= HOLD,
            "the 100ms timer fired while the only worker was blocked ({elapsed:?}); \
             the single-worker caveat in the docs no longer holds"
        );
    }

    struct PanickingTool;

    impl CallableFunction for PanickingTool {
        fn declaration(&self) -> FunctionDeclaration {
            FunctionDeclaration::new(
                "antigravity_panics".to_string(),
                "Panics".to_string(),
                FunctionParameters::new("object".to_string(), json!({}), vec![]),
            )
        }

        fn call(&self, _args: Value) -> crate::FunctionFuture<'_> {
            Box::pin(async move {
                panic!("tool panic 4417");
            })
        }
    }

    struct PanickingService;

    impl ToolService for PanickingService {
        fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
            vec![Arc::new(PanickingTool)]
        }
    }

    #[tokio::test]
    #[should_panic(expected = "tool panic 4417")]
    async fn test_a_panicking_tool_panics_the_caller() {
        // Same as an inline call: the panic is not turned into a tool error.
        let services: Vec<Arc<dyn ToolService>> = vec![Arc::new(PanickingService)];
        let d = ToolDispatcher::new(vec![], &services);
        let _ = d.execute("antigravity_panics", json!({})).await;
    }

    #[tokio::test]
    async fn test_undeclared_registry_tool_is_not_dispatchable() {
        // The global registry has "test_function_global", but a dispatcher
        // that never declared it must refuse to run it.
        let d = ToolDispatcher::new(vec![], &[]);
        assert!(d.is_empty());
        let result = d
            .execute("test_function_global", json!({"param": "x"}))
            .await;
        assert!(result["error"].as_str().unwrap().contains("not registered"));
    }
}
