//! Example: Webhooks + background execution
//!
//! Demonstrates the webhooks, environments and triggers surfaces of the
//! Interactions API:
//!
//! 1. The `/v1beta/webhooks` resource: create/get/list/update/ping/
//!    rotateSigningSecret/delete
//! 2. Per-request `webhook_config` routing on a background interaction, so
//!    lifecycle events (`interaction.completed`, `interaction.failed`, ...)
//!    are pushed to your endpoint instead of requiring polling
//! 3. The `/v1beta/environments` resource: create/get/list/delete lifecycle
//! 4. The `/v1beta/triggers` resource: listing (creation is agent-gated)
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

    // Environments/triggers wire shapes (the live sections below reuse
    // this request) — printed here so a keyless run still shows them.
    let env_request = genai_rs::CreateEnvironmentRequest::new().add_source(
        genai_rs::EnvironmentSource::inline("/etc/motd", "hello from the environments example"),
    );
    println!("=== Environment create (POST /v1beta/environments) ===");
    println!("{}\n", serde_json::to_string_pretty(&env_request)?);
    println!("=== Triggers list (GET /v1beta/triggers) - no body ===\n");

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

            match client.list_webhooks(Some(10), None).await {
                Ok(list) => println!("Registered webhooks: {}", list.webhooks.len()),
                Err(e) => println!("list_webhooks failed: {e}"),
            }

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
            match client.delete_webhook(&id).await {
                Ok(()) => println!("Deleted webhook {id}"),
                Err(e) => println!("delete_webhook failed: {e} - delete {id} manually"),
            }
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

    // =========================================================================
    // Environments: create once, reference from many interactions
    // =========================================================================
    // Verified live 2026-08-08: the full lifecycle works on a standard key.
    println!("\n=== Environments CRUD ===");
    // Print-don't-propagate throughout, like the webhook sections above: an
    // account-gating or transient error must not exit main — the footer (and
    // its delete-what-you-create warning) still has to print.
    match client.create_environment(&env_request).await {
        // A create response without an ID would be a protocol violation;
        // don't paper over it with an empty string (that would turn the
        // get/delete below into requests against the collection URL).
        Ok(genai_rs::Environment {
            id: Some(env_id), ..
        }) => {
            println!("Created environment: {env_id}");

            // A failed read must not skip the delete below and leak the
            // container (see the footer note).
            match client.list_environments(Some(10), None).await {
                Ok(listed) => println!("Environments visible: {}", listed.environments.len()),
                Err(e) => println!("list_environments failed: {e}"),
            }
            match client.get_environment(&env_id).await {
                Ok(fetched) => println!(
                    "Fetched: status={:?} files={:?} bytes={:?}",
                    fetched.status, fetched.file_count, fetched.size_bytes
                ),
                Err(e) => println!("get_environment failed: {e}"),
            }

            match client.delete_environment(&env_id).await {
                Ok(()) => println!("Deleted environment {env_id}"),
                Err(e) => println!(
                    "delete_environment failed: {e} - delete {env_id} manually \
                     so containers don't accumulate"
                ),
            }
        }
        Ok(_) => println!("create_environment returned no ID (protocol violation) - skipping"),
        Err(e) => println!("create_environment failed (tolerated): {e}"),
    }

    // =========================================================================
    // Triggers: server-side scheduled interactions
    // =========================================================================
    // Listing works on any key. Creating a trigger requires its interaction
    // to target a custom agent (an /v1beta/agents resource), and custom-agent
    // creation is gated/allowlisted on standard API keys — so this example
    // only lists. See genai_rs::triggers for the create/run/update surface.
    match client.list_triggers(Some(10), None).await {
        Ok(triggers) => println!("\nTriggers visible: {}", triggers.triggers.len()),
        Err(e) => println!("\nlist_triggers failed (tolerated, gated surface): {e}"),
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
    println!("  [RES#3] in_progress interaction; completion arrives at your webhook");
    println!("  [REQ#4] POST /v1beta/environments, then GET (list + by id), DELETE");
    println!("  [RES#4] environment resource: status, string-encoded file_count/size_bytes");
    println!("  [REQ#5] GET /v1beta/triggers");
    println!("  [RES#5] trigger list ({{}} when none exist)\n");

    println!("--- Production Considerations ---");
    println!("• Store new_signing_secret at create time - it is never returned again");
    println!("• Verify delivery signatures before trusting webhook payloads");
    println!("• Prefer rotate with the default 24h revocation for zero-downtime rollover");
    println!("• The API disables webhooks after repeated delivery failures");
    println!("  (state: disabled_due_to_failed_deliveries) - monitor webhook state");
    println!("• webhook_config overrides registered webhooks per request and echoes");
    println!("  user_metadata on every event - use it to correlate jobs");
    println!("• Environments expire on their own, but delete what you create -");
    println!("  repeated runs otherwise accumulate containers until expiry");
    println!("• Triggers fire with no client process running; pause via");
    println!("  update_trigger(status: paused) and audit via list_trigger_executions");
}
