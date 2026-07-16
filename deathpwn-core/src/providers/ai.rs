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
    /// If set, every `complete()` returns a clone of this forever (infinite mode).
    constant: Option<std::result::Result<String, ProviderError>>,
    calls: AtomicUsize,
}

#[cfg(any(test, feature = "test-support"))]
impl FakeAiProvider {
    /// Full control (canonical): labeled FIFO of Results.
    pub fn new(
        label: impl Into<String>,
        responses: Vec<std::result::Result<String, ProviderError>>,
    ) -> Self {
        FakeAiProvider {
            label: label.into(),
            responses: Mutex::new(responses.into_iter().collect()),
            constant: None,
            calls: AtomicUsize::new(0),
        }
    }

    /// label = `"fake"`, FIFO of Results.
    pub fn scripted(responses: Vec<std::result::Result<String, ProviderError>>) -> Self {
        FakeAiProvider::new("fake", responses)
    }

    /// Labeled FIFO of Results.
    pub fn with_script(
        label: impl Into<String>,
        responses: Vec<std::result::Result<String, ProviderError>>,
    ) -> Self {
        FakeAiProvider::new(label, responses)
    }

    /// label = `"fake"`, FIFO of Results (behaviorally identical to `scripted`).
    pub fn with_responses(responses: Vec<std::result::Result<String, ProviderError>>) -> Self {
        FakeAiProvider::new("fake", responses)
    }

    /// label = `"fake"`, FIFO of `Ok(body)`.
    pub fn scripted_ok(bodies: Vec<String>) -> Self {
        FakeAiProvider::new("fake", bodies.into_iter().map(Ok).collect())
    }

    /// Infinite `Ok(body)` — sets `constant`, never exhausts.
    pub fn ok(body: impl Into<String>) -> Self {
        FakeAiProvider {
            label: "fake".to_string(),
            responses: Mutex::new(VecDeque::new()),
            constant: Some(Ok(body.into())),
            calls: AtomicUsize::new(0),
        }
    }

    /// Infinite `Ok(body)` — alias of `ok`.
    pub fn always(body: impl Into<String>) -> Self {
        FakeAiProvider::ok(body)
    }

    /// Number of times `complete` has been invoked (used to assert failover /
    /// cache behavior in later tasks).
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    /// Alias of `calls()`.
    pub fn call_count(&self) -> usize {
        self.calls()
    }
}

#[cfg(any(test, feature = "test-support"))]
#[async_trait]
impl AiProvider for FakeAiProvider {
    async fn complete(&self, _req: &ChatRequest) -> std::result::Result<String, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        // Infinite mode: clone the constant every call.
        if let Some(constant) = &self.constant {
            return constant.clone();
        }
        // Otherwise pop the FIFO; scripts must cover exactly the expected calls.
        self.responses
            .lock()
            .expect("FakeAiProvider responses mutex poisoned")
            .pop_front()
            .expect("FakeAiProvider exhausted")
    }

    fn label(&self) -> &str {
        &self.label
    }
}

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
        assert_eq!(fake.calls(), 2);
    }

    #[tokio::test]
    #[should_panic(expected = "FakeAiProvider exhausted")]
    async fn fake_provider_panics_when_script_exhausted() {
        // Tests script exactly the expected number of calls; over-calling panics.
        let fake = FakeAiProvider::scripted(vec![Ok("only".to_string())]);
        let req = ChatRequest {
            system: "s".to_string(),
            user: "u".to_string(),
            temperature: 0.0,
        };
        assert_eq!(fake.complete(&req).await, Ok("only".to_string()));
        let _ = fake.complete(&req).await; // exhausted → panic
    }

    #[tokio::test]
    async fn fake_provider_constant_mode_returns_same_body_forever() {
        // `ok`/`always` set `constant` → infinite clone, never exhausts.
        let fake = FakeAiProvider::ok("pong");
        let req = ChatRequest {
            system: "s".to_string(),
            user: "u".to_string(),
            temperature: 0.0,
        };
        assert_eq!(fake.complete(&req).await, Ok("pong".to_string()));
        assert_eq!(fake.complete(&req).await, Ok("pong".to_string()));
        assert_eq!(fake.call_count(), 2);
    }
}
