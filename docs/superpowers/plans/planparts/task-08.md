### Task 8: exec — FeedbackLoop + installer

**Files:**
- Create: `deathpwn-core/src/exec/installer.rs` (core crate)
- Create: `deathpwn-core/src/exec/feedback.rs` (core crate)
- Edit: `deathpwn-core/src/exec/mod.rs` (core crate — add module declarations + re-exports)
- Test: unit tests live in a `#[cfg(test)] mod tests` at the bottom of `installer.rs` and `feedback.rs` (Rust convention; manifest specifies no separate test file). Integration tests are not required for this task — every test is deterministic with fakes.

**Interfaces:**

- Consumes (exact signatures from earlier tasks):
  - From Task 1 (`error.rs`, `config.rs`):
    - `enum DeathpwnError { Config(String), Provider(String), Search(String), Exec(String), Schema(String), Cache(String), Io(#[from] std::io::Error), Cancelled }`
    - `type Result<T> = std::result::Result<T, DeathpwnError>;`
    - `struct Config { provider_a: ProviderConfig, provider_b: ProviderConfig, shell: String, max_goal_steps: u32, max_corrections: u32, artifacts_dir: PathBuf, http_timeout_secs: u64 }`
  - From Task 2 (`schema/`):
    - `enum FailureClass { NotFound, BenignEmpty, FixableUsage, Transient, Fatal }` (`#[serde(rename_all = "snake_case")]`)
    - `struct ExecFailureVerdict { class: FailureClass, corrected_argv: Option<Vec<String>> }`
  - From Task 3 (`providers/`):
    - `struct ChatRequest { system: String, user: String, temperature: f32 }`
    - `enum ProviderError { Network(String), Timeout, Http { status: u16 }, RateLimit, Decode(String) }`
    - `#[async_trait] trait AiProvider: Send + Sync { async fn complete(&self, req: &ChatRequest) -> std::result::Result<String, ProviderError>; fn label(&self) -> &str; }`
    - test-support `FakeAiProvider` (re-exported from `crate::providers`)
  - From Task 7 (`exec/`, `cancel.rs`):
    - `struct CommandSpec { tool: String, argv: Vec<String> }`
    - `struct RunOutcome { exit: Option<i32>, stdout: String, stderr: String, cancelled: bool }`
    - `#[async_trait] trait CommandRunner: Send + Sync { async fn run(&self, spec: &CommandSpec, cancel: CancelToken) -> RunOutcome; async fn run_shell(&self, script: &str, cancel: CancelToken) -> RunOutcome; }`
    - `#[derive(Clone)] struct CancelToken` with `fn cancel(&self)`, `fn is_cancelled(&self) -> bool`, async `cancelled()`
    - test-support `FakeCommandRunner` (re-exported from `crate::exec`)

- Produces (exact signatures later tasks — Task 15 engine — rely on):
  - `pub async fn resolve_install_script(ai: &dyn AiProvider, tool: &str) -> Result<String>` (in `installer.rs`)
  - `pub struct FeedbackLoop<R: CommandRunner> { runner: R, ai: Arc<dyn AiProvider>, max_corrections: u32 }`
  - `pub struct AttemptLog { pub argv: Vec<String>, pub exit: Option<i32>, pub note: String }`
  - `pub struct FeedbackOutcome { pub outcome: RunOutcome, pub attempts: Vec<AttemptLog> }`
  - `impl<R: CommandRunner> FeedbackLoop<R>`:
    - `pub fn new(runner: R, ai: Arc<dyn AiProvider>, max_corrections: u32) -> Self`
    - `pub fn from_config(runner: R, ai: Arc<dyn AiProvider>, config: &Config) -> Self`
    - `pub async fn run(&self, spec: &CommandSpec, cancel: CancelToken) -> Result<FeedbackOutcome>`

**Cargo dependencies:** none new. This task uses `serde_json` (added in Task 2), `async-trait`/`tokio` (added in Tasks 3 & 7). `FeedbackLoop` is a plain generic struct (not a trait impl), so it needs no `#[async_trait]`. No `Cargo.toml` edit is required in this task.

