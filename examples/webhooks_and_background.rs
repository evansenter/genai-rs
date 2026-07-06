//! Example: Webhooks + background execution
//!
//! Demonstrates the webhooks surface of the Interactions API:
//!
//! 1. The `/v1beta/webhooks` resource: create/get/list/update/ping/
//!    rotateSigningSecret/delete
//! 2. Per-request `webhook_config` routing on a background interaction, so
//!    lifecycle events (`interaction.completed`, `interaction.failed`, ...)
//!    are pushed to your endpoint instead of requiring polling
//!
//! Without `GEMINI_API_KEY` the example constructs the requests and prints
//! their wire shapes instead of calling the API, so it can always run.
//!
//! Run with: cargo run --example webhooks_and_background

use genai_rs::{Client, Webhook, WebhookConfig, WebhookEvent, WebhookState, WebhookUpdate};
use std::env;
use std::error::Error;

/// Replace with your HTTPS endpoint. Webhook deliveries are signed; verify
/// them with the signing secret returned on create.
const WEBHOOK_URI: &str = "https://example.com/hooks/genai";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").ok();

    // -------------------------------------------------------------------
    // 1. Resource shapes (always shown)
    // -------------------------------------------------------------------
    let webhook = Webhook::new(
        WEBHOOK_URI,
        vec![
            WebhookEvent::InteractionCompleted,
            WebhookEvent::InteractionFailed,
            WebhookEvent::VideoGenerated,
        ],
    )
    .with_name("example-hook");

    println!("=== Webhook resource (POST /v1beta/webhooks) ===");
    println!("{}\n", serde_json::to_string_pretty(&webhook)?);

    let update = WebhookUpdate::new().with_state(WebhookState::Disabled);
    println!("=== Webhook update (PATCH /v1beta/webhooks/{{id}}?update_mask=state) ===");
    println!("{}\n", serde_json::to_string_pretty(&update)?);

    // -------------------------------------------------------------------
    // 2. Per-request webhook_config on a background interaction
    // -------------------------------------------------------------------
    let client = Client::new(api_key.clone().unwrap_or_else(|| "unused".to_string()));

    let request = client
        .interaction()
        .with_agent("deep-research-preview-04-2026")
        .with_text("Research the history of the Antikythera mechanism")
        .with_background(true)
        .with_store_enabled()
        .with_webhook_config(
            WebhookConfig::new()
                .with_uris(vec![WEBHOOK_URI.to_string()])
                .with_user_metadata(serde_json::json!({"job_id": "research-42"})),
        )
        .build()?;

    println!("=== Background interaction with webhook_config ===");
    println!("{}\n", serde_json::to_string_pretty(&request)?);

    let Some(_) = api_key else {
        println!("GEMINI_API_KEY not set - skipping live API calls.\n");
        print_footer();
        return Ok(());
    };

    // -------------------------------------------------------------------
    // 3. Live: register, inspect, and clean up a webhook
    // -------------------------------------------------------------------
    println!("=== Live webhook lifecycle ===");
    match client.create_webhook(&webhook).await {
        Ok(created) => {
            let id = created.id.clone().unwrap_or_default();
            println!("Created webhook: {id}");
            // Store this secret securely - it is only returned on create.
            println!(
                "Signing secret returned: {}",
                created.new_signing_secret.is_some()
            );

            let list = client.list_webhooks(Some(10), None).await?;
            println!("Registered webhooks: {}", list.webhooks.len());

            // Send a test delivery to the endpoint
            match client.ping_webhook(&id).await {
                Ok(()) => println!("Ping delivered"),
                Err(e) => println!("Ping failed (endpoint unreachable is expected): {e}"),
            }

            // Rotate the signing secret (old secrets valid 24h by default)
            match client.rotate_webhook_signing_secret(&id, None).await {
                Ok(rotated) => println!("Rotated secret: {}", rotated.secret.is_some()),
                Err(e) => println!("Rotate failed: {e}"),
            }

            // Clean up
            client.delete_webhook(&id).await?;
            println!("Deleted webhook {id}");
        }
        Err(e) => println!("Webhook resource not available for this account: {e}"),
    }

    // -------------------------------------------------------------------
    // 4. Live: background interaction with per-request webhook routing
    // -------------------------------------------------------------------
    println!("\n=== Live background interaction ===");
    match client.execute(request).await {
        Ok(response) => {
            println!(
                "Accepted: id={:?} status={:?}",
                response.id, response.status
            );
            println!("Events for this interaction will be pushed to {WEBHOOK_URI}");
            // Cancel so the example doesn't leave a long-running task behind.
            if let Some(id) = &response.id {
                let _ = client.cancel_interaction(id).await;
                println!("Cancelled background interaction (example cleanup)");
            }
        }
        Err(e) => println!("Background interaction failed: {e}"),
    }

    print_footer();
    Ok(())
}

fn print_footer() {
    println!("\n=== Example Complete ===\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST /v1beta/webhooks with uri + subscribed_events");
    println!("  [RES#1] webhook resource incl. new_signing_secret (create only)");
    println!("  [REQ#2] GET /v1beta/webhooks (list), POST :ping, POST :rotateSigningSecret");
    println!("  [RES#2] list/ping/rotate responses");
    println!("  [REQ#3] POST /v1beta/interactions with background + webhook_config");
    println!("  [RES#3] in_progress interaction; completion arrives at your webhook\n");

    println!("--- Production Considerations ---");
    println!("• Store new_signing_secret at create time - it is never returned again");
    println!("• Verify delivery signatures before trusting webhook payloads");
    println!("• Prefer rotate with the default 24h revocation for zero-downtime rollover");
    println!("• The API disables webhooks after repeated delivery failures");
    println!("  (state: disabled_due_to_failed_deliveries) - monitor webhook state");
    println!("• webhook_config overrides registered webhooks per request and echoes");
    println!("  user_metadata on every event - use it to correlate jobs");
}
