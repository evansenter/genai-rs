//! # Data Analysis Example
//!
//! This example demonstrates a data analysis assistant that:
//! - Analyzes CSV data using function calling
//! - Performs statistical calculations
//! - Generates insights and visualizations descriptions
//! - Answers natural language questions about data
//!
//! ## Production Patterns Demonstrated
//!
//! - Function calling for data operations
//! - Natural language to data query translation
//! - Statistical analysis tools
//! - Structured output for reports
//!
//! ## Running
//!
//! ```bash
//! cargo run --example data_analysis
//! ```
//!
//! ## Prerequisites
//!
//! Set the `GEMINI_API_KEY` environment variable with your API key.

use genai_rs::{CallableFunction, Client};
use genai_rs_macros::tool;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::error::Error;

// ============================================================================
// Simulated Data Store
// ============================================================================

/// Represents a row in our sales data
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SalesRecord {
    date: String,
    product: String,
    category: String,
    quantity: i32,
    unit_price: f64,
    region: String,
}

/// Simulated CSV data store
struct DataStore {
    records: Vec<SalesRecord>,
}

impl DataStore {
    fn new() -> Self {
        let records = vec![
            SalesRecord {
                date: "2024-01-15".into(),
                product: "Laptop Pro".into(),
                category: "Electronics".into(),
                quantity: 25,
                unit_price: 1299.99,
                region: "North".into(),
            },
            SalesRecord {
                date: "2024-01-15".into(),
                product: "Wireless Mouse".into(),
                category: "Electronics".into(),
                quantity: 150,
                unit_price: 29.99,
                region: "North".into(),
            },
            SalesRecord {
                date: "2024-01-16".into(),
                product: "Office Chair".into(),
                category: "Furniture".into(),
                quantity: 40,
                unit_price: 299.99,
                region: "South".into(),
            },
            SalesRecord {
                date: "2024-01-16".into(),
                product: "Standing Desk".into(),
                category: "Furniture".into(),
                quantity: 15,
                unit_price: 549.99,
                region: "East".into(),
            },
            SalesRecord {
                date: "2024-01-17".into(),
                product: "Laptop Pro".into(),
                category: "Electronics".into(),
                quantity: 30,
                unit_price: 1299.99,
                region: "West".into(),
            },
            SalesRecord {
                date: "2024-01-17".into(),
                product: "Monitor 4K".into(),
                category: "Electronics".into(),
                quantity: 45,
                unit_price: 449.99,
                region: "South".into(),
            },
            SalesRecord {
                date: "2024-01-18".into(),
                product: "Keyboard Mechanical".into(),
                category: "Electronics".into(),
                quantity: 80,
                unit_price: 149.99,
                region: "North".into(),
            },
            SalesRecord {
                date: "2024-01-18".into(),
                product: "Office Chair".into(),
                category: "Furniture".into(),
                quantity: 25,
                unit_price: 299.99,
                region: "East".into(),
            },
            SalesRecord {
                date: "2024-01-19".into(),
                product: "Webcam HD".into(),
                category: "Electronics".into(),
                quantity: 60,
                unit_price: 79.99,
                region: "West".into(),
            },
            SalesRecord {
                date: "2024-01-19".into(),
                product: "Desk Lamp".into(),
                category: "Furniture".into(),
                quantity: 100,
                unit_price: 45.99,
                region: "North".into(),
            },
        ];
        Self { records }
    }
}

// Global data store for tool functions
fn get_data_store() -> &'static DataStore {
    static DATA_STORE: std::sync::OnceLock<DataStore> = std::sync::OnceLock::new();
    DATA_STORE.get_or_init(DataStore::new)
}

// ============================================================================
// Data Analysis Tool Functions
// ============================================================================

/// Get summary statistics for a numeric column
#[tool(column(description = "Column name: 'quantity' or 'unit_price'"))]
fn get_column_stats(column: String) -> String {
    println!("  [Tool: get_column_stats({})]", column);

    let values: Vec<f64> = get_data_store()
        .records
        .iter()
        .map(|r| match column.as_str() {
            "quantity" => r.quantity as f64,
            "unit_price" => r.unit_price,
            _ => 0.0,
        })
        .collect();

    if values.is_empty() {
        return r#"{"error": "No data found"}"#.to_string();
    }

    let sum: f64 = values.iter().sum();
    let count = values.len() as f64;
    let mean = sum / count;
    let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    // Calculate standard deviation
    let variance: f64 = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count;
    let std_dev = variance.sqrt();

    serde_json::json!({
        "column": column,
        "count": count as i32,
        "sum": sum,
        "mean": format!("{:.2}", mean),
        "min": min,
        "max": max,
        "std_dev": format!("{:.2}", std_dev)
    })
    .to_string()
}

/// Calculate total sales by category or region
#[tool(group_by(description = "Group by field: 'category', 'region', or 'product'"))]
fn get_sales_by_group(group_by: String) -> String {
    println!("  [Tool: get_sales_by_group({})]", group_by);

    let mut totals: HashMap<String, f64> = HashMap::new();
    let mut quantities: HashMap<String, i32> = HashMap::new();

    for record in &get_data_store().records {
        let key = match group_by.as_str() {
            "category" => record.category.clone(),
            "region" => record.region.clone(),
            "product" => record.product.clone(),
            _ => "unknown".to_string(),
        };

        let total = record.quantity as f64 * record.unit_price;
        *totals.entry(key.clone()).or_insert(0.0) += total;
        *quantities.entry(key).or_insert(0) += record.quantity;
    }

    let results: Vec<serde_json::Value> = totals
        .iter()
        .map(|(k, v)| {
            serde_json::json!({
                group_by.clone(): k,
                "total_sales": format!("{:.2}", v),
                "total_quantity": quantities.get(k).unwrap_or(&0)
            })
        })
        .collect();

    serde_json::to_string_pretty(&results).unwrap_or_else(|_| "Error".to_string())
}

