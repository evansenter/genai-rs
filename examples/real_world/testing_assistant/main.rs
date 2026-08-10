//! # Testing Assistant Example
//!
//! This example demonstrates a test generation assistant that:
//! - Generates unit tests from code
//! - Creates test cases for edge conditions
//! - Suggests test coverage improvements
//! - Generates property-based test ideas
//!
//! ## Production Patterns Demonstrated
//!
//! - Code analysis for testable functions
//! - Structured output for test specifications
//! - Language-aware test generation
//! - Test coverage analysis
//!
//! ## Running
//!
//! ```bash
//! cargo run --example testing_assistant
//! ```
//!
//! ## Prerequisites
//!
//! Set the `GEMINI_API_KEY` environment variable with your API key.

use genai_rs::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use std::error::Error;

// ============================================================================
// Test Generation Types
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct TestSuite {
    module_name: String,
    imports: Vec<String>,
    test_cases: Vec<TestCase>,
    setup_code: Option<String>,
    teardown_code: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TestCase {
    name: String,
    description: String,
    category: String, // "unit", "integration", "edge_case", "property"
    test_code: String,
    assertions: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CoverageAnalysis {
    covered_scenarios: Vec<String>,
    missing_scenarios: Vec<String>,
    edge_cases: Vec<String>,
    recommendations: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PropertyTestIdea {
    property_name: String,
    description: String,
    input_strategy: String,
    expected_invariant: String,
}

// ============================================================================
// Testing Assistant Implementation
// ============================================================================

struct TestingAssistant {
    client: Client,
}

impl TestingAssistant {
    fn new(client: Client) -> Self {
        Self { client }
    }

    /// Generate a complete test suite for the given code
    async fn generate_test_suite(
        &self,
        code: &str,
        language: &str,
    ) -> Result<TestSuite, Box<dyn Error>> {
        let test_framework = match language {
            "rust" => "Use #[test] attributes and assert! macros",
            "python" => "Use pytest with assert statements",
            "javascript" | "typescript" => "Use Jest with describe/it/expect",
            _ => "Use standard testing conventions",
        };

        let schema = json!({
            "type": "object",
            "properties": {
                "module_name": {"type": "string"},
                "imports": {"type": "array", "items": {"type": "string"}},
                "test_cases": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "description": {"type": "string"},
                            "category": {"type": "string", "enum": ["unit", "integration", "edge_case", "property"]},
                            "test_code": {"type": "string"},
                            "assertions": {"type": "array", "items": {"type": "string"}}
                        },
                        "required": ["name", "description", "category", "test_code", "assertions"]
                    }
                },
                "setup_code": {"type": "string"},
                "teardown_code": {"type": "string"}
            },
            "required": ["module_name", "imports", "test_cases"]
        });

        let prompt = format!(
            "Generate a comprehensive test suite for this {} code. {}.\n\n\
             Include:\n\
             - Unit tests for each function\n\
             - Edge case tests (empty inputs, boundaries, etc.)\n\
             - Error condition tests\n\
             - Any necessary setup/teardown\n\n\
             ```{}\n{}\n```",
            language, test_framework, language, code
        );

        let response = self
            .client
            .interaction()
            .with_model("gemini-3.6-flash")
            .with_system_instruction(
                "You are an expert test engineer. Generate thorough, maintainable tests \
                 that cover normal cases, edge cases, and error conditions. Use descriptive \
                 test names and include meaningful assertions.",
            )
            .with_text(&prompt)
            .with_response_format(schema)
            .create()
            .await?;

        let text = response.as_text().ok_or("No response text")?;
        let suite: TestSuite = serde_json::from_str(text)?;
        Ok(suite)
    }

    /// Analyze existing tests and suggest improvements
    async fn analyze_coverage(
        &self,
        code: &str,
        existing_tests: &str,
        language: &str,
    ) -> Result<CoverageAnalysis, Box<dyn Error>> {
        let schema = json!({
            "type": "object",
            "properties": {
                "covered_scenarios": {"type": "array", "items": {"type": "string"}},
                "missing_scenarios": {"type": "array", "items": {"type": "string"}},
                "edge_cases": {"type": "array", "items": {"type": "string"}},
                "recommendations": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["covered_scenarios", "missing_scenarios", "edge_cases", "recommendations"]
        });

        let prompt = format!(
            "Analyze the test coverage for this {} code.\n\n\
             Source Code:\n```{}\n{}\n```\n\n\
             Existing Tests:\n```{}\n{}\n```\n\n\
             Identify what's covered, what's missing, and what edge cases should be added.",
            language, language, code, language, existing_tests
        );

        let response = self
            .client
            .interaction()
            .with_model("gemini-3.6-flash")
            .with_system_instruction(
                "You are a test coverage analyst. Evaluate test completeness, \
                 identify gaps, and suggest specific improvements. Be thorough \
                 in identifying edge cases and boundary conditions.",
            )
            .with_text(&prompt)
            .with_response_format(schema)
            .create()
            .await?;

        let text = response.as_text().ok_or("No response text")?;
        let analysis: CoverageAnalysis = serde_json::from_str(text)?;
        Ok(analysis)
    }

    /// Generate property-based test ideas
    async fn suggest_property_tests(
        &self,
        code: &str,
        language: &str,
    ) -> Result<Vec<PropertyTestIdea>, Box<dyn Error>> {
        let schema = json!({
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "property_name": {"type": "string"},
                    "description": {"type": "string"},
                    "input_strategy": {"type": "string"},
                    "expected_invariant": {"type": "string"}
                },
                "required": ["property_name", "description", "input_strategy", "expected_invariant"]
            }
        });

        let prop_framework = match language {
            "rust" => "proptest or quickcheck",
            "python" => "hypothesis",
            "javascript" | "typescript" => "fast-check",
            _ => "property-based testing",
        };

        let prompt = format!(
            "Suggest property-based tests for this {} code using {}.\n\n\
             For each property, describe:\n\
             - The invariant being tested\n\
             - How to generate inputs\n\
             - What should always hold true\n\n\
             ```{}\n{}\n```",
            language, prop_framework, language, code
        );

        let response = self
            .client
            .interaction()
            .with_model("gemini-3.6-flash")
            .with_system_instruction(
                "You are a property-based testing expert. Identify mathematical \
                 properties and invariants that should hold for any valid input. \
                 Focus on properties that would catch subtle bugs.",
            )
            .with_text(&prompt)
            .with_response_format(schema)
            .create()
            .await?;

        let text = response.as_text().ok_or("No response text")?;
        let ideas: Vec<PropertyTestIdea> = serde_json::from_str(text)?;
        Ok(ideas)
    }

    /// Generate a single test for a specific scenario
    async fn generate_single_test(
        &self,
        code: &str,
        scenario: &str,
        language: &str,
    ) -> Result<String, Box<dyn Error>> {
        let prompt = format!(
            "Generate a {} test for this specific scenario:\n\n\
             Scenario: {}\n\n\
             Code to test:\n```{}\n{}\n```\n\n\
             Provide just the test code, ready to copy and paste.",
            language, scenario, language, code
        );

        let response = self
            .client
            .interaction()
            .with_model("gemini-3.6-flash")
            .with_system_instruction(
                "You are a test engineer. Write a focused, well-named test \
                 for the specific scenario. Include setup if needed and \
                 clear assertions.",
            )
            .with_text(&prompt)
            .create()
            .await?;

        Ok(response
            .as_text()
            .unwrap_or("Unable to generate test.")
            .to_string())
    }
}

