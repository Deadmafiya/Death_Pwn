### Task 3: providers — AiProvider trait + Clock + OpenAiClient

**Files:**
- Create: `deathpwn-core/src/providers/mod.rs` (module root + re-exports)
- Create: `deathpwn-core/src/providers/ai.rs` (`ChatRequest`, `ProviderError`, `AiProvider` trait, `FakeAiProvider`)
- Create: `deathpwn-core/src/providers/openai.rs` (`OpenAiClient` + reqwest impl)
- Create: `deathpwn-core/src/clock.rs` (`Clock` trait, `SystemClock`, `FakeClock`)
- Edit: `deathpwn-core/src/lib.rs` (add `pub mod clock;` and `pub mod providers;`)
- Edit: `deathpwn-core/Cargo.toml` (add `async-trait`, `reqwest`, `test-support` feature, dev `tokio`)
- Test: unit tests live in a `#[cfg(test)] mod tests` at the bottom of `ai.rs`, `clock.rs`, and `openai.rs` (Rust convention; manifest specifies no separate test file). The real-HTTP test is an `#[ignore]` `#[tokio::test]` in `openai.rs`.

**Interfaces:**
- Consumes (from Task 1): `enum DeathpwnError` — specifically `DeathpwnError::Provider(String)`; `type Result<T> = std::result::Result<T, DeathpwnError>` (used only by `OpenAiClient::new`).
- Produces (later tasks — Task 4 `FailoverClient`, Task 8 `FeedbackLoop`, Task 9 `Artifacts`, Tasks 11–15 pipeline/engine — rely on these EXACT signatures):
  - `struct ChatRequest { system: String, user: String, temperature: f32 }`
  - `enum ProviderError { Network(String), Timeout, Http { status: u16 }, RateLimit, Decode(String) }`
  - `#[async_trait] trait AiProvider: Send + Sync { async fn complete(&self, req: &ChatRequest) -> std::result::Result<String, ProviderError>; fn label(&self) -> &str; }`
  - `trait Clock: Send + Sync { fn now_ms(&self) -> u64; }`
  - `struct SystemClock;` with `impl Clock for SystemClock`
  - `struct OpenAiClient { base_url, api_key, model, label, http }` with `OpenAiClient::new(base_url, api_key, model, label, http_timeout_secs: u64) -> crate::Result<Self>` and `impl AiProvider for OpenAiClient`
  - test-support (behind `#[cfg(any(test, feature = "test-support"))]`, re-exported): `struct FakeAiProvider` with `new(label, responses: Vec<std::result::Result<String, ProviderError>>)` and `calls() -> usize`; `struct FakeClock` with `new(times: Vec<u64>)` and `fixed(t: u64)`.

---

- [ ] **Step 1: Add Cargo dependencies + test-support feature.** Edit `deathpwn-core/Cargo.toml`. Add to the existing `[dependencies]` (which already has `thiserror`, `serde`, `serde_json` from Tasks 1–2):

```toml
[dependencies]
async-trait = "0.1"
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }

[features]
test-support = []

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt", "rt-multi-thread"] }
```

Rationale: `rustls-tls` avoids a system OpenSSL dependency; `test-support` lets later tasks pull the fakes without `#[cfg(test)]`; `tokio` is dev-only because unit tests use `#[tokio::test]` while the crate itself stays runtime-agnostic (`reqwest` carries its own runtime plumbing).