/// Filter records by criteria and return matching rows
#[tool(
    field(description = "Field to filter: 'category', 'region', 'product', or 'date'"),
    value(description = "Value to match")
)]
fn filter_records(field: String, value: String) -> String {
    println!("  [Tool: filter_records({}, {})]", field, value);

    let filtered: Vec<&SalesRecord> = get_data_store()
        .records
        .iter()
        .filter(|r| {
            let field_value = match field.as_str() {
                "category" => &r.category,
                "region" => &r.region,
                "product" => &r.product,
                "date" => &r.date,
                _ => return false,
            };
            field_value.to_lowercase().contains(&value.to_lowercase())
        })
        .collect();

    if filtered.is_empty() {
        return format!(
            r#"{{"message": "No records found matching {} = {}"}}"#,
            field, value
        );
    }

    // Calculate summary
    let total_sales: f64 = filtered
        .iter()
        .map(|r| r.quantity as f64 * r.unit_price)
        .sum();
    let total_quantity: i32 = filtered.iter().map(|r| r.quantity).sum();

    serde_json::json!({
        "filter": { field: value },
        "record_count": filtered.len(),
        "total_sales": format!("{:.2}", total_sales),
        "total_quantity": total_quantity,
        "records": filtered
    })
    .to_string()
}

/// Get the top N items by sales volume
#[tool(
    n(description = "Number of top items to return"),
    metric(description = "Metric to rank by: 'total_sales' or 'quantity'")
)]
fn get_top_products(n: i32, metric: String) -> String {
    println!("  [Tool: get_top_products({}, {})]", n, metric);

    let mut product_stats: HashMap<String, (f64, i32)> = HashMap::new();

    for record in &get_data_store().records {
        let entry = product_stats
            .entry(record.product.clone())
            .or_insert((0.0, 0));
        entry.0 += record.quantity as f64 * record.unit_price;
        entry.1 += record.quantity;
    }

    let mut sorted: Vec<(String, f64, i32)> = product_stats
        .into_iter()
        .map(|(product, (sales, qty))| (product, sales, qty))
        .collect();

    match metric.as_str() {
        "quantity" => sorted.sort_by_key(|entry| std::cmp::Reverse(entry.2)),
        _ => sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)),
    }

    let top_n: Vec<serde_json::Value> = sorted
        .into_iter()
        .take(n as usize)
        .enumerate()
        .map(|(i, (product, sales, qty))| {
            serde_json::json!({
                "rank": i + 1,
                "product": product,
                "total_sales": format!("{:.2}", sales),
                "total_quantity": qty
            })
        })
        .collect();

    serde_json::to_string_pretty(&top_n).unwrap_or_else(|_| "Error".to_string())
}

/// Get the data schema (column names and types)
#[tool]
fn get_schema() -> String {
    println!("  [Tool: get_schema()]");

    serde_json::json!({
        "table": "sales_data",
        "row_count": get_data_store().records.len(),
        "columns": [
            {"name": "date", "type": "string", "example": "2024-01-15"},
            {"name": "product", "type": "string", "example": "Laptop Pro"},
            {"name": "category", "type": "string", "example": "Electronics"},
            {"name": "quantity", "type": "integer", "example": 25},
            {"name": "unit_price", "type": "float", "example": 1299.99},
            {"name": "region", "type": "string", "example": "North"}
        ],
        "unique_values": {
            "categories": ["Electronics", "Furniture"],
            "regions": ["North", "South", "East", "West"]
        }
    })
    .to_string()
}

// ============================================================================
// Main Demo
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");
    let client = Client::builder(api_key).build()?;

    println!("=== Data Analysis Example ===\n");
    println!(
        "Loaded {} sales records for analysis\n",
        get_data_store().records.len()
    );

    // Collect function declarations
    let functions = vec![
        GetColumnStatsCallable.declaration(),
        GetSalesByGroupCallable.declaration(),
        FilterRecordsCallable.declaration(),
        GetTopProductsCallable.declaration(),
        GetSchemaCallable.declaration(),
    ];

    // Natural language questions about the data
    let questions = vec![
        "What is the structure of this dataset? What columns are available?",
        "What are the total sales by region?",
        "Which are the top 3 products by total sales?",
        "Show me the sales statistics for the quantity column.",
        "How much revenue did Electronics products generate?",
    ];

    for question in questions {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("❓ Question: {}\n", question);

        let result = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_system_instruction(
                "You are a data analyst assistant. Use the available tools to analyze \
                 the sales data and answer questions. Always provide clear, concise \
                 answers with relevant numbers. Format currency values appropriately. \
                 When showing statistics, explain what the numbers mean.",
            )
            .with_text(question)
            .add_functions(functions.clone())
            .create_with_auto_functions()
            .await?;

        // Show which tools were called
        if !result.executions.is_empty() {
            println!("Tools used:");
            for exec in &result.executions {
                println!("  • {} ({:?})", exec.name, exec.duration);
            }
            println!();
        }

        // Show the analysis result
        if let Some(text) = result.response.as_text() {
            println!("📊 Analysis:\n{}\n", text);
        }
    }

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Data Analysis Demo Complete\n");

    println!("--- Production Considerations ---");
    println!("• Connect to real databases (PostgreSQL, BigQuery, etc.)");
    println!("• Implement SQL query generation for complex analysis");
    println!("• Add visualization generation (charts, graphs)");
    println!("• Support for larger datasets with pagination");
    println!("• Add data validation and error handling");
    println!("• Implement caching for expensive computations");

    Ok(())
}
