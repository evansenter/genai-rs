//! # Multi-Turn Customer Support Agent (Manual Functions)
//!
//! This example demonstrates a stateful customer support bot using
//! manual function calling with `create()` and explicit result handling.
//!
//! ## Features
//!
//! - Maintains conversation context across multiple turns
//! - Uses `create()` with manual function execution loop
//! - Full control over function call handling and result formatting
//! - Demonstrates the low-level function calling pattern
//!
//! ## See Also
//!
//! - `multi_turn_agent_auto` - Same example using `create_with_auto_functions()`
//!
//! ## Running
//!
//! ```bash
//! cargo run --example multi_turn_agent_manual
//! ```
//!
//! ## Prerequisites
//!
//! Set the `GEMINI_API_KEY` environment variable with your API key.

use genai_rs::{Client, FunctionDeclaration, Step};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::env;
use std::error::Error;

// ============================================================================
// Simulated Backend Systems
// ============================================================================

/// Customer data store (simulating a CRM)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Customer {
    id: String,
    name: String,
    email: String,
    tier: String, // "basic", "premium", "enterprise"
    account_balance: f64,
    open_tickets: Vec<String>,
}

/// Order information
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Order {
    id: String,
    customer_id: String,
    status: String,
    items: Vec<String>,
    total: f64,
    created_at: String,
}

/// Simulated database
struct Database {
    customers: HashMap<String, Customer>,
    orders: HashMap<String, Order>,
}

impl Database {
    fn new() -> Self {
        let mut customers = HashMap::new();
        let mut orders = HashMap::new();

        // Sample customer
        customers.insert(
            "CUST-001".to_string(),
            Customer {
                id: "CUST-001".to_string(),
                name: "Alice Johnson".to_string(),
                email: "alice@example.com".to_string(),
                tier: "premium".to_string(),
                account_balance: 150.00,
                open_tickets: vec!["TKT-2024-001".to_string()],
            },
        );

        // Sample orders
        orders.insert(
            "ORD-2024-1234".to_string(),
            Order {
                id: "ORD-2024-1234".to_string(),
                customer_id: "CUST-001".to_string(),
                status: "shipped".to_string(),
                items: vec!["Wireless Headphones".to_string(), "USB-C Cable".to_string()],
                total: 89.99,
                created_at: "2024-12-20".to_string(),
            },
        );

        orders.insert(
            "ORD-2024-1235".to_string(),
            Order {
                id: "ORD-2024-1235".to_string(),
                customer_id: "CUST-001".to_string(),
                status: "processing".to_string(),
                items: vec!["Laptop Stand".to_string()],
                total: 49.99,
                created_at: "2024-12-24".to_string(),
            },
        );

        Self { customers, orders }
    }
}

// Global database
fn get_database() -> &'static Database {
    static DATABASE: std::sync::OnceLock<Database> = std::sync::OnceLock::new();
    DATABASE.get_or_init(Database::new)
}

// ============================================================================
// Manual Function Implementations
// ============================================================================

/// Execute a function by name with the given arguments.
/// Returns the result as a JSON value.
fn execute_function(name: &str, args: &Value) -> Value {
    match name {
        "lookup_customer" => {
            let identifier = args["identifier"].as_str().unwrap_or("");
            println!("  [Tool: lookup_customer({})]", identifier);

            let customer = get_database()
                .customers
                .values()
                .find(|c| c.id == identifier || c.email == identifier);

            match customer {
                Some(c) => json!(c),
                None => json!({"error": format!("Customer not found: {}", identifier)}),
            }
        }
        "get_order" => {
            let order_id = args["order_id"].as_str().unwrap_or("");
            println!("  [Tool: get_order({})]", order_id);

            match get_database().orders.get(order_id) {
                Some(order) => json!(order),
                None => json!({"error": format!("Order not found: {}", order_id)}),
            }
        }
        "list_customer_orders" => {
            let customer_id = args["customer_id"].as_str().unwrap_or("");
            println!("  [Tool: list_customer_orders({})]", customer_id);

            let orders: Vec<&Order> = get_database()
                .orders
                .values()
                .filter(|o| o.customer_id == customer_id)
                .collect();

            json!(orders)
        }
        "create_ticket" => {
            let customer_id = args["customer_id"].as_str().unwrap_or("");
            let issue = args["issue"].as_str().unwrap_or("");
            let priority = args["priority"].as_str().unwrap_or("medium");
            println!(
                "  [Tool: create_ticket({}, '{}...', {})]",
                customer_id,
                issue.chars().take(20).collect::<String>(),
                priority
            );

            let ticket_id = format!("TKT-2024-{:04}", rand_simple());
            json!({
                "ticket_id": ticket_id,
                "status": "created",
                "customer_id": customer_id,
                "priority": priority,
                "message": "Ticket created successfully."
            })
        }
        "initiate_refund" => {
            let order_id = args["order_id"].as_str().unwrap_or("");
            let reason = args["reason"].as_str().unwrap_or("");
            println!("  [Tool: initiate_refund({}, '{}')]", order_id, reason);

            match get_database().orders.get(order_id) {
                Some(order) => json!({
                    "refund_id": format!("REF-{}", rand_simple()),
                    "order_id": order_id,
                    "amount": order.total,
                    "status": "pending",
                    "message": "Refund initiated. Amount will be credited within 5-7 business days."
                }),
                None => json!({"error": format!("Order not found: {}", order_id)}),
            }
        }
        _ => json!({"error": format!("Unknown function: {}", name)}),
    }
}

