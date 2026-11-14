//! Client-side function calling: the [`CallableFunction`] trait `#[tool]`
//! implements, [`ToolService`] for tools with shared state, the registry
//! the auto-function loop looks functions up in, and [`FunctionError`].

use serde_json::Value;
use std::collections::HashMap;
use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tracing::warn;

use crate::__private::CallableFunctionFactory;
use crate::FunctionDeclaration;

/// Represents an error that can occur during function execution.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
/// New error variants may be added in future versions.
#[derive(Debug)]
#[non_exhaustive]
pub enum FunctionError {
    /// The model's arguments did not fit the function: one is missing, or
    /// does not deserialize into its parameter's type.
    ArgumentMismatch(String),
    /// The function ran and failed, or its result did not serialize.
    ExecutionError(Box<dyn Error + Send + Sync>),
}

impl std::fmt::Display for FunctionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ArgumentMismatch(msg) => write!(f, "Argument mismatch: {msg}"),
            Self::ExecutionError(err) => write!(f, "Function execution error: {err}"),
        }
    }
}

impl Error for FunctionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ExecutionError(err) => Some(err.as_ref()),
            Self::ArgumentMismatch(_) => None,
        }
    }
}

/// What a failed call sends the model, `{"error": ..}`, and what its log
/// line names: the error and each cause below it
/// ([`ErrorChain`](crate::errors::ErrorChain)). A tool that fails with a
/// `GenaiError::Http` gives the model the transport error too, which that
/// variant's `Display` leaves to `source()` (D-049).
pub(crate) fn error_for_model(error: &FunctionError) -> String {
    crate::errors::ErrorChain(error).to_string()
}

/// The future [`CallableFunction::call`] returns: boxed and `Send`, so the
/// trait stays usable as `dyn CallableFunction` (D-026).
///
/// Build one with `Box::pin(async move { ... })`.
pub type FunctionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Value, FunctionError>> + Send + 'a>>;

/// A function the model can call.
///
/// `#[tool]` (from `genai-rs-macros`) implements it for you. A hand-written
/// implementation, for a function that needs state from a [`ToolService`],
/// returns its result as a [`FunctionFuture`]:
///
/// ```
/// use genai_rs::{CallableFunction, FunctionDeclaration, FunctionError, FunctionFuture};
/// use serde_json::{Value, json};
///
/// struct Greet {
///     prefix: String,
/// }
///
/// impl CallableFunction for Greet {
///     fn declaration(&self) -> FunctionDeclaration {
///         FunctionDeclaration::builder("greet")
///             .with_description("Greets someone")
///             .add_parameter("name", json!({"type": "string"}))
///             .with_required(vec!["name".to_string()])
///             .build()
///     }
///
///     fn call(&self, args: Value) -> FunctionFuture<'_> {
///         Box::pin(async move {
///             let name = args["name"].as_str().ok_or_else(|| {
///                 FunctionError::ArgumentMismatch("missing 'name'".to_string())
///             })?;
///             Ok(json!({ "message": format!("{} {name}", self.prefix) }))
///         })
///     }
/// }
/// # let greet = Greet { prefix: "Hello".to_string() };
/// # let out = futures_util::FutureExt::now_or_never(greet.call(json!({"name": "Ada"})));
/// # assert_eq!(out.unwrap().unwrap(), json!({"message": "Hello Ada"}));
/// ```
pub trait CallableFunction: Send + Sync {
    /// Returns the declaration of the function.
    fn declaration(&self) -> FunctionDeclaration;

    /// Executes the function with the given arguments.
    ///
    /// The arguments are the JSON object the model sent; the result is the
    /// JSON sent back as the function's result. An `Err` is sent to the
    /// model as `{"error": "<message>"}`, so it can recover.
    fn call(&self, args: Value) -> FunctionFuture<'_>;
}

