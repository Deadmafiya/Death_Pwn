### Task 4: providers — FailoverClient

**Files:**
- Create: `deathpwn-core/src/providers/failover.rs` (core crate; the `FailoverClient` struct, `complete_validated`, and its `#[cfg(test)] mod tests`).
- Edit: `deathpwn-core/src/providers/mod.rs` (core crate; register `pub mod failover;` and re-export `pub use failover::FailoverClient;`).
- Edit: `deathpwn-core/Cargo.toml` (core crate; add the `tracing` dependency).
- Test: unit tests live in a `#[cfg(test)] mod tests` inside `deathpwn-core/src/providers/failover.rs` (Rust convention; the manifest does not ask for a separate test file).

**Interfaces:**

- Consumes (from Task 1 — `error.rs`):
  - `enum DeathpwnError` with variant `Provider(String)` (used for the aggregated failure).
  - `type Result<T> = std::result::Result<T, DeathpwnError>;`
- Consumes (from Task 3 — `providers/ai.rs`, `clock.rs`):
  - `struct ChatRequest { system: String, user: String, temperature: f32 }`
  - `enum ProviderError { Network(String), Timeout, Http { status: u16 }, RateLimit, Decode(String) }` (derives `Debug`).
  - `#[async_trait] trait AiProvider: Send + Sync { async fn complete(&self, req: &ChatRequest) -> std::result::Result<String, ProviderError>; fn label(&self) -> &str; }`
  - `trait Clock: Send + Sync { fn now_ms(&self) -> u64; }`
  - test-support (re-exported for other tasks): `struct FakeAiProvider` and `struct FakeClock`. This task relies on the following fake API (authored in Task 3):
    - `FakeAiProvider::new(label: &str, responses: Vec<std::result::Result<String, ProviderError>>) -> FakeAiProvider` — each `complete` call returns the next scripted result in FIFO order; `label()` returns the given label.
    - `FakeClock::new(start_ms: u64) -> FakeClock` — implements `Clock`, returning `start_ms` from `now_ms()`.
    - Assumed re-export paths: `crate::providers::{AiProvider, ChatRequest, ProviderError, FakeAiProvider}` and `crate::clock::{Clock, FakeClock}`.
- Produces (later tasks 11–15 rely on these EXACT signatures):
  - `struct FailoverClient { a: Arc<dyn AiProvider>, b: Arc<dyn AiProvider>, clock: Arc<dyn Clock> }`
  - `impl FailoverClient { pub fn new(a: Arc<dyn AiProvider>, b: Arc<dyn AiProvider>, clock: Arc<dyn Clock>) -> Self }`
  - `impl FailoverClient { pub async fn complete_validated<T, F>(&self, req: &ChatRequest, validate: F) -> Result<T> where F: Fn(&str) -> std::result::Result<T, String> }`
    - Behavior: call A → `validate`; on A **provider error** OR **validation failure** → call B (same request) → `validate`; if both fail → `Err(DeathpwnError::Provider(<aggregated message naming both providers>))`. Each attempt logs label + latency (via injected `Clock`) + outcome. No circuit breaker in v1.

---

**Step 1: Add the `tracing` dependency to `deathpwn-core/Cargo.toml`.**

`FailoverClient` logs each provider attempt. Add `tracing` to the core crate's `[dependencies]`. `serde`, `serde_json`, `async-trait`, and `tokio` (dev) are already present from Tasks 1–3; only `tracing` is new here.

```toml
[dependencies]
tracing = "0.1"
```

- [ ] **Step 1: Commit the dependency change.**

```bash
git add deathpwn-core/Cargo.toml
git commit -m "build(deathpwn): add tracing dep for failover attempt logging"
```

---

#### Cycle 1 — Provider A succeeds

- [ ] **Step 2: Write the failing test.** Create `deathpwn-core/src/providers/failover.rs` with only the test module (no implementation yet), and register the module in `providers/mod.rs`.

Add to `deathpwn-core/src/providers/mod.rs`:

```rust
pub mod failover;
```

