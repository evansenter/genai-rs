# Examples Index

This guide provides an overview of all examples in the `genai-rs` repository, organized by category to help you find what you need.

## Running Examples

All examples require the `GEMINI_API_KEY` environment variable:

```bash
export GEMINI_API_KEY=your-api-key
cargo run --example <example_name>
```

For wire-level debugging:
```bash
LOUD_WIRE=1 cargo run --example <example_name>
```

## Quick Reference

| I want to... | Example |
|--------------|---------|
| Make my first API call | `simple_interaction` |
| Stream responses | `streaming` |
| Use function calling | `auto_function_calling` |
| Have multi-turn conversations | `stateful_interaction` |
| Generate images | `image_generation` |
| Convert text to speech | `text_to_speech` |
| Get structured JSON output | `structured_output` |

## Categories

### Getting Started

| Example | Description | Difficulty |
|---------|-------------|------------|
| `simple_interaction` | Basic single request/response | Beginner |
| `streaming` | Stream text as it arrives | Beginner |
| `stateful_interaction` | Multi-turn conversation with context | Beginner |
| `system_instructions` | Set model behavior and persona | Beginner |

### Function Calling

| Example | Description | Difficulty |
|---------|-------------|------------|
| `auto_function_calling` | Auto-discovery and execution with `#[tool]` | Beginner |
| `manual_function_calling` | Full control over execution loop | Intermediate |
| `tool_service` | Dependency injection for stateful tools | Intermediate |
| `parallel_and_compositional_functions` | Parallel execution, chained calls | Advanced |
| `streaming_auto_functions` | Streaming with auto function execution | Intermediate |

### Built-in Tools

| Example | Description | Difficulty |
|---------|-------------|------------|
| `google_search` | Real-time web grounding | Beginner |
| `code_execution` | Run Python in sandbox | Beginner |
| `url_context` | Fetch and analyze web pages | Beginner |
| `google_maps` | Location-grounded responses with place data | Beginner |
| `computer_use` | Browser automation | Advanced |
| `file_search` | Semantic document retrieval | Intermediate |

### Multimodal Input

| Example | Description | Difficulty |
|---------|-------------|------------|
| `multimodal_image` | Analyze images, resolution control | Beginner |
| `audio_input` | Transcribe and analyze audio | Beginner |
| `video_input` | Analyze video content | Beginner |
| `pdf_input` | Process PDF documents | Beginner |
| `text_input` | Analyze text documents (TXT, JSON, CSV) | Beginner |
| `files_api` | Upload files for reuse | Intermediate |

### Output Modalities

| Example | Description | Difficulty |
|---------|-------------|------------|
| `image_generation` | Generate images from text | Beginner |
| `text_to_speech` | Convert text to audio | Beginner |
| `structured_output` | JSON schema enforcement | Intermediate |

### Conversations

| Example | Description | Difficulty |
|---------|-------------|------------|
| `stateful_interaction` | Server-side context with `previous_interaction_id` | Beginner |
| `explicit_turns` | Client-side history with `Step` arrays | Intermediate |
| `response_passthrough` | Feed `response.output_steps()` back as history | Intermediate |
| `thought_echo` | Manual thought handling in multi-turn | Advanced |

### Advanced Features

| Example | Description | Difficulty |
|---------|-------------|------------|
| `thinking` | Chain-of-thought reasoning levels | Intermediate |
| `deep_research` | Long-running research agent | Advanced |
| `cancel_interaction` | Cancel background tasks | Intermediate |
| `antigravity_agent` | Local Antigravity harness agent with Rust tools and policies (`--features antigravity`) | Advanced |

### Real-World Applications

Located in `examples/real_world/`:

| Example | Description | Difficulty |
|---------|-------------|------------|
| `multi_turn_agent_auto/` | Customer support bot (auto functions) | Intermediate |
| `multi_turn_agent_manual/` | Customer support bot (manual functions) | Intermediate |
| `multi_turn_agent_manual_stateless/` | Stateless multi-turn (no server storage) | Advanced |
| `web_scraper_agent/` | Research agent with Google Search | Intermediate |
| `code_assistant/` | Code analysis and generation | Intermediate |
| `testing_assistant/` | Test generation from code | Intermediate |
| `data_analysis/` | CSV data analysis with functions | Intermediate |
| `rag_system/` | Retrieval-augmented generation | Advanced |

## Example Details

### Getting Started Examples

#### simple_interaction
The most basic example - send a prompt, get a response.
```bash
cargo run --example simple_interaction
```
**Learn**: Basic client setup, making requests, accessing responses.

