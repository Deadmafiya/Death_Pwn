//! Execution boundary: the single trait through which every real OS process is
//! run, plus the value types crossing that boundary.

pub mod feedback;
pub mod installer;
pub mod runner;

pub use feedback::{AttemptLog, FeedbackLoop, FeedbackOutcome};
pub use runner::ShellRunner;

use async_trait::async_trait;

use crate::cancel::CancelToken;

/// A resolved command to execute: the tool plus its already-split arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub tool: String,
    pub argv: Vec<String>,
}

/// The result of running a command. `exit: None` together with
/// `cancelled: true` distinguishes a user abort from a real exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub exit: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub cancelled: bool,
}

/// Which pipe a streamed line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// A single line streamed live from a running process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLine {
    pub stream: Stream,
    pub text: String,
}

/// The one OS-process boundary. Implemented by [`ShellRunner`] in production and
/// by `FakeCommandRunner` in tests.
#[async_trait]
pub trait CommandRunner: Send + Sync {
    /// Run `spec` (tool + argv) through the shell, in its own process group.
    async fn run(&self, spec: &CommandSpec, cancel: CancelToken) -> RunOutcome;

    /// Run a raw shell string (used e.g. by the detector's `command -v`).
    async fn run_shell(&self, script: &str, cancel: CancelToken) -> RunOutcome;
}

#[cfg(any(test, feature = "test-support"))]
pub use test_support::FakeCommandRunner;

#[cfg(any(test, feature = "test-support"))]
mod test_support {
    use std::collections::{HashSet, VecDeque};
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use super::{CommandRunner, CommandSpec, RunOutcome};
    use crate::cancel::CancelToken;

    /// A comprehensive [`CommandRunner`] double serving the detector
    /// (availability probes), the feedback loop (separate run/shell queues plus
    /// availability), and the engine (a single constant outcome). Every input is
    /// recorded so consumers can assert on what was run.
    ///
    /// State is interior-shared via `Arc`, so a `.clone()` observes the same
    /// queues and call log — the feedback loop keeps its own handle while the
    /// test asserts on the original.
    #[derive(Clone, Default)]
    pub struct FakeCommandRunner {
        run_outcomes: Arc<Mutex<VecDeque<RunOutcome>>>,
        shell_outcomes: Arc<Mutex<VecDeque<RunOutcome>>>,
        available: Arc<Mutex<HashSet<String>>>,
        constant: Arc<Mutex<Option<RunOutcome>>>,
        run_calls: Arc<Mutex<Vec<CommandSpec>>>,
        shell_calls: Arc<Mutex<Vec<String>>>,
    }

    /// The fallback outcome for an exhausted queue: exit 0, empty streams, not cancelled.
    fn default_ok() -> RunOutcome {
        RunOutcome {
            exit: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            cancelled: false,
        }
    }

    impl FakeCommandRunner {
        /// Empty double: all queues empty, no constant, nothing available.
        pub fn new() -> Self {
            Self::default()
        }

        /// Pre-load `run()` outcomes to be returned in order.
        pub fn with_outcomes(outcomes: Vec<RunOutcome>) -> Self {
            let runner = Self::new();
            for o in outcomes {
                runner.push_run(o);
            }
            runner
        }

        /// Builder: mark `tool` as present for `command -v` probes.
        pub fn available(self, tool: impl Into<String>) -> Self {
            self.available.lock().unwrap().insert(tool.into());
            self
        }

        /// A double whose every `run()`/`run_shell()` returns `outcome` (wins even
        /// over the probe branch).
        pub fn always(outcome: RunOutcome) -> Self {
            let runner = Self::new();
            *runner.constant.lock().unwrap() = Some(outcome);
            runner
        }

        /// Queue one outcome for the next `run()` (alias of [`push_run`]).
        pub fn push(&self, outcome: RunOutcome) {
            self.push_run(outcome);
        }

        /// Queue one outcome for the next `run()`.
        pub fn push_run(&self, outcome: RunOutcome) {
            self.run_outcomes.lock().unwrap().push_back(outcome);
        }

        /// Queue one outcome for the next non-probe `run_shell()`.
        pub fn push_shell(&self, outcome: RunOutcome) {
            self.shell_outcomes.lock().unwrap().push_back(outcome);
        }

        /// All calls in order: `run()` specs rendered as `"tool arg…"` first, then
        /// `run_shell()` scripts.
        pub fn calls(&self) -> Vec<String> {
            let mut all: Vec<String> = self
                .run_calls
                .lock()
                .unwrap()
                .iter()
                .map(render_spec)
                .collect();
            all.extend(self.shell_calls.lock().unwrap().iter().cloned());
            all
        }

        /// Recorded `run()` specs, in call order.
        pub fn run_calls(&self) -> Vec<CommandSpec> {
            self.run_calls.lock().unwrap().clone()
        }