/// Simple random number for demo purposes
fn rand_simple() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_millis() % 10000) as u32)
        .unwrap_or(1234)
}

// ============================================================================
// Function Declarations (built manually, not via #[tool] macro)
// ============================================================================

fn get_function_declarations() -> Vec<FunctionDeclaration> {
    vec![
        FunctionDeclaration::builder("lookup_customer")
            .description("Look up customer information by ID or email")
            .parameter(
                "identifier",
                json!({
                    "type": "string",
                    "description": "Customer ID (e.g., CUST-001) or email address"
                }),
            )
            .required(vec!["identifier".to_string()])
            .build(),
        FunctionDeclaration::builder("get_order")
            .description("Get order details by order ID")
            .parameter(
                "order_id",
                json!({
                    "type": "string",
                    "description": "Order ID (e.g., ORD-2024-1234)"
                }),
            )
            .required(vec!["order_id".to_string()])
            .build(),
        FunctionDeclaration::builder("list_customer_orders")
            .description("List all orders for a customer")
            .parameter(
                "customer_id",
                json!({
                    "type": "string",
                    "description": "Customer ID (e.g., CUST-001)"
                }),
            )
            .required(vec!["customer_id".to_string()])
            .build(),
        FunctionDeclaration::builder("create_ticket")
            .description("Create a support ticket")
            .parameter(
                "customer_id",
                json!({
                    "type": "string",
                    "description": "Customer ID"
                }),
            )
            .parameter(
                "issue",
                json!({
                    "type": "string",
                    "description": "Description of the issue"
                }),
            )
            .parameter(
                "priority",
                json!({
                    "type": "string",
                    "description": "Priority level: low, medium, high"
                }),
            )
            .required(vec![
                "customer_id".to_string(),
                "issue".to_string(),
                "priority".to_string(),
            ])
            .build(),
        FunctionDeclaration::builder("initiate_refund")
            .description("Initiate a refund for an order")
            .parameter(
                "order_id",
                json!({
                    "type": "string",
                    "description": "Order ID to refund"
                }),
            )
            .parameter(
                "reason",
                json!({
                    "type": "string",
                    "description": "Reason for the refund"
                }),
            )
            .required(vec!["order_id".to_string(), "reason".to_string()])
            .build(),
    ]
}

// ============================================================================
// Support Agent Implementation (Manual Function Calling)
// ============================================================================

/// Conversation session manager with manual function calling
struct SupportSession {
    client: Client,
    last_interaction_id: Option<String>,
    functions: Vec<FunctionDeclaration>,
}

impl SupportSession {
    fn new(client: Client) -> Self {
        Self {
            client,
            last_interaction_id: None,
            functions: get_function_declarations(),
        }
    }

