### Task 13: pipeline: Stage 3 Plan

**Files:**
- Modify: `deathpwn-core/src/pipeline/mod.rs`  (register the new submodule + re-export `Plan`)
- Create: `deathpwn-core/src/pipeline/plan.rs`  (deathpwn-core crate — the Stage 3 planner)
- Test: unit tests live in a `#[cfg(test)] mod tests` at the bottom of `deathpwn-core/src/pipeline/plan.rs` (Rust convention; the manifest names no separate test file for this task)

**Cargo deps:** none new. Stage 3 uses `serde_json` (added in Task 2), `async-trait` (Task 3), and `tokio` as a dev-dependency for `#[tokio::test]` (Task 3). No `Cargo.toml` edit is required for this task.

**Interfaces:**

- Consumes (exact signatures from earlier tasks):
  - Task 1: `type Result<T> = std::result::Result<T, DeathpwnError>;` (`crate::error::Result`); `DeathpwnError::Provider(String)` is the error returned when both providers fail.
  - Task 2: `struct Stage1Understanding { intent: String, params: IntentParams, mode: Mode, goal_summary: String }`; `struct IntentParams { target: Option<String>, ports: Option<String>, url: Option<String>, extra: BTreeMap<String,String> }`; `enum Mode { SingleCommand, GoalCompletion }`; `struct Stage2Knowledge { theory: String, candidates: Vec<CandidateCommand> }`; `struct CandidateCommand { tool: String, argv: Vec<String>, purpose: String }`; `struct Stage3Plan { commands: Vec<PlannedCommand> }`; `struct PlannedCommand { tool: String, argv: Vec<String>, purpose: String, depends_on_prev: bool }`.
  - Task 3: `struct ChatRequest { system: String, user: String, temperature: f32 }`; test-support `FakeAiProvider` and `FakeClock` (re-exported from Task 3 as `crate::providers::FakeAiProvider` and `crate::clock::FakeClock`). Assumed test-support API from Task 3: `FakeAiProvider::with_responses(Vec<String>) -> FakeAiProvider` (returns each scripted string in order from `complete`), `FakeAiProvider::call_count(&self) -> usize` (interior-mutable counter, so an `Arc<FakeAiProvider>` clone still observes it), and `FakeClock::new(start_ms: u64) -> FakeClock`. Adjust the import paths/constructors only if Task 3 named them differently.
  - Task 4: `struct FailoverClient { a: Arc<dyn AiProvider>, b: Arc<dyn AiProvider>, clock: Arc<dyn Clock> }` with `FailoverClient::new(a: Arc<dyn AiProvider>, b: Arc<dyn AiProvider>, clock: Arc<dyn Clock>) -> FailoverClient` and `async fn complete_validated<T, F>(&self, req: &ChatRequest, validate: F) -> Result<T> where F: Fn(&str) -> std::result::Result<T, String>`.
  - Task 9: `struct SessionState` with `SessionState::new()`.
  - Task 10: `struct PlanCache` with `PlanCache::new()`, `fn get(&self, intent: &str, params: &IntentParams) -> Option<&Stage3Plan>`, `fn put(&mut self, intent: &str, params: &IntentParams, plan: Stage3Plan)`.
  - Task 11: `fn session_summary(session: &SessionState) -> String`, callable as `crate::pipeline::session_summary` (Task 11 defines it in the `pipeline` module; if it lives in `understand.rs`, ensure `pipeline/mod.rs` has `pub use understand::session_summary;`).

- Produces (later tasks — Task 15 engine — rely on these exactly):
  - `struct Plan { ai: FailoverClient }`
  - `impl Plan { pub fn new(ai: FailoverClient) -> Self }`
  - `impl Plan { pub async fn run(&self, u: &Stage1Understanding, k: &Stage2Knowledge, session: &SessionState, cache: &mut PlanCache) -> Result<Stage3Plan> }` — cache lookup by `(intent, params)`; on miss build the prompt, call the AI through failover, validate into `Stage3Plan`, `cache.put`, and return; on hit return a clone without calling the AI.

---

#### Cycle 1 — `run` produces a validated plan from the AI, and the prompt carries intent/knowledge/session context

