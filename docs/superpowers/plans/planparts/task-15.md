### Task 15: Goal context + Engine orchestrator

Wires every prior component together. `goal/mod.rs` owns the `GoalContext` /
`StepRecord` types threaded through a goal-completion run; `engine.rs` owns the
orchestrator that runs Step 0 detection, the 4-stage pipeline, execution via the
feedback loop, and the goal loop, streaming `EngineEvent`s over an `mpsc`
channel.

**Files:**
- Create: `deathpwn-core/src/goal/mod.rs`  (core crate — `GoalContext`, `StepRecord`)
- Create: `deathpwn-core/src/engine.rs`  (core crate — `Engine`, `EngineEvent`, goal loop)
- Edit: `deathpwn-core/src/lib.rs`  (core crate — add `pub mod goal;` and `pub mod engine;`)
- Test: unit tests live in `#[cfg(test)] mod tests` inside `goal/mod.rs` and `engine.rs` (Rust convention; manifest specifies no separate test file)

**New dependencies:** none. `tokio` (Task 7), `serde_json` (Task 2), `shell_words`
(Task 6) and `async-trait` (Task 3) are already in `deathpwn-core/Cargo.toml`.
`tempfile` is already a dev-dependency (added Task 9). Nothing to add here.

**Interfaces:**

- Consumes (exact signatures from earlier tasks — do not re-type):
  - `enum DeathpwnError { Config(String), Provider(String), Search(String), Exec(String), Schema(String), Cache(String), Io(#[from] std::io::Error), Cancelled }` and `type Result<T> = std::result::Result<T, DeathpwnError>;` (Task 1)
  - `struct Config { provider_a: ProviderConfig, provider_b: ProviderConfig, shell: String, max_goal_steps: u32, max_corrections: u32, artifacts_dir: PathBuf, http_timeout_secs: u64 }`, `struct ProviderConfig { url: String, key: String, model: String }` (Task 1)
  - `struct Stage1Understanding { intent: String, params: IntentParams, mode: Mode, goal_summary: String }`, `struct IntentParams { target: Option<String>, ports: Option<String>, url: Option<String>, extra: BTreeMap<String,String> }`, `enum Mode { SingleCommand, GoalCompletion }`, `struct Stage2Knowledge { theory: String, candidates: Vec<CandidateCommand> }`, `struct Stage3Plan { commands: Vec<PlannedCommand> }`, `struct PlannedCommand { tool: String, argv: Vec<String>, purpose: String, depends_on_prev: bool }`, `struct Stage4Render { sections: Vec<RenderSection> }`, `struct GoalVerdict { achieved: bool, reason: String, next_step_hint: Option<String> }` (Task 2)
  - `struct ChatRequest { system: String, user: String, temperature: f32 }` (Task 3); `struct FailoverClient { a: Arc<dyn AiProvider>, b: Arc<dyn AiProvider>, clock: Arc<dyn Clock> }` with `async fn complete_validated<T, F>(&self, req: &ChatRequest, validate: F) -> Result<T> where F: Fn(&str) -> std::result::Result<T, String>` (Task 4)
  - `enum InputKind { DirectCommand, RawInput }`, `struct Detector<R: CommandRunner> { runner: R, shell: String }` with `async fn classify(&self, line: &str) -> InputKind` (Task 6)
  - `struct CommandSpec { tool: String, argv: Vec<String> }`, `struct RunOutcome { exit: Option<i32>, stdout: String, stderr: String, cancelled: bool }`, `struct OutputLine { stream: Stream, text: String }`, `enum Stream { Stdout, Stderr }`, `#[derive(Clone)] struct CancelToken` with `fn cancel(&self)` / `fn is_cancelled(&self) -> bool` / async `cancelled()`, `#[async_trait] trait CommandRunner: Send + Sync { async fn run(&self, spec: &CommandSpec, cancel: CancelToken) -> RunOutcome; async fn run_shell(&self, script: &str, cancel: CancelToken) -> RunOutcome; }` (Task 7)
  - `struct FeedbackLoop<R: CommandRunner> { runner: R, ai: Arc<dyn AiProvider>, max_corrections: u32 }`, `struct FeedbackOutcome { outcome: RunOutcome, attempts: Vec<AttemptLog> }` with `async fn run(&self, spec: &CommandSpec, cancel: CancelToken) -> Result<FeedbackOutcome>` (Task 8)
  - `struct SessionState { ... }` with `new()`, `record_command(&str)`, `command_log() -> &[String]` (Task 9); `struct Artifacts { root: PathBuf, session_dir: PathBuf }` with `Artifacts::open(root: PathBuf, clock: &dyn Clock) -> Result<Artifacts>` and `fn write_output(&self, index: usize, outcome: &RunOutcome) -> Result<PathBuf>` (Task 9)
  - `struct PlanCache` with `new()`, `get(intent, params) -> Option<&Stage3Plan>`, `put(intent, params, plan)` (Task 10)
  - `struct Understand { ai: FailoverClient }` `async fn run(&self, raw: &str, session: &SessionState) -> Result<Stage1Understanding>`; `struct Retrieve { ai: FailoverClient, search: Arc<dyn SearchProvider> }` `async fn run(&self, u: &Stage1Understanding) -> Result<Stage2Knowledge>`; `struct Plan { ai: FailoverClient }` `async fn run(&self, u: &Stage1Understanding, k: &Stage2Knowledge, session: &SessionState, cache: &mut PlanCache) -> Result<Stage3Plan>`; `struct Render { ai: FailoverClient }` `async fn run(&self, u: &Stage1Understanding, outcome: &RunOutcome) -> Result<Stage4Render>` (Tasks 11–14)
  - Test-support fakes (Task 3/5/7): `FakeAiProvider`, `FakeClock`, `FakeSearchProvider`, `FakeCommandRunner`

