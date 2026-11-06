//! The auto-function loop's state and its one round, shared by
//! `create_with_auto_functions()` and `create_stream_with_auto_functions()`:
//! tool discovery, function dispatch, and the bookkeeping between rounds
//! (usage, the previous-interaction chain, the max-loop result).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use serde_json::{Value, json};
use tracing::{debug, warn};

use crate::function_calling::{CallableFunction, FunctionRegistry, get_global_function_registry};
use crate::request::InteractionRequest;
use crate::streaming::FunctionExecutionResult;
use crate::{
    GenaiError, InteractionInput, InteractionResponse, OwnedFunctionCallInfo, Step, ToolService,
    UsageMetadata,
};

/// Default maximum iterations for auto function calling.
pub(crate) const DEFAULT_MAX_FUNCTION_CALL_LOOPS: usize = 5;

/// The prefix the API puts on a declared function's name on some turns:
/// `default_api:get_weather` for `get_weather` on the second user turn of a
/// non-streamed multi-turn chain (observed live 2026-10-01).
const DECLARED_FUNCTION_NAMESPACE: &str = "default_api:";

/// What a round's response asks of the loop.
pub(super) enum Round {
    /// No calls left to run: the loop's final response, carrying the usage
    /// of every round.
    Done(InteractionResponse),
    /// Calls to run before the next round.
    Calls {
        /// The round's response, whose id the next round chains on.
        response: InteractionResponse,
        /// The calls the server left for us (MCP calls it answered itself
        /// are not among them).
        calls: Vec<OwnedFunctionCallInfo>,
    },
}

/// The auto-function loop between rounds: the next request, the functions
/// it can call, and what the rounds so far added up to.
pub(super) struct AutoFunctionLoop {
    /// The request the next round sends.
    pub(super) request: InteractionRequest,
    service_functions: HashMap<String, Arc<dyn CallableFunction>>,
    registry: &'static FunctionRegistry,
    /// The API may report 0 input tokens on the final response, so the loop
    /// totals every round's usage itself.
    usage: UsageMetadata,
    /// The response of the last round that ran calls, returned when the loop
    /// reaches its maximum.
    last_response: Option<InteractionResponse>,
}

impl AutoFunctionLoop {
    /// Declares the functions the loop can run on `request` and gathers
    /// them for dispatch.
    pub(super) fn new(
        mut request: InteractionRequest,
        tool_service: Option<&Arc<dyn ToolService>>,
    ) -> Self {
        let service_functions: HashMap<_, _> = tool_service
            .map(|service| {
                service
                    .tools()
                    .into_iter()
                    .map(|f| (f.declaration().name().to_string(), f))
                    .collect()
            })
            .unwrap_or_default();
        auto_discover_tools(&mut request, &service_functions);
        Self {
            request,
            service_functions,
            registry: get_global_function_registry(),
            usage: UsageMetadata::default(),
            last_response: None,
        }
    }

    /// Reads a round's response: its usage joins the total, and its pending
    /// calls decide whether the loop goes on.
    ///
    /// # Errors
    ///
    /// [`GenaiError::MalformedResponse`] for a response without an id: with
    /// storage on (checked before the first round) the API always sends one,
    /// and without it the next round could not chain on this one.
    pub(super) fn read_round(
        &mut self,
        response: InteractionResponse,
    ) -> Result<Round, GenaiError> {
        if response.id.is_none() {
            return Err(GenaiError::MalformedResponse(
                "Response missing interaction ID. Auto-function calling requires stored \
                 interactions (store != false) to maintain conversation context."
                    .to_string(),
            ));
        }
        if let Some(usage) = &response.usage {
            self.usage.accumulate(usage);
        }
        let calls: Vec<_> = response
            .pending_function_calls()
            .iter()
            .map(|call| call.to_owned())
            .collect();
        if calls.is_empty() {
            debug!("No function calls in response, completing the auto-function loop");
            return Ok(Round::Done(self.with_total_usage(response)));
        }
        Ok(Round::Calls { response, calls })
    }

    /// Runs a round's calls in order and chains the next request on the
    /// round: the results become its input, the response its previous
    /// interaction. Returns each call's execution.
    pub(super) async fn run_calls(
        &mut self,
        response: InteractionResponse,
        calls: &[OwnedFunctionCallInfo],
    ) -> Vec<FunctionExecutionResult> {
        debug!("Executing {} function call(s)", calls.len());
        let mut executions = Vec::with_capacity(calls.len());
        let mut results = Vec::with_capacity(calls.len());
        for call in calls {
            let start = Instant::now();
            let (function_name, result) = execute_function(
                &call.name,
                call.args.clone(),
                &self.service_functions,
                self.registry,
            )
            .await;
            let duration = start.elapsed();
            debug!("Function '{function_name}' executed in {duration:?}");
            executions.push(FunctionExecutionResult::new(
                function_name,
                call.id.clone(),
                call.args.clone(),
                result.clone(),
                duration,
            ));
            // Only the result: the server has the call through
            // `previous_interaction_id`, under the call's own name.
            results.push(Step::function_result(
                call.name.clone(),
                call.id.clone(),
                result,
            ));
        }
        self.request.previous_interaction_id = response.id.clone();
        self.request.input = Some(InteractionInput::Steps(results));
        self.last_response = Some(response);
        executions
    }