#### streaming
Stream responses token-by-token as they arrive.
```bash
cargo run --example streaming
```
**Learn**: `create_stream()`, handling `StreamChunk` events, real-time output.

#### stateful_interaction
Build multi-turn conversations with automatic context.
```bash
cargo run --example stateful_interaction
```
**Learn**: `with_previous_interaction()`, conversation continuity.

#### system_instructions
Set model behavior, personas, and output formats.
```bash
cargo run --example system_instructions
```
**Learn**: `with_system_instruction()`, persona configuration.

### Function Calling Examples

#### auto_function_calling
Automatic function discovery and execution using the `#[tool]` macro.
```bash
cargo run --example auto_function_calling
```
**Learn**: `#[tool]` macro, `create_with_auto_functions()`, function calling modes.

#### manual_function_calling
Full control over the function execution loop.
```bash
cargo run --example manual_function_calling
```
**Learn**: Manual loop, `Step::function_result()`, custom execution logic.

#### tool_service
Dependency injection for functions that need shared state.
```bash
cargo run --example tool_service
```
**Learn**: `ToolService` trait, `Arc<RwLock<T>>`, runtime configuration.

#### parallel_and_compositional_functions
Handle parallel calls and function chaining.
```bash
cargo run --example parallel_and_compositional_functions
```
**Learn**: Concurrent execution, compositional patterns.

### Built-in Tools Examples

#### google_search
Use real-time web data for grounded responses.
```bash
cargo run --example google_search
```
**Learn**: `with_google_search()`, `google_search_calls()`/`google_search_results()` steps.

#### code_execution
Run Python code in a sandboxed environment.
```bash
cargo run --example code_execution
```
**Learn**: `with_code_execution()`, execution results.

#### url_context
Fetch and analyze web page content.
```bash
cargo run --example url_context
```
**Learn**: `with_url_context()`, web content analysis.

### Multimodal Examples

#### multimodal_image
Send images for analysis with resolution control.
```bash
cargo run --example multimodal_image
```
**Learn**: `with_content()`, `Content::image_data()`, `Resolution`, image comparison.

#### files_api
Upload files once, reference many times.
```bash
cargo run --example files_api
```
**Learn**: `upload_file()`, `wait_for_file_active()`, `delete_file()`.

### Output Examples

#### image_generation
Generate images from text prompts.
```bash
cargo run --example image_generation
```
**Learn**: `with_image_output()`, `first_image_bytes()`, image iteration.
**Note**: Requires `gemini-3-pro-image-preview` model.

#### text_to_speech
Convert text to spoken audio.
```bash
cargo run --example text_to_speech
```
**Learn**: `with_audio_output()`, `with_voice()`, `SpeechConfig`.
**Note**: Requires `gemini-2.5-pro-preview-tts` model.

#### structured_output
Enforce JSON schema on responses.
```bash
cargo run --example structured_output
```
**Learn**: `with_response_format()`, `with_response_mime_type()`.

### Advanced Examples

#### thinking
Expose chain-of-thought reasoning.
```bash
cargo run --example thinking
```
**Learn**: `with_thinking_level()`, `response.has_thoughts()`, `response.thought_signatures()`.

#### deep_research
Long-running research with background execution.
```bash
cargo run --example deep_research
```
**Learn**: `with_agent()`, `with_background()`, polling patterns.

#### cancel_interaction
Cancel in-progress background tasks.
```bash
cargo run --example cancel_interaction
```
**Learn**: `cancel_interaction()`, task lifecycle management.

## Prerequisites by Example

| Example | Special Requirements |
|---------|---------------------|
| `image_generation` | `gemini-3-pro-image-preview` model access |
| `text_to_speech` | `gemini-2.5-pro-preview-tts` model access |
| `deep_research` | Deep Research agent access |
| `computer_use` | Computer Use capability access |
| `file_search` | Pre-configured file search store |
| `google_search` | Google Search grounding access |

## Example Progression

**New to genai-rs?** Follow this path:

1. `simple_interaction` - Basic usage
2. `streaming` - Real-time responses
3. `stateful_interaction` - Multi-turn
4. `auto_function_calling` - Function calling
5. `structured_output` - JSON responses
6. `multimodal_image` - Image input
7. Pick examples matching your use case

**Building a chatbot?**

1. `stateful_interaction` - Basic multi-turn
2. `system_instructions` - Set persona
3. `auto_function_calling` - Add capabilities
4. `real_world/multi_turn_agent_auto/` - Full example

**Building a data pipeline?**

1. `structured_output` - Enforce schemas
2. `files_api` - Handle documents
3. `real_world/data_analysis/` - Full example