- Produces (later tasks — TUI Task 16 — rely on these EXACT names/types):
  - `struct StepRecord { command: String, summary: String }`
  - `struct GoalContext { goal_summary: String, mode: Mode, steps_taken: u32, history: Vec<StepRecord> }`
  - `enum EngineEvent { Output(OutputLine), Rendered(Stage4Render), Error(String), Done }`
  - `struct Engine<R: CommandRunner>` with fields `detector, understand, retrieve, plan, render, feedback, session, cache, artifacts, ai, config`
  - `impl<R: CommandRunner> Engine<R> { async fn handle_line(&mut self, line: &str, tx: mpsc::Sender<EngineEvent>, cancel: CancelToken) -> Result<()>; async fn goal_check(&self, ctx: &GoalContext) -> Result<GoalVerdict>; }`

> Upstream constructor assumptions (all natural `new`/factory fns from the tasks
> above; if a task named one differently, adjust only the call site — the
> `Engine` code is unaffected): `Detector::new(runner, shell)`,
> `FeedbackLoop::new(runner, ai, max_corrections)`, `Understand::new(ai)`,
> `Retrieve::new(ai, search)`, `Plan::new(ai)`, `Render::new(ai)`,
> `SessionState::new()`, `PlanCache::new()`, `FailoverClient::new(a, b, clock)`,
> `CancelToken::new()`, and Task 3/5/7 fakes exposing
> `FakeAiProvider::always(&str)` (same response every call),
> `FakeAiProvider::scripted(Vec<String>)` (each response once, then repeats the
> last), `FakeClock::new()`, `FakeCommandRunner::always(RunOutcome)`,
> `FakeSearchProvider::empty()`. `Config`/`ProviderConfig` have public fields
> (spec §3: "Tests build `Config` directly").

---

#### Cycle A — `goal` module (`GoalContext` + `StepRecord`)

