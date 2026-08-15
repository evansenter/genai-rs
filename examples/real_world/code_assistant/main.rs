//! # Code Assistant Example
//!
//! This example demonstrates a code analysis and generation assistant that:
//! - Analyzes code structure and complexity
//! - Generates documentation and comments
//! - Suggests refactoring improvements
//! - Explains code behavior
//!
//! ## Production Patterns Demonstrated
//!
//! - Structured output for consistent responses
//! - System prompts for specialized behavior
//! - Code-focused interactions
//! - Multi-step analysis workflows
//!
//! ## Running
//!
//! ```bash
//! cargo run --example code_assistant
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
// Code Analysis Types
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct CodeAnalysis {
    summary: String,
    complexity: ComplexityInfo,
    functions: Vec<FunctionInfo>,
    suggestions: Vec<String>,
    potential_issues: Vec<Issue>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ComplexityInfo {
    level: String,
    score: u32,
    reasoning: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct FunctionInfo {
    name: String,
    purpose: String,
    parameters: Vec<String>,
    return_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Issue {
    severity: String,
    description: String,
    line_hint: String,
    fix_suggestion: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct DocumentationResult {
    module_doc: String,
    function_docs: Vec<FunctionDoc>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FunctionDoc {
    function_name: String,
    doc_comment: String,
}

// ============================================================================
// Code Assistant Implementation
// ============================================================================

struct CodeAssistant {
    client: Client,
}

impl CodeAssistant {
    fn new(client: Client) -> Self {
        Self { client }
    }

    /// Analyze code structure and provide insights
    async fn analyze_code(
        &self,
        code: &str,
        language: &str,
    ) -> Result<CodeAnalysis, Box<dyn Error>> {
        let schema = json!({
            "type": "object",
            "properties": {
                "summary": {"type": "string"},
                "complexity": {
                    "type": "object",
                    "properties": {
                        "level": {"type": "string", "enum": ["low", "medium", "high"]},
                        "score": {"type": "integer", "minimum": 1, "maximum": 10},
                        "reasoning": {"type": "string"}
                    },
                    "required": ["level", "score", "reasoning"]
                },
                "functions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "purpose": {"type": "string"},
                            "parameters": {"type": "array", "items": {"type": "string"}},
                            "return_type": {"type": "string"}
                        },
                        "required": ["name", "purpose", "parameters", "return_type"]
                    }
                },
                "suggestions": {"type": "array", "items": {"type": "string"}},
                "potential_issues": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "severity": {"type": "string", "enum": ["info", "warning", "error"]},
                            "description": {"type": "string"},
                            "line_hint": {"type": "string"},
                            "fix_suggestion": {"type": "string"}
                        },
                        "required": ["severity", "description", "line_hint", "fix_suggestion"]
                    }
                }
            },
            "required": ["summary", "complexity", "functions", "suggestions", "potential_issues"]
        });

        let prompt = format!(
            "Analyze the following {} code. Identify functions, assess complexity, \
             and provide improvement suggestions.\n\n```{}\n{}\n```",
            language, language, code
        );

        let response = self
            .client
            .interaction()
            .with_model("gemini-3.6-flash")
            .with_system_instruction(
                "You are an expert code analyst. Analyze code thoroughly and provide \
                 actionable insights. Focus on code quality, maintainability, and \
                 potential bugs. Be specific in your suggestions.",
            )
            .with_text(&prompt)
            .with_response_format(schema)
            .create()
            .await?;

        let text = response.as_text().ok_or("No response text")?;
        let analysis: CodeAnalysis = serde_json::from_str(text)?;
        Ok(analysis)
    }

    /// Generate documentation for code
    async fn generate_documentation(
        &self,
        code: &str,
        language: &str,
    ) -> Result<DocumentationResult, Box<dyn Error>> {
        let schema = json!({
            "type": "object",
            "properties": {
                "module_doc": {"type": "string"},
                "function_docs": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "function_name": {"type": "string"},
                            "doc_comment": {"type": "string"}
                        },
                        "required": ["function_name", "doc_comment"]
                    }
                }
            },
            "required": ["module_doc", "function_docs"]
        });

        let doc_style = match language {
            "rust" => "Rust doc comments (///) with # Examples sections where appropriate",
            "python" => "Python docstrings with Args, Returns, and Raises sections",
            "javascript" | "typescript" => "JSDoc comments with @param and @returns tags",
            _ => "standard documentation comments",
        };

        let prompt = format!(
            "Generate comprehensive documentation for this {} code using {}.\n\n```{}\n{}\n```",
            language, doc_style, language, code
        );

        let response = self
            .client
            .interaction()
            .with_model("gemini-3.6-flash")
            .with_system_instruction(
                "You are a technical writer specializing in code documentation. \
                 Write clear, concise documentation that explains the purpose, \
                 parameters, return values, and usage examples. Follow language \
                 conventions and best practices.",
            )
            .with_text(&prompt)
            .with_response_format(schema)
            .create()
            .await?;

        let text = response.as_text().ok_or("No response text")?;
        let docs: DocumentationResult = serde_json::from_str(text)?;
        Ok(docs)
    }

    /// Explain what code does in plain language
    async fn explain_code(&self, code: &str, language: &str) -> Result<String, Box<dyn Error>> {
        let prompt = format!(
            "Explain what this {} code does in simple terms. Include:\n\
             1. Overall purpose\n\
             2. Step-by-step breakdown of the logic\n\
             3. Any important details or edge cases\n\n\
             ```{}\n{}\n```",
            language, language, code
        );

        let response = self
            .client
            .interaction()
            .with_model("gemini-3.6-flash")
            .with_system_instruction(
                "You are a patient programming tutor. Explain code clearly and \
                 thoroughly, using analogies where helpful. Assume the reader \
                 has basic programming knowledge but may not be familiar with \
                 advanced concepts or this specific language.",
            )
            .with_text(&prompt)
            .create()
            .await?;

        let text = response.as_text().ok_or("No response text")?;
        Ok(text.to_string())
    }

    /// Suggest refactoring improvements
    async fn suggest_refactoring(
        &self,
        code: &str,
        language: &str,
    ) -> Result<String, Box<dyn Error>> {
        let prompt = format!(
            "Review this {} code and suggest refactoring improvements. Consider:\n\
             - Code organization and structure\n\
             - Naming conventions\n\
             - Error handling\n\
             - Performance optimizations\n\
             - Design patterns that could apply\n\
             - Reducing code duplication\n\n\
             Provide specific, actionable suggestions with code examples.\n\n\
             ```{}\n{}\n```",
            language, language, code
        );

        let response = self
            .client
            .interaction()
            .with_model("gemini-3.6-flash")
            .with_system_instruction(
                "You are a senior software engineer conducting a code review. \
                 Focus on maintainability, readability, and best practices. \
                 Provide concrete refactoring examples, not just general advice.",
            )
            .with_text(&prompt)
            .create()
            .await?;

        let text = response.as_text().ok_or("No response text")?;
        Ok(text.to_string())
    }
}