    /// Process a customer message with manual function call handling
    async fn process_message(&mut self, message: &str) -> Result<String, Box<dyn Error>> {
        // Build and execute the initial interaction request
        //
        // Inheritance behavior with previousInteractionId:
        // - systemInstruction: NOT inherited (set explicitly on each turn if needed)
        // - tools: NOT inherited (must send on every turn that needs function calling)
        // - conversation history: IS inherited
        //
        // Note: with_system_instruction() is available on all builder states.
        let mut response = match &self.last_interaction_id {
            Some(prev_id) => {
                // Subsequent turns: chain to previous, include system instruction
                self.client
                    .interaction()
                    .with_model("gemini-3-flash-preview")
                    .with_text(message)
                    .add_functions(self.functions.clone())
                    .with_store_enabled()
                    .with_previous_interaction(prev_id)
                    .with_system_instruction(Self::system_prompt())
                    .create()
                    .await?
            }
            None => {
                // First turn: same setup, system instruction included
                self.client
                    .interaction()
                    .with_model("gemini-3-flash-preview")
                    .with_text(message)
                    .add_functions(self.functions.clone())
                    .with_store_enabled()
                    .with_system_instruction(Self::system_prompt())
                    .create()
                    .await?
            }
        };

        // Manual function calling loop
        const MAX_ITERATIONS: usize = 5;
        for _ in 0..MAX_ITERATIONS {
            let function_calls = response.function_calls();

            // No function calls means we're done
            if function_calls.is_empty() {
                break;
            }

            // Execute each function call and collect result steps
            let mut results = Vec::new();
            for call in &function_calls {
                let result = execute_function(call.name, call.args);
                results.push(Step::function_result(call.name, call.id, result));
            }

            // Send function results back to the model
            //
            // Note: Function result returns do NOT need tools to be re-sent.
            // The model already knows about available tools from the interaction
            // that triggered the function call. Only new user message turns need
            // to include tools (because tools are NOT inherited across turns).
            response = self
                .client
                .interaction()
                .with_model("gemini-3-flash-preview")
                .with_previous_interaction(response.id.as_ref().ok_or("Missing interaction ID")?)
                .with_history(results)
                .with_store_enabled()
                .create()
                .await?;
        }

        // Update session state
        self.last_interaction_id = response.id.clone();

        // Extract and return the text response
        Ok(response
            .as_text()
            .unwrap_or("I apologize, but I couldn't process that request. Please try again.")
            .to_string())
    }

    /// Generate system prompt for the support agent
    fn system_prompt() -> &'static str {
        r#"You are a friendly and professional customer support agent for TechGadgets Inc.

Your responsibilities:
1. Help customers with order inquiries, returns, and refunds
2. Look up customer and order information using the available tools
3. Create support tickets for complex issues
4. Provide accurate, helpful responses based on real data

Guidelines:
- Always verify customer identity before sharing account details
- Be empathetic and solution-oriented
- If you can't resolve an issue, create a support ticket
- For refunds, confirm the order details before proceeding
- Premium and enterprise customers should receive priority attention

Remember to:
- Use the lookup_customer tool to find customer information
- Use get_order or list_customer_orders for order inquiries
- Use create_ticket for issues requiring human follow-up
- Use initiate_refund only after confirming with the customer"#
    }
}

// ============================================================================
// Main Demo
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");
    let client = Client::builder(api_key).build()?;

    println!("=== Multi-Turn Customer Support Agent (Manual Functions) ===\n");
    println!("Simulating a customer support conversation...\n");

    let mut session = SupportSession::new(client);

    // Simulate a multi-turn conversation
    let conversation = [
        "Hi, I'm Alice Johnson and I need help with my recent order.",
        "Can you tell me the status of order ORD-2024-1234?",
        "I received the headphones but they're not working. Can I get a refund?",
        "Yes, please proceed with the refund.",
    ];

    for (turn, message) in conversation.iter().enumerate() {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("👤 Customer (Turn {}): {}\n", turn + 1, message);

        match session.process_message(message).await {
            Ok(response) => {
                println!("🤖 Agent:\n{}\n", response);
            }
            Err(e) => {
                eprintln!("Error processing message: {}", e);
                println!(
                    "🤖 Agent: I apologize, but I'm experiencing technical difficulties. Please try again later.\n"
                );
            }
        }
    }

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Conversation Complete\n");

    // Display session info
    println!("--- Session Summary ---");
    if let Some(id) = &session.last_interaction_id {
        println!("Last interaction ID: {}", id);
    }
    println!("Conversation turns: {}", conversation.len());

    println!("\n--- Manual vs Auto Function Calling ---");
    println!("• This example uses create() + manual loop");
    println!("• See multi_turn_agent_auto for create_with_auto_functions()");
    println!("• Manual gives more control over function execution");
    println!("• Auto is simpler for most use cases");

    Ok(())
}