    /// The last round's response, with every round's usage, once the loop
    /// has run `max_loops` rounds without a final answer.
    ///
    /// # Errors
    ///
    /// [`GenaiError::InvalidInput`] when no round ran: `max_loops` was 0.
    pub(super) fn max_loops_response(
        mut self,
        max_loops: usize,
    ) -> Result<InteractionResponse, GenaiError> {
        let response = self.last_response.take().ok_or_else(|| {
            GenaiError::InvalidInput(format!(
                "max_function_call_loops ({max_loops}) must be at least 1"
            ))
        })?;
        warn!(
            "Reached maximum function call loops ({max_loops}); returning the last response. \
             The model may be stuck in a loop."
        );
        Ok(self.with_total_usage(response))
    }

    fn with_total_usage(&self, response: InteractionResponse) -> InteractionResponse {
        InteractionResponse {
            usage: Some(self.usage.clone()),
            ..response
        }
    }
}

/// Declares the functions the auto-function loop can execute.
///
/// A tool service was set on purpose, so its functions are always declared,
/// next to any tools set explicitly (skipping a name already declared).
/// The global `#[tool]` registry is only consulted when no tools were set;
/// a service function shadows a registry one of the same name.
fn auto_discover_tools(
    request: &mut InteractionRequest,
    service_functions: &HashMap<String, Arc<dyn CallableFunction>>,
) {
    let mut service_declarations: Vec<_> = service_functions
        .values()
        .map(|f| f.declaration())
        .collect();
    // HashMap order is arbitrary; keep the wire deterministic.
    service_declarations.sort_by(|a, b| a.name().cmp(b.name()));

    let mut declarations = match &request.tools {
        Some(_) => Vec::new(),
        None => {
            let mut registry = get_global_function_registry().all_declarations();
            registry.retain(|decl| {
                let shadowed = service_functions.contains_key(decl.name());
                if shadowed {
                    warn!(
                        "Tool service function '{}' shadows global registry function with same name",
                        decl.name()
                    );
                }
                !shadowed
            });
            registry.sort_by(|a, b| a.name().cmp(b.name()));
            registry
        }
    };
    declarations.extend(service_declarations);
    if declarations.is_empty() {
        return;
    }

    let tools = request.tools.get_or_insert_with(Vec::new);
    for declaration in declarations {
        let declared = tools.iter().any(
            |tool| matches!(tool, crate::Tool::Function { name, .. } if name == declaration.name()),
        );
        if !declared {
            tools.push(declaration.into_tool());
        }
    }
}

/// Executes the function a call names. A `default_api:`-prefixed name that
/// matches no function falls back to its bare form.
///
/// Returns the name of the function that ran (the call's own name when none
/// matched) and its result as JSON. The function result sent back to the API
/// must still carry the call's name.
async fn execute_function<'n>(
    name: &'n str,
    args: Value,
    service_functions: &HashMap<String, Arc<dyn CallableFunction>>,
    function_registry: &FunctionRegistry,
) -> (&'n str, Value) {
    // A tool service's function (dependency-injected) shadows a `#[tool]`
    // one of the same name.
    let lookup = |name: &str| {
        service_functions
            .get(name)
            .map(|f| &**f)
            .or_else(|| function_registry.get(name))
    };
    let name = match name.strip_prefix(DECLARED_FUNCTION_NAMESPACE) {
        Some(bare) if lookup(name).is_none() && lookup(bare).is_some() => {
            debug!("Resolved namespaced function call '{name}' to '{bare}'");
            bare
        }
        _ => name,
    };
    // Failures go back to the model as JSON, so it can retry or adapt,
    // rather than failing the loop.
    let result = match lookup(name) {
        Some(function) => function.call(args).await.unwrap_or_else(|e| {
            warn!(
                "Function execution failed (recoverable): function='{name}', error='{e}'. \
                 The error will be sent to the model, which may retry or adapt."
            );
            json!({ "error": e.to_string() })
        }),
        None => {
            warn!(
                "Function not found in registry or tool service: function='{name}'. \
                 Informing model."
            );
            json!({ "error": format!("Function '{name}' is not available or not found.") })
        }
    };
    (name, result)
}

#[cfg(test)]
#[path = "round_tests.rs"]
mod tests;
