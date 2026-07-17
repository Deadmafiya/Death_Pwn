//! Engine: single-stage NL→command dispatch. Streams [`EngineEvent`]s to
//! the UI over an `mpsc` channel.

use tokio::sync::mpsc;

use crate::cancel::CancelToken;
use crate::error::{DeathpwnError, Result};
use crate::exec::{CommandRunner, CommandSpec, OutputLine, RunOutcome, Stream};
use crate::providers::{ChatRequest, FailoverClient};
use crate::schema::Stage4Render;

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
    /// The engine entered a new phase (classifying, thinking, executing, etc.).
    PhaseChange(Phase),
    /// The engine finished handling the current input line.
    Done,
}

/// Visual phases the engine moves through during a pipeline run, shown in the
/// TUI status bar with an animated spinner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Thinking,
    Executing { tool: String },
}

impl Phase {
    /// Human-readable label for the status bar.
    pub fn label(&self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Thinking => "thinking...",
            Phase::Executing { .. } => "executing command...",
        }
    }

    /// Color key for the status bar — the TUI maps these to ratatui colors.
    pub fn color_key(&self) -> &'static str {
        match self {
            Phase::Idle => "darkgray",
            Phase::Thinking => "blue",
            Phase::Executing { .. } => "yellow",
        }
    }
}

/// Top-level orchestrator. Generic over the `CommandRunner` for executing
/// resolved commands.
pub struct Engine<R: CommandRunner> {
    runner: R,
    ai: FailoverClient,
}

impl<R: CommandRunner> Engine<R> {
    /// Assemble an engine from its already-constructed components.
    pub fn new(runner: R, ai: FailoverClient) -> Self {
        Self { runner, ai }
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
        let _ = self.send_phase(Phase::Idle, &tx).await;
        tx.send(EngineEvent::Done)
            .await
            .map_err(|_| DeathpwnError::Cancelled)?;
        Ok(())
    }

    async fn dispatch(
        &mut self,
        line: &str,
        tx: &mpsc::Sender<EngineEvent>,
        cancel: CancelToken,
    ) -> Result<()> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(());
        }

        // 1. AI: NL → command
        self.send_phase(Phase::Thinking, tx).await?;

        let req = ChatRequest {
            system: "You are a BlackArch Linux pentesting terminal. Output ONLY the exact shell command — no explanation, no markdown, no prose. Always use the most specific tool for the task.".to_string(),
            user: trimmed.to_string(),
            temperature: 0.0,
        };

        let spec = self.ai.complete_validated(&req, clean_command).await?;

        // 2. Execute
        let cmd_line = format!("$ {} {}", spec.tool, spec.argv.join(" "));
        self.send_banner(cmd_line, tx).await?;

        self.send_phase(Phase::Executing { tool: spec.tool.clone() }, tx).await?;
        let outcome = self.runner.run(&spec, cancel).await;
        self.stream_output(&outcome, tx).await?;

        Ok(())
    }

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

    async fn send_phase(&self, phase: Phase, tx: &mpsc::Sender<EngineEvent>) -> Result<()> {
        tx.send(EngineEvent::PhaseChange(phase))
            .await
            .map_err(|_| DeathpwnError::Cancelled)
    }

    async fn send_banner(&self, text: impl Into<String>, tx: &mpsc::Sender<EngineEvent>) -> Result<()> {
        tx.send(EngineEvent::Output(OutputLine {
            stream: Stream::Banner,
            text: text.into(),
        }))
        .await
        .map_err(|_| DeathpwnError::Cancelled)
    }
}

/// Strip code fences and leading $, then split into tool + argv
fn clean_command(content: &str) -> std::result::Result<CommandSpec, String> {
    let content = content.trim();
    if content.is_empty() {
        return Err("Empty response".to_string());
    }
    let mut cleaned = content;
    if cleaned.starts_with("```") {
        if let Some(newline_idx) = cleaned.find('\n') {
            cleaned = &cleaned[newline_idx + 1..];
        }
        if cleaned.ends_with("```") {
            cleaned = &cleaned[..cleaned.len() - 3];
        }
        cleaned = cleaned.trim();
    }
    let cleaned = cleaned.strip_prefix('$').unwrap_or(cleaned).trim();
    if cleaned.is_empty() {
        return Err("Command is empty after stripping markdown".to_string());
    }
    let tokens = shell_words::split(cleaned)
        .map_err(|e| format!("Invalid command syntax: {e}"))?;
    let mut iter = tokens.into_iter();
    let tool = iter.next().ok_or_else(|| "Empty command line".to_string())?;
    Ok(CommandSpec {
        tool,
        argv: iter.collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::clock::FakeClock;
    use crate::exec::FakeCommandRunner;
    use crate::providers::FakeAiProvider;

    fn ok_outcome() -> RunOutcome {
        RunOutcome {
            exit: Some(0),
            stdout: "done".to_string(),
            stderr: String::new(),
            cancelled: false,
        }
    }

    #[tokio::test]
    async fn dispatch_resolves_and_executes() {
        let a = Arc::new(FakeAiProvider::with_responses(vec![Ok("nmap -sV 10.0.0.5".to_string())]));
        let b = Arc::new(FakeAiProvider::with_responses(vec![Ok("nmap -sV 10.0.0.5".to_string())]));
        let ai = FailoverClient::new(a, b, Arc::new(FakeClock::fixed(0)));
        let runner = FakeCommandRunner::always(ok_outcome());

        let mut engine = Engine::new(runner, ai);

        let (tx, mut rx) = mpsc::channel(1024);
        engine.handle_line("scan network", tx, CancelToken::new()).await.unwrap();

        let mut banner = String::new();
        while let Ok(event) = rx.try_recv() {
            if let EngineEvent::Output(line) = event {
                if line.stream == Stream::Banner {
                    banner = line.text;
                }
            }
        }
        assert!(banner.contains("nmap -sV 10.0.0.5"), "got banner: {banner}");
    }
}
