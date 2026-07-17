//! Engine: orchestrates Step 0 detection, the 4-stage pipeline, command
//! execution via the feedback loop, and the goal-completion loop. Streams
//! [`EngineEvent`]s to the UI over an `mpsc` channel.

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
    GoalVerdict, Mode, PlannedCommand, RenderBody, Stage1Understanding, Stage2Knowledge,
    Stage4Render,
};
use crate::session::{Artifacts, Finding, SessionState, Target};

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
    /// Progress update for the status bar: the target under work (if known) and
    /// the number of commands executed so far this run (GOAL §9 status line).
    Progress { target: Option<String>, step: u32 },
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
                // Step 0 resolved this to a real command: run it in the shell
                // with the feedback loop, but do NOT spend an AI call rendering
                // it — a plain terminal command shows its own output (GOAL §3).
                let spec = parse_command(line)?;
                self.exec_direct(&spec, line, tx, cancel).await?;
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
                        // Execute the FULL planned chain, not just the first
                        // command (GOAL §3: a single request may still expand to
                        // an ordered sequence).
                        self.exec_chain(&plan.commands, &understanding, tx, cancel)
                            .await?;
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
        // The AI's suggestion for what to try next round, carried across
        // iterations so each step builds on the last (GOAL §5).
        let mut next_hint: Option<String> = None;

        // Safety cap (spec §9): stop unconditionally at max_goal_steps.
        while ctx.steps_taken < self.config.max_goal_steps {
            // Ctrl+X / Ctrl+C: stop the whole chain promptly (GOAL §6).
            if cancel.is_cancelled() {
                break;
            }

            // Plan the NEXT step from the evolving history + last hint — this is
            // uncached and history-aware, so the loop advances instead of
            // repeating the first command (GOAL §3/§5).
            let history: Vec<(String, String)> = ctx
                .history
                .iter()
                .map(|s| (s.command.clone(), s.summary.clone()))
                .collect();
            let plan = self
                .plan
                .next_step(
                    understanding,
                    knowledge,
                    &self.session,
                    &history,
                    next_hint.as_deref(),
                )
                .await?;
            if plan.commands.is_empty() {
                break;
            }

            // Execute the whole planned chain for this round, recording each
            // command in the goal history.
            for command in &plan.commands {
                if cancel.is_cancelled() {
                    break;
                }
                let spec = CommandSpec {
                    tool: command.tool.clone(),
                    argv: command.argv.clone(),
                };
                let rendered = format!("{} {}", spec.tool, spec.argv.join(" "));
                let outcome = self
                    .exec_and_render(&spec, &command.purpose, understanding, tx, cancel.clone())
                    .await?;
                ctx.record_step(StepRecord {
                    command: rendered,
                    summary: summarize_outcome(&outcome),
                });
                if ctx.steps_taken >= self.config.max_goal_steps {
                    break;
                }
            }

            if cancel.is_cancelled() {
                break;
            }

            // Goal-achieved check; keep the hint to steer the next round (GOAL §5).
            let verdict = self.goal_check(&ctx).await?;
            if verdict.achieved {
                break;
            }
            next_hint = verdict.next_step_hint;
        }

        Ok(())
    }

    /// Run a directly-typed shell command through the feedback loop and stream
    /// its raw output — no AI Stage-4 render (GOAL §3: resolved commands execute
    /// immediately in the shell, no AI).
    async fn exec_direct(
        &mut self,
        spec: &CommandSpec,
        label: &str,
        tx: &mpsc::Sender<EngineEvent>,
        cancel: CancelToken,
    ) -> Result<RunOutcome> {
        let fb = self.feedback.run(spec, cancel).await?;
        let index = self.session.command_log().len();
        self.session.record_command(label);
        self.artifacts.write_output(index, &fb.outcome)?;
        self.stream_output(&fb.outcome, tx).await?;
        self.send_progress(tx).await?;
        Ok(fb.outcome)
    }

    /// Execute an ordered plan chain, rendering each command's result.
    async fn exec_chain(
        &mut self,
        commands: &[PlannedCommand],
        understanding: &Stage1Understanding,
        tx: &mpsc::Sender<EngineEvent>,
        cancel: CancelToken,
    ) -> Result<()> {
        for command in commands {
            if cancel.is_cancelled() {
                break;
            }
            let spec = CommandSpec {
                tool: command.tool.clone(),
                argv: command.argv.clone(),
            };
            self.exec_and_render(&spec, &command.purpose, understanding, tx, cancel.clone())
                .await?;
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

        // Fold discovered target/params into session state so later stages and
        // follow-ups resolve without re-stating them (GOAL §7).
        self.ingest_params(understanding);

        self.stream_output(&fb.outcome, tx).await?;

        let rendered = self.render.run(understanding, &fb.outcome).await?;
        // Fold structured findings into the session before handing the render to
        // the UI (GOAL §7: remembers prior findings).
        self.ingest_render(&rendered);
        self.send_progress(tx).await?;
        tx.send(EngineEvent::Rendered(rendered))
            .await
            .map_err(|_| DeathpwnError::Cancelled)?;

        Ok(fb.outcome)
    }

    /// Emit a status-bar progress update reflecting the latest target and the
    /// number of commands run so far this session (GOAL §9).
    async fn send_progress(&self, tx: &mpsc::Sender<EngineEvent>) -> Result<()> {
        let target = self.session.targets().last().map(|t| t.value.clone());
        let step = self.session.command_log().len() as u32;
        tx.send(EngineEvent::Progress { target, step })
            .await
            .map_err(|_| DeathpwnError::Cancelled)
    }

    /// Stream a command's stdout/stderr to the UI as `Output` events.
    async fn stream_output(
        &self,
        outcome: &RunOutcome,
        tx: &mpsc::Sender<EngineEvent>,
    ) -> Result<()> {
        if !outcome.stdout.is_empty() {
            tx.send(EngineEvent::Output(OutputLine {
                stream: Stream::Stdout,
                text: outcome.stdout.clone(),
            }))
            .await
            .map_err(|_| DeathpwnError::Cancelled)?;
        }
        if !outcome.stderr.is_empty() {
            tx.send(EngineEvent::Output(OutputLine {
                stream: Stream::Stderr,
                text: outcome.stderr.clone(),
            }))
            .await
            .map_err(|_| DeathpwnError::Cancelled)?;
        }
        Ok(())
    }

    /// Record the request's concrete target/url into the session (GOAL §7).
    fn ingest_params(&mut self, understanding: &Stage1Understanding) {
        if let Some(target) = understanding
            .params
            .target
            .as_deref()
            .filter(|t| !t.trim().is_empty())
        {
            self.session.add_target(Target {
                value: target.to_string(),
            });
        }
        if let Some(url) = understanding
            .params
            .url
            .as_deref()
            .filter(|u| !u.trim().is_empty())
        {
            self.session.add_target(Target {
                value: url.to_string(),
            });
        }
    }

    /// Fold a Stage-4 render's structured findings into the session so the goal
    /// loop and follow-ups can reason over what was discovered (GOAL §7).
    fn ingest_render(&mut self, render: &Stage4Render) {
        for section in &render.sections {
            if let RenderBody::Findings(items) = &section.body {
                for item in items {
                    self.session.add_finding(Finding {
                        severity: item.severity.clone(),
                        title: item.title.clone(),
                        detail: item.detail.clone(),
                    });
                }
            }
        }
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

/// One-line summary of a command outcome for the goal-check history.
fn summarize_outcome(outcome: &RunOutcome) -> String {
    match outcome.exit {
        Some(0) => "completed successfully".to_string(),
        Some(code) => format!("exited with code {code}"),
        None if outcome.cancelled => "cancelled by user".to_string(),
        None => "did not produce an exit code".to_string(),
    }
}

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
            Arc::new(FakeClock::fixed(0)),
        )
    }

    fn scripted_failover(responses: Vec<String>) -> FailoverClient {
        FailoverClient::new(
            Arc::new(FakeAiProvider::scripted_ok(responses)),
            Arc::new(FakeAiProvider::always("{}")),
            Arc::new(FakeClock::fixed(0)),
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
        let clock = FakeClock::fixed(0);
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

    /// Drain the channel, returning (rendered_count, output_count).
    fn count_events(rx: &mut mpsc::Receiver<EngineEvent>) -> (usize, usize) {
        let (mut rendered, mut output) = (0, 0);
        while let Ok(event) = rx.try_recv() {
            match event {
                EngineEvent::Rendered(_) => rendered += 1,
                EngineEvent::Output(_) => output += 1,
                _ => {}
            }
        }
        (rendered, output)
    }

    /// Like `build_engine` but lets a test choose the plan JSON and whether the
    /// Step 0 detector resolves the input as a real command (DirectCommand) —
    /// `available("ls")` makes the `command -v` probe succeed.
    fn build_engine_full(
        goal_check_responses: Vec<String>,
        max_goal_steps: u32,
        plan_json: &str,
        detector_resolves: bool,
    ) -> (Engine<FakeCommandRunner>, TempDir) {
        let detector_runner = if detector_resolves {
            FakeCommandRunner::always(ok_outcome()).available("ls")
        } else {
            FakeCommandRunner::always(missing_outcome())
        };
        let detector = Detector::new(detector_runner, "/bin/sh".to_string());
        let feedback = FeedbackLoop::new(
            FakeCommandRunner::always(ok_outcome()),
            Arc::new(FakeAiProvider::always("{}")),
            2,
        );
        let understand = Understand::new(failover(UNDERSTAND_JSON));
        let retrieve =
            Retrieve::new(failover(KNOWLEDGE_JSON), Arc::new(FakeSearchProvider::empty()));
        let plan = Plan::new(failover(plan_json));
        let render = Render::new(failover(RENDER_JSON));
        let tmp = tempfile::tempdir().expect("tempdir");
        let artifacts =
            Artifacts::open(tmp.path().to_path_buf(), &FakeClock::fixed(0)).expect("artifacts");
        let config = Config {
            provider_a: ProviderConfig {
                url: "http://a".into(),
                key: "ka".into(),
                model: "ma".into(),
            },
            provider_b: ProviderConfig {
                url: "http://b".into(),
                key: "kb".into(),
                model: "mb".into(),
            },
            shell: "/bin/sh".into(),
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

    #[tokio::test]
    async fn goal_loop_stops_at_step_cap() {
        // goal_check always says "not achieved"; the cap must halt the loop.
        // The loop calls goal_check once per iteration (max_goal_steps times), so
        // the scripted queue must cover all 3 calls — the strict FakeAiProvider
        // panics on exhaustion (see FAKE-CONTRACT), and provider A panicking never
        // falls over to B. (Brief under-provisioned this to 1 response.)
        let (mut engine, _tmp) = build_engine(vec![VERDICT_FALSE.to_string(); 3], 3);
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

    // A single-command request whose plan expands to a 3-command chain must run
    // ALL three, not just the first (GOAL §3). UNDERSTAND_JSON is goal_completion,
    // so this test overrides the understanding to single_command via a plan with
    // multiple commands and a mode-single understanding.
    const UNDERSTAND_SINGLE_JSON: &str = r#"{
        "intent": "scan the host",
        "params": {"target": "10.0.0.5", "ports": null, "url": null, "extra": {}},
        "mode": "single_command",
        "goal_summary": "scan 10.0.0.5"
    }"#;

    const PLAN_CHAIN_JSON: &str = r#"{
        "commands": [
            {"tool": "nmap", "argv": ["-sn", "10.0.0.5"], "purpose": "ping sweep", "depends_on_prev": false},
            {"tool": "nmap", "argv": ["-sV", "10.0.0.5"], "purpose": "service scan", "depends_on_prev": true},
            {"tool": "whatweb", "argv": ["10.0.0.5"], "purpose": "web fingerprint", "depends_on_prev": true}
        ]
    }"#;

    #[tokio::test]
    async fn single_command_mode_executes_full_chain() {
        // Understand returns single_command; the plan has 3 commands. All 3 must
        // run → 3 Rendered events. No goal_check calls in single-command mode.
        let detector = Detector::new(
            FakeCommandRunner::always(missing_outcome()),
            "/bin/sh".to_string(),
        );
        let feedback = FeedbackLoop::new(
            FakeCommandRunner::always(ok_outcome()),
            Arc::new(FakeAiProvider::always("{}")),
            2,
        );
        let understand = Understand::new(failover(UNDERSTAND_SINGLE_JSON));
        let retrieve =
            Retrieve::new(failover(KNOWLEDGE_JSON), Arc::new(FakeSearchProvider::empty()));
        let plan = Plan::new(failover(PLAN_CHAIN_JSON));
        let render = Render::new(failover(RENDER_JSON));
        let tmp = tempfile::tempdir().expect("tempdir");
        let artifacts =
            Artifacts::open(tmp.path().to_path_buf(), &FakeClock::fixed(0)).expect("artifacts");
        let config = Config {
            provider_a: ProviderConfig {
                url: "http://a".into(),
                key: "ka".into(),
                model: "ma".into(),
            },
            provider_b: ProviderConfig {
                url: "http://b".into(),
                key: "kb".into(),
                model: "mb".into(),
            },
            shell: "/bin/sh".into(),
            max_goal_steps: 12,
            max_corrections: 2,
            artifacts_dir: tmp.path().to_path_buf(),
            http_timeout_secs: 30,
        };
        let mut engine = Engine::new(
            detector,
            understand,
            retrieve,
            plan,
            render,
            feedback,
            SessionState::new(),
            PlanCache::new(),
            artifacts,
            failover("{}"),
            config,
        );
        let (tx, mut rx) = mpsc::channel(1024);
        engine
            .handle_line("scan 10.0.0.5", tx, CancelToken::new())
            .await
            .expect("handle_line ok");
        assert_eq!(
            count_rendered(&mut rx),
            3,
            "all three planned commands must execute"
        );
    }

    #[tokio::test]
    async fn direct_command_skips_ai_render() {
        // Detector resolves the token → DirectCommand → shell exec, NO Stage-4
        // render (GOAL §3). We expect Output events but zero Rendered events.
        let (mut engine, _tmp) =
            build_engine_full(vec![], 12, PLAN_JSON, /* detector_resolves */ true);
        let (tx, mut rx) = mpsc::channel(1024);
        engine
            .handle_line("ls -la", tx, CancelToken::new())
            .await
            .expect("handle_line ok");
        let (rendered, output) = count_events(&mut rx);
        assert_eq!(rendered, 0, "direct commands must not invoke the AI renderer");
        assert!(output >= 1, "direct command output must be streamed");
    }

    #[tokio::test]
    async fn cancelled_token_halts_goal_loop_immediately() {
        // A pre-cancelled token must stop the loop before any command runs
        // (GOAL §6: Ctrl+X stops the whole chain).
        let (mut engine, _tmp) = build_engine(vec![VERDICT_FALSE.to_string(); 3], 3);
        let (tx, mut rx) = mpsc::channel(1024);
        let cancel = CancelToken::new();
        cancel.cancel();
        engine
            .handle_line("get a shell on 10.0.0.5", tx, cancel)
            .await
            .expect("handle_line ok");
        assert_eq!(
            count_rendered(&mut rx),
            0,
            "a cancelled token must stop the loop before executing anything"
        );
    }
}
