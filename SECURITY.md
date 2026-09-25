# Security Policy

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Report privately through GitHub's **Private Vulnerability Reporting** (the
"Report a vulnerability" button in the repository's Security tab), or contact
the maintainers directly. We aim to respond within 48 hours. Please include:

- A description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fixes (optional)

## How the library handles secrets

- **The API key is sent in the `X-Goog-Api-Key` request header**, never in
  the URL, so it doesn't end up in URLs or proxy access logs.
- **`Client` and `ClientBuilder` redact the key in `Debug` output**
  (`api_key: "[REDACTED]"`).
- **The built-in wire inspectors redact secrets.** `LoudWirePrinter` (what
  `LOUD_WIRE=1` installs) and `TracingForwarder` redact secret fields, for
  example third-party retrieval `api_key`s in tool configs, and truncate
  base64 payloads. A custom `WireInspector` receives the raw events, so it
  owns what it writes out (see
  [docs/LOGGING_STRATEGY.md](docs/LOGGING_STRATEGY.md)).
- **Request and response bodies are logged only at `debug`.** They include
  prompts, base64 media and function arguments, so treat debug logs as
  sensitive.
- **`GenaiError::Api.message` is a truncated copy of the error response
  body.** It doesn't contain your key, but it may echo parts of your request.

## Transport

- All API traffic uses HTTPS.
- TLS is provided by `rustls` (reqwest's `rustls` feature), verifying against
  the OS trust store. Minimal containers need a CA bundle installed.

## Best practices for users

Load the key from the environment or a secrets manager; never hardcode it:

```rust,no_run
use genai_rs::Client;

let api_key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
let client = Client::new(api_key);
# let _ = client;
```

When logging errors, the status code and `request_id` are always safe. Treat
`message` as containing request data:

```rust
use genai_rs::GenaiError;

fn log_error(err: &GenaiError) {
    match err {
        GenaiError::Api { status_code, request_id, .. } => {
            eprintln!("API error {status_code} (request_id={request_id:?})");
        }
        other => eprintln!("error: {other}"),
    }
}
# let _ = log_error;
```

**Validate function arguments.** Arguments to your `#[tool]` and
`CallableFunction` implementations come from the model. The library passes
them through unchanged. Bound their size, and never pass them unchecked to a
shell, SQL or the filesystem.

**Treat model output as untrusted input** wherever you render or execute it.

## Dependency checks in CI

- `cargo audit` runs in the `Security Audit` workflow (`audit.yml`) when
  `Cargo.toml` or `Cargo.lock` changes, and weekly. A failing scheduled run
  opens a tracking issue.
- `clippy` with `-D warnings` runs on every pull request.
