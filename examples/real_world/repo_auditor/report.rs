//! Audit report types: the JSON schema the agent's `finish` tool must
//! satisfy, the Rust structs it parses into, and the severity table shared
//! with the `classify_severity` tool.

use serde::Deserialize;
use serde_json::{Value, json};

/// The one place severity policy lives. The `classify_severity` tool
/// answers from this table, so the model's report can be cross-checked
/// against it deterministically after the run.
pub const SEVERITY_TABLE: &[(&str, &str)] = &[
    ("sql_injection", "critical"),
    ("command_injection", "critical"),
    ("hardcoded_credentials", "high"),
    ("path_traversal", "high"),
    ("insecure_deserialization", "high"),
    ("weak_crypto", "medium"),
    ("other", "low"),
];

/// Severity for a category per [`SEVERITY_TABLE`] (unknown categories are
/// `low` — the enum schema should prevent them, but never panic in a tool).
pub fn severity_for(category: &str) -> &'static str {
    SEVERITY_TABLE
        .iter()
        .find(|(c, _)| *c == category)
        .map_or("low", |(_, s)| s)
}

/// All category names, for the tool parameter and report schema enums.
pub fn categories() -> Vec<&'static str> {
    SEVERITY_TABLE.iter().map(|(c, _)| *c).collect()
}

/// JSON schema for the structured audit report (the harness `finish` tool's
/// output schema, set via `with_response_schema`).
pub fn schema() -> Value {
    let severities = ["critical", "high", "medium", "low"];
    json!({
        "type": "object",
        "properties": {
            "repo_summary": {
                "type": "string",
                "description": "One paragraph: what the audited project does."
            },
            "findings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file": {"type": "string", "description": "Workspace-relative path."},
                        "title": {"type": "string"},
                        "category": {"type": "string", "enum": categories()},
                        "severity": {
                            "type": "string",
                            "enum": severities,
                            "description": "Must be the classify_severity tool's answer."
                        },
                        "recommendation": {"type": "string"}
                    },
                    "required": ["file", "title", "category", "severity", "recommendation"]
                }
            },
            "overall_risk": {"type": "string", "enum": severities}
        },
        "required": ["repo_summary", "findings", "overall_risk"]
    })
}

#[derive(Debug, Deserialize)]
pub struct AuditReport {
    pub repo_summary: String,
    pub findings: Vec<Finding>,
    pub overall_risk: String,
}

#[derive(Debug, Deserialize)]
pub struct Finding {
    pub file: String,
    pub title: String,
    pub category: String,
    pub severity: String,
    pub recommendation: String,
}

/// Pretty-print the report and cross-check every finding's severity against
/// [`SEVERITY_TABLE`] — a deterministic correctness check that doesn't
/// depend on LLM phrasing. Returns the number of mismatches.
pub fn render(report: &AuditReport) -> usize {
    println!("Repo summary: {}\n", report.repo_summary);
    println!("Findings ({}):", report.findings.len());

    let mut mismatches = 0;
    for (i, f) in report.findings.iter().enumerate() {
        println!(
            "  {}. [{}] {} — {}",
            i + 1,
            f.severity.to_uppercase(),
            f.file,
            f.title
        );
        println!("     category: {} | fix: {}", f.category, f.recommendation);
        let expected = severity_for(&f.category);
        if f.severity != expected {
            mismatches += 1;
            println!(
                "     !! severity mismatch: classifier says '{expected}' for '{}'",
                f.category
            );
        }
    }
    println!("\nOverall risk: {}", report.overall_risk.to_uppercase());
    if mismatches == 0 {
        println!("Severity cross-check: all findings match the classifier table.");
    } else {
        println!("Severity cross-check: {mismatches} finding(s) diverge from the classifier.");
    }
    mismatches
}
