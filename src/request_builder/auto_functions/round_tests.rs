use super::*;
use crate::{FunctionDeclaration, FunctionFuture};

/// A request with input and a model, as `build()` returns one.
fn request() -> InteractionRequest {
    serde_json::from_value(json!({"model": "test-model", "input": "hi"})).unwrap()
}

fn parse_response(value: Value) -> InteractionResponse {
    serde_json::from_value(value).unwrap()
}

/// A response with one pending `get_time` call.
fn calling_response(id: &str, input_tokens: u32) -> InteractionResponse {
    parse_response(json!({
        "id": id,
        "status": "requires_action",
        "steps": [{
            "type": "function_call",
            "id": "call-1",
            "name": "get_time",
            "arguments": {"tz": "UTC"}
        }],
        "usage": {"total_input_tokens": input_tokens, "total_tokens": input_tokens}
    }))
}

struct Clock;

impl CallableFunction for Clock {
    fn declaration(&self) -> FunctionDeclaration {
        FunctionDeclaration::builder("get_time")
            .with_description("Returns the time")
            .build()
    }

    fn call(&self, args: Value) -> FunctionFuture<'_> {
        Box::pin(async move { Ok(json!({"time": "12:00", "tz": args["tz"]})) })
    }
}

struct ClockService;

impl ToolService for ClockService {
    fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
        vec![Arc::new(Clock)]
    }
}

#[test]
fn test_new_without_service_declares_no_service_functions() {
    let state = AutoFunctionLoop::new(request(), None);
    assert!(state.service_functions.is_empty());
}

#[test]
fn test_new_declares_the_service_functions() {
    let service: Arc<dyn ToolService> = Arc::new(ClockService);
    let state = AutoFunctionLoop::new(request(), Some(&service));
    let tools = state.request.tools.as_ref().expect("the service's tool");
    assert!(
        tools
            .iter()
            .any(|t| matches!(t, crate::Tool::Function { name, .. } if name == "get_time"))
    );
}

#[test]
fn test_read_round_requires_an_interaction_id() {
    let mut state = AutoFunctionLoop::new(request(), None);
    let err = state
        .read_round(parse_response(json!({"status": "completed"})))
        .err()
        .expect("no id");
    assert!(matches!(err, GenaiError::MalformedResponse(_)), "{err:?}");
}

#[tokio::test]
async fn test_a_round_chains_the_next_request_and_totals_usage() {
    let service: Arc<dyn ToolService> = Arc::new(ClockService);
    let mut state = AutoFunctionLoop::new(request(), Some(&service));

    let Round::Calls { response, calls } = state.read_round(calling_response("i1", 10)).unwrap()
    else {
        panic!("a pending call continues the loop");
    };
    let executions = state.run_calls(response, &calls).await;
    assert_eq!(executions.len(), 1);
    assert_eq!(executions[0].name, "get_time");
    assert_eq!(executions[0].call_id, "call-1");
    assert_eq!(executions[0].result, json!({"time": "12:00", "tz": "UTC"}));
    assert_eq!(state.request.previous_interaction_id.as_deref(), Some("i1"));
    let Some(InteractionInput::Steps(steps)) = &state.request.input else {
        panic!("the results are the next input");
    };
    assert!(matches!(
        steps.as_slice(),
        [Step::FunctionResult { call_id, .. }] if call_id == "call-1"
    ));

    let done = parse_response(json!({
        "id": "i2",
        "status": "completed",
        "steps": [{"type": "model_output", "content": [{"type": "text", "text": "noon"}]}],
        "usage": {"total_input_tokens": 0, "total_tokens": 5}
    }));
    let Round::Done(done) = state.read_round(done).unwrap() else {
        panic!("no pending call ends the loop");
    };
    let usage = done.usage.expect("the total usage");
    assert_eq!(usage.total_input_tokens, Some(10));
    assert_eq!(usage.total_tokens, Some(15));
}

#[tokio::test]
async fn test_max_loops_response_is_the_last_round_with_total_usage() {
    let mut state = AutoFunctionLoop::new(request(), None);
    for (id, tokens) in [("i1", 3), ("i2", 4)] {
        let Round::Calls { response, calls } =
            state.read_round(calling_response(id, tokens)).unwrap()
        else {
            panic!("a pending call continues the loop");
        };
        state.run_calls(response, &calls).await;
    }
    let last = state.max_loops_response(2).unwrap();
    assert_eq!(last.id.as_deref(), Some("i2"));
    assert_eq!(last.usage.unwrap().total_input_tokens, Some(7));
}

#[test]
fn test_max_loops_response_without_a_round_is_invalid_input() {
    let state = AutoFunctionLoop::new(request(), None);
    let err = state.max_loops_response(0).unwrap_err();
    assert!(matches!(err, GenaiError::InvalidInput(_)), "{err:?}");
}

#[tokio::test]
async fn test_execute_function_not_found_returns_error_json() {
    // A function missing from both the service map and the global registry
    // must produce a recoverable JSON error (sent back to the model), not a
    // hard failure.
    let service_functions: HashMap<String, Arc<dyn CallableFunction>> = HashMap::new();
    let (_, result) = execute_function(
        "__genai_rs_test_nonexistent_function__",
        json!({"arg": 1}),
        &service_functions,
        get_global_function_registry(),
    )
    .await;

    let error = result
        .get("error")
        .and_then(|v| v.as_str())
        .expect("Missing function should produce an error object");
    assert!(
        error.contains("__genai_rs_test_nonexistent_function__"),
        "Error should name the missing function: {error}"
    );
}

#[tokio::test]
async fn test_execute_function_resolves_a_namespaced_name() {
    let service_functions: HashMap<String, Arc<dyn CallableFunction>> =
        HashMap::from([("get_time".to_string(), Arc::new(Clock) as _)]);
    let (name, result) = execute_function(
        "default_api:get_time",
        json!({"tz": "UTC"}),
        &service_functions,
        get_global_function_registry(),
    )
    .await;
    assert_eq!(name, "get_time");
    assert_eq!(result["time"], "12:00");
}

#[test]
fn test_default_max_function_call_loops() {
    // Documented default used by with_max_function_call_loops() tests
    assert_eq!(DEFAULT_MAX_FUNCTION_CALL_LOOPS, 5);
}
