### Task 11: pipeline — Stage 1 Understand

Stage 1 turns the operator's raw English line plus the current `SessionState`
into a validated `Stage1Understanding`. It is pure orchestration over the AI
seam: build a prompt (embedding a compact session summary), send it through the
`FailoverClient`, and validate the returned text into the typed schema struct.
No `GoalContext` is referenced here — per the manifest note, `GoalContext` is
owned by Task 15 and constructed in the engine; Stage 1 returns
`Stage1Understanding` only.

**Files:**
- Create: `deathpwn-core/src/pipeline/mod.rs`  (core crate — declares + re-exports the pipeline submodules)
- Create: `deathpwn-core/src/pipeline/understand.rs`  (core crate — `Understand`, `session_summary`, `build_request`)
- Modify: `deathpwn-core/src/lib.rs`  (core crate — add `pub mod pipeline;`)
- Test: unit tests live in a `#[cfg(test)] mod tests` at the bottom of `deathpwn-core/src/pipeline/understand.rs` (Rust convention; manifest specifies no separate test file).

**Interfaces:**

- Consumes (exact signatures from earlier tasks — do not re-type):
  - Task 4: `struct FailoverClient { a: Arc<dyn AiProvider>, b: Arc<dyn AiProvider>, clock: Arc<dyn Clock> }`; constructor `FailoverClient::new(a: Arc<dyn AiProvider>, b: Arc<dyn AiProvider>, clock: Arc<dyn Clock>) -> FailoverClient`; and `async fn complete_validated<T, F>(&self, req: &ChatRequest, validate: F) -> Result<T> where F: Fn(&str) -> std::result::Result<T, String>`.
  - Task 3: `struct ChatRequest { system: String, user: String, temperature: f32 }`; `#[async_trait] trait AiProvider: Send + Sync`; `trait Clock: Send + Sync`; test-support fakes `FakeAiProvider` (constructor `FakeAiProvider::ok(&str) -> FakeAiProvider` returns that content on every `complete`) and `FakeClock` (`FakeClock::new(Vec<u64>) -> FakeClock` canonical; `FakeClock::fixed(u64) -> FakeClock` for a constant clock), both re-exported behind `#[cfg(any(test, feature = "test-support"))]`.
  - Task 2: `struct Stage1Understanding { intent: String, params: IntentParams, mode: Mode, goal_summary: String }`; `struct IntentParams { target: Option<String>, ports: Option<String>, url: Option<String>, extra: BTreeMap<String,String> }`; `enum Mode { SingleCommand, GoalCompletion }` (`#[serde(rename_all = "snake_case")]` → `single_command` / `goal_completion`).
  - Task 9: `struct SessionState { targets: Vec<Target>, hosts: Vec<String>, ports_by_host: BTreeMap<String,Vec<u16>>, services: Vec<String>, findings: Vec<Finding>, command_log: Vec<String> }` with `new()`, `add_target(&mut self, target: Target)`, `add_ports(&mut self, host: &str, ports: Vec<u16>)`, and field getters `targets()`, `hosts()`, `ports_by_host()`, `services()`, `findings()`; `struct Target { value: String }` (public `value` field).
  - Task 1: `type Result<T> = std::result::Result<T, DeathpwnError>;` (in `crate::error`).
- Produces (later tasks — Task 15 engine — rely on these):
  - `struct Understand { ai: FailoverClient }`.
  - `Understand::new(ai: FailoverClient) -> Understand`.
  - `async fn run(&self, raw: &str, session: &SessionState) -> Result<Stage1Understanding>`.
  - `pub fn session_summary(session: &SessionState) -> String` (compact context string; `"no prior session context"` when the session is empty).
  - `pub fn build_request(raw: &str, session: &SessionState) -> ChatRequest` (pure prompt builder; exposed so the prompt is unit-testable without hitting the AI seam).

