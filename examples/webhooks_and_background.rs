//! Webhooks, background interactions, environments and triggers.
//!
//! 1. The webhooks resource: create, list, update, ping, rotate the signing
//!    secret, delete.
//! 2. Per-request `webhook_config` on a background interaction, so lifecycle
//!    events (`interaction.completed`, ...) are pushed to your endpoint
//!    instead of polled for.
//! 3. The environments resource: create, list, get, delete.
//! 4. The triggers resource: listing. Creating a trigger requires a custom
//!    agent, which standard API keys cannot create, so only the request body
//!    is shown.
//!
//! Everything the example creates is deleted again, including when a later
//! step fails. Without `GEMINI_API_KEY` it prints the request bodies instead.
//!
//! Run with: `cargo run --example webhooks_and_background`

use genai_rs::{
    Client, CreateEnvironmentRequest, EnvironmentSource, InteractionInput, InteractionRequest,
    TriggerCreateParams, Webhook, WebhookConfig, WebhookEvent, WebhookState, WebhookUpdate,
};
use std::env;
use std::error::Error;

/// Replace with your HTTPS endpoint. Deliveries are signed; verify them with
/// the signing secret returned on create.
const WEBHOOK_URI: &str = "https://example.com/hooks/genai";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty());
    let client = Client::new(api_key.clone().unwrap_or_else(|| "unused".to_string()));

    let webhook = Webhook::new(
        WEBHOOK_URI,
        vec![
            WebhookEvent::InteractionCompleted,
            WebhookEvent::InteractionFailed,
        ],
    )
    .with_name("genai-rs-example");
    let background = client
        .interaction()
        .with_agent(genai_rs::DEFAULT_DEEP_RESEARCH_AGENT)
        .with_text("Research the history of the Antikythera mechanism")
        .with_background(true)
        .with_webhook_config(
            WebhookConfig::new()
                .with_uris(vec![WEBHOOK_URI.to_string()])
                // Echoed on every event, to correlate deliveries with jobs.
                .with_user_metadata(serde_json::json!({"job_id": "research-42"})),
        )
        .build()?;
    let environment = CreateEnvironmentRequest::new().add_source(EnvironmentSource::inline(
        "/etc/motd",
        "hello from genai-rs",
    ));
    let trigger = TriggerCreateParams::new(
        "0 9 * * 1-5",
        "UTC",
        InteractionRequest {
            agent: Some("my-custom-agent".to_string()),
            input: InteractionInput::Text("Daily repo audit".to_string()),
            ..Default::default()
        },
    )
    .with_display_name("weekday-audit");

    if api_key.is_none() {
        println!("GEMINI_API_KEY not set; request bodies only.\n");
        for (label, body) in [
            ("POST /v1beta/webhooks", serde_json::to_value(&webhook)?),
            (
                "POST /v1beta/interactions",
                serde_json::to_value(&background)?,
            ),
            (
                "POST /v1beta/environments",
                serde_json::to_value(&environment)?,
            ),
            ("POST /v1beta/triggers", serde_json::to_value(&trigger)?),
        ] {
            println!("{label}\n{}\n", serde_json::to_string_pretty(&body)?);
        }
        return Ok(());
    }

    webhook_lifecycle(&client, &webhook).await?;
    background_interaction(&client, background).await?;
    environment_lifecycle(&client, &environment).await?;

    let triggers = client.list_triggers(Some(10), None).await?;
    println!(
        "\nTriggers visible to this key: {}",
        triggers.triggers.len()
    );
    println!(
        "A trigger create body (needs a custom agent):\n{}",
        serde_json::to_string_pretty(&trigger)?
    );

    Ok(())
}

async fn webhook_lifecycle(client: &Client, webhook: &Webhook) -> Result<(), Box<dyn Error>> {
    println!("--- Webhook ---");
    let created = client.create_webhook(webhook).await?;
    let id = created
        .id
        .filter(|id| !id.is_empty())
        .ok_or("create_webhook returned no ID")?;
    // The secret is only ever returned here; store it.
    println!(
        "Created {id}; signing secret returned: {}",
        created.new_signing_secret.is_some()
    );

    let result = async {
        let listed = client.list_webhooks(Some(10), None).await?;
        println!("Registered webhooks: {}", listed.webhooks.len());

        let updated = client
            .update_webhook(
                &id,
                &WebhookUpdate::new().with_state(WebhookState::Disabled),
                Some("state"),
            )
            .await?;
        println!("Updated state: {:?}", updated.state);

        client.ping_webhook(&id).await?;
        println!("Ping sent");

        // The previous secret stays valid for a grace period by default.
        let rotated = client.rotate_webhook_signing_secret(&id, None).await?;
        println!("Rotated; new secret returned: {}", rotated.secret.is_some());
        Ok::<_, genai_rs::GenaiError>(())
    }
    .await;

    client.delete_webhook(&id).await?;
    println!("Deleted {id}");
    Ok(result?)
}

async fn background_interaction(
    client: &Client,
    request: InteractionRequest,
) -> Result<(), Box<dyn Error>> {
    println!("\n--- Background interaction with webhook_config ---");
    let response = client.execute(request).await?;
    let id = response.id.ok_or("background interaction has no ID")?;
    println!(
        "Accepted {id} (status {:?}); events go to {WEBHOOK_URI}",
        response.status
    );

    // Don't leave a research agent running on the example's behalf.
    let cancelled = client.cancel_interaction(&id).await?;
    println!("Cancelled: status {:?}", cancelled.status);
    Ok(())
}

async fn environment_lifecycle(
    client: &Client,
    request: &CreateEnvironmentRequest,
) -> Result<(), Box<dyn Error>> {
    println!("\n--- Environment ---");
    let created = client.create_environment(request).await?;
    let id = created
        .id
        .filter(|id| !id.is_empty())
        .ok_or("create_environment returned no ID")?;
    println!("Created {id}");

    let result = async {
        let listed = client.list_environments(Some(10), None).await?;
        println!("Environments visible: {}", listed.environments.len());
        let fetched = client.get_environment(&id).await?;
        println!(
            "Fetched: status {:?}, {:?} file(s), {:?} bytes",
            fetched.status, fetched.file_count, fetched.size_bytes
        );
        Ok::<_, genai_rs::GenaiError>(())
    }
    .await;

    // Environments expire on their own, but repeated runs would pile up.
    client.delete_environment(&id).await?;
    println!("Deleted {id}");
    Ok(result?)
}
