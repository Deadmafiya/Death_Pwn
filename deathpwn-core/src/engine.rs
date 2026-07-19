//! Engine: single-stage NL→command dispatch. Streams [`EngineEvent`]s to
//! the UI over an `mpsc` channel.

use tokio::sync::mpsc;

use crate::cancel::CancelToken;
use crate::error::{DeathpwnError, Result};
use crate::exec::{CommandRunner, CommandSpec, OutputLine, Stream};
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
    /// The current directory of the shell session has changed.
    Cwd(String),
    /// Surfaced when a command has been resolved by the AI but not executed.
    Resolved(String),
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
    preferences: std::collections::HashMap<String, String>,
    shell: String,
}

impl<R: CommandRunner> Engine<R> {
    /// Assemble an engine from its already-constructed components.
    pub fn new(
        runner: R,
        ai: FailoverClient,
        preferences: std::collections::HashMap<String, String>,
        shell: String,
    ) -> Self {
        Self {
            runner,
            ai,
            preferences,
            shell,
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
        resolve_only: bool,
        tx: mpsc::Sender<EngineEvent>,
        cancel: CancelToken,
    ) -> Result<()> {
        if resolve_only {
            if let Err(e) = self.resolve_only(line, &tx).await {
                tx.send(EngineEvent::Error(e.to_string()))
                    .await
                    .map_err(|_| DeathpwnError::Cancelled)?;
            }
        } else {
            if let Err(e) = self.dispatch(line, &tx, cancel).await {
                tx.send(EngineEvent::Error(e.to_string()))
                    .await
                    .map_err(|_| DeathpwnError::Cancelled)?;
            }
        }
        if let Some(cwd) = self.runner.get_cwd().await {
            let _ = tx
                .send(EngineEvent::Cwd(cwd.to_string_lossy().to_string()))
                .await;
        }
        let _ = self.send_phase(Phase::Idle, &tx).await;
        tx.send(EngineEvent::Done)
            .await
            .map_err(|_| DeathpwnError::Cancelled)?;
        Ok(())
    }

    async fn resolve_only(&mut self, line: &str, tx: &mpsc::Sender<EngineEvent>) -> Result<()> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(());
        }

        self.send_phase(Phase::Thinking, tx).await?;

        let mut system_prompt = "You are a BlackArch Linux pentesting zsh terminal. Output ONLY the exact shell command — no explanation, no markdown, no prose.\nCRITICAL RULE: User command preferences OVERRIDE all default tool selections.".to_string();
        if !self.preferences.is_empty() {
            let json_raw = serde_json::to_string_pretty(&self.preferences).unwrap_or_default();
            system_prompt.push_str("\n\nUSER COMMAND PREFERENCES:\nThe user has configured specific preferred commands for the tasks below. If the intent or concept of the user request relates to one of these tasks (even if rephrased or partially matching), you MUST use the preferred tool/command specified below (customizing arguments/targets as needed). NEVER substitute a preferred tool with another tool (e.g. do not substitute arp-scan with nmap) if a matching preference exists.\n\nConfigured Preferences:\n\n");
            system_prompt.push_str(&json_raw);
        }

        let req = ChatRequest {
            system: system_prompt,
            user: trimmed.to_string(),
            temperature: 0.0,
        };

        let spec = self.ai.complete_validated(&req, clean_command).await?;
        let mut words = vec![spec.tool.clone()];
        words.extend(spec.argv.clone());
        let cmd_line = shell_words::join(words);

        tx.send(EngineEvent::Resolved(cmd_line))
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

        // Check if the input resolves to a direct runnable command.
        let detector = crate::detector::Detector::new(&self.runner, self.shell.clone());
        if detector.classify(trimmed).await == crate::detector::InputKind::DirectCommand {
            if let Ok(tokens) = shell_words::split(trimmed) {
                let mut iter = tokens.into_iter();
                if let Some(tool) = iter.next() {
                    let spec = CommandSpec {
                        tool,
                        argv: iter.collect(),
                    };
                    let mut words = vec![spec.tool.clone()];
                    words.extend(spec.argv.clone());
                    let cmd_line = format!("$ {}", shell_words::join(words));
                    self.send_banner(cmd_line, tx).await?;
                    self.send_phase(
                        Phase::Executing {
                            tool: spec.tool.clone(),
                        },
                        tx,
                    )
                    .await?;

                    let (line_tx, mut line_rx) = mpsc::channel::<OutputLine>(256);
                    let event_tx_clone = tx.clone();
                    let forward_handle = tokio::spawn(async move {
                        while let Some(line) = line_rx.recv().await {
                            let _ = event_tx_clone.send(EngineEvent::Output(line)).await;
                        }
                    });
                    let _outcome = self.runner.run_streaming(&spec, line_tx, cancel).await;
                    let _ = forward_handle.await;
                    return Ok(());
                }
            }
        }

        // 1. AI: NL → command
        self.send_phase(Phase::Thinking, tx).await?;

        let mut system_prompt = "You are a BlackArch Linux pentesting terminal. Output ONLY the exact shell command — no explanation, no markdown, no prose.\nCRITICAL RULE: User command preferences OVERRIDE all default tool selections.".to_string();
        if !self.preferences.is_empty() {
            let json_raw = serde_json::to_string_pretty(&self.preferences).unwrap_or_default();
            system_prompt.push_str("\n\nUSER COMMAND PREFERENCES:\nThe user has configured specific preferred commands for the tasks below. If the intent or concept of the user request relates to one of these tasks (even if rephrased or partially matching), you MUST use the preferred tool/command specified below (customizing arguments/targets as needed). NEVER substitute a preferred tool with another tool (e.g. do not substitute arp-scan with nmap) if a matching preference exists.\n\nConfigured Preferences:\n\n");
            system_prompt.push_str(&json_raw);
        }