- [ ] **Step 1: Write the failing test** — register the module and create `deathpwn-core/src/pipeline/plan.rs` containing only its test module (no implementation yet, so it fails to compile).

  First, add to `deathpwn-core/src/pipeline/mod.rs`:

  ```rust
  pub mod plan;
  pub use plan::Plan;
  ```

  Then create `deathpwn-core/src/pipeline/plan.rs` with:

  ```rust
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
          FailoverClient::new(a, b, Arc::new(FakeClock::new(0)))
      }

      #[test]
      fn build_prompt_embeds_intent_knowledge_and_candidates() {
          let (system, user) =
              build_prompt(&understanding(), &knowledge(), &SessionState::new());
          assert!(!system.is_empty(), "system prompt must not be empty");
          assert!(user.contains("scan ports on 192.168.1.1"));
          assert!(user.contains("nmap"));
          assert!(user.contains("service/version detection"));
      }

      #[tokio::test]
      async fn run_calls_ai_and_parses_plan() {
          let a = Arc::new(FakeAiProvider::with_responses(vec![plan_json()]));
          let b = Arc::new(FakeAiProvider::with_responses(vec![plan_json()]));
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
  }
  ```

- [ ] **Step 2: Run test to verify it fails** — `cargo test -p deathpwn-core pipeline::plan`. Expected: fails to compile — `cannot find function 'build_prompt' in this scope` and `cannot find function, tuple struct or tuple variant 'Plan' in this scope` (neither `Plan` nor `build_prompt` exists yet).

- [ ] **Step 3: Implement** — add the implementation above the test module in `deathpwn-core/src/pipeline/plan.rs`. This first cut calls the AI unconditionally (no cache yet; the `cache` parameter is intentionally unused this cycle).

  ```rust
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
          let _ = cache; // cache lookup/store is added in the next cycle
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
          user.push_str(&format!(
              "- {} {} -- {}\n",
              c.tool,
              c.argv.join(" "),
              c.purpose
          ));
      }
      user.push_str("\n## Session context\n");
      user.push_str(&crate::pipeline::session_summary(session));
      user.push('\n');
      (SYSTEM_PROMPT.to_string(), user)
  }
  ```

- [ ] **Step 4: Run test to verify it passes** — `cargo test -p deathpwn-core pipeline::plan`. Expected: PASS — `build_prompt_embeds_intent_knowledge_and_candidates` and `run_calls_ai_and_parses_plan` both green (2 passed).

- [ ] **Step 5: Commit** — `git add deathpwn-core/src/pipeline/mod.rs deathpwn-core/src/pipeline/plan.rs` && `git commit -m "feat(deathpwn): add Stage 3 Plan planner with AI-validated Stage3Plan output"`

---

#### Cycle 2 — identical `(intent, params)` hits the cache and does not call the AI again

- [ ] **Step 6: Write the failing test** — append this test to the `mod tests` block in `deathpwn-core/src/pipeline/plan.rs`.

  ```rust
      #[tokio::test]
      async fn second_identical_call_hits_cache() {
          // Two scripted responses per provider so a cache miss on the second call
          // would still succeed — this isolates the failure to the call count.
          let a = Arc::new(FakeAiProvider::with_responses(vec![plan_json(), plan_json()]));
          let b = Arc::new(FakeAiProvider::with_responses(vec![plan_json(), plan_json()]));
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
  ```

- [ ] **Step 7: Run test to verify it fails** — `cargo test -p deathpwn-core pipeline::plan::tests::second_identical_call_hits_cache`. Expected: FAIL — assertion `left == right` panics with `left: 2, right: 1` because the current `run` calls the AI on every invocation (no cache lookup/store yet).

- [ ] **Step 8: Implement** — replace the `run` method body in `deathpwn-core/src/pipeline/plan.rs` to add the cache seam (lookup first, store on miss).

  ```rust
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
  ```

- [ ] **Step 9: Run test to verify it passes** — `cargo test -p deathpwn-core pipeline::plan`. Expected: PASS — all three tests green (`build_prompt_embeds_intent_knowledge_and_candidates`, `run_calls_ai_and_parses_plan`, `second_identical_call_hits_cache`; 3 passed). Run `cargo build -p deathpwn-core` to confirm the crate still compiles with no unused-variable warning for `cache`.

- [ ] **Step 10: Commit** — `git add deathpwn-core/src/pipeline/plan.rs` && `git commit -m "feat(deathpwn): cache Stage3Plan by (intent, params) to skip redundant AI calls"`
