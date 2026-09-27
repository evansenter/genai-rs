# Resources

Each `/v1beta` resource is reached through a handle on `Client`:
`client.agents()` returns an `Agents` handle, `client.webhooks()` a
`Webhooks` handle, `client.triggers()` a `Triggers` handle,
`client.environments()` an `Environments` handle, `client.credentials()` a
`Credentials` handle, `client.voices()` a `Voices` handle and
`client.files()` a `Files` handle, whose methods are the resource's verbs. A
nested resource is a plain accessor that binds no ID:
`client.environments().files()` takes the environment ID in each method, as
in Python. Lists return a builder that ends in `.send()` (one page),
`.pages()` or `.items()` (every page). The conventions and the paging
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
| `client.environments.create(sources=, network=)` | `client.environments().create(&CreateEnvironmentRequest::new().add_source(s).with_network(n))` |
| `client.environments.create(from_environment=)` | `client.environments().create(&CreateEnvironmentRequest::from_environment(id))` |
| `client.environments.get(id)` | `client.environments().get(id)` |
| `client.environments.list(page_size=, page_token=)` | `client.environments().list().with_page_size(n).with_page_token(t).send()` |
| (no equivalent) | `client.environments().list().pages()` / `.items()` |
| `client.environments.delete(id)` | `client.environments().delete(id)` |
| `client.environments.files.list(environment, path, recursive=, page_size=, page_token=)` | `client.environments().files().list(environment_id, path).with_recursive(r).with_page_size(n).with_page_token(t).send()` |
| (no equivalent) | `client.environments().files().list(environment_id, path).pages()` / `.items()` |
| `client.environments.files.upload(path=, file=, environment_id=, mime_type=, overwrite=, extract=)` | `client.environments().files().upload(environment_id, path, EnvironmentFileUpload::new(data, mime_type).with_overwrite(o).with_extract(x))` |
| `client.environments.files.download(path=, environment_id=)` | Not modeled yet |
| `client.credentials.create(id=, type_="bearer_token", token=, ...)` | `client.credentials().create(&CreateCredentialRequest::bearer_token(token).with_id(id))` |
| `client.credentials.create(type_=, ...)` (any type) | `client.credentials().create(&CreateCredentialRequest::new(CredentialConfig::...))` |
| `client.credentials.get(id)` | `client.credentials().get(id)` |
| `client.credentials.list(page_size=, page_token=)` | `client.credentials().list().with_page_size(n).with_page_token(t).send()` |
| (no equivalent) | `client.credentials().list().pages()` / `.items()` |
| `client.credentials.update(id, update_mask=, type_=, token=, ...)` | `client.credentials().update(id, &CredentialUpdate { token: Some(t), ..CredentialUpdate::new(type) }.with_update_mask(m))` |
| `client.credentials.delete(id)` | `client.credentials().delete(id)` |
| `client.voices.create(voice={"type": "prompted", ...}, store=True)` | `client.voices().create(&CreateVoiceRequest::prompted(prompt).with_display_name(n))` |
| `client.voices.create(voice={"type": "replicated", ...}, store=)` | `client.voices().create(&CreateVoiceRequest::replicated(source, consent).with_store(s))` |
| `client.voices.get(id)` | `client.voices().get(id)` |
| `client.voices.list(page_size=, page_token=, search=, type_=, gender=, language_code=, region_code=, accent=, persona=, contexts=, pitch=)` | `client.voices().list().with_page_size(n).with_page_token(t).with_search(q).with_voice_type(v).with_gender(g).with_language_code(l).with_region_code(r).with_accent(a).with_persona(p).with_context(c).with_pitch(p).send()` |
| (no equivalent) | `client.voices().list().pages()` / `.items()` |
| `client.voices.delete(id)` | `client.voices().delete(id)` |
| `client.files.upload(file=path, config={"mime_type": m, "display_name": n})` | `client.files().upload(FileUpload::from_path(path).with_mime_type(m).with_display_name(n))` |
| `client.files.upload(file=io_object, config={"mime_type": m})` | `client.files().upload(FileUpload::from_bytes(data, m))` |
| `client.files.upload(config={"name": ...})` (a chosen `files/<id>`) | Not modeled yet: the server assigns the name |
| `client.files.get(name=)` | `client.files().get(name)` |
| `client.files.list(config={"page_size": n, "page_token": t})` (one page) | `client.files().list().with_page_size(n).with_page_token(t).send()` |
| iterating `client.files.list()` | `client.files().list().items()` (or `.pages()`) |
| `client.files.delete(name=)` | `client.files().delete(name)` |
| (no equivalent) | `client.files().wait_until_active(name, PollOptions::new())` |
| `client.files.download(file=)`, `client.files.register_files(uris=)` | Not modeled yet |