**Fake contract this task relies on** (aligns with Tasks 3 & 7 — the fake authors must expose exactly these):
- `FakeAiProvider::scripted(responses: Vec<std::result::Result<String, ProviderError>>) -> FakeAiProvider` — hands out responses in call order; `fn calls(&self) -> usize` returns how many times `complete` was invoked (shared via interior `Arc<Mutex<_>>` so clones/`Arc` wrapping observe the same counter).
- `FakeCommandRunner::new() -> FakeCommandRunner` (`Clone`, interior-shared state) with builder method `available(self, tool: impl Into<String>) -> Self` (marks `command -v` for that tool as exit 0), interior-mutable enqueue methods `push_run(&self, outcome: RunOutcome)` (enqueue next `run()` result), `push_shell(&self, outcome: RunOutcome)` (enqueue next non-`command -v` `run_shell()` result); and observers `fn run_calls(&self) -> Vec<CommandSpec>`, `fn shell_calls(&self) -> Vec<String>`. `run_shell` scripts containing `command -v` consult the availability set; other `run_shell` calls (installs) pop the shell queue.
- `CancelToken::new() -> CancelToken` (real type from Task 7).

---

- [ ] **Step 1: Wire the `installer` module and write its failing tests.** Add the module declaration to `exec/mod.rs` and create `installer.rs` containing only its test module (the function under test does not exist yet).

  Add to `deathpwn-core/src/exec/mod.rs`:

  ```rust
  pub mod installer;
  ```

  Create `deathpwn-core/src/exec/installer.rs`:

  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::error::DeathpwnError;
      use crate::providers::{FakeAiProvider, ProviderError};

      #[tokio::test]
      async fn resolve_strips_code_fence_and_returns_command() {
          let ai = FakeAiProvider::scripted(vec![Ok::<String, ProviderError>(
              "```sh\npacman -S --noconfirm nmap\n```".to_string(),
          )]);
          let script = resolve_install_script(&ai, "nmap").await.unwrap();
          assert_eq!(script, "pacman -S --noconfirm nmap");
      }

      #[tokio::test]
      async fn resolve_errors_on_empty_response() {
          let ai = FakeAiProvider::scripted(vec![Ok::<String, ProviderError>(String::new())]);
          let err = resolve_install_script(&ai, "nmap").await.unwrap_err();
          assert!(matches!(err, DeathpwnError::Exec(_)));
      }
  }
  ```

- [ ] **Step 2: Run test to verify it fails.**

  ```
  cargo test -p deathpwn-core resolve_
  ```

  Expected: fails to compile — `cannot find function \`resolve_install_script\` in this scope`.