- [ ] **Step 2: Write the failing test — `AiProvider` trait + `FakeAiProvider`.** Create `deathpwn-core/src/providers/ai.rs` containing ONLY this test module for now (types come in Step 4):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_provider_returns_scripted_results_in_order_and_counts_calls() {
        let fake = FakeAiProvider::new(
            "A",
            vec![Ok("first".to_string()), Err(ProviderError::RateLimit)],
        );
        assert_eq!(fake.label(), "A");

        let req = ChatRequest {
            system: "s".to_string(),
            user: "u".to_string(),
            temperature: 0.0,
        };

        assert_eq!(fake.complete(&req).await, Ok("first".to_string()));
        assert_eq!(fake.complete(&req).await, Err(ProviderError::RateLimit));
        // Exhausted script → deterministic default network error.
        assert!(matches!(
            fake.complete(&req).await,
            Err(ProviderError::Network(_))
        ));
        assert_eq!(fake.calls(), 3);
    }
}
```

- [ ] **Step 3: Run test to verify it fails.** Command: `cargo test -p deathpwn-core fake_provider_returns_scripted_results`. Expected: fails to compile — `error[E0432]`/`cannot find` for `FakeAiProvider`, `ChatRequest`, `ProviderError`, and the `providers` module is not declared in `lib.rs`.

- [ ] **Step 4: Implement `ai.rs` + wire `providers/mod.rs` + `lib.rs`.**

Prepend to `deathpwn-core/src/providers/ai.rs` (above the `mod tests` from Step 2):

```rust
use async_trait::async_trait;

/// A single chat completion request: system + user turns and sampling temperature.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest {
    pub system: String,
    pub user: String,
    pub temperature: f32,
}

