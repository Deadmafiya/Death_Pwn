### Task 14: pipeline — Stage 4 Render

**Files:**
- Create: `deathpwn-core/src/pipeline/render.rs`  (core crate)
- Edit: `deathpwn-core/src/pipeline/mod.rs`  (core crate — register the `render` submodule + re-export `Render`; the module file itself was created in Task 11)
- Test: unit tests live in a `#[cfg(test)] mod tests` block inside `deathpwn-core/src/pipeline/render.rs` (Rust convention; the manifest names no separate test file)

**Cargo deps introduced by this task:** none. Stage 4 only needs `serde_json` (added in Task 2), the `FailoverClient`/`ChatRequest` from `crate::providers` (Task 4), and the test fakes `FakeAiProvider`/`FakeClock` (Task 3). `tokio` is already a dev/test dependency from Task 3. No `Cargo.toml` change is required for this task.

**Interfaces:**
- Consumes (exact signatures from earlier tasks):
  - `struct ChatRequest { system: String, user: String, temperature: f32 }` (Task 3)
  - `struct FailoverClient { a: Arc<dyn AiProvider>, b: Arc<dyn AiProvider>, clock: Arc<dyn Clock> }` with `FailoverClient::new(a: Arc<dyn AiProvider>, b: Arc<dyn AiProvider>, clock: Arc<dyn Clock>) -> Self` and `async fn complete_validated<T, F>(&self, req: &ChatRequest, validate: F) -> Result<T> where F: Fn(&str) -> std::result::Result<T, String>` (Task 4)
  - `struct RunOutcome { exit: Option<i32>, stdout: String, stderr: String, cancelled: bool }` (Task 7)
  - `struct Stage1Understanding { intent: String, params: IntentParams, mode: Mode, goal_summary: String }` and `struct Stage4Render { sections: Vec<RenderSection> }` (Task 2)
  - `enum DeathpwnError { ... Provider(String) ... }` and `type Result<T> = std::result::Result<T, DeathpwnError>;` (Task 1)
  - test-support `FakeAiProvider::new(label: impl Into<String>, script: Vec<std::result::Result<String, ProviderError>>)` and `FakeClock::new(start_ms: u64)` (Task 3)
- Produces (later tasks — Task 15 engine — rely on these EXACT signatures):
  - `struct Render { ai: FailoverClient }`
  - `impl Render { fn new(ai: FailoverClient) -> Self }`
  - `impl Render { async fn run(&self, u: &Stage1Understanding, outcome: &RunOutcome) -> Result<Stage4Render> }`
  - `pub(crate) fn build_render_prompt(u: &Stage1Understanding, outcome: &RunOutcome) -> String`

---

Stage 4 turns a command's `RunOutcome` (plus the originating intent) into a typed `Stage4Render` by calling the AI through `FailoverClient::complete_validated`, validating with the `Stage4Render` serde parser. Per §8 of the spec this stage is **not cached** — its output is a pure function of live command output. Two behaviors are exercised: (1) a canned valid `Stage4Render` parses back into the exact struct, and (2) unparseable content from both providers exhausts failover and surfaces `DeathpwnError::Provider`. A third pure test pins the prompt builder so the intent and captured output actually reach the model.

#### Cycle 1 — the prompt builder embeds intent + output

- [ ] **Step 1: Write the failing test** — create `deathpwn-core/src/pipeline/render.rs` with the doc comment and a test module, and register the module in `pipeline/mod.rs` so the test is compiled.

  Add these two lines to `deathpwn-core/src/pipeline/mod.rs` (alongside the other stage modules created in Task 11):

  ```rust
  pub mod render;
  pub use render::Render;
  ```

  Create `deathpwn-core/src/pipeline/render.rs`:

  ```rust
  //! Pipeline Stage 4 — Render.
  //!
  //! Turns a command's `RunOutcome` (plus the originating intent) into a typed
  //! `Stage4Render` via the AI. This stage is intentionally NOT cached: its
  //! output is a function of live command output, which changes on every run.

  #[cfg(test)]
  mod tests {
      use super::build_render_prompt;
      use crate::exec::RunOutcome;
      use crate::schema::{IntentParams, Mode, Stage1Understanding};
      use std::collections::BTreeMap;

      fn sample_understanding() -> Stage1Understanding {
          Stage1Understanding {
              intent: "port_scan".to_string(),
              params: IntentParams {
                  target: Some("192.168.1.1".to_string()),
                  ports: Some("1-1024".to_string()),
                  url: None,
                  extra: BTreeMap::new(),
              },
              mode: Mode::SingleCommand,
              goal_summary: "scan common ports on the host".to_string(),
          }
      }

      #[test]
      fn prompt_embeds_intent_and_output() {
          let u = sample_understanding();
          let outcome = RunOutcome {
              exit: Some(0),
              stdout: "22/tcp open ssh".to_string(),
              stderr: "warning: slow".to_string(),
              cancelled: false,
          };

          let prompt = build_render_prompt(&u, &outcome);

          assert!(prompt.contains("port_scan"), "intent missing from prompt");
          assert!(
              prompt.contains("scan common ports on the host"),
              "goal_summary missing from prompt"
          );
          assert!(prompt.contains("22/tcp open ssh"), "stdout missing from prompt");
          assert!(prompt.contains("warning: slow"), "stderr missing from prompt");
          assert!(prompt.contains("Exit code: 0"), "exit code missing from prompt");
      }

      #[test]
      fn prompt_reports_no_exit_code_when_cancelled() {
          let u = sample_understanding();
          let outcome = RunOutcome {
              exit: None,
              stdout: String::new(),
              stderr: String::new(),
              cancelled: true,
          };

          let prompt = build_render_prompt(&u, &outcome);

          assert!(prompt.contains("Cancelled: true"));
          assert!(
              prompt.contains("none (terminated without a normal exit code)"),
              "missing exit code should be described, not shown as a number"
          );
      }
  }
  ```