**Cargo dependencies:** none introduced by this task. `serde_json` (Task 2), `tokio` as a dev-dependency for `#[tokio::test]` (Task 3), and `async-trait` (Task 3) are already present in `deathpwn-core/Cargo.toml`. Stage 1's `run` is an inherent `async fn`, not a trait method, so no new `#[async_trait]` usage is needed.

---

- [ ] **Step 1: Write the failing test (session_summary)** — create the module wiring and the first test. This is the only new symbol referenced, so the crate fails to compile solely on `session_summary`.

  Add to `deathpwn-core/src/lib.rs`:
  ```rust
  pub mod pipeline;
  ```

  Create `deathpwn-core/src/pipeline/mod.rs`:
  ```rust
  pub mod understand;
  ```

  Create `deathpwn-core/src/pipeline/understand.rs`:
  ```rust
  #[cfg(test)]
  mod tests {
      use crate::session::{SessionState, Target};

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
  }
  ```

- [ ] **Step 2: Run test to verify it fails** — command:
  ```
  cargo test -p deathpwn-core understand
  ```
  Expected: fails to compile — `error[E0425]: cannot find function \`session_summary\` in this scope` (referenced from both tests via `super::session_summary`).

- [ ] **Step 3: Implement session_summary** — add above the `#[cfg(test)]` block in `deathpwn-core/src/pipeline/understand.rs`:
  ```rust
  use crate::session::SessionState;

  /// Build a compact, deterministic context string describing what the session
  /// already knows, so Stage 1 can resolve follow-ups ("scan those ports")
  /// without the operator re-stating the target. Empty session → a stable
  /// placeholder that keeps the prompt clean.
  pub fn session_summary(session: &SessionState) -> String {
      let mut parts: Vec<String> = Vec::new();

      if !session.targets().is_empty() {
          let targets: Vec<&str> = session
              .targets()
              .iter()
              .map(|t| t.value.as_str())
              .collect();
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
  ```

- [ ] **Step 4: Run test to verify it passes** — command:
  ```
  cargo test -p deathpwn-core understand
  ```
  Expected: PASS — `test pipeline::understand::tests::summary_lists_targets_and_ports ... ok` and `... summary_empty_session_is_placeholder ... ok`.

- [ ] **Step 5: Commit** —
  ```
  git add deathpwn-core/src/lib.rs deathpwn-core/src/pipeline/mod.rs deathpwn-core/src/pipeline/understand.rs
  git commit -m "feat(deathpwn): add Stage 1 session_summary context builder"
  ```

---

- [ ] **Step 6: Write the failing test (build_request)** — append this test to the `mod tests` block in `deathpwn-core/src/pipeline/understand.rs`:
  ```rust
      #[test]
      fn request_embeds_session_summary_and_raw_line() {
          let mut s = SessionState::new();
          s.add_target(Target {
              value: "10.0.0.1".to_string(),
          });

          let req = super::build_request("scan the top ports", &s);

          // The operator's raw request must survive into the prompt verbatim.
          assert!(req.user.contains("scan the top ports"), "user prompt missing raw line: {}", req.user);
          // Session context must be embedded so follow-ups resolve.
          assert!(req.user.contains("10.0.0.1"), "user prompt missing session context: {}", req.user);
          // A schema-directing system prompt must be present.
          assert!(!req.system.is_empty(), "system prompt is empty");
          // Deterministic decoding for a classification-style stage.
          assert_eq!(req.temperature, 0.0);
      }
  ```

- [ ] **Step 7: Run test to verify it fails** — command:
  ```
  cargo test -p deathpwn-core understand
  ```
  Expected: fails to compile — `error[E0425]: cannot find function \`build_request\` in this scope`.