/// A provider of callable functions with shared state/dependencies.
///
/// Implement this trait on structs that need to provide tools with access to
/// shared resources like databases, APIs, or configuration. This enables
/// dependency injection for tool functions.
///
/// # Example
///
/// ```no_run
/// use genai_rs::{CallableFunction, ToolService, FunctionDeclaration};
/// use std::sync::Arc;
/// # use genai_rs::FunctionFuture;
/// # struct GetWeatherTool { api_key: String }
/// # impl CallableFunction for GetWeatherTool {
/// #     fn declaration(&self) -> FunctionDeclaration {
/// #         FunctionDeclaration::builder("get_weather").build()
/// #     }
/// #     fn call(&self, _args: serde_json::Value) -> FunctionFuture<'_> {
/// #         Box::pin(async move { Ok(serde_json::json!({"key_len": self.api_key.len()})) })
/// #     }
/// # }
///
/// struct WeatherService {
///     api_key: String,
/// }
///
/// impl ToolService for WeatherService {
///     fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
///         vec![
///             Arc::new(GetWeatherTool { api_key: self.api_key.clone() }),
///         ]
///     }
/// }
///
/// # async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
/// // Use with InteractionBuilder:
/// let service = Arc::new(WeatherService { api_key: "...".into() });
/// client.interaction()
///     .with_model(genai_rs::DEFAULT_MODEL)
///     .with_text("What's the weather in Tokyo?")
///     .with_tool_service(service)
///     .create_with_auto_functions()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub trait ToolService: Send + Sync {
    /// Returns the callable functions provided by this service.
    ///
    /// Each function can hold references to shared state from the service.
    fn tools(&self) -> Vec<Arc<dyn CallableFunction>>;
}

// `#[tool]` registrations (`crate::__private::CallableFunctionFactory`),
// collected into the global registry below.
inventory::collect!(CallableFunctionFactory);

/// A registry for callable functions.
pub(crate) struct FunctionRegistry {
    functions: HashMap<String, Box<dyn CallableFunction>>,
}

impl FunctionRegistry {
    /// Creates a new empty function registry.
    fn new() -> Self {
        Self {
            functions: HashMap::new(),
        }
    }

    /// Registers a function directly.
    fn register_raw(&mut self, function: Box<dyn CallableFunction>) {
        let name = function.declaration().name().to_string();
        if self.functions.contains_key(&name) {
            warn!(
                "Duplicate function name in auto-registration: function='{}'. Last registration will be used.",
                name
            );
        }
        self.functions.insert(name, function);
    }

    /// Retrieves a function by its name.
    pub(crate) fn get(&self, name: &str) -> Option<&dyn CallableFunction> {
        self.functions.get(name).map(std::convert::AsRef::as_ref)
    }

    /// Returns an iterator over all registered function declarations.
    pub(crate) fn all_declarations(&self) -> Vec<FunctionDeclaration> {
        self.functions.values().map(|f| f.declaration()).collect()
    }
}

/// Global registry, populated automatically via inventory.
static GLOBAL_FUNCTION_REGISTRY: std::sync::LazyLock<FunctionRegistry> =
    std::sync::LazyLock::new(|| {
        let mut registry = FunctionRegistry::new();

        for factory in inventory::iter::<CallableFunctionFactory> {
            let function = (factory.factory_fn)();
            registry.register_raw(function);
        }

        registry
    });

