use crate::cache::PlanCache;
use crate::error::Result;
use crate::providers::failover::FailoverClient;
use crate::providers::ChatRequest;
use crate::schema::{Stage1Understanding, Stage2Knowledge, Stage3Plan};
use crate::session::SessionState;

const SYSTEM_PROMPT: &str = "You are the planning stage of an offensive-security assistant. \
Given the operator's intent, the retrieved knowledge, and the current session context, \
produce a concrete, ordered execution plan. Respond with ONLY a JSON object of the form \
{\"commands\":[{\"tool\":string,\"argv\":[string],\"purpose\":string,\"depends_on_prev\":bool}]}. \
In SingleCommand mode emit exactly one command. In GoalCompletion mode emit an ordered chain, \
setting depends_on_prev to true when a step consumes the previous step's result. \
Output no prose and no markdown fences.";

pub struct Plan {
    ai: FailoverClient,
}

impl Plan {
    pub fn new(ai: FailoverClient) -> Self {
        Self { ai }
    }

    pub async fn run(
        &self,
        u: &Stage1Understanding,
        k: &Stage2Knowledge,
        session: &SessionState,
        cache: &mut PlanCache,
    ) -> Result<Stage3Plan> {
        if let Some(hit) = cache.get(&u.intent, &u.params) {
            return Ok(hit.clone());
        }
        let (system, user) = build_prompt(u, k, session);
        let req = ChatRequest {
            system,
            user,
            temperature: 0.2,
        };
        let plan: Stage3Plan = self
            .ai
            .complete_validated(&req, |s| {
                serde_json::from_str::<Stage3Plan>(s).map_err(|e| e.to_string())
            })
            .await?;
        cache.put(&u.intent, &u.params, plan.clone());
        Ok(plan)
    }
}

fn build_prompt(
    u: &Stage1Understanding,
    k: &Stage2Knowledge,
    session: &SessionState,
) -> (String, String) {
    let mut user = String::new();
    user.push_str("## Intent\n");
    user.push_str(&u.intent);
    user.push_str("\n\n## Goal\n");
    user.push_str(&u.goal_summary);
    user.push_str(&format!("\n\n## Mode\n{:?}\n\n## Params\n", u.mode));
    if let Some(target) = &u.params.target {
        user.push_str(&format!("target = {target}\n"));
    }
    if let Some(ports) = &u.params.ports {
        user.push_str(&format!("ports = {ports}\n"));
    }
    if let Some(url) = &u.params.url {
        user.push_str(&format!("url = {url}\n"));
    }
    for (key, value) in &u.params.extra {
        user.push_str(&format!("{key} = {value}\n"));
    }
    user.push_str("\n## Theory\n");
    user.push_str(&k.theory);
    user.push_str("\n\n## Candidate commands\n");
    for c in &k.candidates {
        user.push_str(&format!("- {} {} -- {}\n", c.tool, c.argv.join(" "), c.purpose));
    }
    user.push_str("\n## Session context\n");
    user.push_str(&crate::pipeline::session_summary(session));
    user.push('\n');
    (SYSTEM_PROMPT.to_string(), user)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::PlanCache;
    use crate::clock::FakeClock;
    use crate::providers::failover::FailoverClient;
    use crate::providers::FakeAiProvider;
    use crate::schema::{
        CandidateCommand, IntentParams, Mode, Stage1Understanding, Stage2Knowledge,
    };
    use crate::session::SessionState;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn plan_json() -> String {
        r#"{"commands":[{"tool":"nmap","argv":["-sV","192.168.1.1"],"purpose":"service scan","depends_on_prev":false}]}"#
            .to_string()
    }

    fn understanding() -> Stage1Understanding {
        Stage1Understanding {
            intent: "scan ports on 192.168.1.1".to_string(),
            params: IntentParams {
                target: Some("192.168.1.1".to_string()),
                ports: None,
                url: None,
                extra: BTreeMap::new(),
            },
            mode: Mode::SingleCommand,
            goal_summary: "enumerate open services on the host".to_string(),
        }
    }

    fn knowledge() -> Stage2Knowledge {
        Stage2Knowledge {
            theory: "nmap -sV performs service/version detection".to_string(),
            candidates: vec![CandidateCommand {
                tool: "nmap".to_string(),
                argv: vec!["-sV".to_string(), "192.168.1.1".to_string()],
                purpose: "service scan".to_string(),
            }],
        }
    }

    fn failover_with(a: Arc<FakeAiProvider>, b: Arc<FakeAiProvider>) -> FailoverClient {
        FailoverClient::new(a, b, Arc::new(FakeClock::fixed(0)))
    }

    #[test]
    fn build_prompt_embeds_intent_knowledge_and_candidates() {
        let (system, user) = build_prompt(&understanding(), &knowledge(), &SessionState::new());
        assert!(!system.is_empty(), "system prompt must not be empty");
        assert!(user.contains("scan ports on 192.168.1.1"));
        assert!(user.contains("nmap"));
        assert!(user.contains("service/version detection"));
    }

    #[tokio::test]
    async fn run_calls_ai_and_parses_plan() {
        let a = Arc::new(FakeAiProvider::with_responses(vec![Ok(plan_json())]));
        let b = Arc::new(FakeAiProvider::with_responses(vec![Ok(plan_json())]));
        let stage = Plan::new(failover_with(a.clone(), b.clone()));
        let mut cache = PlanCache::new();

        let out = stage
            .run(&understanding(), &knowledge(), &SessionState::new(), &mut cache)
            .await
            .unwrap();

        assert_eq!(out.commands.len(), 1);
        assert_eq!(out.commands[0].tool, "nmap");
        assert_eq!(out.commands[0].argv, vec!["-sV", "192.168.1.1"]);
        assert!(!out.commands[0].depends_on_prev);
        assert_eq!(a.call_count(), 1);
    }

    #[tokio::test]
    async fn second_identical_call_hits_cache() {
        // Two scripted responses per provider so a cache miss on the second call
        // would still succeed — this isolates the failure to the call count.
        let a = Arc::new(FakeAiProvider::with_responses(vec![Ok(plan_json()), Ok(plan_json())]));
        let b = Arc::new(FakeAiProvider::with_responses(vec![Ok(plan_json()), Ok(plan_json())]));
        let stage = Plan::new(failover_with(a.clone(), b.clone()));
        let mut cache = PlanCache::new();
        let u = understanding();
        let k = knowledge();
        let session = SessionState::new();

        let first = stage.run(&u, &k, &session, &mut cache).await.unwrap();
        let second = stage.run(&u, &k, &session, &mut cache).await.unwrap();

        assert_eq!(first, second, "cached plan must equal the freshly planned one");
        assert_eq!(
            a.call_count(),
            1,
            "second identical (intent, params) call must hit the cache, not the AI"
        );
    }
}
