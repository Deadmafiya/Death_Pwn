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
        let clock = Arc::new(FakeClock::fixed(0));
        let client = FailoverClient::new(a, b, clock);

        let out = client
            .complete_validated(&req(), parse)
            .await
            .expect("A errors, B succeeds and validates");

        assert_eq!(out, Probe { n: 7 });
    }

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
        let clock = Arc::new(FakeClock::fixed(0));
        let client = FailoverClient::new(a, b, clock);

        let out = client
            .complete_validated(&req(), parse)
            .await
            .expect("A validation fails, B succeeds");

        assert_eq!(out, Probe { n: 9 });
    }
}