- [ ] **Step 1: Write the failing test.** Create `deathpwn-core/src/goal/mod.rs` with the types' test only referencing them, and the `#[cfg(test)]` block below. (The impl in Step 3 fills in the types.)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Mode;

    #[test]
    fn record_step_appends_history_and_counts() {
        let mut ctx = GoalContext::new("get a shell".to_string(), Mode::GoalCompletion);
        assert_eq!(ctx.steps_taken, 0);
        assert!(ctx.history.is_empty());

        ctx.record_step(StepRecord {
            command: "nmap -sV 10.0.0.5".to_string(),
            summary: "found ssh".to_string(),
        });
        ctx.record_step(StepRecord {
            command: "hydra -l root ssh://10.0.0.5".to_string(),
            summary: "no creds".to_string(),
        });

        assert_eq!(ctx.steps_taken, 2);
        assert_eq!(ctx.history.len(), 2);
        assert_eq!(ctx.history[0].command, "nmap -sV 10.0.0.5");
        assert_eq!(ctx.history[1].summary, "no creds");
        assert_eq!(ctx.mode, Mode::GoalCompletion);
    }
}
```

- [ ] **Step 2: Run test to verify it fails.** `cargo test -p deathpwn-core goal::tests::record_step_appends_history_and_counts`. Expected: fails to compile — `cannot find type GoalContext in this scope` (and `StepRecord`), plus `unresolved module goal` until `lib.rs` declares it.

- [ ] **Step 3: Implement.** Add `pub mod goal;` to `deathpwn-core/src/lib.rs`, then write the types at the top of `deathpwn-core/src/goal/mod.rs` (above the test module):

```rust
//! Goal-completion context threaded through the engine's multi-step loop.

use crate::schema::Mode;

/// One executed step in a goal-completion run, with a one-line outcome summary.
#[derive(Debug, Clone, PartialEq)]
pub struct StepRecord {
    pub command: String,
    pub summary: String,
}

/// Mutable context for a goal-completion session, threaded through the loop.
#[derive(Debug, Clone, PartialEq)]
pub struct GoalContext {
    pub goal_summary: String,
    pub mode: Mode,
    pub steps_taken: u32,
    pub history: Vec<StepRecord>,
}

impl GoalContext {
    /// Start a fresh context for the given goal and mode.
    pub fn new(goal_summary: String, mode: Mode) -> Self {
        Self {
            goal_summary,
            mode,
            steps_taken: 0,
            history: Vec::new(),
        }
    }