- [ ] **Step 8: Implement build_request** — add to the non-test region of `deathpwn-core/src/pipeline/understand.rs` (add the `ChatRequest` import next to the existing `SessionState` import):
  ```rust
  use crate::providers::ChatRequest;

  const SYSTEM_PROMPT: &str = "You are the understanding stage of deathPWN, an offensive-security terminal. \
Convert the operator's raw English request into exactly one JSON object matching this schema and output nothing else: \
{\"intent\": string, \"params\": {\"target\": string|null, \"ports\": string|null, \"url\": string|null, \"extra\": object}, \
\"mode\": \"single_command\"|\"goal_completion\", \"goal_summary\": string}. \
Use \"single_command\" for a one-shot request and \"goal_completion\" for an open-ended objective. \
Reuse the target/ports/url from the session context when the request refers to them implicitly.";

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
  ```

- [ ] **Step 9: Run test to verify it passes** — command:
  ```
  cargo test -p deathpwn-core understand
  ```
  Expected: PASS — `test pipeline::understand::tests::request_embeds_session_summary_and_raw_line ... ok` (plus the two prior tests still ok).

- [ ] **Step 10: Commit** —
  ```
  git add deathpwn-core/src/pipeline/understand.rs
  git commit -m "feat(deathpwn): build Stage 1 chat request from raw line + session"
  ```

---

- [ ] **Step 11: Write the failing tests (Understand::run + failover-through)** — append to the `mod tests` block in `deathpwn-core/src/pipeline/understand.rs`. Uses the Task 3 fakes (`FakeAiProvider`, `FakeClock`) driving the Task 4 `FailoverClient`:
  ```rust
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
  ```

- [ ] **Step 12: Run test to verify it fails** — command:
  ```
  cargo test -p deathpwn-core understand
  ```
  Expected: fails to compile — `error[E0433]: failed to resolve` / `cannot find type \`Understand\`` and `no function or associated item named \`new\``, because `Understand` does not exist yet.

- [ ] **Step 13: Implement Understand** — add the remaining imports and the struct to the non-test region of `deathpwn-core/src/pipeline/understand.rs`. Final top-of-file import block:
  ```rust
  use crate::error::Result;
  use crate::providers::{ChatRequest, FailoverClient};
  use crate::schema::Stage1Understanding;
  use crate::session::SessionState;
  ```
  (Replace the earlier `use crate::session::SessionState;` and `use crate::providers::ChatRequest;` lines with this consolidated block so nothing is imported twice.)

  Then add:
  ```rust
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
      pub async fn run(
          &self,
          raw: &str,
          session: &SessionState,
      ) -> Result<Stage1Understanding> {
          let req = build_request(raw, session);
          self.ai
              .complete_validated(&req, |content| {
                  serde_json::from_str::<Stage1Understanding>(content)
                      .map_err(|e| e.to_string())
              })
              .await
      }
  }
  ```

- [ ] **Step 14: Run test to verify it passes** — command:
  ```
  cargo test -p deathpwn-core understand
  ```
  Expected: PASS — `run_parses_canned_understanding ... ok` and `run_falls_over_to_b_when_a_returns_bad_json ... ok` (all five tests in the module ok).

- [ ] **Step 15: Commit** —
  ```
  git add deathpwn-core/src/pipeline/understand.rs
  git commit -m "feat(deathpwn): implement Stage 1 Understand run + validation"
  ```

---

- [ ] **Step 16: Re-export the Stage 1 surface** — update `deathpwn-core/src/pipeline/mod.rs` so Task 15 (engine) can import from `crate::pipeline`:
  ```rust
  pub mod understand;

  pub use understand::{build_request, session_summary, Understand};
  ```

- [ ] **Step 17: Run the full core suite to verify nothing regressed** — command:
  ```
  cargo test -p deathpwn-core
  ```
  Expected: PASS — the whole `deathpwn-core` suite is green, including the five `pipeline::understand::tests::*` tests. Default `cargo test` runs no network/subprocess (no `#[ignore]` integration test is introduced by this task; Stage 1 has no real I/O of its own).

- [ ] **Step 18: Final commit** —
  ```
  git add deathpwn-core/src/pipeline/mod.rs
  git commit -m "feat(deathpwn): re-export Stage 1 Understand from pipeline module"
  ```