// ============================================================================
// Demo Code Samples
// ============================================================================

const SAMPLE_CODE: &str = r#"
/// A simple stack implementation
pub struct Stack<T> {
    items: Vec<T>,
    max_size: usize,
}

impl<T> Stack<T> {
    pub fn new(max_size: usize) -> Self {
        Stack {
            items: Vec::new(),
            max_size,
        }
    }

    pub fn push(&mut self, item: T) -> Result<(), &'static str> {
        if self.items.len() >= self.max_size {
            return Err("Stack overflow");
        }
        self.items.push(item);
        Ok(())
    }

    pub fn pop(&mut self) -> Option<T> {
        self.items.pop()
    }

    pub fn peek(&self) -> Option<&T> {
        self.items.last()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}
"#;

const EXISTING_TESTS: &str = r#"
#[test]
fn test_new_stack() {
    let stack: Stack<i32> = Stack::new(10);
    assert!(stack.is_empty());
}

#[test]
fn test_push_and_pop() {
    let mut stack = Stack::new(10);
    stack.push(42).unwrap();
    assert_eq!(stack.pop(), Some(42));
}
"#;

// ============================================================================
// Main Demo
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");
    let client = Client::builder(api_key).build()?;
    let assistant = TestingAssistant::new(client);

    println!("=== Testing Assistant Example ===\n");

    // Demo 1: Generate Test Suite
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🧪 GENERATE TEST SUITE\n");
    println!("Analyzing Stack implementation and generating tests...\n");

    match assistant.generate_test_suite(SAMPLE_CODE, "rust").await {
        Ok(suite) => {
            println!("Generated Test Suite: {}", suite.module_name);
            println!("========================\n");

            if !suite.imports.is_empty() {
                println!("Imports:");
                for import in &suite.imports {
                    println!("  {}", import);
                }
                println!();
            }

            if let Some(setup) = &suite.setup_code
                && !setup.is_empty()
            {
                println!("Setup:\n{}\n", setup);
            }

            println!("Test Cases ({} total):", suite.test_cases.len());
            for (i, test) in suite.test_cases.iter().enumerate() {
                println!("\n{}. {} [{}]", i + 1, test.name, test.category);
                println!("   Description: {}", test.description);
                println!("   Assertions: {}", test.assertions.join(", "));
                println!("   Code:");
                for line in test.test_code.lines().take(8) {
                    println!("   | {}", line);
                }
                if test.test_code.lines().count() > 8 {
                    println!("   | ... (truncated)");
                }
            }
        }
        Err(e) => eprintln!("Test generation failed: {}", e),
    }

    // Demo 2: Analyze Coverage
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 COVERAGE ANALYSIS\n");
    println!("Analyzing existing test coverage...\n");

    match assistant
        .analyze_coverage(SAMPLE_CODE, EXISTING_TESTS, "rust")
        .await
    {
        Ok(analysis) => {
            println!("✅ Covered Scenarios:");
            for scenario in &analysis.covered_scenarios {
                println!("   • {}", scenario);
            }

            println!("\n❌ Missing Scenarios:");
            for scenario in &analysis.missing_scenarios {
                println!("   • {}", scenario);
            }

            println!("\n⚠️ Edge Cases to Add:");
            for edge in &analysis.edge_cases {
                println!("   • {}", edge);
            }

            println!("\n💡 Recommendations:");
            for rec in &analysis.recommendations {
                println!("   • {}", rec);
            }
        }
        Err(e) => eprintln!("Coverage analysis failed: {}", e),
    }

    // Demo 3: Property-Based Test Ideas
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🔬 PROPERTY-BASED TEST IDEAS\n");

    match assistant.suggest_property_tests(SAMPLE_CODE, "rust").await {
        Ok(ideas) => {
            println!("Property-Based Test Suggestions:");
            for (i, idea) in ideas.iter().enumerate() {
                println!("\n{}. {}", i + 1, idea.property_name);
                println!("   Description: {}", idea.description);
                println!("   Input Strategy: {}", idea.input_strategy);
                println!("   Invariant: {}", idea.expected_invariant);
            }
        }
        Err(e) => eprintln!("Property test suggestions failed: {}", e),
    }

    // Demo 4: Generate Specific Test
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🎯 SPECIFIC TEST GENERATION\n");

    let scenario =
        "Test that pushing to a full stack returns an error and doesn't modify the stack";
    println!("Scenario: {}\n", scenario);

    match assistant
        .generate_single_test(SAMPLE_CODE, scenario, "rust")
        .await
    {
        Ok(test) => {
            println!("Generated Test:\n");
            println!("{}", test);
        }
        Err(e) => eprintln!("Single test generation failed: {}", e),
    }

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Testing Assistant Demo Complete\n");

    println!("--- Production Considerations ---");
    println!("• Integrate with CI/CD to generate tests on code changes");
    println!("• Add mutation testing suggestions");
    println!("• Generate mocks and test fixtures");
    println!("• Support for integration and E2E test generation");
    println!("• Track test generation history for regression");

    Ok(())
}