- [ ] **Step 3: Implement `resolve_install_script` + sanitizer.** Insert this above the `mod tests` block in `installer.rs`:

  ```rust
  use crate::error::{DeathpwnError, Result};
  use crate::providers::{AiProvider, ChatRequest};

  const INSTALL_SYSTEM: &str = "You are a package resolver for BlackArch Linux. \
  Given a missing command-line tool, reply with ONLY the single shell command that \
  installs it (e.g. `pacman -S --noconfirm nmap`, an AUR helper invocation, or \
  `go install ...`). No prose, no explanation, no code fences.";

  /// Ask the AI for the BlackArch install command for `tool` and return the
  /// sanitized shell script to run. Errors if the model returns nothing usable.
  pub async fn resolve_install_script(ai: &dyn AiProvider, tool: &str) -> Result<String> {
      let req = ChatRequest {
          system: INSTALL_SYSTEM.to_string(),
          user: format!(
              "Missing tool: {tool}\nReturn only the shell command to install it on BlackArch Linux."
          ),
          temperature: 0.0,
      };
      let raw = ai
          .complete(&req)
          .await
          .map_err(|e| DeathpwnError::Provider(format!("{e:?}")))?;
      let script = sanitize(&raw);
      if script.is_empty() {
          return Err(DeathpwnError::Exec(format!(
              "no install command produced for `{tool}`"
          )));
      }
      Ok(script)
  }

  /// Take the first meaningful line, dropping code fences and stray backticks.
  fn sanitize(raw: &str) -> String {
      raw.lines()
          .map(str::trim)
          .filter(|l| !l.is_empty() && !l.starts_with("```"))
          .map(|l| l.trim_matches('`').trim())
          .find(|l| !l.is_empty())
          .unwrap_or("")
          .to_string()
  }
  ```

- [ ] **Step 4: Run test to verify it passes.**

  ```
  cargo test -p deathpwn-core resolve_
  ```

  Expected: PASS — `resolve_strips_code_fence_and_returns_command` and `resolve_errors_on_empty_response` both green.

- [ ] **Step 5: Commit.**

  ```
  git add deathpwn-core/src/exec/installer.rs deathpwn-core/src/exec/mod.rs && \
  git commit -m "feat(deathpwn): add exec installer resolving BlackArch install commands"
  ```

- [ ] **Step 6: Wire the `feedback` module and write the FixableUsage + cap tests.** Add the module declaration and re-exports to `exec/mod.rs`, then create `feedback.rs` with its test module only.

  Add to `deathpwn-core/src/exec/mod.rs`:

  ```rust
  pub mod feedback;

  pub use feedback::{AttemptLog, FeedbackLoop, FeedbackOutcome};
  ```

  Create `deathpwn-core/src/exec/feedback.rs`:

  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::exec::{CommandSpec, FakeCommandRunner, RunOutcome};
      use crate::providers::{FakeAiProvider, ProviderError};
      use crate::cancel::CancelToken;
      use std::sync::Arc;

      fn ok(stdout: &str) -> RunOutcome {
          RunOutcome { exit: Some(0), stdout: stdout.to_string(), stderr: String::new(), cancelled: false }
      }
      fn fail(code: i32, stderr: &str) -> RunOutcome {
          RunOutcome { exit: Some(code), stdout: String::new(), stderr: stderr.to_string(), cancelled: false }
      }
      fn fixable_json() -> String {
          r#"{"class":"fixable_usage","corrected_argv":["nmap","-sV","10.0.0.1"]}"#.to_string()
      }

      #[tokio::test]
      async fn fixable_usage_applies_correction_and_retries() {
          let runner = FakeCommandRunner::new().available("nmap");
          runner.push_run(fail(2, "unrecognized option '--badflag'"));
          runner.push_run(ok("Nmap scan report for 10.0.0.1"));
          let ai = Arc::new(FakeAiProvider::scripted(vec![
              Ok::<String, ProviderError>(fixable_json()),
          ]));

          let fb = FeedbackLoop::new(runner.clone(), ai.clone(), 2);
          let spec = CommandSpec {
              tool: "nmap".into(),
              argv: vec!["nmap".into(), "--badflag".into(), "10.0.0.1".into()],
          };
          let out = fb.run(&spec, CancelToken::new()).await.unwrap();

          assert_eq!(out.outcome.exit, Some(0));
          assert_eq!(ai.calls(), 1, "exactly one classify call");
          let runs = runner.run_calls();
          assert_eq!(runs.len(), 2, "initial run + one corrected retry");
          assert_eq!(
              runs[1].argv,
              vec!["nmap".to_string(), "-sV".to_string(), "10.0.0.1".to_string()],
              "retry uses corrected argv"
          );
          assert!(out.attempts.iter().any(|a| a.note.contains("fixable_usage")));
          assert_eq!(out.attempts.last().unwrap().note, "ok");
      }

      #[tokio::test]
      async fn correction_cap_halts_retries() {
          let runner = FakeCommandRunner::new().available("nmap");
          runner.push_run(fail(2, "bad"));
          runner.push_run(fail(2, "bad"));
          runner.push_run(fail(2, "bad"));
          let ai = Arc::new(FakeAiProvider::scripted(vec![
              Ok::<String, ProviderError>(fixable_json()),
              Ok::<String, ProviderError>(fixable_json()),
              Ok::<String, ProviderError>(fixable_json()),
          ]));

          let fb = FeedbackLoop::new(runner.clone(), ai.clone(), 2);
          let spec = CommandSpec { tool: "nmap".into(), argv: vec!["nmap".into(), "x".into()] };
          let out = fb.run(&spec, CancelToken::new()).await.unwrap();

          assert_eq!(out.outcome.exit, Some(2), "returns the last failing outcome");
          assert_eq!(runner.run_calls().len(), 3, "initial + 2 corrections, then stop");
          assert_eq!(ai.calls(), 3, "classify each failing run until cap");
          assert!(out.attempts.iter().any(|a| a.note.contains("cap")));
      }
  }
  ```

