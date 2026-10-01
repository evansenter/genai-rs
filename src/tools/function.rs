use super::Tool;
use serde::{Deserialize, Serialize};

/// Represents a function that can be called by the model.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct FunctionDeclaration {
    name: String,
    description: String,
    parameters: FunctionParameters,
}

/// Represents the parameters schema for a function.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct FunctionParameters {
    #[serde(rename = "type")]
    type_: String,
    properties: serde_json::Value,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    required: Vec<String>,
}

impl FunctionDeclaration {
    /// Creates a new FunctionDeclaration with the given fields.
    ///
    /// This is primarily intended for internal use by the macro system.
    /// For manual construction, prefer using `FunctionDeclaration::builder()`.
    #[doc(hidden)]
    pub fn new(name: String, description: String, parameters: FunctionParameters) -> Self {
        Self {
            name,
            description,
            parameters,
        }
    }

    /// Creates a builder for ergonomic FunctionDeclaration construction
    #[must_use]
    pub fn builder(name: impl Into<String>) -> FunctionDeclarationBuilder {
        FunctionDeclarationBuilder::new(name)
    }

    /// Returns the function name
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the function description
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns a reference to the function parameters
    #[must_use]
    pub fn parameters(&self) -> &FunctionParameters {
        &self.parameters
    }

    /// Converts this FunctionDeclaration into a Tool for API requests
    #[must_use]
    pub fn into_tool(self) -> Tool {
        Tool::Function {
            name: self.name,
            description: self.description,
            parameters: self.parameters,
        }
    }
}

impl FunctionParameters {
    /// Creates a new FunctionParameters with the given fields.
    ///
    /// This is primarily intended for internal use by the macro system.
    /// For manual construction, prefer using `FunctionDeclaration::builder()`.
    #[doc(hidden)]
    pub fn new(type_: String, properties: serde_json::Value, required: Vec<String>) -> Self {
        Self {
            type_,
            properties,
            required,
        }
    }

    /// Returns the parameter type (typically "object")
    #[must_use]
    pub fn type_(&self) -> &str {
        &self.type_
    }

    /// Returns the properties schema
    #[must_use]
    pub fn properties(&self) -> &serde_json::Value {
        &self.properties
    }

    /// Returns the list of required parameter names
    #[must_use]
    pub fn required(&self) -> &[String] {
        &self.required
    }
}

/// Builder for ergonomic FunctionDeclaration creation
#[derive(Debug)]
pub struct FunctionDeclarationBuilder {
    name: String,
    description: String,
    properties: serde_json::Value,
    required: Vec<String>,
}

impl FunctionDeclarationBuilder {
    /// Creates a new builder with the given function name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            properties: serde_json::Value::Object(serde_json::Map::new()),
            required: Vec::new(),
        }
    }

    /// Sets the function description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Adds a parameter to the function schema (replacing one of the same
    /// name).
    #[must_use]
    pub fn add_parameter(mut self, name: &str, schema: serde_json::Value) -> Self {
        if let serde_json::Value::Object(ref mut map) = self.properties {
            map.insert(name.to_string(), schema);
        }
        self
    }

    /// Sets the list of required parameter names.
    #[must_use]
    pub fn with_required(mut self, required: Vec<String>) -> Self {
        self.required = required;
        self
    }

    /// Builds the FunctionDeclaration
    ///
    /// # Validation
    ///
    /// This method performs validation and logs warnings for:
    /// - Empty or whitespace-only function names
    /// - Required parameters that don't exist in the properties schema
    ///
    /// These are warnings rather than errors: the API is the authority on
    /// what it accepts.
    pub fn build(self) -> FunctionDeclaration {
        // Validate function name
        if self.name.trim().is_empty() {
            tracing::warn!(
                "FunctionDeclaration built with empty or whitespace-only name. \
                This will likely be rejected by the API."
            );
        }

        // Validate required parameters exist in properties
        if let serde_json::Value::Object(ref props) = self.properties {
            for req in &self.required {
                if !props.contains_key(req) {
                    tracing::warn!(
                        "FunctionDeclaration '{}' requires parameter '{}' which is not defined in properties. \
                        This will likely cause API errors.",
                        self.name,
                        req
                    );
                }
            }
        }

        FunctionDeclaration {
            name: self.name,
            description: self.description,
            parameters: FunctionParameters {
                type_: "object".to_string(),
                properties: self.properties,
                required: self.required,
            },
        }
    }
}

#[cfg(test)]
#[path = "function_tests.rs"]
mod tests;
