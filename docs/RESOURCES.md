# Resources

Each `/v1beta` resource is reached through a handle on `Client`:
`client.agents()` returns an `Agents` handle whose methods are the
resource's verbs. Lists return a builder that ends in `.send()` (one page),
`.pages()` or `.items()` (every page). The conventions and the paging rules
are in [Resource handles and list builders](BUILDER_API.md#resource-handles-and-list-builders),
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

## From the `Client` methods (0.10)

A `None` positional argument becomes an omitted setter.

| 0.10 | Now |
|------|-----|
| `client.create_agent(&agent)` | `client.agents().create(&agent)` |
| `client.get_agent(id)` | `client.agents().get(id)` |
| `client.list_agents(size, token, parent)` | `client.agents().list().with_page_size(size).with_page_token(token).with_parent(parent).send()` |
| `client.delete_agent(id)` | `client.agents().delete(id)` |
