//! Step 0 detector: decide *command* vs *raw natural-language input* the way a
//! shell would, without a wordlist. The leading token is resolved against the
//! user's shell via `command -v` so aliases, functions, and builtins count.

use crate::cancel::CancelToken;
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

    /// Expose the underlying command runner.
    pub fn runner(&self) -> &R {
        &self.runner
    }

    /// Decide whether `line` is a direct command or raw natural-language input.
    ///
    /// Empty/whitespace-only lines are raw. Otherwise the leading token
    /// (honoring shell quoting) is resolved via `command -v -- <token>` run
    /// through the configured shell; exit `0` means the token is a real
    /// executable, builtin, function, or alias → [`InputKind::DirectCommand`].
    pub async fn classify(&self, line: &str) -> InputKind {
        if line.trim().is_empty() {
            return InputKind::RawInput;
        }
        // Extract the leading token, respecting quotes. Unbalanced quotes or a
        // line that yields no first token → nothing runnable up front.
        let token = match shell_words::split(line) {
            Ok(tokens) => match tokens.into_iter().next() {
                Some(t) if !t.is_empty() => t,
                _ => return InputKind::RawInput,
            },
            Err(_) => return InputKind::RawInput,
        };
        // Quote the token so a token with shell metacharacters cannot alter the
        // probe command, then resolve it via the runner's shell.
        let script = format!("command -v -- {}", shell_words::quote(&token));
        let outcome = self.runner.run_shell(&script, CancelToken::new()).await;
        match outcome.exit {
            Some(0) => InputKind::DirectCommand,
            _ => InputKind::RawInput,
        }
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

    #[tokio::test]
    async fn known_command_is_direct_command() {
        let runner = FakeCommandRunner::new().available("nmap");
        let detector = Detector::new(runner, "/bin/sh".to_string());
        assert_eq!(
            detector.classify("nmap -sV 10.0.0.1").await,
            InputKind::DirectCommand
        );
    }

    #[tokio::test]
    async fn unknown_leading_token_is_raw_input() {
        // No scripted resolution → miss default (exit 127) → RawInput.
        let runner = FakeCommandRunner::new();
        let detector = Detector::new(runner, "/bin/sh".to_string());
        assert_eq!(
            detector.classify("scan the target for open ports").await,
            InputKind::RawInput
        );
    }

    #[tokio::test]
    async fn leading_token_drives_decision_across_pipe() {
        // `foobar` is unknown even though the line parses as a shell construct.
        // `baz` is available, but it is not the leading token so it is never probed.
        let runner = FakeCommandRunner::new().available("baz");
        let detector = Detector::new(runner, "/bin/sh".to_string());
        assert_eq!(detector.classify("foobar | baz").await, InputKind::RawInput);
    }

    #[tokio::test]
    async fn quoted_leading_token_is_tokenized_before_resolution() {
        // The leading token is supplied in quoted form; `shell_words` must strip
        // the quotes so the bare `nmap` is what gets probed via `command -v`.
        //
        // NOTE: this deviates from the plan's original assertion, which used a
        // multi-word available tool (`"my scanner"`). That case is unsatisfiable
        // against the committed Task 7 `FakeCommandRunner`, whose probe parser
        // extracts the tool via `split_whitespace().last()` and so can never
        // match a tool containing a space (and `shell_words::quote` leaves stray
        // quote chars on the last word). Production `classify` still shell-quotes
        // the token before resolution — that guards against metacharacter
        // injection into the probe; it just isn't observable through this fake.
        let runner = FakeCommandRunner::new().available("nmap");
        let detector = Detector::new(runner, "/bin/sh".to_string());
        assert_eq!(
            detector.classify("\"nmap\" -sV 10.0.0.1").await,
            InputKind::DirectCommand
        );
    }

    #[tokio::test]
    async fn unbalanced_quotes_are_raw_input() {
        let runner = FakeCommandRunner::new();
        let detector = Detector::new(runner, "/bin/sh".to_string());
        assert_eq!(
            detector.classify("echo \"unterminated").await,
            InputKind::RawInput
        );
    }
}