        /// Recorded `run_shell()` scripts, in call order.
        pub fn shell_calls(&self) -> Vec<String> {
            self.shell_calls.lock().unwrap().clone()
        }
    }

    /// Render a spec as `"tool arg1 arg2"`.
    fn render_spec(spec: &CommandSpec) -> String {
        std::iter::once(spec.tool.clone())
            .chain(spec.argv.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[async_trait]
    impl CommandRunner for FakeCommandRunner {
        async fn run(&self, spec: &CommandSpec, _cancel: CancelToken) -> RunOutcome {
            self.run_calls.lock().unwrap().push(spec.clone());
            if let Some(c) = self.constant.lock().unwrap().clone() {
                return c;
            }
            self.run_outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(default_ok)
        }

        async fn run_shell(&self, script: &str, _cancel: CancelToken) -> RunOutcome {
            self.shell_calls.lock().unwrap().push(script.to_string());
            if let Some(c) = self.constant.lock().unwrap().clone() {
                return c;
            }
            if script.contains("command -v") {
                // Availability probe: the tool is the last shell-word (leading
                // `--` stripped). Present → exit 0; absent → 127.
                let tool = script
                    .split_whitespace()
                    .last()
                    .map(|w| w.strip_prefix("--").unwrap_or(w))
                    .unwrap_or("");
                let present = self.available.lock().unwrap().contains(tool);
                return RunOutcome {
                    exit: Some(if present { 0 } else { 127 }),
                    stdout: String::new(),
                    stderr: String::new(),
                    cancelled: false,
                };
            }
            self.shell_outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(default_ok)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel::CancelToken;

    #[tokio::test]
    async fn fake_returns_scripted_outcomes_in_order() {
        let fake = FakeCommandRunner::new();
        fake.push(RunOutcome {
            exit: Some(0),
            stdout: "first".to_string(),
            stderr: String::new(),
            cancelled: false,
        });
        fake.push_shell(RunOutcome {
            exit: Some(1),
            stdout: String::new(),
            stderr: "boom".to_string(),
            cancelled: false,
        });

        let spec = CommandSpec {
            tool: "nmap".to_string(),
            argv: vec!["-sV".to_string(), "host".to_string()],
        };
        let a = fake.run(&spec, CancelToken::new()).await;
        assert_eq!(a.exit, Some(0));
        assert_eq!(a.stdout, "first");

        // A non-probe shell script pops the separate shell queue.
        let b = fake.run_shell("echo boom", CancelToken::new()).await;
        assert_eq!(b.exit, Some(1));
        assert_eq!(b.stderr, "boom");

        // Inputs are recorded so consumers (detector, feedback loop) can assert:
        // run() specs (rendered "tool arg…") first, then run_shell() scripts.
        let calls = fake.calls();
        assert_eq!(calls[0], "nmap -sV host");
        assert_eq!(calls[1], "echo boom");
        assert_eq!(fake.run_calls().len(), 1);
        assert_eq!(fake.shell_calls(), vec!["echo boom".to_string()]);
    }

    #[tokio::test]
    async fn fake_probe_reports_availability() {
        let fake = FakeCommandRunner::new().available("nmap");

        // Known tool → exit 0. The tool is the last shell-word, leading `--` stripped.
        let hit = fake
            .run_shell("command -v -- nmap", CancelToken::new())
            .await;
        assert_eq!(hit.exit, Some(0));

        // Unknown tool → 127 (the detector maps this to RawInput).
        let miss = fake
            .run_shell("command -v -- unknown", CancelToken::new())
            .await;
        assert_eq!(miss.exit, Some(127));
    }

    #[tokio::test]
    async fn fake_defaults_to_success_when_script_exhausted() {
        let fake = FakeCommandRunner::new();
        let out = fake.run_shell("echo hi", CancelToken::new()).await;
        assert_eq!(out.exit, Some(0));
        assert!(!out.cancelled);
        assert_eq!(out.stdout, "");
    }

    #[tokio::test]
    async fn fake_always_returns_constant() {
        let fake = FakeCommandRunner::always(RunOutcome {
            exit: Some(42),
            stdout: "k".to_string(),
            stderr: String::new(),
            cancelled: false,
        });
        let spec = CommandSpec {
            tool: "whatever".to_string(),
            argv: vec![],
        };
        assert_eq!(fake.run(&spec, CancelToken::new()).await.exit, Some(42));
        // Constant wins even over the probe branch.
        assert_eq!(
            fake.run_shell("command -v -- nmap", CancelToken::new())
                .await
                .exit,
            Some(42)
        );
    }

    #[test]
    fn output_line_carries_stream_and_text() {
        let line = OutputLine {
            stream: Stream::Stderr,
            text: "warn".to_string(),
        };
        assert_eq!(line.stream, Stream::Stderr);
        assert_eq!(line.text, "warn");
    }
}
