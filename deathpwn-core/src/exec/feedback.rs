//! The execution feedback loop: availability check, auto-install, run, and
//! AI-driven self-correction on non-zero exit (GOAL.md §4 / spec §6).

use std::sync::Arc;

use crate::cancel::CancelToken;
use crate::config::Config;
use crate::error::{DeathpwnError, Result};
use crate::exec::{CommandRunner, CommandSpec, RunOutcome};
use crate::providers::{AiProvider, ChatRequest};
use crate::schema::{ExecFailureVerdict, FailureClass};

const CLASSIFY_SYSTEM: &str = "You are an exit-code triage engine. Given a failed \
shell command, its exit code, and its stderr/stdout, reply with ONLY a JSON object \
matching {\"class\": one of not_found|benign_empty|fixable_usage|transient|fatal, \
\"corrected_argv\": array of strings or null}. Use fixable_usage with a corrected_argv \
when the command has a usage/flag error you can repair. No prose.";

/// One logged execution attempt inside the feedback loop.
#[derive(Debug, Clone)]
pub struct AttemptLog {
    pub argv: Vec<String>,
    pub exit: Option<i32>,
    pub note: String,
}

/// Final result of a feedback-loop run: the terminal outcome plus the full attempt trail.
#[derive(Debug, Clone)]
pub struct FeedbackOutcome {
    pub outcome: RunOutcome,
    pub attempts: Vec<AttemptLog>,
}

/// Wraps a `CommandRunner` with availability checks, auto-install, and
/// AI-driven self-correction (GOAL.md §4 / spec §6).
///
/// The classify/install AI steps use dual-provider failover (GOAL.md §8): the
/// primary `ai` is tried first, and `ai_b` (when present) is tried on a provider
/// error. Construct with [`with_failover`](Self::with_failover) to enable it.
pub struct FeedbackLoop<R: CommandRunner> {
    runner: R,
    ai: Arc<dyn AiProvider>,
    ai_b: Option<Arc<dyn AiProvider>>,
    max_corrections: u32,
}

impl<R: CommandRunner> FeedbackLoop<R> {
    pub fn new(runner: R, ai: Arc<dyn AiProvider>, max_corrections: u32) -> Self {
        Self {
            runner,
            ai,
            ai_b: None,
            max_corrections,
        }
    }

    /// Like [`new`](Self::new) but with a fallback provider for the loop's AI
    /// steps (classify + install-resolve), matching the dual-provider policy of
    /// every pipeline stage (GOAL.md §8).
    pub fn with_failover(
        runner: R,
        ai: Arc<dyn AiProvider>,
        ai_b: Arc<dyn AiProvider>,
        max_corrections: u32,
    ) -> Self {
        Self {
            runner,
            ai,
            ai_b: Some(ai_b),
            max_corrections,
        }
    }

    pub fn from_config(runner: R, ai: Arc<dyn AiProvider>, config: &Config) -> Self {
        Self {
            runner,
            ai,
            ai_b: None,
            max_corrections: config.max_corrections,
        }
    }

    pub async fn run(&self, spec: &CommandSpec, cancel: CancelToken) -> Result<FeedbackOutcome> {
        let mut attempts: Vec<AttemptLog> = Vec::new();
        let mut current = spec.clone();
        let mut corrections: u32 = 0;
        let mut installs: u32 = 0;

        // 1. availability check → auto-install on miss (not counted as a correction).
        if !self.is_available(&current.tool, &cancel).await {
            match self.install(&current.tool, &cancel).await {
                Ok(note) => {
                    installs += 1;
                    attempts.push(AttemptLog {
                        argv: vec![format!("<install {}>", current.tool)],
                        exit: None,
                        note,
                    });
                }
                Err(e) => {
                    attempts.push(AttemptLog {
                        argv: vec![format!("<install {}>", current.tool)],
                        exit: None,
                        note: format!("install skipped: {e}"),
                    });
                }
            }
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
                    match self.install(&current.tool, &cancel).await {
                        Ok(note) => {
                            installs += 1;
                            attempts.push(AttemptLog {
                                argv: vec![format!("<install {}>", current.tool)],
                                exit: None,
                                note,
                            });
                        }
                        Err(e) => {
                            attempts.push(AttemptLog {
                                argv: vec![format!("<install {}>", current.tool)],
                                exit: None,
                                note: format!("install failed: {e}"),
                            });
                            return Ok(FeedbackOutcome { outcome, attempts });
                        }
                    }
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
                        note: format!(
                            "fixable_usage: retry {}/{}",
                            corrections, self.max_corrections
                        ),
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
        }
    }