- [ ] **Step 2: Run test to verify it fails** — `cargo test -p deathpwn-core pipeline::render`.
  Expected: fails to compile — `error[E0432]: unresolved import` / `cannot find function build_render_prompt in module super` (the function does not exist yet).

- [ ] **Step 3: Implement** — add the prompt builder above the test module in `deathpwn-core/src/pipeline/render.rs` (types are fully qualified so no module-level `use` is needed yet):

  ```rust
  /// Build the user prompt embedding the intent, exit status, cancellation
  /// flag, and captured stdout/stderr so the model renders only real output.
  pub(crate) fn build_render_prompt(
      u: &crate::schema::Stage1Understanding,
      outcome: &crate::exec::RunOutcome,
  ) -> String {
      let exit = match outcome.exit {
          Some(code) => code.to_string(),
          None => "none (terminated without a normal exit code)".to_string(),
      };
      format!(
          "Intent: {intent}\nGoal: {goal}\nCancelled: {cancelled}\nExit code: {exit}\n\n\
           --- STDOUT ---\n{stdout}\n--- STDERR ---\n{stderr}\n",
          intent = u.intent,
          goal = u.goal_summary,
          cancelled = outcome.cancelled,
          exit = exit,
          stdout = outcome.stdout,
          stderr = outcome.stderr,
      )
  }
  ```

- [ ] **Step 4: Run test to verify it passes** — `cargo test -p deathpwn-core pipeline::render`.
  Expected: PASS — `test pipeline::render::tests::prompt_embeds_intent_and_output ... ok` and `... prompt_reports_no_exit_code_when_cancelled ... ok`.

- [ ] **Step 5: Commit** — `git add deathpwn-core/src/pipeline/render.rs deathpwn-core/src/pipeline/mod.rs` && `git commit -m "feat(deathpwn): stage 4 render prompt builder"`

#### Cycle 2 — `Render::run` parses via failover, and both-provider failure errors

- [ ] **Step 6: Write the failing test** — add these two `#[tokio::test]` cases to the existing `mod tests` in `deathpwn-core/src/pipeline/render.rs`. Update the `super` import at the top of the test module from `use super::build_render_prompt;` to bring in `Render` too, and add the imports the new tests need:

  ```rust
  // change the existing `use super::build_render_prompt;` line to:
  use super::{build_render_prompt, Render};
  // and add these imports to the test module:
  use crate::clock::FakeClock;
  use crate::error::DeathpwnError;
  use crate::providers::{AiProvider, FailoverClient, FakeAiProvider};
  use crate::schema::{RenderBody, RenderSection, SectionKind, Stage4Render};
  use std::sync::Arc;
  ```

  ```rust
  #[tokio::test]
  async fn run_parses_canned_render_from_provider_a() {
      let u = sample_understanding();
      let outcome = RunOutcome {
          exit: Some(0),
          stdout: "22/tcp open ssh".to_string(),
          stderr: String::new(),
          cancelled: false,
      };

      // Build the expected value and serialize it, so the wire form matches
      // whatever serde representation Task 2 chose for RenderBody.
      let expected = Stage4Render {
          sections: vec![RenderSection {
              title: "Open Ports".to_string(),
              kind: SectionKind::Table,
              body: RenderBody::Table {
                  headers: vec!["port".to_string(), "state".to_string()],
                  rows: vec![vec!["22".to_string(), "open".to_string()]],
              },
          }],
      };
      let canned = serde_json::to_string(&expected).unwrap();

      let a: Arc<dyn AiProvider> = Arc::new(FakeAiProvider::new("A", vec![Ok(canned)]));
      let b: Arc<dyn AiProvider> = Arc::new(FakeAiProvider::new("B", vec![]));
      let clock = Arc::new(FakeClock::new(0));
      let render = Render::new(FailoverClient::new(a, b, clock));

      let got = render.run(&u, &outcome).await.expect("stage 4 should succeed");
      assert_eq!(got, expected);
  }

  #[tokio::test]
  async fn run_errors_when_both_providers_return_unparseable() {
      let u = sample_understanding();
      let outcome = RunOutcome {
          exit: Some(1),
          stdout: String::new(),
          stderr: "boom".to_string(),
          cancelled: false,
      };

      // Both providers "succeed" at the HTTP level but return non-JSON, so
      // validation fails on A, failover tries B, B also fails to validate.
      let a: Arc<dyn AiProvider> =
          Arc::new(FakeAiProvider::new("A", vec![Ok("not json".to_string())]));
      let b: Arc<dyn AiProvider> =
          Arc::new(FakeAiProvider::new("B", vec![Ok("also not json".to_string())]));
      let clock = Arc::new(FakeClock::new(0));
      let render = Render::new(FailoverClient::new(a, b, clock));

      let err = render.run(&u, &outcome).await.expect_err("both providers invalid");
      assert!(
          matches!(err, DeathpwnError::Provider(_)),
          "expected aggregated Provider error, got {err:?}"
      );
  }
  ```

