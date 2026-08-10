//! # Multi-Turn Customer Support Agent (Auto Functions)
//!
//! This example demonstrates a stateful customer support bot using
//! `create_with_auto_functions()` for automatic function execution.
//!
//! ## Features
//!
//! - Maintains conversation context across multiple turns
//! - Uses `create_with_auto_functions()` for seamless tool execution
//! - Provides personalized responses based on customer data
//! - Handles escalation and handoff scenarios
//!
//! ## See Also
//!
//! - `multi_turn_agent_manual` - Same example using manual function calling
//!
//! ## Running
//!
//! ```bash
//! cargo run --example multi_turn_agent_auto
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

// Global database (for tool functions)
fn get_database() -> &'static Database {
    static DATABASE: std::sync::OnceLock<Database> = std::sync::OnceLock::new();
    DATABASE.get_or_init(Database::new)
}

// ============================================================================
// Tool Functions for Customer Support
// ============================================================================

/// Look up customer information by ID or email
#[tool(identifier(description = "Customer ID (e.g., CUST-001) or email address"))]
fn lookup_customer(identifier: String) -> String {
    println!("  [Tool: lookup_customer({})]", identifier);

    // Search by ID or email
    let customer = get_database()
        .customers
        .values()
        .find(|c| c.id == identifier || c.email == identifier);

    match customer {
        Some(c) => serde_json::to_string_pretty(c).unwrap_or_else(|_| "Error".to_string()),
        None => format!(r#"{{"error": "Customer not found: {}"}}"#, identifier),
    }
}

/// Get order details by order ID
#[tool(order_id(description = "Order ID (e.g., ORD-2024-1234)"))]
fn get_order(order_id: String) -> String {
    println!("  [Tool: get_order({})]", order_id);

    match get_database().orders.get(&order_id) {
        Some(order) => serde_json::to_string_pretty(order).unwrap_or_else(|_| "Error".to_string()),
        None => format!(r#"{{"error": "Order not found: {}"}}"#, order_id),
    }
}

/// List all orders for a customer
#[tool(customer_id(description = "Customer ID (e.g., CUST-001)"))]
fn list_customer_orders(customer_id: String) -> String {
    println!("  [Tool: list_customer_orders({})]", customer_id);

    let orders: Vec<&Order> = get_database()
        .orders
        .values()
        .filter(|o| o.customer_id == customer_id)
        .collect();

    serde_json::to_string_pretty(&orders).unwrap_or_else(|_| "Error".to_string())
}

/// Create a support ticket
#[tool(
    customer_id(description = "Customer ID"),
    issue(description = "Description of the issue"),
    priority(description = "Priority level: low, medium, high")
)]
fn create_ticket(customer_id: String, issue: String, priority: String) -> String {
    println!(
        "  [Tool: create_ticket({}, '{}', {})]",
        customer_id,
        issue.chars().take(30).collect::<String>(),
        priority
    );

    // In production, this would create a ticket in your ticketing system
    let ticket_id = format!("TKT-2024-{:04}", rand_simple());

    format!(
        r#"{{"ticket_id": "{}", "status": "created", "customer_id": "{}", "priority": "{}", "message": "Ticket created successfully. A support agent will review your issue within 24 hours."}}"#,
        ticket_id, customer_id, priority
    )
}

/// Initiate a refund for an order
#[tool(
    order_id(description = "Order ID to refund"),
    reason(description = "Reason for the refund")
)]
fn initiate_refund(order_id: String, reason: String) -> String {
    println!("  [Tool: initiate_refund({}, '{}')]", order_id, reason);

    match get_database().orders.get(&order_id) {
        Some(order) => {
            // In production, this would initiate a real refund process
            format!(
                r#"{{"refund_id": "REF-{}", "order_id": "{}", "amount": {}, "status": "pending", "estimated_days": 5, "message": "Refund initiated. Amount will be credited within 5-7 business days."}}"#,
                rand_simple(),
                order_id,
                order.total
            )
        }
        None => format!(r#"{{"error": "Order not found: {}"}}"#, order_id),
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
// Support Agent Implementation
// ============================================================================

/// Conversation session manager
struct SupportSession {
    client: Client,
    customer_id: Option<String>,
    last_interaction_id: Option<String>,
}

impl SupportSession {
    fn new(client: Client) -> Self {
        Self {
            client,
            customer_id: None,
            last_interaction_id: None,
        }
    }

    /// Process a customer message and return the agent's response
    async fn process_message(&mut self, message: &str) -> Result<String, Box<dyn Error>> {
        // Collect function declarations
        let functions = vec![
            LookupCustomerCallable.declaration(),
            GetOrderCallable.declaration(),
            ListCustomerOrdersCallable.declaration(),
            CreateTicketCallable.declaration(),
            InitiateRefundCallable.declaration(),
        ];

        // Build and execute the interaction
        //
        // Inheritance behavior with previousInteractionId:
        // - systemInstruction: NOT inherited (set explicitly on each turn if needed)
        // - tools: NOT inherited (must send on every turn that needs function calling)
        // - conversation history: IS inherited
        //
        // Note: with_system_instruction() is available on all builder states.
        // For create_with_auto_functions(), the SDK reuses the request internally,
        // so system_instruction is automatically present on all internal iterations.
        let result = match &self.last_interaction_id {
            Some(prev_id) => {
                // Subsequent turns: chain to previous, include system instruction
                self.client
                    .interaction()
                    .with_model("gemini-3.6-flash")
                    .with_text(message)
                    .add_functions(functions)
                    .with_store_enabled()
                    .with_previous_interaction(prev_id)
                    .with_system_instruction(self.system_prompt())
                    .create_with_auto_functions()
                    .await?
            }
            None => {
                // First turn: same setup, system instruction included
                self.client
                    .interaction()
                    .with_model("gemini-3.6-flash")
                    .with_text(message)
                    .add_functions(functions)
                    .with_store_enabled()
                    .with_system_instruction(self.system_prompt())
                    .create_with_auto_functions()
                    .await?
            }
        };

        // Update session state
        self.last_interaction_id = result.response.id.clone();

        // Extract and return the text response
        Ok(result
            .response
            .as_text()
            .unwrap_or("I apologize, but I couldn't process that request. Please try again.")
            .to_string())
    }

    /// Generate system prompt for the support agent
    fn system_prompt(&self) -> String {
        let customer_context = match &self.customer_id {
            Some(id) => format!("Current customer: {}", id),
            None => "No customer identified yet.".to_string(),
        };

        format!(
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

{}

Remember to:
- Use the lookup_customer tool to find customer information
- Use get_order or list_customer_orders for order inquiries
- Use create_ticket for issues requiring human follow-up
- Use initiate_refund only after confirming with the customer"#,
            customer_context
        )
    }
}

// ============================================================================
// Main Demo
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");
    let client = Client::builder(api_key).build()?;

    println!("=== Multi-Turn Customer Support Agent (Auto Functions) ===\n");
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

    println!("\n--- Production Considerations ---");
    println!("• Implement proper authentication before accessing customer data");
    println!("• Add rate limiting for function calls");
    println!("• Log all interactions for quality assurance");
    println!("• Implement sentiment analysis for escalation triggers");
    println!("• Add human handoff capability for complex issues");
    println!("• Use conversation summarization for long sessions");

    Ok(())
}