- [ ] **Step 7: Run test to verify it fails.**

  ```
  cargo test -p deathpwn-core -- fixable_usage_applies_correction_and_retries correction_cap_halts_retries
  ```

  Expected: fails to compile — `cannot find type \`FeedbackLoop\` in this scope` (and `FeedbackOutcome`/`AttemptLog` not found).

- [ ] **Step 8: Implement the FeedbackLoop skeleton with the FixableUsage + cap behavior.** Insert this above the `mod tests` block in `feedback.rs`. Non-`FixableUsage` verdicts are handled by a catch-all for now (later steps add their dedicated arms).

  ```rust
  use std::sync::Arc;

  use crate::cancel::CancelToken;
  use crate::config::Config;
  use crate::error::{DeathpwnError, Result};
  use crate::exec::installer::resolve_install_script;
  use crate::exec::{CommandRunner, CommandSpec, RunOutcome};
  use crate::providers::{AiProvider, ChatRequest};
  use crate::schema::{ExecFailureVerdict, FailureClass};

  const CLASSIFY_SYSTEM: &str = "You are an exit-code triage engine. Given a failed \
  shell command, its exit code, and its stderr/stdout, reply with ONLY a JSON object \
  matching {\"class\": one of not_found|benign_empty|fixable_usage|transient|fatal, \
  \"corrected_argv\": array of strings or null}. Use fixable_usage with a corrected_argv \
  when the command has a usage/flag error you can repair. No prose.";

  /// One logged execution attempt inside the feedback loop.
  pub struct AttemptLog {
      pub argv: Vec<String>,
      pub exit: Option<i32>,
      pub note: String,
  }

  /// Final result of a feedback-loop run: the terminal outcome plus the full attempt trail.
  pub struct FeedbackOutcome {
      pub outcome: RunOutcome,
      pub attempts: Vec<AttemptLog>,
  }

  /// Wraps a `CommandRunner` with availability checks, auto-install, and
  /// AI-driven self-correction (GOAL.md §4 / spec §6).
  pub struct FeedbackLoop<R: CommandRunner> {
      runner: R,
      ai: Arc<dyn AiProvider>,
      max_corrections: u32,
  }

  impl<R: CommandRunner> FeedbackLoop<R> {
      pub fn new(runner: R, ai: Arc<dyn AiProvider>, max_corrections: u32) -> Self {
          Self { runner, ai, max_corrections }
      }

      pub fn from_config(runner: R, ai: Arc<dyn AiProvider>, config: &Config) -> Self {
          Self { runner, ai, max_corrections: config.max_corrections }
      }

      pub async fn run(&self, spec: &CommandSpec, cancel: CancelToken) -> Result<FeedbackOutcome> {
          let mut attempts: Vec<AttemptLog> = Vec::new();
          let mut current = spec.clone();
          let mut corrections: u32 = 0;

          // 1. availability check (install path added in a later step).
          if !self.is_available(&current.tool, &cancel).await {
              attempts.push(AttemptLog {
                  argv: vec![format!("<unavailable {}>", current.tool)],
                  exit: None,
                  note: "tool not available".into(),
              });
          }

          loop {
              if cancel.is_cancelled() {
                  attempts.push(AttemptLog {
                      argv: current.argv.clone(),
                      exit: None,
                      note: "cancelled".into(),
                  });
                  return Ok(FeedbackOutcome {
                      outcome: RunOutcome {
                          exit: None,
                          stdout: String::new(),
                          stderr: String::new(),
                          cancelled: true,
                      },
                      attempts,
                  });
              }

              // 2. run.
              let outcome = self.runner.run(&current, cancel.clone()).await;

              if outcome.cancelled {
                  attempts.push(AttemptLog {
                      argv: current.argv.clone(),
                      exit: outcome.exit,
                      note: "cancelled".into(),
                  });
                  return Ok(FeedbackOutcome { outcome, attempts });
              }
              if outcome.exit == Some(0) {
                  attempts.push(AttemptLog {
                      argv: current.argv.clone(),
                      exit: outcome.exit,
                      note: "ok".into(),
                  });
                  return Ok(FeedbackOutcome { outcome, attempts });
              }

              // 3. non-zero → classify.
              let verdict = self.classify(&current, &outcome).await?;
              match verdict.class {
                  FailureClass::FixableUsage => {
                      if corrections >= self.max_corrections {
                          attempts.push(AttemptLog {
                              argv: current.argv.clone(),
                              exit: outcome.exit,
                              note: "fixable_usage: correction cap reached".into(),
                          });
                          return Ok(FeedbackOutcome { outcome, attempts });
                      }
                      corrections += 1;
                      let corrected = verdict
                          .corrected_argv
                          .clone()
                          .unwrap_or_else(|| current.argv.clone());
                      attempts.push(AttemptLog {
                          argv: current.argv.clone(),
                          exit: outcome.exit,
                          note: format!("fixable_usage: retry {}/{}", corrections, self.max_corrections),
                      });
                      current.argv = corrected;
                      continue;
                  }
                  other => {
                      attempts.push(AttemptLog {
                          argv: current.argv.clone(),
                          exit: outcome.exit,
                          note: format!("unhandled: {other:?}"),
                      });
                      return Ok(FeedbackOutcome { outcome, attempts });
                  }
              }
          }
      }

      async fn is_available(&self, tool: &str, cancel: &CancelToken) -> bool {
          let script = format!("command -v -- {tool}");
          let out = self.runner.run_shell(&script, cancel.clone()).await;
          out.exit == Some(0)
      }

      async fn classify(
          &self,
          spec: &CommandSpec,
          outcome: &RunOutcome,
      ) -> Result<ExecFailureVerdict> {
          let req = ChatRequest {
              system: CLASSIFY_SYSTEM.to_string(),
              user: format!(
                  "Command: {} {}\nExit code: {:?}\nStderr:\n{}\nStdout:\n{}",
                  spec.tool,
                  spec.argv.join(" "),
                  outcome.exit,
                  outcome.stderr,
                  outcome.stdout,
              ),
              temperature: 0.0,
          };
          let raw = self
              .ai
              .complete(&req)
              .await
              .map_err(|e| DeathpwnError::Provider(format!("{e:?}")))?;
          let verdict: ExecFailureVerdict = serde_json::from_str(raw.trim())
              .map_err(|e| DeathpwnError::Schema(format!("exec failure verdict parse: {e}")))?;
          Ok(verdict)
      }
  }
  ```