    async fn is_available(&self, tool: &str, cancel: &CancelToken) -> bool {
        let script = format!("command -v -- {tool}");
        let out = self.runner.run_shell(&script, cancel.clone()).await;
        out.exit == Some(0)
    }

    /// Complete a request against provider A, failing over to provider B (when
    /// configured) if A returns a provider-level error (GOAL.md §8: every AI
    /// call — including the feedback loop's classify/install steps — is
    /// resilient). Aggregates both errors when neither succeeds.
    async fn complete_failover(&self, req: &ChatRequest) -> Result<String> {
        let first = self.ai.complete(req).await;
        match first {
            Ok(content) => Ok(content),
            Err(err_a) => match &self.ai_b {
                Some(b) => b.complete(req).await.map_err(|err_b| {
                    DeathpwnError::Provider(format!("A: {err_a:?}; B: {err_b:?}"))
                }),
                None => Err(DeathpwnError::Provider(format!("{err_a:?}"))),
            },
        }
    }

    async fn install(&self, tool: &str, cancel: &CancelToken) -> Result<String> {
        let req = crate::exec::installer::install_request(tool);
        let raw = self.complete_failover(&req).await?;
        let script = crate::exec::installer::sanitize_install(&raw);
        if script.is_empty() {
            return Err(DeathpwnError::Exec(format!(
                "no install command produced for `{tool}`"
            )));
        }
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
        let raw = self.complete_failover(&req).await?;
        let json = extract_json(raw.trim());
        let verdict: ExecFailureVerdict = serde_json::from_str(&json)
            .map_err(|e| DeathpwnError::Schema(format!("exec failure verdict parse: {e}")))?;
        Ok(verdict)
    }
}