    /// Record one executed step and advance the step counter.
    pub fn record_step(&mut self, record: StepRecord) {
        self.history.push(record);
        self.steps_taken += 1;
    }
}
```

- [ ] **Step 4: Run test to verify it passes.** `cargo test -p deathpwn-core goal::tests::record_step_appends_history_and_counts`. Expected: `test goal::tests::record_step_appends_history_and_counts ... ok` (1 passed).

- [ ] **Step 5: Commit.** `git add deathpwn-core/src/goal/mod.rs deathpwn-core/src/lib.rs && git commit -m "feat(deathpwn): add GoalContext and StepRecord for the goal loop"`

---

#### Cycle B — `Engine` orchestrator + goal loop

Two behaviors are pinned together because they exercise one loop: the goal loop
must (1) stop when the AI reports the goal achieved, and (2) halt
unconditionally at the `max_goal_steps` safety cap. Both tests are written
first; the failing state is a compile error (the `engine` module does not exist
yet), after which the full orchestrator is implemented.

- [ ] **Step 6: Write the failing tests.** Create `deathpwn-core/src/engine.rs` containing only the `#[cfg(test)]` block below (the non-test code lands in Step 8). The shared `build_engine` helper wires every stage with fakes: the detector's runner always reports `command -v` failure so input is treated as `RawInput`; the feedback loop's runner always succeeds so no install/correction AI calls occur; each pipeline stage's `FailoverClient` returns canned stage JSON; the goal-check `FailoverClient` is scripted per test.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use tempfile::TempDir;

    use crate::clock::FakeClock;
    use crate::config::{Config, ProviderConfig};
    use crate::exec::FakeCommandRunner;
    use crate::providers::FakeAiProvider;
    use crate::search::FakeSearchProvider;

    const UNDERSTAND_JSON: &str = r#"{
        "intent": "gain a shell",
        "params": {"target": "10.0.0.5", "ports": null, "url": null, "extra": {}},
        "mode": "goal_completion",
        "goal_summary": "get a shell on 10.0.0.5"
    }"#;

    const KNOWLEDGE_JSON: &str = r#"{
        "theory": "enumerate services then exploit",
        "candidates": [
            {"tool": "nmap", "argv": ["-sV", "10.0.0.5"], "purpose": "scan services"}
        ]
    }"#;

    const PLAN_JSON: &str = r#"{
        "commands": [
            {"tool": "nmap", "argv": ["-sV", "10.0.0.5"], "purpose": "scan services", "depends_on_prev": false}
        ]
    }"#;

    // Empty sections parse regardless of RenderBody's serde representation (Task 2).
    const RENDER_JSON: &str = r#"{"sections": []}"#;

    const VERDICT_FALSE: &str =
        r#"{"achieved": false, "reason": "still working", "next_step_hint": "keep going"}"#;
    const VERDICT_TRUE: &str =
        r#"{"achieved": true, "reason": "shell obtained", "next_step_hint": null}"#;

    fn failover(response: &str) -> FailoverClient {
        FailoverClient::new(
            Arc::new(FakeAiProvider::always(response)),
            Arc::new(FakeAiProvider::always("{}")),
            Arc::new(FakeClock::new()),
        )
    }

    fn scripted_failover(responses: Vec<String>) -> FailoverClient {
        FailoverClient::new(
            Arc::new(FakeAiProvider::scripted(responses)),
            Arc::new(FakeAiProvider::always("{}")),
            Arc::new(FakeClock::new()),
        )
    }

    fn ok_outcome() -> RunOutcome {
        RunOutcome {
            exit: Some(0),
            stdout: "done".to_string(),
            stderr: String::new(),
            cancelled: false,
        }
    }

    fn missing_outcome() -> RunOutcome {
        RunOutcome {
            exit: Some(1),
            stdout: String::new(),
            stderr: String::new(),
            cancelled: false,
        }
    }

    fn build_engine(
        goal_check_responses: Vec<String>,
        max_goal_steps: u32,
    ) -> (Engine<FakeCommandRunner>, TempDir) {
        // Detector runner: `command -v` always non-zero -> every line is RawInput.
        let detector = Detector::new(
            FakeCommandRunner::always(missing_outcome()),
            "/bin/sh".to_string(),
        );
        // Feedback runner: availability check and command both succeed, so no
        // installer/correction AI calls happen during the loop.
        let feedback = FeedbackLoop::new(
            FakeCommandRunner::always(ok_outcome()),
            Arc::new(FakeAiProvider::always("{}")),
            2,
        );

        let understand = Understand::new(failover(UNDERSTAND_JSON));
        let retrieve = Retrieve::new(failover(KNOWLEDGE_JSON), Arc::new(FakeSearchProvider::empty()));
        let plan = Plan::new(failover(PLAN_JSON));
        let render = Render::new(failover(RENDER_JSON));

        let tmp = tempfile::tempdir().expect("tempdir");
        let clock = FakeClock::new();
        let artifacts = Artifacts::open(tmp.path().to_path_buf(), &clock).expect("artifacts");

        let config = Config {
            provider_a: ProviderConfig {
                url: "http://a".to_string(),
                key: "ka".to_string(),
                model: "ma".to_string(),
            },
            provider_b: ProviderConfig {
                url: "http://b".to_string(),
                key: "kb".to_string(),
                model: "mb".to_string(),
            },
            shell: "/bin/sh".to_string(),
            max_goal_steps,
            max_corrections: 2,
            artifacts_dir: tmp.path().to_path_buf(),
            http_timeout_secs: 30,
        };

        let engine = Engine::new(
            detector,
            understand,
            retrieve,
            plan,
            render,
            feedback,
            SessionState::new(),
            PlanCache::new(),
            artifacts,
            scripted_failover(goal_check_responses),
            config,
        );
        (engine, tmp)
    }

    fn count_rendered(rx: &mut mpsc::Receiver<EngineEvent>) -> usize {
        let mut count = 0;
        while let Ok(event) = rx.try_recv() {
            if let EngineEvent::Rendered(_) = event {
                count += 1;
            }
        }
        count
    }

    #[tokio::test]
    async fn goal_loop_stops_at_step_cap() {
        // goal_check always says "not achieved"; the cap must halt the loop.
        let (mut engine, _tmp) = build_engine(vec![VERDICT_FALSE.to_string()], 3);
        let (tx, mut rx) = mpsc::channel(1024);

        engine
            .handle_line("get a shell on 10.0.0.5", tx, CancelToken::new())
            .await
            .expect("handle_line ok");

        // Exactly max_goal_steps executions, then the cap stops it.
        assert_eq!(count_rendered(&mut rx), 3, "cap must halt a stuck goal loop");
    }

    #[tokio::test]
    async fn goal_loop_runs_until_achieved() {
        // false, false, then true -> exactly 3 rounds, well under the cap of 12.
        let (mut engine, _tmp) = build_engine(
            vec![
                VERDICT_FALSE.to_string(),
                VERDICT_FALSE.to_string(),
                VERDICT_TRUE.to_string(),
            ],
            12,
        );
        let (tx, mut rx) = mpsc::channel(1024);

        engine
            .handle_line("get a shell on 10.0.0.5", tx, CancelToken::new())
            .await
            .expect("handle_line ok");

        assert_eq!(count_rendered(&mut rx), 3, "loop must stop once goal achieved");
    }
}
```

- [ ] **Step 7: Run tests to verify they fail.** `cargo test -p deathpwn-core engine::tests`. Expected: fails to compile — `failed to resolve: use of undeclared crate or module engine` / `cannot find type Engine`, `EngineEvent`, etc. (the module and orchestrator do not exist yet).

- [ ] **Step 8: Implement.** Add `pub mod engine;` to `deathpwn-core/src/lib.rs` (below `pub mod goal;`), then write the full orchestrator at the top of `deathpwn-core/src/engine.rs`, above the test module from Step 6:

```rust
//! Engine: orchestrates Step 0 detection, the 4-stage pipeline, command
//! execution via the feedback loop, and the goal-completion loop. Streams
//! [`EngineEvent`]s to the UI over an `mpsc` channel.

