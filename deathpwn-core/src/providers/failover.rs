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
        let clock = Arc::new(FakeClock::fixed(0));
        let client = FailoverClient::new(a, b, clock);

        let out = client
            .complete_validated(&req(), parse)
            .await
            .expect("A succeeds and validates");

        assert_eq!(out, Probe { n: 1 });
    }
}
