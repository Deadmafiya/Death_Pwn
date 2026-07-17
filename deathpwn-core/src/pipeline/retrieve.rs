use std::sync::Arc;

use crate::error::Result;
use crate::providers::failover::FailoverClient;
use crate::providers::ChatRequest;
use crate::schema::{Stage1Understanding, Stage2Knowledge};
use crate::search::{SearchProvider, SearchResult};

/// Sampling temperature for the retrieval stage. Low, since we want stable,
/// grounded candidate commands rather than creative variety.
const RETRIEVE_TEMPERATURE: f32 = 0.2;

/// System prompt: pins the model to emit ONLY JSON matching `Stage2Knowledge`.
const RETRIEVE_SYSTEM: &str = "You are the retrieval stage of an offensive-security \
assistant. Given an operator intent and web search results about relevant tooling and \
techniques, produce a concise theory and a list of concrete candidate commands. Respond \
with ONLY a JSON object matching this schema: {\"theory\": string, \"candidates\": \
[{\"tool\": string, \"argv\": [string], \"purpose\": string}]}. Emit no prose outside \
the JSON.";

/// Build the web-search query for Stage 2 from the understanding.
pub fn build_query(u: &Stage1Understanding) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(u.intent.trim().to_string());
    if let Some(target) = u.params.target.as_deref() {
        let target = target.trim();
        if !target.is_empty() {
            parts.push(target.to_string());
        }
    }
    if let Some(url) = u.params.url.as_deref() {
        let url = url.trim();
        if !url.is_empty() {
            parts.push(url.to_string());
        }
    }
    parts.push("kali OR blackarch command usage example".to_string());
    parts.join(" ")
}

/// Build the `ChatRequest` for Stage 2. When `results` is empty the user prompt
/// explicitly tells the model no search results were found and to rely on its own
/// knowledge — the graceful-degrade path (spec §8 Stage 2).
fn build_request(u: &Stage1Understanding, results: &[SearchResult]) -> ChatRequest {
    let mut user = String::new();
    user.push_str(&format!("Operator intent: {}\n", u.intent));
    user.push_str(&format!("Goal summary: {}\n", u.goal_summary));
    if let Some(target) = u.params.target.as_deref() {
        user.push_str(&format!("Target: {}\n", target));
    }
    if let Some(ports) = u.params.ports.as_deref() {
        user.push_str(&format!("Ports: {}\n", ports));
    }
    if let Some(url) = u.params.url.as_deref() {
        user.push_str(&format!("URL: {}\n", url));
    }
    for (k, v) in &u.params.extra {
        user.push_str(&format!("{k}: {v}\n"));
    }
    user.push('\n');
    if results.is_empty() {
        user.push_str(
            "No search results were found. Rely on your own knowledge of offensive-security \
             tooling to propose candidate commands.\n",
        );
    } else {
        user.push_str("Search results:\n");
        for (i, r) in results.iter().enumerate() {
            user.push_str(&format!("{}. {}\n   {}\n   {}\n", i + 1, r.title, r.url, r.snippet));
        }
    }
    ChatRequest {
        system: RETRIEVE_SYSTEM.to_string(),
        user,
        temperature: RETRIEVE_TEMPERATURE,
    }
}

/// Stage 2 of the pipeline: retrieve. Turns a Stage 1 understanding into
/// candidate commands, grounded in web search when available.
pub struct Retrieve {
    ai: FailoverClient,
    search: Arc<dyn SearchProvider>,
}

impl Retrieve {
    pub fn new(ai: FailoverClient, search: Arc<dyn SearchProvider>) -> Self {
        Self { ai, search }
    }