/// Provides access to the global function registry.
/// This is intended for internal use by the client for automatic function execution.
pub(crate) fn get_global_function_registry() -> &'static FunctionRegistry {
    &GLOBAL_FUNCTION_REGISTRY
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FunctionDeclaration;
    use serde_json::json;

    /// An error whose `Display` leaves its cause to `source()`, as
    /// `GenaiError::Http`'s does.
    #[derive(Debug)]
    struct Outer(std::io::Error);

    impl std::fmt::Display for Outer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("HTTP request error")
        }
    }

    impl Error for Outer {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn a_failed_tool_tells_the_model_every_cause() {
        let error = FunctionError::ExecutionError(Box::new(Outer(std::io::Error::other(
            "connection refused",
        ))));
        assert_eq!(
            error_for_model(&error),
            "Function execution error: HTTP request error: connection refused"
        );
        let mismatch = FunctionError::ArgumentMismatch("missing city".to_string());
        assert_eq!(
            error_for_model(&mismatch),
            "Argument mismatch: missing city"
        );
    }

    // Dummy function for testing purposes.
    // In real usage, this would be generated by the macro.
    struct TestFunctionGlobal;

    impl CallableFunction for TestFunctionGlobal {
        fn declaration(&self) -> FunctionDeclaration {
            FunctionDeclaration::new(
                "test_function_global".to_string(),
                "A global test function".to_string(),
                crate::FunctionParameters::new(
                    "object".to_string(),
                    json!({"param": {"type": "string"}}),
                    vec!["param".to_string()],
                ),
            )
        }

        fn call(&self, args: Value) -> crate::FunctionFuture<'_> {
            Box::pin(async move {
                args.get("param").and_then(Value::as_str).map_or_else(
                    || {
                        Err(FunctionError::ArgumentMismatch(
                            "Missing param for Global".to_string(),
                        ))
                    },
                    |p| Ok(json!({ "result": format!("Global says: Hello, {p}") })),
                )
            })
        }
    }

    // Manually create a factory function for the test, similar to what the macro would do.
    fn test_function_global_callable_factory() -> Box<dyn CallableFunction> {
        Box::new(TestFunctionGlobal)
    }

    // Simulate macro-based registration for testing `FunctionRegistry::new()`
    // This needs to be outside the test function to be collected by inventory.
    crate::__private::submit! {
        crate::__private::CallableFunctionFactory::new(test_function_global_callable_factory)
    }

    #[test]
    fn test_global_registry_population_and_access() {
        let registry = get_global_function_registry(); // Access the global registry
        let retrieved_func = registry.get("test_function_global");
        assert!(
            retrieved_func.is_some(),
            "Function 'test_function_global' should be in the global registry."
        );
        assert_eq!(
            retrieved_func.unwrap().declaration().name(),
            "test_function_global"
        );
    }

    #[tokio::test]
    async fn test_call_global_registered_function() {
        let registry = get_global_function_registry();
        let retrieved_func = registry
            .get("test_function_global")
            .expect("Global function not found");

        let args = json!({ "param": "GlobalInventoryWorld" });
        let result = retrieved_func.call(args).await;
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            json!({ "result": "Global says: Hello, GlobalInventoryWorld" })
        );
    }

    // Test ToolService trait for dependency injection

    /// A tool that holds shared state from its service.
    struct GreetTool {
        greeting_prefix: String,
    }

    impl CallableFunction for GreetTool {
        fn declaration(&self) -> FunctionDeclaration {
            FunctionDeclaration::new(
                "greet".to_string(),
                "Greets a person with a custom prefix".to_string(),
                crate::FunctionParameters::new(
                    "object".to_string(),
                    json!({"name": {"type": "string"}}),
                    vec!["name".to_string()],
                ),
            )
        }

        fn call(&self, args: Value) -> crate::FunctionFuture<'_> {
            Box::pin(async move {
                args.get("name").and_then(Value::as_str).map_or_else(
                    || {
                        Err(FunctionError::ArgumentMismatch(
                            "Missing 'name' argument".to_string(),
                        ))
                    },
                    |name| Ok(json!({ "message": format!("{} {name}!", self.greeting_prefix) })),
                )
            })
        }
    }

    /// A service that provides tools with shared configuration.
    struct GreetingService {
        prefix: String,
    }

    impl ToolService for GreetingService {
        fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
            vec![Arc::new(GreetTool {
                greeting_prefix: self.prefix.clone(),
            })]
        }
    }

    #[test]
    fn test_tool_service_returns_tools() {
        let service = GreetingService {
            prefix: "Hello".to_string(),
        };
        let tools = service.tools();

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].declaration().name(), "greet");
    }

    #[tokio::test]
    async fn test_tool_service_tool_can_be_called() {
        let service = GreetingService {
            prefix: "Howdy".to_string(),
        };
        let tools = service.tools();
        let greet_tool = &tools[0];

        let result = greet_tool.call(json!({ "name": "Partner" })).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), json!({ "message": "Howdy Partner!" }));
    }

    #[tokio::test]
    async fn test_tool_service_with_different_config() {
        // Demonstrate that different service instances produce different tool behavior
        let formal_service = GreetingService {
            prefix: "Good morning, Mr.".to_string(),
        };
        let casual_service = GreetingService {
            prefix: "Hey".to_string(),
        };

        let formal_tools = formal_service.tools();
        let casual_tools = casual_service.tools();

        let formal_result = formal_tools[0].call(json!({ "name": "Smith" })).await;
        let casual_result = casual_tools[0].call(json!({ "name": "Joe" })).await;

        assert_eq!(
            formal_result.unwrap(),
            json!({ "message": "Good morning, Mr. Smith!" })
        );
        assert_eq!(casual_result.unwrap(), json!({ "message": "Hey Joe!" }));
    }

    #[test]
    fn test_registry_returns_none_for_unknown_function() {
        let registry = get_global_function_registry();

        // Looking up a function that doesn't exist should return None
        let result = registry.get("this_function_definitely_does_not_exist_xyz123");
        assert!(
            result.is_none(),
            "Registry should return None for unknown functions"
        );
    }

    #[test]
    fn test_registry_all_declarations_contains_registered() {
        let registry = get_global_function_registry();
        let declarations = registry.all_declarations();

        // Our test function should be in the list
        let names: Vec<_> = declarations.iter().map(FunctionDeclaration::name).collect();
        assert!(
            names.contains(&"test_function_global"),
            "all_declarations should include registered function"
        );
    }

    #[test]
    fn test_tool_service_tools_are_independent() {
        // Verify that calling tools() multiple times returns independent instances
        let service = GreetingService {
            prefix: "Hi".to_string(),
        };

        let tools1 = service.tools();
        let tools2 = service.tools();

        // Both should have the same declaration
        assert_eq!(
            tools1[0].declaration().name(),
            tools2[0].declaration().name()
        );

        // But they should be separate Arc instances (different pointers)
        // This ensures each call to tools() creates fresh tool instances
        assert!(!Arc::ptr_eq(&tools1[0], &tools2[0]));
    }

    #[test]
    fn test_registry_duplicate_registration_last_wins() {
        // Test that when two functions with the same name are registered,
        // the last one wins (and a warning is logged)
        let mut registry = FunctionRegistry::new();

        // First function
        struct FirstFunc;
        impl CallableFunction for FirstFunc {
            fn declaration(&self) -> FunctionDeclaration {
                FunctionDeclaration::new(
                    "duplicate_name".to_string(),
                    "First function".to_string(),
                    crate::FunctionParameters::new("object".to_string(), json!({}), vec![]),
                )
            }
            fn call(&self, _args: Value) -> crate::FunctionFuture<'_> {
                Box::pin(async move { Ok(json!("first")) })
            }
        }

        // Second function with same name
        struct SecondFunc;
        impl CallableFunction for SecondFunc {
            fn declaration(&self) -> FunctionDeclaration {
                FunctionDeclaration::new(
                    "duplicate_name".to_string(),
                    "Second function".to_string(),
                    crate::FunctionParameters::new("object".to_string(), json!({}), vec![]),
                )
            }
            fn call(&self, _args: Value) -> crate::FunctionFuture<'_> {
                Box::pin(async move { Ok(json!("second")) })
            }
        }

        // Register first, then second with same name
        registry.register_raw(Box::new(FirstFunc));
        registry.register_raw(Box::new(SecondFunc));

        // Last registration should win
        let func = registry
            .get("duplicate_name")
            .expect("Function should exist");
        assert_eq!(
            func.declaration().description(),
            "Second function",
            "Last registered function should win"
        );
    }

    #[test]
    fn test_empty_tool_service() {
        // A tool service that provides no tools
        struct EmptyService;

        impl ToolService for EmptyService {
            fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
                vec![]
            }
        }

        let service = EmptyService;
        let tools = service.tools();

        assert!(tools.is_empty(), "Empty service should return no tools");
    }
}