Create `deathpwn-core/src/providers/failover.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::clock::FakeClock;
    use crate::providers::failover::FailoverClient;
    use crate::providers::{ChatRequest, FakeAiProvider};

    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct Probe {
        n: i64,
    }

    fn parse(s: &str) -> std::result::Result<Probe, String> {
        serde_json::from_str::<Probe>(s).map_err(|e| e.to_string())
    }

    fn req() -> ChatRequest {
        ChatRequest {
            system: "sys".to_string(),
            user: "usr".to_string(),
            temperature: 0.0,
        }
    }

    #[tokio::test]
    async fn provider_a_ok_returns_validated_value() {
        let a = Arc::new(FakeAiProvider::new(
            "A",
            vec![Ok(r#"{"n":1}"#.to_string())],
        ));
        let b = Arc::new(FakeAiProvider::new(
            "B",
            vec![Ok(r#"{"n":2}"#.to_string())],
        ));
        let clock = Arc::new(FakeClock::new(0));
        let client = FailoverClient::new(a, b, clock);

        let out = client
            .complete_validated(&req(), parse)
            .await
            .expect("A succeeds and validates");

        assert_eq!(out, Probe { n: 1 });
    }
}
```

- [ ] **Step 2: Run test to verify it fails.**

```bash
cargo test -p deathpwn-core failover
```

Expected: fails to compile — `cannot find type FailoverClient in this scope` / `unresolved import crate::providers::failover::FailoverClient`.

- [ ] **Step 2: Implement (A-only path).** Prepend the implementation above the `mod tests` block in `deathpwn-core/src/providers/failover.rs`:

```rust
use std::sync::Arc;

use crate::clock::Clock;
use crate::error::{DeathpwnError, Result};
use crate::providers::ai::{AiProvider, ChatRequest};

/// Two-provider failover in front of any pair of `AiProvider`s.
/// Circuit breaker is intentionally omitted in v1.
pub struct FailoverClient {
    a: Arc<dyn AiProvider>,
    b: Arc<dyn AiProvider>,
    clock: Arc<dyn Clock>,
}

impl FailoverClient {
    pub fn new(a: Arc<dyn AiProvider>, b: Arc<dyn AiProvider>, clock: Arc<dyn Clock>) -> Self {
        Self { a, b, clock }
    }

    pub async fn complete_validated<T, F>(&self, req: &ChatRequest, validate: F) -> Result<T>
    where
        F: Fn(&str) -> std::result::Result<T, String>,
    {
        let label = self.a.label().to_string();
        let start = self.clock.now_ms();
        let content = self
            .a
            .complete(req)
            .await
            .map_err(|e| DeathpwnError::Provider(format!("{label}: provider error: {e:?}")))?;
        let latency_ms = self.clock.now_ms().saturating_sub(start);
        tracing::info!(provider = %label, latency_ms, outcome = "ok", "provider call succeeded");
        validate(&content)
            .map_err(|e| DeathpwnError::Provider(format!("{label}: validation failed: {e}")))
    }
}
```

Note: fields `b` and `clock` are stored but `b` is not read yet; expect a transient `field is never read` warning until Cycle 2. It compiles.

- [ ] **Step 2: Run test to verify it passes.**

```bash
cargo test -p deathpwn-core failover
```

Expected: `provider_a_ok_returns_validated_value` PASSES (1 passed).

- [ ] **Step 2: Commit.**

```bash
git add deathpwn-core/src/providers/failover.rs deathpwn-core/src/providers/mod.rs
git commit -m "feat(deathpwn): FailoverClient returns validated primary-provider response"
```

---

#### Cycle 2 — Provider A errors, B succeeds

- [ ] **Step 3: Write the failing test.** Add a second test fn inside the existing `mod tests`:

```rust
    #[tokio::test]
    async fn provider_a_error_falls_back_to_b() {
        let a = Arc::new(FakeAiProvider::new(
            "A",
            vec![Err(crate::providers::ProviderError::Timeout)],
        ));
        let b = Arc::new(FakeAiProvider::new(
            "B",
            vec![Ok(r#"{"n":7}"#.to_string())],
        ));
        let clock = Arc::new(FakeClock::new(0));
        let client = FailoverClient::new(a, b, clock);

        let out = client
            .complete_validated(&req(), parse)
            .await
            .expect("A errors, B succeeds and validates");

        assert_eq!(out, Probe { n: 7 });
    }
```

- [ ] **Step 3: Run test to verify it fails.**

```bash
cargo test -p deathpwn-core failover
```

Expected: `provider_a_error_falls_back_to_b` FAILS — the A-only implementation returns `Err(DeathpwnError::Provider("A: provider error: Timeout"))`, so `.expect(...)` panics: `A errors, B succeeds and validates`.

- [ ] **Step 3: Implement (A error → B fallback).** Replace the whole `complete_validated` method body:

```rust
    pub async fn complete_validated<T, F>(&self, req: &ChatRequest, validate: F) -> Result<T>
    where
        F: Fn(&str) -> std::result::Result<T, String>,
    {
        // Provider A.
        let label_a = self.a.label().to_string();
        let start_a = self.clock.now_ms();
        match self.a.complete(req).await {
            Ok(content) => {
                let latency_ms = self.clock.now_ms().saturating_sub(start_a);
                tracing::info!(provider = %label_a, latency_ms, outcome = "ok", "provider call succeeded");
                return validate(&content).map_err(|e| {
                    DeathpwnError::Provider(format!("{label_a}: validation failed: {e}"))
                });
            }
            Err(e) => {
                let latency_ms = self.clock.now_ms().saturating_sub(start_a);
                tracing::warn!(provider = %label_a, latency_ms, outcome = "error", error = ?e, "provider call failed");
            }
        }

        // Provider B.
        let label_b = self.b.label().to_string();
        let start_b = self.clock.now_ms();
        let content = self
            .b
            .complete(req)
            .await
            .map_err(|e| DeathpwnError::Provider(format!("{label_b}: provider error: {e:?}")))?;
        let latency_ms = self.clock.now_ms().saturating_sub(start_b);
        tracing::info!(provider = %label_b, latency_ms, outcome = "ok", "provider call succeeded");
        validate(&content)
            .map_err(|e| DeathpwnError::Provider(format!("{label_b}: validation failed: {e}")))
    }
```

- [ ] **Step 3: Run test to verify it passes.**

```bash
cargo test -p deathpwn-core failover
```

Expected: both tests PASS (2 passed).

- [ ] **Step 3: Commit.**

```bash
git add deathpwn-core/src/providers/failover.rs
git commit -m "feat(deathpwn): FailoverClient falls back to provider B on A error"
```

---

#### Cycle 3 — Provider A returns bad JSON (validation fails), B succeeds

- [ ] **Step 4: Write the failing test.** Add a third test fn inside `mod tests`:

```rust
    #[tokio::test]
    async fn provider_a_bad_json_falls_back_to_b() {
        let a = Arc::new(FakeAiProvider::new(
            "A",
            vec![Ok("not valid json".to_string())],
        ));
        let b = Arc::new(FakeAiProvider::new(
            "B",
            vec![Ok(r#"{"n":9}"#.to_string())],
        ));
        let clock = Arc::new(FakeClock::new(0));
        let client = FailoverClient::new(a, b, clock);

        let out = client
            .complete_validated(&req(), parse)
            .await
            .expect("A validation fails, B succeeds");

        assert_eq!(out, Probe { n: 9 });
    }
```

- [ ] **Step 4: Run test to verify it fails.**

```bash
cargo test -p deathpwn-core failover
```

Expected: `provider_a_bad_json_falls_back_to_b` FAILS — the Cycle 2 implementation returns A's validation error immediately (it only falls back on A *provider error*, not validation failure), so `.expect(...)` panics: `A validation fails, B succeeds`.

- [ ] **Step 4: Implement (fall back on validation failure too; unify into a loop).** Replace the whole `complete_validated` method body:

```rust
    pub async fn complete_validated<T, F>(&self, req: &ChatRequest, validate: F) -> Result<T>
    where
        F: Fn(&str) -> std::result::Result<T, String>,
    {
        let mut last_error = String::from("no providers configured");

        for provider in [&self.a, &self.b] {
            let label = provider.label().to_string();
            let start = self.clock.now_ms();
            match provider.complete(req).await {
                Ok(content) => {
                    let latency_ms = self.clock.now_ms().saturating_sub(start);
                    match validate(&content) {
                        Ok(value) => {
                            tracing::info!(provider = %label, latency_ms, outcome = "ok", "provider call succeeded");
                            return Ok(value);
                        }
                        Err(verr) => {
                            tracing::warn!(provider = %label, latency_ms, outcome = "validation_failed", error = %verr, "provider response failed validation");
                            last_error = format!("{label}: validation failed: {verr}");
                        }
                    }
                }
                Err(perr) => {
                    let latency_ms = self.clock.now_ms().saturating_sub(start);
                    tracing::warn!(provider = %label, latency_ms, outcome = "error", error = ?perr, "provider call failed");
                    last_error = format!("{label}: provider error: {perr:?}");
                }
            }
        }

        Err(DeathpwnError::Provider(last_error))
    }
```

- [ ] **Step 4: Run test to verify it passes.**

```bash
cargo test -p deathpwn-core failover
```

