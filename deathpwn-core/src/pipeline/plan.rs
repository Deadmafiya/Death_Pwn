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

const NEXT_STEP_SYSTEM: &str = "You are the planning stage of an offensive-security assistant \
driving a multi-step goal to completion. You are given the goal, retrieved knowledge, the \
session context, the history of commands already run with their outcomes, and an optional \
hint for what to try next. Produce ONLY the NEXT action(s) to advance the goal — do NOT \
repeat a command already in the history unless re-running it with different arguments is \
clearly warranted. Respond with ONLY a JSON object of the form \
{\"commands\":[{\"tool\":string,\"argv\":[string],\"purpose\":string,\"depends_on_prev\":bool}]}. \
Emit the smallest ordered chain that makes progress. Output no prose and no markdown fences.";

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

    /// Plan the NEXT action(s) for a goal-completion loop. Unlike [`run`], this
    /// is deliberately **uncached** and history-aware: it feeds the accumulated
    /// step history plus the goal-check's `next_step_hint` to the AI so each
    /// round advances instead of repeating the first plan (GOAL.md §3/§5).
    pub async fn next_step(
        &self,
        u: &Stage1Understanding,
        k: &Stage2Knowledge,
        session: &SessionState,
        history: &[(String, String)],
        hint: Option<&str>,
    ) -> Result<Stage3Plan> {
        let user = build_next_step_prompt(u, k, session, history, hint);
        let req = ChatRequest {
            system: NEXT_STEP_SYSTEM.to_string(),
            user,
            temperature: 0.2,
        };
        self.ai
            .complete_validated(&req, |s| {
                serde_json::from_str::<Stage3Plan>(s).map_err(|e| e.to_string())
            })
            .await
    }
}

/// Build the user prompt for [`Plan::next_step`]: goal + knowledge + session +
/// the history of `(command, outcome_summary)` pairs + an optional next hint.
fn build_next_step_prompt(
    u: &Stage1Understanding,
    k: &Stage2Knowledge,
    session: &SessionState,
    history: &[(String, String)],
    hint: Option<&str>,
) -> String {
    let mut user = String::new();
    user.push_str("## Goal\n");
    user.push_str(&u.goal_summary);
    user.push_str("\n\n## Original intent\n");
    user.push_str(&u.intent);
    user.push_str("\n\n## Theory\n");
    user.push_str(&k.theory);
    user.push_str("\n\n## Candidate commands\n");
    for c in &k.candidates {
        user.push_str(&format!("- {} {} -- {}\n", c.tool, c.argv.join(" "), c.purpose));
    }
    user.push_str("\n## Session context\n");
    user.push_str(&crate::pipeline::session_summary(session));
    user.push_str("\n\n## Steps already executed\n");
    if history.is_empty() {
        user.push_str("(none yet — this is the first step)\n");
    } else {
        for (i, (command, summary)) in history.iter().enumerate() {
            user.push_str(&format!("{}. {} => {}\n", i + 1, command, summary));
        }
    }
    if let Some(hint) = hint.filter(|h| !h.trim().is_empty()) {
        user.push_str("\n## Suggested next step\n");
        user.push_str(hint);
        user.push('\n');
    }
    user
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

    #[tokio::test]
    async fn next_step_is_uncached_and_calls_ai_every_time() {
        // next_step must NOT consult the cache: two calls → two AI calls, so the
        // goal loop keeps planning fresh steps instead of repeating a cached one.
        let a = Arc::new(FakeAiProvider::with_responses(vec![
            Ok(plan_json()),
            Ok(plan_json()),
        ]));
        let b = Arc::new(FakeAiProvider::with_responses(vec![]));
        let stage = Plan::new(failover_with(a.clone(), b.clone()));
        let u = understanding();
        let k = knowledge();
        let session = SessionState::new();

        stage
            .next_step(&u, &k, &session, &[], None)
            .await
            .unwrap();
        stage
            .next_step(&u, &k, &session, &[], None)
            .await
            .unwrap();

        assert_eq!(
            a.call_count(),
            2,
            "next_step must call the AI every time (no caching)"
        );
    }

    #[tokio::test]
    async fn next_step_prompt_embeds_history_and_hint() {
        let history = vec![
            (
                "nmap -sV 192.168.1.1".to_string(),
                "found ssh on 22".to_string(),
            ),
            ("whoami".to_string(), "root".to_string()),
        ];
        let user = build_next_step_prompt(
            &understanding(),
            &knowledge(),
            &SessionState::new(),
            &history,
            Some("try hydra against ssh"),
        );
        assert!(user.contains("nmap -sV 192.168.1.1"), "missing prior command");
        assert!(user.contains("found ssh on 22"), "missing prior outcome");
        assert!(user.contains("try hydra against ssh"), "missing next-step hint");
    }

    #[test]
    fn next_step_prompt_notes_empty_history() {
        let user =
            build_next_step_prompt(&understanding(), &knowledge(), &SessionState::new(), &[], None);
        assert!(
            user.contains("first step"),
            "empty history must be flagged as the first step: {user}"
        );
    }
}
