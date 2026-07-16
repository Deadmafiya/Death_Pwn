//! Step 0 detector: decide *command* vs *raw natural-language input* the way a
//! shell would, without a wordlist. The leading token is resolved against the
//! user's shell via `command -v` so aliases, functions, and builtins count.

use crate::exec::CommandRunner;

/// Classification of a single input line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    /// The leading token resolves to a runnable command; run it directly.
    DirectCommand,
    /// Nothing runnable up front; treat the whole line as natural language.
    RawInput,
}

/// Resolves an input line to an [`InputKind`] using a [`CommandRunner`].
pub struct Detector<R: CommandRunner> {
    runner: R,
    shell: String,
}

impl<R: CommandRunner> Detector<R> {
    /// Build a detector over a command runner and the configured shell.
    pub fn new(runner: R, shell: String) -> Detector<R> {
        Detector { runner, shell }
    }

    /// The shell this detector is configured against.
    pub fn shell(&self) -> &str {
        &self.shell
    }

    /// Decide whether `line` is a direct command or raw natural-language input.
    pub async fn classify(&self, line: &str) -> InputKind {
        if line.trim().is_empty() {
            return InputKind::RawInput;
        }
        // Non-empty resolution is added in the next cycle; default to raw.
        InputKind::RawInput
    }
}

#[cfg(test)]
mod tests {
    use super::{Detector, InputKind};
    use crate::exec::FakeCommandRunner;

    #[tokio::test]
    async fn empty_line_is_raw_input() {
        let detector = Detector::new(FakeCommandRunner::new(), "/bin/sh".to_string());
        assert_eq!(detector.classify("").await, InputKind::RawInput);
    }

    #[tokio::test]
    async fn whitespace_only_line_is_raw_input() {
        let detector = Detector::new(FakeCommandRunner::new(), "/bin/sh".to_string());
        assert_eq!(detector.classify("   \t  ").await, InputKind::RawInput);
    }

    #[tokio::test]
    async fn detector_exposes_configured_shell() {
        let detector = Detector::new(FakeCommandRunner::new(), "/usr/bin/zsh".to_string());
        assert_eq!(detector.shell(), "/usr/bin/zsh");
    }
}