- [ ] **Step 9: Run test to verify it passes.**

  ```
  cargo test -p deathpwn-core -- fixable_usage_applies_correction_and_retries correction_cap_halts_retries
  ```

  Expected: PASS — both the retry and cap tests are green.

- [ ] **Step 10: Commit.**

  ```
  git add deathpwn-core/src/exec/feedback.rs deathpwn-core/src/exec/mod.rs && \
  git commit -m "feat(deathpwn): add FeedbackLoop with AI-driven usage-fix retry and cap"
  ```

- [ ] **Step 11: Write the failing test for the missing-tool install path.** Append this test to the `mod tests` block in `feedback.rs`:

  ```rust
      #[tokio::test]
      async fn missing_tool_is_installed_then_run() {
          // No `.available(...)` → `command -v` reports the tool missing.
          let runner = FakeCommandRunner::new();
          runner.push_shell(ok("")); // install command result (run_shell, not `command -v`)
          runner.push_run(ok("Nmap scan report for 10.0.0.1")); // the retried command
          let ai = Arc::new(FakeAiProvider::scripted(vec![
              Ok::<String, ProviderError>("pacman -S --noconfirm nmap".to_string()),
          ]));

          let fb = FeedbackLoop::new(runner.clone(), ai.clone(), 2);
          let spec = CommandSpec {
              tool: "nmap".into(),
              argv: vec!["nmap".into(), "-sV".into(), "10.0.0.1".into()],
          };
          let out = fb.run(&spec, CancelToken::new()).await.unwrap();

          assert_eq!(out.outcome.exit, Some(0));
          assert_eq!(ai.calls(), 1, "only the install resolution call, no classify");
          assert!(out.attempts.iter().any(|a| a.note.contains("installed via")));
          assert!(runner.shell_calls().iter().any(|s| s.contains("pacman -S")));
      }
  ```

