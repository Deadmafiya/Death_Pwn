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
}
