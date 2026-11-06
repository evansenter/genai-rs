//! Webhooks: every endpoint's request, the create and list responses, list
//! paging, and signing-secret rotation.

use super::*;

#[tokio::test]
async fn webhook_endpoints_send_the_documented_requests() {
    let stub = ok_stub().await;
    let client = stub.client();
    let c = &client;
    assert_wire(
        &stub,
        vec![
            wire(
                "POST",
                "/v1beta/webhooks",
                Some(json!({
                    "uri": "https://example.com/hook",
                    "subscribed_events": ["interaction.completed", "batch.failed"],
                    "name": "hook"
                })),
                async move {
                    c.webhooks()
                        .create(
                            &Webhook::new(
                                "https://example.com/hook",
                                vec![
                                    WebhookEvent::InteractionCompleted,
                                    WebhookEvent::BatchFailed,
                                ],
                            )
                            .with_name("hook"),
                        )
                        .await
                },
            ),
            wire(
                "GET",
                "/v1beta/webhooks/wh-1",
                None,
                c.webhooks().get("wh-1"),
            ),
            wire("GET", "/v1beta/webhooks", None, c.webhooks().list().send()),
            wire(
                "GET",
                "/v1beta/webhooks?page_size=5&page_token=a%2Fb%3D",
                None,
                c.webhooks()
                    .list()
                    .with_page_size(5)
                    .with_page_token("a/b=")
                    .send(),
            ),
            // The mask rides in the query; the body holds only the fields.
            wire(
                "PATCH",
                "/v1beta/webhooks/wh-1?update_mask=uri%2Cstate",
                Some(json!({"uri": "https://example.com/v2", "state": "disabled"})),
                async move {
                    let update = WebhookUpdate::new()
                        .with_uri("https://example.com/v2")
                        .with_state(WebhookState::Disabled)
                        .with_update_mask("uri,state");
                    c.webhooks().update("wh-1", &update).await
                },
            ),
            wire(
                "PATCH",
                "/v1beta/webhooks/wh-1",
                Some(json!({"subscribed_events": ["video.generated"]})),
                async move {
                    let update = WebhookUpdate::new()
                        .with_subscribed_events(vec![WebhookEvent::VideoGenerated]);
                    c.webhooks().update("wh-1", &update).await
                },
            ),
            wire(
                "DELETE",
                "/v1beta/webhooks/wh-1",
                None,
                c.webhooks().delete("wh-1"),
            ),
            wire(
                "POST",
                "/v1beta/webhooks/wh-1:ping",
                Some(json!({})),
                c.webhooks().ping("wh-1"),
            ),
            wire(
                "POST",
                "/v1beta/webhooks/wh-1:rotateSigningSecret",
                Some(json!({})),
                c.webhooks().rotate_signing_secret("wh-1", None),
            ),
            wire(
                "POST",
                "/v1beta/webhooks/wh-1:rotateSigningSecret",
                Some(json!({"revocation_behavior": "revoke_previous_secrets_immediately"})),
                c.webhooks().rotate_signing_secret(
                    "wh-1",
                    Some(RevocationBehavior::RevokePreviousSecretsImmediately),
                ),
            ),
        ],
    )
    .await;
}

#[cfg(not(genai_strict_unknown))]
#[tokio::test]
async fn webhook_create_response_parses_the_secret_and_preserves_unknowns() {
    let wire = json!({
        "id": "wh-1",
        "name": "hook",
        "uri": "https://example.com/hook",
        "subscribed_events": ["interaction.completed", "interaction.paused"],
        "state": "enabled",
        "new_signing_secret": "whsec_full",
        "signing_secrets": [{"truncated_secret": "whsec_...ab", "expire_time": "2026-09-25T00:00:00Z"}],
        "create_time": "2026-09-24T01:00:00Z",
        "update_time": "2026-09-24T01:00:00Z",
        "delivery_stats": {"failed": 0}
    });
    let stub = Stub::replying(vec![Reply::json(200, wire.clone())]).await;

    let webhook = stub
        .client()
        .webhooks()
        .create(&Webhook::new("https://example.com/hook", vec![]))
        .await
        .unwrap();

    assert_eq!(webhook.id.as_deref(), Some("wh-1"));
    assert_eq!(webhook.state, Some(WebhookState::Enabled));
    assert_eq!(webhook.new_signing_secret.as_deref(), Some("whsec_full"));
    assert_eq!(
        webhook.subscribed_events[0],
        WebhookEvent::InteractionCompleted
    );
    assert_eq!(
        webhook.subscribed_events[1].unknown_event_type(),
        Some("interaction.paused")
    );
    let secrets = webhook.signing_secrets.as_deref().unwrap();
    assert_eq!(secrets[0].truncated_secret.as_deref(), Some("whsec_...ab"));
    assert!(secrets[0].expire_time.is_some());
    assert!(webhook.create_time.is_some() && webhook.update_time.is_some());
    assert_eq!(webhook.extra["delivery_stats"], json!({"failed": 0}));

    // Unknown events and unmodeled fields serialize back as received.
    let back = serde_json::to_value(&webhook).unwrap();
    assert_eq!(back["subscribed_events"], wire["subscribed_events"]);
    assert_eq!(back["delivery_stats"], wire["delivery_stats"]);
}

