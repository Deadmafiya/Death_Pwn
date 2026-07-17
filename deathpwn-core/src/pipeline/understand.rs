use crate::error::Result;
use crate::providers::{ChatRequest, FailoverClient};
use crate::schema::Stage1Understanding;
use crate::session::SessionState;

const SYSTEM_PROMPT: &str = "You are the understanding stage of deathPWN, an offensive-security terminal. \
Convert the operator's raw English request into exactly one JSON object matching this schema and output nothing else: \
{\"intent\": string, \"params\": {\"target\": string|null, \"ports\": string|null, \"url\": string|null, \"extra\": object}, \
\"mode\": \"single_command\"|\"goal_completion\", \"goal_summary\": string}. \
Use \"single_command\" for a one-shot request and \"goal_completion\" for an open-ended objective. \
Reuse the target/ports/url from the session context when the request refers to them implicitly.";

/// Build a compact, deterministic context string describing what the session
/// already knows, so Stage 1 can resolve follow-ups ("scan those ports")
/// without the operator re-stating the target. Empty session → a stable
/// placeholder that keeps the prompt clean.
pub fn session_summary(session: &SessionState) -> String {
    let mut parts: Vec<String> = Vec::new();

    if !session.targets().is_empty() {
        let targets: Vec<&str> = session.targets().iter().map(|t| t.value.as_str()).collect();
        parts.push(format!("targets: {}", targets.join(", ")));
    }

    if !session.hosts().is_empty() {
        parts.push(format!("hosts: {}", session.hosts().join(", ")));
    }

    if !session.ports_by_host().is_empty() {
        let mut entries: Vec<String> = Vec::new();
        for (host, ports) in session.ports_by_host() {
            let ports: Vec<String> = ports.iter().map(|p| p.to_string()).collect();
            entries.push(format!("{}=[{}]", host, ports.join(",")));
        }
        parts.push(format!("ports: {}", entries.join("; ")));
    }

    if !session.services().is_empty() {
        parts.push(format!("services: {}", session.services().join(", ")));
    }

    if !session.findings().is_empty() {
        parts.push(format!("findings: {}", session.findings().len()));
    }

    if parts.is_empty() {
        "no prior session context".to_string()
    } else {
        parts.join("\n")
    }
}

/// Pure prompt builder: embeds the session summary and the raw operator line
/// into a `ChatRequest`. Kept separate from `Understand::run` so the prompt is
/// unit-testable without the AI seam.
pub fn build_request(raw: &str, session: &SessionState) -> ChatRequest {
    let user = format!(
        "Session context:\n{}\n\nOperator request:\n{}",
        session_summary(session),
        raw
    );
    ChatRequest {
        system: SYSTEM_PROMPT.to_string(),
        user,
        temperature: 0.0,
    }
}

/// Stage 1 of the pipeline: raw English + session context → validated
/// `Stage1Understanding`. Pure orchestration over the AI seam.
pub struct Understand {
    ai: FailoverClient,
}

impl Understand {
    pub fn new(ai: FailoverClient) -> Understand {
        Understand { ai }
    }

    /// Send the built request through the failover client, validating the
    /// returned text into `Stage1Understanding`. A parse failure on provider A
    /// triggers failover to provider B inside `complete_validated`; if both
    /// fail, the aggregated `DeathpwnError::Provider` is returned.
    pub async fn run(&self, raw: &str, session: &SessionState) -> Result<Stage1Understanding> {
        let req = build_request(raw, session);
        self.ai
            .complete_validated(&req, |content| {
                serde_json::from_str::<Stage1Understanding>(content).map_err(|e| e.to_string())
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use crate::session::{SessionState, Target};

    use crate::clock::FakeClock;
    use crate::providers::{AiProvider, FailoverClient, FakeAiProvider};
    use crate::schema::Mode;
    use std::sync::Arc;

    const GOOD_JSON: &str = r#"{"intent":"port_scan","params":{"target":"10.0.0.5","ports":"1-1000","url":null,"extra":{}},"mode":"single_command","goal_summary":"Scan the top ports on 10.0.0.5"}"#;

    fn failover(a_resp: &str, b_resp: &str) -> FailoverClient {
        let a: Arc<dyn AiProvider> = Arc::new(FakeAiProvider::ok(a_resp));
        let b: Arc<dyn AiProvider> = Arc::new(FakeAiProvider::ok(b_resp));
        FailoverClient::new(a, b, Arc::new(FakeClock::fixed(0)))
    }

    #[test]
    fn summary_lists_targets_and_ports() {
        let mut s = SessionState::new();
        s.add_target(Target {
            value: "10.0.0.1".to_string(),
        });
        s.add_ports("10.0.0.1", vec![22, 80]);

        let summary = super::session_summary(&s);

        assert!(summary.contains("10.0.0.1"), "summary missing target: {summary}");
        assert!(summary.contains("22"), "summary missing port 22: {summary}");
        assert!(summary.contains("80"), "summary missing port 80: {summary}");
    }

    #[test]
    fn summary_empty_session_is_placeholder() {
        let s = SessionState::new();
        assert_eq!(super::session_summary(&s), "no prior session context");
    }

    #[test]
    fn request_embeds_session_summary_and_raw_line() {
        let mut s = SessionState::new();
        s.add_target(Target {
            value: "10.0.0.1".to_string(),
        });

        let req = super::build_request("scan the top ports", &s);

        // The operator's raw request must survive into the prompt verbatim.
        assert!(
            req.user.contains("scan the top ports"),
            "user prompt missing raw line: {}",
            req.user
        );
        // Session context must be embedded so follow-ups resolve.
        assert!(
            req.user.contains("10.0.0.1"),
            "user prompt missing session context: {}",
            req.user
        );
        // A schema-directing system prompt must be present.
        assert!(!req.system.is_empty(), "system prompt is empty");
        // Deterministic decoding for a classification-style stage.
        assert_eq!(req.temperature, 0.0);
    }

    #[tokio::test]
    async fn run_parses_canned_understanding() {
        let understand = super::Understand::new(failover(GOOD_JSON, GOOD_JSON));
        let session = SessionState::new();

        let out = understand
            .run("scan 10.0.0.5", &session)
            .await
            .expect("valid JSON from provider A must parse");

        assert_eq!(out.intent, "port_scan");
        assert_eq!(out.params.target, Some("10.0.0.5".to_string()));
        assert_eq!(out.params.ports, Some("1-1000".to_string()));
        assert_eq!(out.mode, Mode::SingleCommand);
        assert_eq!(out.goal_summary, "Scan the top ports on 10.0.0.5");
    }

    #[tokio::test]
    async fn run_falls_over_to_b_when_a_returns_bad_json() {
        // Provider A returns unparseable text → validation fails → FailoverClient
        // retries provider B, whose JSON is valid. Stage 1 must still succeed.
        let understand = super::Understand::new(failover("not json at all", GOOD_JSON));
        let session = SessionState::new();

        let out = understand
            .run("scan 10.0.0.5", &session)
            .await
            .expect("provider B valid JSON must parse after A fails validation");

        assert_eq!(out.intent, "port_scan");
        assert_eq!(out.mode, Mode::SingleCommand);
    }
}
