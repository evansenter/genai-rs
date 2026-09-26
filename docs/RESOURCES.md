# Resources

Each `/v1beta` resource is reached through a handle on `Client`:
`client.agents()` returns an `Agents` handle, `client.webhooks()` a
`Webhooks` handle and `client.triggers()` a `Triggers` handle, whose methods
are the resource's verbs. Lists return a builder that ends in `.send()` (one
page), `.pages()` or `.items()` (every page). The conventions and the paging
rules are in
[Resource handles and list builders](BUILDER_API.md#resource-handles-and-list-builders),
and the reasoning in D-016 (`DECISIONS.md`).

Resources not listed below still use `Client` methods; see their module
documentation.

```rust,no_run
use futures_util::TryStreamExt;
use genai_rs::Agent;

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
let agent = client.agents().get("customer-sentinel").await?;
let all: Vec<Agent> = client.agents().list().items().try_collect().await?;
# let _ = (agent, all);
# Ok(())
# }
```

## Python to Rust

The handles follow the resource groups of the Python SDK (`google-genai`).
Required arguments are positional, as in Python; optional list arguments
become `with_*` setters.

| Python (`google-genai` 2.25) | genai-rs |
|------------------------------|----------|
| `client.agents.create(id=..., ...)` | `client.agents().create(&Agent::new(id)...)` |
| `client.agents.get(id)` | `client.agents().get(id)` |
| `client.agents.list(page_size=, page_token=, parent=)` | `client.agents().list().with_page_size(n).with_page_token(t).with_parent(p).send()` |
| (no equivalent) | `client.agents().list().pages()` / `.items()` |
| `client.agents.delete(id)` | `client.agents().delete(id)` |
| `client.webhooks.create(uri=, subscribed_events=, name=)` | `client.webhooks().create(&Webhook::new(uri, events).with_name(n))` |
| `client.webhooks.get(id)` | `client.webhooks().get(id)` |
| `client.webhooks.list(page_size=, page_token=)` | `client.webhooks().list().with_page_size(n).with_page_token(t).send()` |
| (no equivalent) | `client.webhooks().list().pages()` / `.items()` |
| `client.webhooks.update(id, update_mask=, name=, state=, ...)` | `client.webhooks().update(id, &WebhookUpdate::new().with_state(s).with_update_mask(m))` |
| `client.webhooks.delete(id)` | `client.webhooks().delete(id)` |
| `client.webhooks.ping(id)` | `client.webhooks().ping(id)` |
| `client.webhooks.rotate_signing_secret(id, revocation_behavior=)` | `client.webhooks().rotate_signing_secret(id, Some(behavior))` |
| `client.triggers.create(schedule=, time_zone=, interaction=, display_name=, ...)` | `client.triggers().create(&TriggerCreateParams::new(schedule, time_zone, interaction).with_display_name(n))` |
| `client.triggers.get(id)` | `client.triggers().get(id)` |
| `client.triggers.list(page_size=, page_token=)` | `client.triggers().list().with_page_size(n).with_page_token(t).send()` |
| `client.triggers.list(filter_=)` | Not modeled yet: the filter syntax is unverified |
| (no equivalent) | `client.triggers().list().pages()` / `.items()` |
| `client.triggers.update(id, display_name=, status=)` | `client.triggers().update(id, &TriggerUpdate::new().with_status(s))` |
| `client.triggers.delete(id)` | `client.triggers().delete(id)` |
| `client.triggers.run(trigger_id)` | `client.triggers().run(trigger_id)` |
| `client.triggers.list_executions(trigger_id, page_size=, page_token=)` | `client.triggers().list_executions(trigger_id).with_page_size(n).with_page_token(t).send()` |
| (no equivalent) | `client.triggers().list_executions(trigger_id).pages()` / `.items()` |

## From the `Client` methods (0.10)

A `None` positional argument becomes an omitted setter.

| 0.10 | Now |
|------|-----|
| `client.create_agent(&agent)` | `client.agents().create(&agent)` |
| `client.get_agent(id)` | `client.agents().get(id)` |
| `client.list_agents(size, token, parent)` | `client.agents().list().with_page_size(size).with_page_token(token).with_parent(parent).send()` |
| `client.delete_agent(id)` | `client.agents().delete(id)` |
| `client.create_webhook(&webhook)` | `client.webhooks().create(&webhook)` |
| `client.get_webhook(id)` | `client.webhooks().get(id)` |
| `client.list_webhooks(size, token)` | `client.webhooks().list().with_page_size(size).with_page_token(token).send()` |
| `client.update_webhook(id, &update, Some(mask))` | `client.webhooks().update(id, &update.with_update_mask(mask))` |
| `client.update_webhook(id, &update, None)` | `client.webhooks().update(id, &update)` |
| `client.delete_webhook(id)` | `client.webhooks().delete(id)` |
| `client.ping_webhook(id)` | `client.webhooks().ping(id)` |
| `client.rotate_webhook_signing_secret(id, behavior)` | `client.webhooks().rotate_signing_secret(id, behavior)` |
| `client.create_trigger(&params)` | `client.triggers().create(&params)` |
| `client.get_trigger(id)` | `client.triggers().get(id)` |
| `client.list_triggers(size, token)` | `client.triggers().list().with_page_size(size).with_page_token(token).send()` |
| `client.update_trigger(id, &update)` | `client.triggers().update(id, &update)` |
| `client.delete_trigger(id)` | `client.triggers().delete(id)` |
| `client.run_trigger(id)` | `client.triggers().run(id)` |
| `client.list_trigger_executions(id, size, token)` | `client.triggers().list_executions(id).with_page_size(size).with_page_token(token).send()` |
