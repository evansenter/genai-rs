# Security Policy

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

If you discover a security vulnerability in genai-rs, please report it privately using one of these methods:

1. **GitHub Private Vulnerability Reporting**: Use the "Report a vulnerability" button in the Security tab
2. **Email**: Contact the maintainers directly

We take security issues seriously and will respond within 48 hours. Please include:
- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Any suggested fixes (optional)

## Security Design

### API Key Handling

- **Redacted Debug Output**: `Client` and `ClientBuilder` implement custom `Debug` traits that display `[REDACTED]` instead of the actual API key. This prevents accidental exposure in logs, error messages, or debug output.

- **No Key Logging**: The library never logs API keys. Request logging only includes the request body, not the URL (which contains the key as a query parameter).

- **Error Messages**: Error messages from the API do not include the API key. Error bodies are truncated to prevent log flooding.

### Dependency Security

- **cargo-audit**: The CI pipeline runs `cargo-audit` on every PR to check for known vulnerabilities in dependencies.

- **Minimal Dependencies**: The library uses a minimal set of well-maintained dependencies.

### Input Validation

- **Function Arguments**: User-defined functions are responsible for validating their own arguments. The library passes `serde_json::Value` from the model to the function without modification.

- **User Prompts**: Text prompts are passed directly to the API. Since they're not used in SQL, shell commands, or HTML contexts, no sanitization is required.

### Secure Defaults

- **HTTPS Only**: All API communication uses HTTPS (enforced by the base URL).

- **rustls-tls**: The library uses `rustls` instead of native TLS for consistent, memory-safe TLS implementation.

## Best Practices for Users

### API Key Management

```rust
// Good: Load API key from environment variable
let api_key = std::env::var("GEMINI_API_KEY")
    .expect("GEMINI_API_KEY must be set");
let client = Client::new(api_key);

// Bad: Hardcoding API keys
let client = Client::new("AIza...".to_string()); // Never do this!
```

### Avoid Exposing the Client in Logs

The `Client` struct's `Debug` implementation redacts the API key, but avoid logging the client unnecessarily:

```rust
let client = Client::new(api_key);

// Safe: API key is redacted
println!("{:?}", client);
// Output: Client { api_key: "[REDACTED]", http_client: ... }

// But better: Don't log at all unless needed for debugging
```

### HTTP Client Logging

**Warning**: If you enable verbose HTTP client logging (e.g., via `RUST_LOG=reqwest=debug`), API keys may be exposed in URL query parameters. Avoid verbose HTTP logging in production or ensure logs are properly secured.

### Function Calling Security

When implementing callable functions, validate all arguments:

```rust,ignore
use genai_rs::CallableFunction;
use genai_rs_macros::tool;

#[tool(city(description = "The city name"))]
fn get_weather(city: String) -> String {
    // Validate input length to prevent abuse
    if city.len() > 100 {
        return r#"{"error": "City name too long"}"#.to_string();
    }

    // Safe to use after validation
    fetch_weather_data(&city)
}
```

### Error Handling

Handle errors without exposing sensitive information:

```rust
match client.interaction().create().await {
    Ok(response) => { /* handle success */ }
    Err(GenaiError::Api { status_code, message, request_id }) => {
        // Log status and request_id (safe), but be careful with message
        log::error!("API error {}: request_id={:?}", status_code, request_id);
        // The message is already sanitized by the library
    }
    Err(e) => {
        log::error!("Error: {}", e);
    }
}
```

## Security Audit Checklist

The following areas were reviewed as part of the security audit:

| Area | Status | Notes |
|------|--------|-------|
| API Key Handling | ✅ Pass | Custom Debug impl redacts keys |
| Dependency Vulnerabilities | ✅ Pass | `cargo audit` finds no issues |
| Error Message Leakage | ✅ Pass | Error bodies truncated, no key exposure |
| Input Validation | ✅ Pass | Appropriate for library design |
| HTTPS Enforcement | ✅ Pass | Base URL uses HTTPS |
| TLS Implementation | ✅ Pass | Uses rustls (memory-safe) |

## CI Security Checks

The following security-related checks run on every pull request:

1. **cargo-audit**: Checks for known vulnerabilities in dependencies
2. **clippy**: Catches common bugs and security anti-patterns
3. **cargo check**: Ensures code compiles without errors