/// Expected provider-level failures. Distinct from `DeathpwnError`: the
/// FailoverClient (Task 4) reacts to these, retrying on the fallback provider.
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderError {
    Network(String),
    Timeout,
    Http { status: u16 },
    RateLimit,
    Decode(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderError::Network(m) => write!(f, "network error: {m}"),
            ProviderError::Timeout => write!(f, "request timed out"),
            ProviderError::Http { status } => write!(f, "http status {status}"),
            ProviderError::RateLimit => write!(f, "rate limited"),
            ProviderError::Decode(m) => write!(f, "decode error: {m}"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// The AI completion boundary. `complete` performs one call; `label` names the
/// provider for failover logging/diagnostics.
#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn complete(&self, req: &ChatRequest) -> std::result::Result<String, ProviderError>;
    fn label(&self) -> &str;
}

#[cfg(any(test, feature = "test-support"))]
use std::collections::VecDeque;
#[cfg(any(test, feature = "test-support"))]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(any(test, feature = "test-support"))]
use std::sync::Mutex;

/// Test double for `AiProvider`: replays pre-scripted results in order and
/// counts calls. Shared across tasks via the `test-support` feature.
#[cfg(any(test, feature = "test-support"))]
pub struct FakeAiProvider {
    label: String,
    responses: Mutex<VecDeque<std::result::Result<String, ProviderError>>>,
    calls: AtomicUsize,
}

#[cfg(any(test, feature = "test-support"))]
impl FakeAiProvider {
    pub fn new(
        label: impl Into<String>,
        responses: Vec<std::result::Result<String, ProviderError>>,
    ) -> Self {
        FakeAiProvider {
            label: label.into(),
            responses: Mutex::new(responses.into_iter().collect()),
            calls: AtomicUsize::new(0),
        }
    }

    /// Number of times `complete` has been invoked (used to assert failover /
    /// cache behavior in later tasks).
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[cfg(any(test, feature = "test-support"))]
#[async_trait]
impl AiProvider for FakeAiProvider {
    async fn complete(&self, _req: &ChatRequest) -> std::result::Result<String, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.responses
            .lock()
            .expect("FakeAiProvider responses mutex poisoned")
            .pop_front()
            .unwrap_or_else(|| Err(ProviderError::Network("no scripted response".to_string())))
    }

    fn label(&self) -> &str {
        &self.label
    }
}
```

Create `deathpwn-core/src/providers/mod.rs`:

```rust
pub mod ai;
pub mod openai;

pub use ai::{AiProvider, ChatRequest, ProviderError};
pub use openai::OpenAiClient;

#[cfg(any(test, feature = "test-support"))]
pub use ai::FakeAiProvider;
```

Note: `mod.rs` declares `pub mod openai;` now, so `openai.rs` must exist for the crate to compile. Create it as an empty placeholder file for this step (it is fully implemented in Step 12); an empty `.rs` file is valid Rust and compiles cleanly.

Edit `deathpwn-core/src/lib.rs` — add these module declarations alongside the existing ones (`error`, `config`, `schema`):

```rust
pub mod clock;
pub mod providers;
```

`clock.rs` must also exist for the crate to compile; create it as an empty placeholder now (fully implemented in Step 8).

- [ ] **Step 5: Run test to verify it passes.** Command: `cargo test -p deathpwn-core fake_provider_returns_scripted_results`. Expected: `test providers::ai::tests::fake_provider_returns_scripted_results_in_order_and_counts_calls ... ok` — 1 passed.

- [ ] **Step 6: Commit.** `git add deathpwn-core/Cargo.toml deathpwn-core/src/lib.rs deathpwn-core/src/providers/mod.rs deathpwn-core/src/providers/ai.rs deathpwn-core/src/providers/openai.rs deathpwn-core/src/clock.rs && git commit -m "feat(deathpwn): add AiProvider trait, ProviderError, ChatRequest, and FakeAiProvider"`

- [ ] **Step 7: Write the failing test — `Clock` / `SystemClock` / `FakeClock`.** Replace the empty `deathpwn-core/src/clock.rs` with ONLY this test module for now (types come in Step 9):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_replays_scripted_times_then_holds_last() {
        let clock = FakeClock::new(vec![1_000, 1_200, 1_500]);
        assert_eq!(clock.now_ms(), 1_000);
        assert_eq!(clock.now_ms(), 1_200);
        assert_eq!(clock.now_ms(), 1_500);
        // Exhausted script → repeats the final value (stable for latency math).
        assert_eq!(clock.now_ms(), 1_500);
    }

    #[test]
    fn fake_clock_fixed_always_returns_same_value() {
        let clock = FakeClock::fixed(42);
        assert_eq!(clock.now_ms(), 42);
        assert_eq!(clock.now_ms(), 42);
    }

    #[test]
    fn system_clock_returns_plausible_epoch_millis() {
        let clock = SystemClock;
        // Any real run is well after 2020-01-01T00:00:00Z (1_577_836_800_000 ms).
        assert!(clock.now_ms() > 1_577_836_800_000);
    }
}
```

- [ ] **Step 8: Run test to verify it fails.** Command: `cargo test -p deathpwn-core clock`. Expected: fails to compile — `cannot find type FakeClock`/`SystemClock` in this scope.

- [ ] **Step 9: Implement `clock.rs`.** Prepend to `deathpwn-core/src/clock.rs` (above the `mod tests` from Step 7):

```rust
/// Wall-clock source in milliseconds since the Unix epoch. Injected everywhere
/// timing matters (failover latency, artifact dir names) so tests never touch
/// the real clock.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// Real clock backed by `SystemTime`.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

#[cfg(any(test, feature = "test-support"))]
use std::collections::VecDeque;
#[cfg(any(test, feature = "test-support"))]
use std::sync::Mutex;

/// Test clock: returns scripted timestamps in order, then repeats the last one.
/// Shared across tasks via the `test-support` feature.
#[cfg(any(test, feature = "test-support"))]
pub struct FakeClock {
    times: Mutex<VecDeque<u64>>,
    last: Mutex<u64>,
}

#[cfg(any(test, feature = "test-support"))]
impl FakeClock {
    pub fn new(times: Vec<u64>) -> Self {
        FakeClock {
            times: Mutex::new(times.into_iter().collect()),
            last: Mutex::new(0),
        }
    }

    pub fn fixed(t: u64) -> Self {
        FakeClock::new(vec![t])
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Clock for FakeClock {
    fn now_ms(&self) -> u64 {
        let mut q = self.times.lock().expect("FakeClock times mutex poisoned");
        match q.pop_front() {
            Some(v) => {
                *self.last.lock().expect("FakeClock last mutex poisoned") = v;
                v
            }
            None => *self.last.lock().expect("FakeClock last mutex poisoned"),
        }
    }
}
```

- [ ] **Step 10: Run test to verify it passes.** Command: `cargo test -p deathpwn-core clock`. Expected: 3 passed (`fake_clock_replays_scripted_times_then_holds_last`, `fake_clock_fixed_always_returns_same_value`, `system_clock_returns_plausible_epoch_millis`).

- [ ] **Step 11: Commit.** `git add deathpwn-core/src/clock.rs && git commit -m "feat(deathpwn): add Clock trait with SystemClock and FakeClock"`

- [ ] **Step 12: Write the failing test — `OpenAiClient` body-building + response parsing.** Replace the empty `deathpwn-core/src/providers/openai.rs` with ONLY this test module for now (impl comes in Step 14). These are the deterministic, no-network tests; the real-HTTP test is added in Step 16.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_content_extracts_first_choice_message() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"hello world"}}]}"#;
        assert_eq!(parse_content(body).unwrap(), "hello world");
    }

    #[test]
    fn parse_content_rejects_empty_choices() {
        let body = r#"{"choices":[]}"#;
        assert!(matches!(parse_content(body), Err(ProviderError::Decode(_))));
    }

    #[test]
    fn parse_content_rejects_malformed_json() {
        assert!(matches!(
            parse_content("this is not json"),
            Err(ProviderError::Decode(_))
        ));
    }

    #[test]
    fn build_body_shapes_openai_chat_messages() {
        let client = OpenAiClient::new(
            "https://api.example.com/v1",
            "sk-test",
            "gpt-test",
            "A",
            30,
        )
        .unwrap();
        let req = ChatRequest {
            system: "sys".to_string(),
            user: "usr".to_string(),
            temperature: 0.5,
        };

        let body = client.build_body(&req);

        assert_eq!(body["model"], serde_json::json!("gpt-test"));
        assert_eq!(body["temperature"].as_f64().unwrap(), 0.5);
        assert_eq!(body["messages"][0]["role"], serde_json::json!("system"));
        assert_eq!(body["messages"][0]["content"], serde_json::json!("sys"));
        assert_eq!(body["messages"][1]["role"], serde_json::json!("user"));
        assert_eq!(body["messages"][1]["content"], serde_json::json!("usr"));
    }

    #[test]
    fn label_reflects_configured_label() {
        let client =
            OpenAiClient::new("https://api.example.com/v1", "sk-test", "gpt-test", "B", 30)
                .unwrap();
        assert_eq!(client.label(), "B");
    }
}
```

- [ ] **Step 13: Run test to verify it fails.** Command: `cargo test -p deathpwn-core --lib providers::openai`. Expected: fails to compile — `cannot find function parse_content`, `cannot find type OpenAiClient` in this scope.

- [ ] **Step 14: Implement `openai.rs`.** Prepend to `deathpwn-core/src/providers/openai.rs` (above the `mod tests` from Step 12):

```rust
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use super::ai::{AiProvider, ChatRequest, ProviderError};