- [ ] **Step 7: Run test to verify it fails** — `cargo test -p deathpwn-core pipeline::render`.
  Expected: fails to compile — `cannot find type Render in module super` / `no function or associated item named new` (the `Render` struct and `run` method do not exist yet).

- [ ] **Step 8: Implement** — add the imports, system prompt constant, struct, and impl to the top of `deathpwn-core/src/pipeline/render.rs`, directly under the module doc comment and above `build_render_prompt`. After this edit the non-test portion of the file reads in full:

  ```rust
  //! Pipeline Stage 4 — Render.
  //!
  //! Turns a command's `RunOutcome` (plus the originating intent) into a typed
  //! `Stage4Render` via the AI. This stage is intentionally NOT cached: its
  //! output is a function of live command output, which changes on every run.

  use crate::error::Result;
  use crate::providers::{ChatRequest, FailoverClient};
  use crate::schema::{Stage1Understanding, Stage4Render};

  const RENDER_SYSTEM_PROMPT: &str = "You are the rendering stage of an offensive-security \
  terminal. Given the user's intent and the raw output of a command that was run, produce ONLY a \
  JSON object matching the Stage4Render schema: \
  {\"sections\":[{\"title\":<string>,\"kind\":\"table\"|\"key_value\"|\"text\"|\"findings\",\"body\":<body>}]}. \
  Summarize the command output faithfully and never invent data that is not present in it. \
  Emit no prose and no markdown fences — output JSON only.";

  /// Stage 4: render a command outcome into typed display sections.
  pub struct Render {
      ai: FailoverClient,
  }

  impl Render {
      /// Construct the stage over a configured failover AI client.
      pub fn new(ai: FailoverClient) -> Self {
          Self { ai }
      }

      /// Feed the intent + stdout/stderr/exit to the AI and parse a
      /// `Stage4Render`. On an A-side error or a validation failure the failover
      /// client retries provider B; if both fail the aggregated error surfaces as
      /// `DeathpwnError::Provider`. This stage is never cached.
      pub async fn run(
          &self,
          u: &Stage1Understanding,
          outcome: &crate::exec::RunOutcome,
      ) -> Result<Stage4Render> {
          let req = ChatRequest {
              system: RENDER_SYSTEM_PROMPT.to_string(),
              user: build_render_prompt(u, outcome),
              temperature: 0.1,
          };
          self.ai
              .complete_validated(&req, |content| {
                  serde_json::from_str::<Stage4Render>(content).map_err(|e| e.to_string())
              })
              .await
      }
  }

  /// Build the user prompt embedding the intent, exit status, cancellation
  /// flag, and captured stdout/stderr so the model renders only real output.
  pub(crate) fn build_render_prompt(
      u: &crate::schema::Stage1Understanding,
      outcome: &crate::exec::RunOutcome,
  ) -> String {
      let exit = match outcome.exit {
          Some(code) => code.to_string(),
          None => "none (terminated without a normal exit code)".to_string(),
      };
      format!(
          "Intent: {intent}\nGoal: {goal}\nCancelled: {cancelled}\nExit code: {exit}\n\n\
           --- STDOUT ---\n{stdout}\n--- STDERR ---\n{stderr}\n",
          intent = u.intent,
          goal = u.goal_summary,
          cancelled = outcome.cancelled,
          exit = exit,
          stdout = outcome.stdout,
          stderr = outcome.stderr,
      )
  }
  ```

  (The `#[cfg(test)] mod tests { ... }` block from Steps 1 and 6 remains unchanged below this code.)

- [ ] **Step 9: Run test to verify it passes** — `cargo test -p deathpwn-core pipeline::render`.
  Expected: PASS — all four tests green: `prompt_embeds_intent_and_output`, `prompt_reports_no_exit_code_when_cancelled`, `run_parses_canned_render_from_provider_a`, `run_errors_when_both_providers_return_unparseable`.

- [ ] **Step 10: Commit** — `git add deathpwn-core/src/pipeline/render.rs` && `git commit -m "feat(deathpwn): stage 4 render via failover AI, uncached"`