- [ ] **Step 12: Run test to verify it fails.**

  ```
  cargo test -p deathpwn-core missing_tool_is_installed_then_run
  ```

  Expected: PASS/FAIL assertion failure — the loop currently only logs `tool not available` and never installs, so `ai.calls()` is `0` (asserted `1`) and no attempt note contains `installed via`.

- [ ] **Step 13: Implement the install path.** Add the `install` helper and replace the availability branch and the `run` match's catch-all with a `NotFound` arm. Replace the `is_available`-miss block at the top of `run` with:

  ```rust
          let mut corrections: u32 = 0;
          let mut installs: u32 = 0;

          // 1. availability check → auto-install on miss (not counted as a correction).
          if !self.is_available(&current.tool, &cancel).await {
              let note = self.install(&current.tool, &cancel).await?;
              installs += 1;
              attempts.push(AttemptLog {
                  argv: vec![format!("<install {}>", current.tool)],
                  exit: None,
                  note,
              });
          }
  ```

  (Remove the earlier standalone `let mut corrections: u32 = 0;` line, since it is now declared alongside `installs` above.) Replace the `other => { ... }` catch-all arm in the `match verdict.class` with a dedicated `NotFound` arm plus a narrowed catch-all:

  ```rust
                  FailureClass::NotFound => {
                      if installs >= self.max_corrections {
                          attempts.push(AttemptLog {
                              argv: current.argv.clone(),
                              exit: outcome.exit,
                              note: "not_found: install cap reached".into(),
                          });
                          return Ok(FeedbackOutcome { outcome, attempts });
                      }
                      attempts.push(AttemptLog {
                          argv: current.argv.clone(),
                          exit: outcome.exit,
                          note: "not_found".into(),
                      });
                      let note = self.install(&current.tool, &cancel).await?;
                      installs += 1;
                      attempts.push(AttemptLog {
                          argv: vec![format!("<install {}>", current.tool)],
                          exit: None,
                          note,
                      });
                      continue;
                  }
                  other => {
                      attempts.push(AttemptLog {
                          argv: current.argv.clone(),
                          exit: outcome.exit,
                          note: format!("unhandled: {other:?}"),
                      });
                      return Ok(FeedbackOutcome { outcome, attempts });
                  }
  ```

  Add the `install` helper inside `impl<R: CommandRunner> FeedbackLoop<R>` (next to `is_available`):

  ```rust
      async fn install(&self, tool: &str, cancel: &CancelToken) -> Result<String> {
          let script = resolve_install_script(self.ai.as_ref(), tool).await?;
          let out = self.runner.run_shell(&script, cancel.clone()).await;
          if out.exit == Some(0) {
              Ok(format!("installed via `{script}`"))
          } else {
              Err(DeathpwnError::Exec(format!(
                  "install of `{tool}` failed (exit {:?}): {}",
                  out.exit, out.stderr
              )))
          }
      }
  ```