/// OpenAI-compatible chat client. POSTs to `{base}/chat/completions` with a
/// bearer key and extracts `choices[0].message.content`.
pub struct OpenAiClient {
    base_url: String,
    api_key: String,
    model: String,
    label: String,
    http: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: String,
}

impl OpenAiClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        label: impl Into<String>,
        http_timeout_secs: u64,
    ) -> crate::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(http_timeout_secs))
            .build()
            .map_err(|e| crate::DeathpwnError::Provider(e.to_string()))?;
        Ok(OpenAiClient {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            label: label.into(),
            http,
        })
    }

    /// Build the OpenAI chat-completions request body. Pure — unit-tested.
    fn build_body(&self, req: &ChatRequest) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "temperature": req.temperature,
            "messages": [
                { "role": "system", "content": req.system },
                { "role": "user", "content": req.user },
            ],
        })
    }
}

/// Parse a chat-completions response body into the first message content. Pure —
/// unit-tested independently of the network.
fn parse_content(body: &str) -> std::result::Result<String, ProviderError> {
    let parsed: ChatCompletionResponse =
        serde_json::from_str(body).map_err(|e| ProviderError::Decode(e.to_string()))?;
    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or_else(|| ProviderError::Decode("no choices in response".to_string()))
}

#[async_trait]
impl AiProvider for OpenAiClient {
    async fn complete(&self, req: &ChatRequest) -> std::result::Result<String, ProviderError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&self.build_body(req))
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ProviderError::Timeout
                } else {
                    ProviderError::Network(e.to_string())
                }
            })?;

        let status = resp.status();
        if status.as_u16() == 429 {
            return Err(ProviderError::RateLimit);
        }
        if !status.is_success() {
            return Err(ProviderError::Http {
                status: status.as_u16(),
            });
        }

        let text = resp
            .text()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;
        parse_content(&text)
    }

    fn label(&self) -> &str {
        &self.label
    }
}
```

- [ ] **Step 15: Run test to verify it passes.** Command: `cargo test -p deathpwn-core --lib providers::openai`. Expected: 5 passed (`parse_content_extracts_first_choice_message`, `parse_content_rejects_empty_choices`, `parse_content_rejects_malformed_json`, `build_body_shapes_openai_chat_messages`, `label_reflects_configured_label`).

- [ ] **Step 16: Write the `#[ignore]` real-HTTP integration test.** Add this second test module to the bottom of `deathpwn-core/src/providers/openai.rs` (after the existing `mod tests`). It hits a live OpenAI-compatible endpoint, so it is ignored by default to keep `cargo test` deterministic:

```rust
#[cfg(test)]
mod integration {
    use super::*;

    #[tokio::test]
    #[ignore = "hits a live OpenAI-compatible endpoint; run with `cargo test -- --ignored`"]
    async fn openai_client_completes_against_live_endpoint() {
        let base = std::env::var("DEATHPWN_PROVIDER_A_URL")
            .expect("set DEATHPWN_PROVIDER_A_URL for the live test");
        let key = std::env::var("DEATHPWN_PROVIDER_A_KEY")
            .expect("set DEATHPWN_PROVIDER_A_KEY for the live test");
        let model = std::env::var("DEATHPWN_PROVIDER_A_MODEL")
            .expect("set DEATHPWN_PROVIDER_A_MODEL for the live test");

        let client = OpenAiClient::new(base, key, model, "A", 30).unwrap();
        let req = ChatRequest {
            system: "You reply with a single lowercase word and nothing else.".to_string(),
            user: "Reply with the word: pong".to_string(),
            temperature: 0.0,
        };

        let out = client
            .complete(&req)
            .await
            .expect("live completion failed");
        assert!(!out.trim().is_empty(), "expected non-empty content");
    }
}
```

- [ ] **Step 17: Run test to verify it compiles and is skipped by default.** Command: `cargo test -p deathpwn-core --lib providers::openai`. Expected: the 5 unit tests pass and the integration test is reported as ignored, e.g. `test providers::openai::integration::openai_client_completes_against_live_endpoint ... ignored`. (Optionally verify manually with real env vars via `cargo test -p deathpwn-core -- --ignored openai_client_completes_against_live_endpoint`.)

- [ ] **Step 18: Full-suite sanity + commit.** Run `cargo test -p deathpwn-core` (all Task 3 tests green, integration ignored) then commit: `git add deathpwn-core/src/providers/openai.rs && git commit -m "feat(deathpwn): add OpenAiClient AiProvider impl with ignored live-endpoint test"`