/// Extract the first complete JSON object from a string that may have trailing
/// text or markdown. Finds the outermost `{...}` pair, counting nesting depth.
fn extract_json(raw: &str) -> String {
    let start = match raw.find('{') {
        Some(idx) => idx,
        None => return raw.to_string(),
    };
    let mut depth = 0u32;
    let mut end = start;
    for (i, ch) in raw[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    if end > start {
        raw[start..end].to_string()
    } else {
        raw.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel::CancelToken;
    use crate::exec::{CommandSpec, FakeCommandRunner, RunOutcome};
    use crate::providers::{FakeAiProvider, ProviderError};
    use std::sync::Arc;

    fn ok(stdout: &str) -> RunOutcome {
        RunOutcome {
            exit: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
            cancelled: false,
        }
    }
    fn fail(code: i32, stderr: &str) -> RunOutcome {
        RunOutcome {
            exit: Some(code),
            stdout: String::new(),
            stderr: stderr.to_string(),
            cancelled: false,
        }
    }
    fn fixable_json() -> String {
        r#"{"class":"fixable_usage","corrected_argv":["nmap","-sV","10.0.0.1"]}"#.to_string()
    }

    #[tokio::test]
    async fn fixable_usage_applies_correction_and_retries() {
        let runner = FakeCommandRunner::new().available("nmap");
        runner.push_run(fail(2, "unrecognized option '--badflag'"));
        runner.push_run(ok("Nmap scan report for 10.0.0.1"));
        let ai = Arc::new(FakeAiProvider::scripted(vec![Ok::<String, ProviderError>(
            fixable_json(),
        )]));

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
            vec![
                "nmap".to_string(),
                "-sV".to_string(),
                "10.0.0.1".to_string()
            ],
            "retry uses corrected argv"
        );
        assert!(out
            .attempts
            .iter()
            .any(|a| a.note.contains("fixable_usage")));
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
        let spec = CommandSpec {
            tool: "nmap".into(),
            argv: vec!["nmap".into(), "x".into()],
        };
        let out = fb.run(&spec, CancelToken::new()).await.unwrap();

        assert_eq!(
            out.outcome.exit,
            Some(2),
            "returns the last failing outcome"
        );
        assert_eq!(
            runner.run_calls().len(),
            3,
            "initial + 2 corrections, then stop"
        );
        assert_eq!(ai.calls(), 3, "classify each failing run until cap");
        assert!(out.attempts.iter().any(|a| a.note.contains("cap")));
    }

    #[tokio::test]
    async fn missing_tool_is_installed_then_run() {
        // No `.available(...)` → `command -v` reports the tool missing.
        let runner = FakeCommandRunner::new();
        runner.push_shell(ok("")); // install command result (run_shell, not `command -v`)
        runner.push_run(ok("Nmap scan report for 10.0.0.1")); // the retried command
        let ai = Arc::new(FakeAiProvider::scripted(vec![Ok::<String, ProviderError>(
            "pacman -S --noconfirm nmap".to_string(),
        )]));

        let fb = FeedbackLoop::new(runner.clone(), ai.clone(), 2);
        let spec = CommandSpec {
            tool: "nmap".into(),
            argv: vec!["nmap".into(), "-sV".into(), "10.0.0.1".into()],
        };
        let out = fb.run(&spec, CancelToken::new()).await.unwrap();

        assert_eq!(out.outcome.exit, Some(0));
        assert_eq!(
            ai.calls(),
            1,
            "only the install resolution call, no classify"
        );
        assert!(out
            .attempts
            .iter()
            .any(|a| a.note.contains("installed via")));
        assert!(runner.shell_calls().iter().any(|s| s.contains("pacman -S")));
    }

    #[tokio::test]
    async fn benign_empty_is_reported_without_retry() {
        let runner = FakeCommandRunner::new().available("grep");
        runner.push_run(fail(1, ""));
        let ai = Arc::new(FakeAiProvider::scripted(vec![Ok::<String, ProviderError>(
            r#"{"class":"benign_empty","corrected_argv":null}"#.to_string(),
        )]));

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
        let ai = Arc::new(FakeAiProvider::scripted(vec![Ok::<String, ProviderError>(
            r#"{"class":"fatal","corrected_argv":null}"#.to_string(),
        )]));

        let fb = FeedbackLoop::new(runner.clone(), ai.clone(), 2);
        let spec = CommandSpec {
            tool: "nmap".into(),
            argv: vec!["nmap".into(), "10.0.0.1".into()],
        };
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
        let ai = Arc::new(FakeAiProvider::scripted(vec![Ok::<String, ProviderError>(
            r#"{"class":"transient","corrected_argv":null}"#.to_string(),
        )]));

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

    #[tokio::test]
    async fn classify_fails_over_to_provider_b() {
        // Provider A errors on the classify call; the loop must fail over to B
        // (GOAL §8) rather than aborting the command. B's verdict (fixable_usage)
        // then drives a corrected retry that succeeds.
        let runner = FakeCommandRunner::new().available("nmap");
        runner.push_run(fail(2, "unrecognized option '--badflag'"));
        runner.push_run(ok("Nmap scan report for 10.0.0.1"));
        let a = Arc::new(FakeAiProvider::scripted(vec![
            Err::<String, ProviderError>(ProviderError::RateLimit),
        ]));
        let b = Arc::new(FakeAiProvider::scripted(vec![Ok::<String, ProviderError>(
            fixable_json(),
        )]));

        let fb = FeedbackLoop::with_failover(runner.clone(), a.clone(), b.clone(), 2);
        let spec = CommandSpec {
            tool: "nmap".into(),
            argv: vec!["nmap".into(), "--badflag".into(), "10.0.0.1".into()],
        };
        let out = fb.run(&spec, CancelToken::new()).await.unwrap();

        assert_eq!(out.outcome.exit, Some(0), "corrected retry succeeds");
        assert_eq!(a.call_count(), 1, "provider A was tried");
        assert_eq!(b.call_count(), 1, "provider B handled the failover");
        assert_eq!(runner.run_calls().len(), 2, "initial run + corrected retry");
    }

    #[tokio::test]
    async fn classify_without_fallback_surfaces_provider_error() {
        // No fallback configured: a provider error on classify is surfaced as a
        // DeathpwnError rather than silently swallowed.
        let runner = FakeCommandRunner::new().available("nmap");
        runner.push_run(fail(2, "boom"));
        let ai = Arc::new(FakeAiProvider::scripted(vec![
            Err::<String, ProviderError>(ProviderError::Timeout),
        ]));

        let fb = FeedbackLoop::new(runner.clone(), ai.clone(), 2);
        let spec = CommandSpec {
            tool: "nmap".into(),
            argv: vec!["nmap".into(), "x".into()],
        };
        let err = fb.run(&spec, CancelToken::new()).await.unwrap_err();
        assert!(matches!(err, DeathpwnError::Provider(_)));
    }
}