#[cfg(not(genai_strict_unknown))]
#[tokio::test]
async fn webhook_list_keeps_the_page_token_and_drops_only_undeserializable_entries() {
    let stub = Stub::replying(vec![Reply::json(
        200,
        json!({
            "webhooks": [
                {"id": "wh-1", "uri": "https://a.example", "subscribed_events": [], "state": "paused_by_admin"},
                {"id": "wh-2", "uri": 42},
                {"id": "wh-3", "uri": "https://c.example", "state": "disabled_due_to_failed_deliveries"}
            ],
            "next_page_token": "page-2"
        }),
    )])
    .await;

    let list = stub
        .client()
        .webhooks()
        .list()
        .with_page_size(3)
        .send()
        .await
        .unwrap();

    let ids: Vec<_> = list
        .webhooks
        .iter()
        .filter_map(|w| w.id.as_deref())
        .collect();
    assert_eq!(ids, ["wh-1", "wh-3"]);
    assert_eq!(
        list.webhooks[0]
            .state
            .as_ref()
            .unwrap()
            .unknown_state_type(),
        Some("paused_by_admin")
    );
    assert_eq!(
        list.webhooks[1].state,
        Some(WebhookState::DisabledDueToFailedDeliveries)
    );
    assert_eq!(list.next_page_token.as_deref(), Some("page-2"));
}

#[tokio::test]
async fn webhook_list_items_resend_raw_byte_page_tokens_percent_encoded() {
    // Live webhook page tokens are raw bytes, control characters included
    // (observed 2026-09-26: "rC\n\x19B\x17<name>\n&B$<id>"). The next
    // request must carry them percent-encoded byte for byte, or the server
    // restarts the list.
    let token = "rC\n\u{19}B\u{17}hook-1\n&B$wh-1";
    let stub = Stub::replying(vec![
        Reply::json(
            200,
            json!({
                "webhooks": [{"id": "wh-1", "uri": "https://a.example"}],
                "next_page_token": token
            }),
        ),
        Reply::json(
            200,
            json!({"webhooks": [{"id": "wh-2", "uri": "https://b.example"}]}),
        ),
    ])
    .await;
    let client = stub.client();

    let webhooks: Vec<Webhook> = client
        .webhooks()
        .list()
        .with_page_size(1)
        .items()
        .try_collect()
        .await
        .unwrap();

    let ids: Vec<_> = webhooks.iter().filter_map(|w| w.id.as_deref()).collect();
    assert_eq!(ids, ["wh-1", "wh-2"]);
    assert_eq!(
        targets(&stub),
        [
            "/v1beta/webhooks?page_size=1",
            "/v1beta/webhooks?page_size=1&page_token=rC%0A%19B%17hook-1%0A%26B%24wh-1",
        ]
    );
}

#[cfg(not(genai_strict_unknown))]
#[tokio::test]
async fn rotate_signing_secret_sends_unknown_behaviors_verbatim_and_returns_the_secret() {
    let stub = Stub::replying(vec![Reply::json(200, json!({"secret": "whsec_rotated"}))]).await;
    let behavior: RevocationBehavior =
        serde_json::from_value(json!("revoke_previous_secrets_after_week")).unwrap();
    assert!(behavior.is_unknown());

    let rotated = stub
        .client()
        .webhooks()
        .rotate_signing_secret("wh-1", Some(behavior))
        .await
        .unwrap();

    assert_eq!(rotated.secret.as_deref(), Some("whsec_rotated"));
    let [request] = stub.requests().try_into().unwrap();
    assert_eq!(
        request.json(),
        json!({"revocation_behavior": "revoke_previous_secrets_after_week"})
    );
}