- [ ] **Step 14: Run test to verify it passes.**

  ```
  cargo test -p deathpwn-core missing_tool_is_installed_then_run
  ```

  Expected: PASS. Re-run the full file to confirm no regressions: `cargo test -p deathpwn-core --lib exec::feedback` → all green.

- [ ] **Step 15: Commit.**

  ```
  git add deathpwn-core/src/exec/feedback.rs && \
  git commit -m "feat(deathpwn): add availability check and auto-install to FeedbackLoop"
  ```

- [ ] **Step 16: Write the failing tests for BenignEmpty, Fatal, and Transient.** Append these three tests to the `mod tests` block in `feedback.rs`:

  ```rust
      #[tokio::test]
      async fn benign_empty_is_reported_without_retry() {
          let runner = FakeCommandRunner::new().available("grep");
          runner.push_run(fail(1, ""));
          let ai = Arc::new(FakeAiProvider::scripted(vec![
              Ok::<String, ProviderError>(r#"{"class":"benign_empty","corrected_argv":null}"#.to_string()),
          ]));

          let fb = FeedbackLoop::new(runner.clone(), ai.clone(), 2);
          let spec = CommandSpec {
              tool: "grep".into(),
              argv: vec!["grep".into(), "foo".into(), "f.txt".into()],
          };
          let out = fb.run(&spec, CancelToken::new()).await.unwrap();

          assert_eq!(out.outcome.exit, Some(1));
          assert_eq!(runner.run_calls().len(), 1, "no retry on benign empty");
          assert_eq!(ai.calls(), 1);
          assert_eq!(out.attempts.last().unwrap().note, "benign_empty");
      }

      #[tokio::test]
      async fn fatal_stops_immediately() {
          let runner = FakeCommandRunner::new().available("nmap");
          runner.push_run(fail(1, "permission denied"));
          let ai = Arc::new(FakeAiProvider::scripted(vec![
              Ok::<String, ProviderError>(r#"{"class":"fatal","corrected_argv":null}"#.to_string()),
          ]));

          let fb = FeedbackLoop::new(runner.clone(), ai.clone(), 2);
          let spec = CommandSpec { tool: "nmap".into(), argv: vec!["nmap".into(), "10.0.0.1".into()] };
          let out = fb.run(&spec, CancelToken::new()).await.unwrap();

          assert_eq!(out.outcome.exit, Some(1));
          assert_eq!(runner.run_calls().len(), 1, "no retry on fatal");
          assert_eq!(out.attempts.last().unwrap().note, "fatal");
      }

      #[tokio::test]
      async fn transient_retries_once_and_counts_toward_cap() {
          let runner = FakeCommandRunner::new().available("nmap");
          runner.push_run(fail(1, "temporary failure in name resolution"));
          runner.push_run(ok("Nmap scan report for scanme.nmap.org"));
          let ai = Arc::new(FakeAiProvider::scripted(vec![
              Ok::<String, ProviderError>(r#"{"class":"transient","corrected_argv":null}"#.to_string()),
          ]));

          let fb = FeedbackLoop::new(runner.clone(), ai.clone(), 2);
          let spec = CommandSpec {
              tool: "nmap".into(),
              argv: vec!["nmap".into(), "scanme.nmap.org".into()],
          };
          let out = fb.run(&spec, CancelToken::new()).await.unwrap();

          assert_eq!(out.outcome.exit, Some(0));
          assert_eq!(runner.run_calls().len(), 2, "one transient retry");
          assert!(out.attempts.iter().any(|a| a.note.contains("transient")));
          assert_eq!(out.attempts.last().unwrap().note, "ok");
      }
  ```

- [ ] **Step 17: Run test to verify it fails.**

  ```
  cargo test -p deathpwn-core -- benign_empty_is_reported_without_retry fatal_stops_immediately transient_retries_once_and_counts_toward_cap
  ```

  Expected: FAIL — all three verdicts currently hit the `other =>` catch-all, so the last attempt note is `unhandled: BenignEmpty` / `unhandled: Fatal` / `unhandled: Transient` (asserted `benign_empty` / `fatal` / retry-then-`ok`), and the transient case never retries (`run_calls().len()` is `1`, asserted `2`).

