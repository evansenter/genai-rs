# Data Analysis Example

A natural language data analysis assistant using function calling for CSV/tabular data exploration.

## Overview

This example demonstrates a data analyst assistant that:

1. **Schema Discovery**: Understands data structure automatically
2. **Statistical Analysis**: Calculates summary statistics
3. **Aggregation**: Groups and summarizes data by dimensions
4. **Natural Language Queries**: Translates questions to data operations

## Architecture

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  Natural Lang   │────▶│  Gemini Model   │────▶│  Tool Calls     │
│    Question     │     │  (Reasoning)    │     │  (Data Ops)     │
└─────────────────┘     └─────────────────┘     └─────────────────┘
                               │                        │
                               ▼                        ▼
                        ┌─────────────────┐     ┌─────────────────┐
                        │  Interpreted    │◀────│  Data Store     │
                        │    Answer       │     │  (CSV/DB)       │
                        └─────────────────┘     └─────────────────┘
```

## Available Tools

| Tool | Purpose |
|------|---------|
| `get_schema` | Discover table structure and columns |
| `get_column_stats` | Calculate min, max, mean, std_dev |
| `get_sales_by_group` | Aggregate by category, region, product |
| `filter_records` | Query data with filters |
| `get_top_products` | Rank products by sales or quantity |

## Running

```bash
export GEMINI_API_KEY=your_api_key
cargo run --example data_analysis
```

## Sample Interaction

```
❓ Question: What are the total sales by region?
  [Tool: get_sales_by_group(region)]

📊 Analysis:
Here's the sales breakdown by region:

| Region | Total Sales | Total Quantity |
|--------|-------------|----------------|
| North  | $49,247.75  | 355            |
| South  | $32,249.55  | 85             |
| East   | $15,749.60  | 40             |
| West   | $43,799.10  | 90             |

The North region leads in both sales volume and quantity sold.
```

## Data Structure

The example uses simulated sales data:

```rust,ignore
struct SalesRecord {
    date: String,       // "2024-01-15"
    product: String,    // "Laptop Pro"
    category: String,   // "Electronics"
    quantity: i32,      // 25
    unit_price: f64,    // 1299.99
    region: String,     // "North"
}
```

## Tool Implementations

### Statistical Analysis

```rust,ignore
use genai_rs_macros::tool;

#[tool(column(description = "Column: 'quantity' or 'unit_price'"))]
fn get_column_stats(column: String) -> String {
    // Calculate: count, sum, mean, min, max, std_dev
}
```

### Group Aggregation

```rust,ignore
use genai_rs_macros::tool;

#[tool(group_by(description = "Group by: 'category', 'region', 'product'"))]
fn get_sales_by_group(group_by: String) -> String {
    // Sum sales and quantity by group
}
```

## Production Enhancements

### Database Integration

```rust,ignore
// Connect to real databases
async fn execute_query(sql: &str) -> Result<DataFrame, Error> {
    let pool = PgPool::connect(&database_url).await?;
    sqlx::query(sql).fetch_all(&pool).await
}
```

### SQL Generation

```rust,ignore
// Natural language to SQL
async fn nl_to_sql(question: &str) -> String {
    client.interaction()
        .with_text(format!(
            "Convert to SQL for this schema: {}
             Question: {}",
            schema, question
        ))
        .create().await?
}
```

### Visualization

```rust,ignore
// Generate chart specifications
async fn suggest_visualization(data: &DataFrame) -> ChartSpec {
    // Return chart type, axes, and data mappings
}
```

### Large Dataset Handling

```rust,ignore
// Pagination and sampling for big data
struct PaginatedResult {
    data: Vec<Row>,
    total_rows: usize,
    page: usize,
    has_more: bool,
}
```

## Natural Language Capabilities

The assistant can answer questions like:

- "What columns are in this dataset?"
- "Show me sales trends by month"
- "Which products are underperforming?"
- "Compare Electronics vs Furniture sales"
- "What's the average order value by region?"

## Error Handling

The tools handle edge cases gracefully:

- Empty datasets return informative messages
- Invalid column names return available options
- Filters with no matches explain the criteria

## Best Practices

1. **Start with Schema**: Always understand data structure first
2. **Validate Inputs**: Check column names and filter values
3. **Limit Results**: Use top-N and pagination for large results
4. **Format Numbers**: Use appropriate precision for currency/percentages
5. **Explain Context**: Include what the numbers mean, not just values
