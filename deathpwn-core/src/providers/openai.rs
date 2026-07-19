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
        let client =
            OpenAiClient::new("https://api.example.com/v1", "sk-test", "gpt-test", "A", 30)
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

        let out = client.complete(&req).await.expect("live completion failed");
        assert!(!out.trim().is_empty(), "expected non-empty content");
    }
}