// ============================================================================
// Demo Code Samples
// ============================================================================

const SAMPLE_RUST_CODE: &str = r#"
fn process_items(items: Vec<String>) -> Vec<String> {
    let mut result = Vec::new();
    for i in 0..items.len() {
        let item = items[i].clone();
        if item.len() > 0 {
            let processed = item.to_uppercase();
            if !result.contains(&processed) {
                result.push(processed);
            }
        }
    }
    return result;
}

fn calculate_stats(numbers: Vec<i32>) -> (f64, i32, i32) {
    let sum: i32 = numbers.iter().sum();
    let avg = sum as f64 / numbers.len() as f64;
    let max = *numbers.iter().max().unwrap();
    let min = *numbers.iter().min().unwrap();
    (avg, max, min)
}
"#;

// ============================================================================
// Main Demo
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");
    let client = Client::builder(api_key).build()?;
    let assistant = CodeAssistant::new(client);

    println!("=== Code Assistant Example ===\n");

    // Demo 1: Code Analysis
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 CODE ANALYSIS\n");
    println!("Analyzing sample Rust code...\n");

    match assistant.analyze_code(SAMPLE_RUST_CODE, "rust").await {
        Ok(analysis) => {
            println!("Summary: {}\n", analysis.summary);
            println!(
                "Complexity: {} (score: {}/10)\n  {}",
                analysis.complexity.level, analysis.complexity.score, analysis.complexity.reasoning
            );

            println!("\nFunctions found:");
            for func in &analysis.functions {
                println!(
                    "  • {} -> {}\n    Purpose: {}",
                    func.name, func.return_type, func.purpose
                );
            }

            if !analysis.suggestions.is_empty() {
                println!("\nSuggestions:");
                for (i, suggestion) in analysis.suggestions.iter().enumerate() {
                    println!("  {}. {}", i + 1, suggestion);
                }
            }

            if !analysis.potential_issues.is_empty() {
                println!("\nPotential Issues:");
                for issue in &analysis.potential_issues {
                    println!(
                        "  [{:?}] {}\n    Fix: {}",
                        issue.severity.to_uppercase(),
                        issue.description,
                        issue.fix_suggestion
                    );
                }
            }
        }
        Err(e) => eprintln!("Analysis failed: {}", e),
    }

    // Demo 2: Generate Documentation
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📝 DOCUMENTATION GENERATION\n");

    match assistant
        .generate_documentation(SAMPLE_RUST_CODE, "rust")
        .await
    {
        Ok(docs) => {
            println!("Module Documentation:");
            println!("{}\n", docs.module_doc);

            println!("Function Documentation:");
            for func_doc in &docs.function_docs {
                println!("--- {} ---", func_doc.function_name);
                println!("{}\n", func_doc.doc_comment);
            }
        }
        Err(e) => eprintln!("Documentation generation failed: {}", e),
    }

    // Demo 3: Code Explanation
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("💡 CODE EXPLANATION\n");

    let simple_code = r#"
fn fibonacci(n: u32) -> u32 {
    match n {
        0 => 0,
        1 => 1,
        _ => fibonacci(n - 1) + fibonacci(n - 2),
    }
}
"#;

    match assistant.explain_code(simple_code, "rust").await {
        Ok(explanation) => {
            println!("{}", explanation);
        }
        Err(e) => eprintln!("Explanation failed: {}", e),
    }

    // Demo 4: Refactoring Suggestions
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🔧 REFACTORING SUGGESTIONS\n");

    match assistant
        .suggest_refactoring(SAMPLE_RUST_CODE, "rust")
        .await
    {
        Ok(suggestions) => {
            println!("{}", suggestions);
        }
        Err(e) => eprintln!("Refactoring suggestions failed: {}", e),
    }

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Code Assistant Demo Complete\n");

    println!("--- Production Considerations ---");
    println!("• Integrate with IDE extensions for real-time analysis");
    println!("• Add support for analyzing entire codebases");
    println!("• Implement caching for repeated analyses");
    println!("• Add language-specific linting rule integration");
    println!("• Track analysis history for improvement trends");

    Ok(())
}