Python's `environments.files.upload` also takes a path or a file object as
`file` and guesses a missing `mime_type`; `EnvironmentFileUpload` takes the
bytes and an explicit MIME type.

Python's `files.upload` guesses a missing `mime_type` from a path, as
`FileUpload::from_path` does from the extension, and needs one for a file
object, as `FileUpload::from_bytes` does. A path upload's display name
defaults to the file name.

Python's `voices.list` takes a list of values for each filter, and the API
matches any of them (verified live 2026-09-27). genai-rs sends one value per
filter, and a second `with_*` call replaces the first. Python's `contexts`
argument is the `context` query parameter, set by `with_context`.

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
| `client.create_environment(&request)` | `client.environments().create(&request)` |
| `client.get_environment(id)` | `client.environments().get(id)` |
| `client.list_environments(size, token)` | `client.environments().list().with_page_size(size).with_page_token(token).send()` |
| `client.delete_environment(id)` | `client.environments().delete(id)` |
| `client.upload_file(path)` | `client.files().upload(FileUpload::from_path(path))` |
| `client.upload_file_with_mime(path, mime_type)` | `client.files().upload(FileUpload::from_path(path).with_mime_type(mime_type))` |
| `client.upload_file_chunked(path)` and its `_with_mime` / `_with_options` forms | `client.files().upload(FileUpload::from_path(path))`, which streams from disk; `.with_mime_type(mime_type)` for an explicit type |
| `client.upload_file_bytes(data, mime_type, Some(name))` | `client.files().upload(FileUpload::from_bytes(data, mime_type).with_display_name(name))` |
| `client.upload_file_bytes(data, mime_type, None)` | `client.files().upload(FileUpload::from_bytes(data, mime_type))` |
| `client.get_file(name)` | `client.files().get(name)` |
| `client.list_files(size, token)` | `client.files().list().with_page_size(size).with_page_token(token).send()` |
| `client.delete_file(name)` | `client.files().delete(name)` |
| `client.wait_for_file_ready(&file, poll_interval, timeout)` | `client.files().wait_until_active(&file.name, PollOptions::new().with_poll_interval(poll_interval).with_timeout(timeout))` |

## Methods added after 0.10

These were on `main` after 0.10 but never released; they moved to handles
before their first release.

| Before | Now |
|--------|-----|
| `client.list_environment_files(env, path, recursive, size, token)` | `client.environments().files().list(env, path).with_recursive(recursive).with_page_size(size).with_page_token(token).send()` |
| `client.upload_environment_file(env, path, data, mime_type, EnvironmentFileUpload { overwrite, extract })` | `client.environments().files().upload(env, path, EnvironmentFileUpload::new(data, mime_type).with_overwrite(overwrite).with_extract(extract))` |
| `client.create_credential(&request)` | `client.credentials().create(&request)` |
| `client.get_credential(id)` | `client.credentials().get(id)` |
| `client.list_credentials(size, token)` | `client.credentials().list().with_page_size(size).with_page_token(token).send()` |
| `client.update_credential(id, &update, Some(mask))` | `client.credentials().update(id, &update.with_update_mask(mask))` |
| `client.update_credential(id, &update, None)` | `client.credentials().update(id, &update)` |
| `client.delete_credential(id)` | `client.credentials().delete(id)` |
| `client.create_voice(&request)` | `client.voices().create(&request)` |
| `client.get_voice(id)` | `client.voices().get(id)` |
| `client.list_voices(&ListVoicesParams::new().with_search(q).with_page_size(size))` | `client.voices().list().with_search(q).with_page_size(size).send()` |
| `client.delete_voice(id)` | `client.voices().delete(id)` |