use std::collections::BTreeMap;

use tokio::sync::mpsc;

use crate::cache::PlanCache;
use crate::cancel::CancelToken;
use crate::config::Config;
use crate::detector::{Detector, InputKind};
use crate::error::{DeathpwnError, Result};
use crate::exec::{CommandRunner, CommandSpec, FeedbackLoop, OutputLine, RunOutcome, Stream};
use crate::goal::{GoalContext, StepRecord};
use crate::pipeline::{Plan, Render, Retrieve, Understand};
use crate::providers::{ChatRequest, FailoverClient};
use crate::schema::{
    GoalVerdict, IntentParams, Mode, Stage1Understanding, Stage2Knowledge, Stage4Render,
};
use crate::session::{Artifacts, SessionState};

const GOAL_CHECK_SYSTEM: &str = "You are the goal-completion judge for an offensive-security \
assistant. Given the goal and the history of executed commands with their outcomes, decide \
whether the goal is achieved. Respond with ONLY a JSON object of the form \
{\"achieved\": <bool>, \"reason\": <string>, \"next_step_hint\": <string|null>} and nothing else.";

/// Events streamed from the engine to the UI over an `mpsc` channel.
#[derive(Debug)]
pub enum EngineEvent {
    /// Raw command output as it becomes available.
    Output(OutputLine),
    /// A structured, AI-rendered view of a command's result.
    Rendered(Stage4Render),
    /// A recoverable error, surfaced as text instead of crashing the UI.
    Error(String),
    /// The engine finished handling the current input line.
    Done,
}

/// Top-level orchestrator. Generic over the `CommandRunner` used by both the
/// Step 0 detector and the execution feedback loop.
pub struct Engine<R: CommandRunner> {
    detector: Detector<R>,
    understand: Understand,
    retrieve: Retrieve,
    plan: Plan,
    render: Render,
    feedback: FeedbackLoop<R>,
    session: SessionState,
    cache: PlanCache,
    artifacts: Artifacts,
    ai: FailoverClient,
    config: Config,
}