        let req = ChatRequest {
            system: system_prompt,
            user: trimmed.to_string(),
            temperature: 0.0,
        };

        let spec = self.ai.complete_validated(&req, clean_command).await?;

        // 2. Execute
        let mut words = vec![spec.tool.clone()];
        words.extend(spec.argv.clone());
        let cmd_line = format!("$ {}", shell_words::join(words));
        self.send_banner(cmd_line, tx).await?;

        self.send_phase(
            Phase::Executing {
                tool: spec.tool.clone(),
            },
            tx,
        )
        .await?;

        let (line_tx, mut line_rx) = mpsc::channel::<OutputLine>(256);
        let event_tx_clone = tx.clone();

        let forward_handle = tokio::spawn(async move {
            while let Some(line) = line_rx.recv().await {
                let _ = event_tx_clone.send(EngineEvent::Output(line)).await;
            }
        });

        let _outcome = self.runner.run_streaming(&spec, line_tx, cancel).await;
        let _ = forward_handle.await;

        Ok(())
    }

    async fn send_phase(&self, phase: Phase, tx: &mpsc::Sender<EngineEvent>) -> Result<()> {
        tx.send(EngineEvent::PhaseChange(phase))
            .await
            .map_err(|_| DeathpwnError::Cancelled)
    }

    async fn send_banner(
        &self,
        text: impl Into<String>,
        tx: &mpsc::Sender<EngineEvent>,
    ) -> Result<()> {
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
    let tokens = shell_words::split(cleaned).map_err(|e| format!("Invalid command syntax: {e}"))?;
    let mut iter = tokens.into_iter();
    let tool = iter
        .next()
        .ok_or_else(|| "Empty command line".to_string())?;
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
    use crate::exec::{FakeCommandRunner, RunOutcome};
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
        let a = Arc::new(FakeAiProvider::with_responses(vec![Ok(
            "nmap -sV 10.0.0.5".to_string(),
        )]));
        let b = Arc::new(FakeAiProvider::with_responses(vec![Ok(
            "nmap -sV 10.0.0.5".to_string(),
        )]));
        let ai = FailoverClient::new(a, b, Arc::new(FakeClock::fixed(0)));
        let runner = FakeCommandRunner::new();
        runner.push(ok_outcome());

        let mut engine = Engine::new(
            runner,
            ai,
            std::collections::HashMap::new(),
            "/bin/sh".to_string(),
        );

        let (tx, mut rx) = mpsc::channel(1024);
        engine
            .handle_line("scan network", false, tx, CancelToken::new())
            .await
            .unwrap();

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

    #[tokio::test]
    async fn preferences_sent_to_ai() {
        let a = Arc::new(FakeAiProvider::with_responses(vec![Ok(
            "sudo arp-scan --local".to_string(),
        )]));
        let b = Arc::new(FakeAiProvider::with_responses(vec![Ok(
            "sudo arp-scan --local".to_string(),
        )]));

        let ai = FailoverClient::new(a, b, Arc::new(FakeClock::fixed(0)));
        let runner = FakeCommandRunner::new();
        runner.push(ok_outcome());

        let mut prefs = std::collections::HashMap::new();
        prefs.insert(
            "host discovery".to_string(),
            "sudo arp-scan --local".to_string(),
        );

        let mut engine = Engine::new(runner, ai, prefs, "/bin/sh".to_string());

        let (tx, mut rx) = mpsc::channel(1024);
        engine
            .handle_line("Host Discovery", false, tx, CancelToken::new())
            .await
            .unwrap();

        let mut banner = String::new();
        while let Ok(event) = rx.try_recv() {
            if let EngineEvent::Output(line) = event {
                if line.stream == Stream::Banner {
                    banner = line.text;
                }
            }
        }
        assert!(
            banner.contains("sudo arp-scan --local"),
            "got banner: {banner}"
        );
    }

    #[tokio::test]
    async fn test_resolve_only_bypasses_detector() {
        let a = Arc::new(FakeAiProvider::with_responses(vec![Ok(
            "python3 -c 'print(2+2)'".to_string(),
        )]));
        let b = Arc::new(FakeAiProvider::with_responses(vec![Ok(
            "python3 -c 'print(2+2)'".to_string(),
        )]));
        let ai = FailoverClient::new(a, b, Arc::new(FakeClock::fixed(0)));
        let runner = FakeCommandRunner::new();
        let mut engine = Engine::new(
            runner,
            ai,
            std::collections::HashMap::new(),
            "/bin/sh".to_string(),
        );

        let (tx, mut rx) = mpsc::channel(1024);
        engine
            .handle_line(
                "python -c print('hello world') and a sum of 2 and 3",
                true,
                tx,
                CancelToken::new(),
            )
            .await
            .unwrap();

        let mut resolved = String::new();
        while let Ok(event) = rx.try_recv() {
            if let EngineEvent::Resolved(cmd) = event {
                resolved = cmd;
            }
        }
        assert_eq!(resolved, "python3 -c 'print(2+2)'");
    }
}