    /// Build a query, search the web, then ask the AI (with failover + schema
    /// validation) for a `Stage2Knowledge`. Empty search results degrade
    /// gracefully via the prompt built in `build_request`.
    pub async fn run(&self, u: &Stage1Understanding) -> Result<Stage2Knowledge> {
        let query = build_query(u);
        let results = self.search.search(&query).await?;
        let req = build_request(u, &results);
        self.ai
            .complete_validated(&req, |content: &str| {
                serde_json::from_str::<Stage2Knowledge>(content).map_err(|e| e.to_string())
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{IntentParams, Mode, Stage1Understanding};
    use std::collections::BTreeMap;

    fn sample_understanding() -> Stage1Understanding {
        Stage1Understanding {
            intent: "scan for open ports".to_string(),
            params: IntentParams {
                target: Some("192.168.1.1".to_string()),
                ports: Some("1-1000".to_string()),
                url: None,
                extra: BTreeMap::new(),
            },
            mode: Mode::SingleCommand,
            goal_summary: "enumerate open ports on host".to_string(),
        }
    }

    #[test]
    fn build_query_includes_intent_and_target() {
        let u = sample_understanding();
        let q = build_query(&u);
        assert!(q.contains("scan for open ports"), "query missing intent: {q}");
        assert!(q.contains("192.168.1.1"), "query missing target: {q}");
    }

    #[test]
    fn prompt_embeds_results_when_present() {
        let u = sample_understanding();
        let results = vec![SearchResult {
            title: "nmap cheat sheet".to_string(),
            url: "https://example.com/nmap".to_string(),
            snippet: "nmap -p- scans all 65535 ports".to_string(),
        }];
        let req = build_request(&u, &results);
        assert_eq!(req.system, RETRIEVE_SYSTEM);
        assert_eq!(req.temperature, RETRIEVE_TEMPERATURE);
        assert!(req.user.contains("nmap cheat sheet"), "prompt missing result title: {}", req.user);
        assert!(req.user.contains("scan for open ports"), "prompt missing intent");
        assert!(!req.user.contains("No search results were found"));
    }

    #[test]
    fn prompt_degrades_when_no_results() {
        let u = sample_understanding();
        let req = build_request(&u, &[]);
        assert!(req.user.contains("No search results were found"), "prompt missing degrade note: {}", req.user);
        assert!(req.user.contains("your own knowledge"), "prompt missing self-knowledge instruction");
    }

    #[test]
    fn prompt_differs_with_and_without_results() {
        let u = sample_understanding();
        let with = build_request(
            &u,
            &[SearchResult {
                title: "nmap cheat sheet".to_string(),
                url: "https://example.com/nmap".to_string(),
                snippet: "nmap -p- scans all ports".to_string(),
            }],
        );
        let without = build_request(&u, &[]);
        assert_ne!(with.user, without.user, "graceful-degrade prompt must differ");
    }

    use crate::clock::FakeClock;
    use crate::providers::FakeAiProvider;
    use crate::search::FakeSearchProvider;

    fn failover_returning(canned: &str) -> FailoverClient {
        // Provider A succeeds with the canned JSON; B is present but never reached.
        let a = Arc::new(FakeAiProvider::with_script("A", vec![Ok(canned.to_string())]));
        let b = Arc::new(FakeAiProvider::with_script("B", vec![]));
        let clock = Arc::new(FakeClock::fixed(0));
        FailoverClient::new(a, b, clock)
    }

    #[tokio::test]
    async fn run_returns_parsed_knowledge_with_results() {
        let canned = r#"{"theory":"scan ports with nmap","candidates":[{"tool":"nmap","argv":["-p-","192.168.1.1"],"purpose":"full port scan"}]}"#;
        let ai = failover_returning(canned);
        let search = Arc::new(FakeSearchProvider::new(vec![SearchResult {
            title: "nmap".to_string(),
            url: "https://example.com".to_string(),
            snippet: "port scanner".to_string(),
        }]));
        let retrieve = Retrieve::new(ai, search);

        let k = retrieve.run(&sample_understanding()).await.unwrap();
        assert_eq!(k.theory, "scan ports with nmap");
        assert_eq!(k.candidates.len(), 1);
        assert_eq!(k.candidates[0].tool, "nmap");
        assert_eq!(
            k.candidates[0].argv,
            vec!["-p-".to_string(), "192.168.1.1".to_string()]
        );
        assert_eq!(k.candidates[0].purpose, "full port scan");
    }

    #[tokio::test]
    async fn run_returns_parsed_knowledge_when_search_empty() {
        let canned = r#"{"theory":"use own knowledge of nmap","candidates":[]}"#;
        let ai = failover_returning(canned);
        let search = Arc::new(FakeSearchProvider::new(vec![]));
        let retrieve = Retrieve::new(ai, search);

        let k = retrieve.run(&sample_understanding()).await.unwrap();
        assert_eq!(k.theory, "use own knowledge of nmap");
        assert!(k.candidates.is_empty());
    }
}