impl<R: CommandRunner> Engine<R> {
    /// Assemble an engine from its already-constructed components.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        detector: Detector<R>,
        understand: Understand,
        retrieve: Retrieve,
        plan: Plan,
        render: Render,
        feedback: FeedbackLoop<R>,
        session: SessionState,
        cache: PlanCache,
        artifacts: Artifacts,
        ai: FailoverClient,
        config: Config,
    ) -> Self {
        Self {
            detector,
            understand,
            retrieve,
            plan,
            render,
            feedback,
            session,
            cache,
            artifacts,
            ai,
            config,
        }
    }

    /// Handle one line of user input end to end, streaming events over `tx`.
    ///
    /// Recoverable pipeline errors are surfaced as [`EngineEvent::Error`] so the
    /// UI never crashes; a closed channel (receiver gone) returns
    /// [`DeathpwnError::Cancelled`].
    pub async fn handle_line(
        &mut self,
        line: &str,
        tx: mpsc::Sender<EngineEvent>,
        cancel: CancelToken,
    ) -> Result<()> {
        if let Err(e) = self.dispatch(line, &tx, cancel).await {
            tx.send(EngineEvent::Error(e.to_string()))
                .await
                .map_err(|_| DeathpwnError::Cancelled)?;
        }
        tx.send(EngineEvent::Done)
            .await
            .map_err(|_| DeathpwnError::Cancelled)?;
        Ok(())
    }

    /// Ask the AI whether the goal has been achieved given the current context.
    pub async fn goal_check(&self, ctx: &GoalContext) -> Result<GoalVerdict> {
        let history = ctx
            .history
            .iter()
            .map(|step| format!("- {} => {}", step.command, step.summary))
            .collect::<Vec<_>>()
            .join("\n");
        let req = ChatRequest {
            system: GOAL_CHECK_SYSTEM.to_string(),
            user: format!(
                "Goal: {}\nSteps taken so far: {}\nHistory:\n{}\n\nReturn the GoalVerdict JSON now.",
                ctx.goal_summary, ctx.steps_taken, history
            ),
            temperature: 0.0,
        };
        self.ai
            .complete_validated(&req, |content| {
                serde_json::from_str::<GoalVerdict>(content).map_err(|e| e.to_string())
            })
            .await
    }

    async fn dispatch(
        &mut self,
        line: &str,
        tx: &mpsc::Sender<EngineEvent>,
        cancel: CancelToken,
    ) -> Result<()> {
        match self.detector.classify(line).await {
            InputKind::DirectCommand => {
                let spec = parse_command(line)?;
                let understanding = direct_understanding(line);
                self.exec_and_render(&spec, line, &understanding, tx, cancel)
                    .await?;
            }
            InputKind::RawInput => {
                let understanding = self.understand.run(line, &self.session).await?;
                let knowledge = self.retrieve.run(&understanding).await?;
                match understanding.mode {
                    Mode::SingleCommand => {
                        let plan = self
                            .plan
                            .run(&understanding, &knowledge, &self.session, &mut self.cache)
                            .await?;
                        if let Some(first) = plan.commands.first() {
                            let spec = CommandSpec {
                                tool: first.tool.clone(),
                                argv: first.argv.clone(),
                            };
                            let label = first.purpose.clone();
                            self.exec_and_render(&spec, &label, &understanding, tx, cancel)
                                .await?;
                        }
                    }
                    Mode::GoalCompletion => {
                        self.run_goal_loop(&understanding, &knowledge, tx, cancel)
                            .await?;
                    }
                }
            }
        }
        Ok(())
    }

    async fn run_goal_loop(
        &mut self,
        understanding: &Stage1Understanding,
        knowledge: &Stage2Knowledge,
        tx: &mpsc::Sender<EngineEvent>,
        cancel: CancelToken,
    ) -> Result<()> {
        let mut ctx =
            GoalContext::new(understanding.goal_summary.clone(), understanding.mode.clone());

        // Safety cap (spec §9): stop unconditionally at max_goal_steps.
        while ctx.steps_taken < self.config.max_goal_steps {
            let plan = self
                .plan
                .run(understanding, knowledge, &self.session, &mut self.cache)
                .await?;
            let first = match plan.commands.first() {
                Some(cmd) => cmd,
                None => break,
            };
            let spec = CommandSpec {
                tool: first.tool.clone(),
                argv: first.argv.clone(),
            };
            let command = format!("{} {}", spec.tool, spec.argv.join(" "));
            let label = first.purpose.clone();

            let outcome = self
                .exec_and_render(&spec, &label, understanding, tx, cancel.clone())
                .await?;

            ctx.record_step(StepRecord {
                command,
                summary: summarize_outcome(&outcome),
            });

            let verdict = self.goal_check(&ctx).await?;
            if verdict.achieved {
                break;
            }
        }

        Ok(())
    }

    async fn exec_and_render(
        &mut self,
        spec: &CommandSpec,
        label: &str,
        understanding: &Stage1Understanding,
        tx: &mpsc::Sender<EngineEvent>,
        cancel: CancelToken,
    ) -> Result<RunOutcome> {
        let fb = self.feedback.run(spec, cancel).await?;

        let index = self.session.command_log().len();
        self.session.record_command(label);
        self.artifacts.write_output(index, &fb.outcome)?;

        if !fb.outcome.stdout.is_empty() {
            tx.send(EngineEvent::Output(OutputLine {
                stream: Stream::Stdout,
                text: fb.outcome.stdout.clone(),
            }))
            .await
            .map_err(|_| DeathpwnError::Cancelled)?;
        }

        let rendered = self.render.run(understanding, &fb.outcome).await?;
        tx.send(EngineEvent::Rendered(rendered))
            .await
            .map_err(|_| DeathpwnError::Cancelled)?;

        Ok(fb.outcome)
    }
}