Expected: all three tests PASS (3 passed).

- [ ] **Step 4: Commit.**

```bash
git add deathpwn-core/src/providers/failover.rs
git commit -m "feat(deathpwn): FailoverClient fails over to B when A output fails validation"
```

---

#### Cycle 4 — Both providers fail → aggregated error naming both

- [ ] **Step 5: Write the failing test.** Add a fourth test fn inside `mod tests`. Provider A errors and provider B returns unparseable output, so both legs fail:

```rust
    #[tokio::test]
    async fn both_providers_fail_yields_aggregated_error() {
        let a = Arc::new(FakeAiProvider::new(
            "A",
            vec![Err(crate::providers::ProviderError::RateLimit)],
        ));
        let b = Arc::new(FakeAiProvider::new(
            "B",
            vec![Ok("garbage".to_string())],
        ));
        let clock = Arc::new(FakeClock::new(0));
        let client = FailoverClient::new(a, b, clock);

        let err = client
            .complete_validated(&req(), parse)
            .await
            .expect_err("both providers fail");

        match err {
            crate::error::DeathpwnError::Provider(msg) => {
                assert!(msg.contains("A:"), "aggregated error must name provider A: {msg}");
                assert!(msg.contains("B:"), "aggregated error must name provider B: {msg}");
            }
            other => panic!("expected DeathpwnError::Provider, got {other:?}"),
        }
    }
```

- [ ] **Step 5: Run test to verify it fails.**

```bash
cargo test -p deathpwn-core failover
```

Expected: `both_providers_fail_yields_aggregated_error` FAILS — the Cycle 3 implementation returns only `last_error` (provider B's message), so `msg.contains("A:")` is false and the assertion panics: `aggregated error must name provider A`.

- [ ] **Step 5: Implement (aggregate every failed attempt).** Replace the whole `complete_validated` method body:

```rust
    pub async fn complete_validated<T, F>(&self, req: &ChatRequest, validate: F) -> Result<T>
    where
        F: Fn(&str) -> std::result::Result<T, String>,
    {
        let mut errors: Vec<String> = Vec::new();

        for provider in [&self.a, &self.b] {
            let label = provider.label().to_string();
            let start = self.clock.now_ms();
            match provider.complete(req).await {
                Ok(content) => {
                    let latency_ms = self.clock.now_ms().saturating_sub(start);
                    match validate(&content) {
                        Ok(value) => {
                            tracing::info!(provider = %label, latency_ms, outcome = "ok", "provider call succeeded");
                            return Ok(value);
                        }
                        Err(verr) => {
                            tracing::warn!(provider = %label, latency_ms, outcome = "validation_failed", error = %verr, "provider response failed validation");
                            errors.push(format!("{label}: validation failed: {verr}"));
                        }
                    }
                }
                Err(perr) => {
                    let latency_ms = self.clock.now_ms().saturating_sub(start);
                    tracing::warn!(provider = %label, latency_ms, outcome = "error", error = ?perr, "provider call failed");
                    errors.push(format!("{label}: provider error: {perr:?}"));
                }
            }
        }

        Err(DeathpwnError::Provider(format!(
            "all providers failed: {}",
            errors.join("; ")
        )))
    }
```

- [ ] **Step 5: Run test to verify it passes.**

```bash
cargo test -p deathpwn-core failover
```

Expected: all four tests PASS (4 passed): `provider_a_ok_returns_validated_value`, `provider_a_error_falls_back_to_b`, `provider_a_bad_json_falls_back_to_b`, `both_providers_fail_yields_aggregated_error`.

- [ ] **Step 5: Re-export `FailoverClient` for downstream tasks.** Add to `deathpwn-core/src/providers/mod.rs` (the pipeline tasks 11–15 import `crate::providers::FailoverClient`):

```rust
pub use failover::FailoverClient;
```

- [ ] **Step 5: Run the full core suite to confirm nothing regressed and the re-export compiles.**

```bash
cargo test -p deathpwn-core
```

Expected: entire `deathpwn-core` suite PASSES, including the four `failover::tests::*` tests. No `#[ignore]` integration test is added in this task — `FailoverClient` does no real network I/O of its own (it drives injected `AiProvider`s), so every test here is deterministic.

- [ ] **Step 5: Final commit.**

```bash
git add deathpwn-core/src/providers/failover.rs deathpwn-core/src/providers/mod.rs
git commit -m "feat(deathpwn): aggregate both-provider failures and re-export FailoverClient"
```