- [ ] **Step 18: Implement the remaining verdict arms.** Replace the `other => { ... }` catch-all in the `match verdict.class` with explicit `BenignEmpty`, `Fatal`, and `Transient` arms (the `FixableUsage` and `NotFound` arms are unchanged). The full match now reads:

  ```rust
              let verdict = self.classify(&current, &outcome).await?;
              match verdict.class {
                  FailureClass::BenignEmpty => {
                      attempts.push(AttemptLog {
                          argv: current.argv.clone(),
                          exit: outcome.exit,
                          note: "benign_empty".into(),
                      });
                      return Ok(FeedbackOutcome { outcome, attempts });
                  }
                  FailureClass::Fatal => {
                      attempts.push(AttemptLog {
                          argv: current.argv.clone(),
                          exit: outcome.exit,
                          note: "fatal".into(),
                      });
                      return Ok(FeedbackOutcome { outcome, attempts });
                  }
                  FailureClass::NotFound => {
                      if installs >= self.max_corrections {
                          attempts.push(AttemptLog {
                              argv: current.argv.clone(),
                              exit: outcome.exit,
                              note: "not_found: install cap reached".into(),
                          });
                          return Ok(FeedbackOutcome { outcome, attempts });
                      }
                      attempts.push(AttemptLog {
                          argv: current.argv.clone(),
                          exit: outcome.exit,
                          note: "not_found".into(),
                      });
                      let note = self.install(&current.tool, &cancel).await?;
                      installs += 1;
                      attempts.push(AttemptLog {
                          argv: vec![format!("<install {}>", current.tool)],
                          exit: None,
                          note,
                      });
                      continue;
                  }
                  FailureClass::FixableUsage => {
                      if corrections >= self.max_corrections {
                          attempts.push(AttemptLog {
                              argv: current.argv.clone(),
                              exit: outcome.exit,
                              note: "fixable_usage: correction cap reached".into(),
                          });
                          return Ok(FeedbackOutcome { outcome, attempts });
                      }
                      corrections += 1;
                      let corrected = verdict
                          .corrected_argv
                          .clone()
                          .unwrap_or_else(|| current.argv.clone());
                      attempts.push(AttemptLog {
                          argv: current.argv.clone(),
                          exit: outcome.exit,
                          note: format!("fixable_usage: retry {}/{}", corrections, self.max_corrections),
                      });
                      current.argv = corrected;
                      continue;
                  }
                  FailureClass::Transient => {
                      if corrections >= self.max_corrections {
                          attempts.push(AttemptLog {
                              argv: current.argv.clone(),
                              exit: outcome.exit,
                              note: "transient: correction cap reached".into(),
                          });
                          return Ok(FeedbackOutcome { outcome, attempts });
                      }
                      corrections += 1;
                      attempts.push(AttemptLog {
                          argv: current.argv.clone(),
                          exit: outcome.exit,
                          note: format!("transient: retry {}/{}", corrections, self.max_corrections),
                      });
                      continue;
                  }
              }
  ```

- [ ] **Step 19: Run test to verify it passes.**

  ```
  cargo test -p deathpwn-core -- benign_empty_is_reported_without_retry fatal_stops_immediately transient_retries_once_and_counts_toward_cap
  ```

  Expected: PASS — all three arms behave as asserted.

- [ ] **Step 20: Run the full core suite and build to confirm no regressions.**

  ```
  cargo test -p deathpwn-core && cargo build -p deathpwn-core
  ```

  Expected: PASS — every Task 8 test green (`resolve_*`, `fixable_usage_*`, `correction_cap_*`, `missing_tool_*`, `benign_empty_*`, `fatal_*`, `transient_*`) with no compiler warnings from the new modules; `#![forbid(unsafe_code)]` still holds.

- [ ] **Step 21: Final commit.**

  ```
  git add deathpwn-core/src/exec/feedback.rs && \
  git commit -m "feat(deathpwn): complete FeedbackLoop verdict handling (benign/fatal/transient)"
  ```