/// Split a direct-command line into a `CommandSpec` (leading token = tool).
fn parse_command(line: &str) -> Result<CommandSpec> {
    let tokens = shell_words::split(line)
        .map_err(|e| DeathpwnError::Exec(format!("cannot parse command line: {e}")))?;
    let mut iter = tokens.into_iter();
    let tool = iter
        .next()
        .ok_or_else(|| DeathpwnError::Exec("empty command line".to_string()))?;
    Ok(CommandSpec {
        tool,
        argv: iter.collect(),
    })
}

/// Minimal understanding for a directly-typed command (no AI stage 1 needed).
fn direct_understanding(line: &str) -> Stage1Understanding {
    Stage1Understanding {
        intent: line.to_string(),
        params: IntentParams {
            target: None,
            ports: None,
            url: None,
            extra: BTreeMap::new(),
        },
        mode: Mode::SingleCommand,
        goal_summary: line.to_string(),
    }
}

/// One-line summary of a command outcome for the goal-check history.
fn summarize_outcome(outcome: &RunOutcome) -> String {
    match outcome.exit {
        Some(0) => "completed successfully".to_string(),
        Some(code) => format!("exited with code {code}"),
        None if outcome.cancelled => "cancelled by user".to_string(),
        None => "did not produce an exit code".to_string(),
    }
}
```

- [ ] **Step 9: Run tests to verify they pass.** `cargo test -p deathpwn-core engine::tests`. Expected: `test engine::tests::goal_loop_stops_at_step_cap ... ok` and `test engine::tests::goal_loop_runs_until_achieved ... ok` (2 passed). The cap test executes exactly 3 commands (3 `Rendered` events) before `steps_taken` hits the cap of 3; the achieved test executes exactly 3 (`false`, `false`, `true`) before breaking. Also run the whole crate to confirm no regression: `cargo test -p deathpwn-core`.

- [ ] **Step 10: Commit.** `git add deathpwn-core/src/engine.rs deathpwn-core/src/lib.rs && git commit -m "feat(deathpwn): add Engine orchestrator with goal loop and step cap"`
