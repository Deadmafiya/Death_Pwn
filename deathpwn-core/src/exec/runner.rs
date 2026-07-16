//! Real subprocess runner: `$SHELL -c <script>` in its own process group, with
//! SIGTERM→SIGKILL group cancellation and optional live line streaming.

use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use super::{CommandRunner, CommandSpec, OutputLine, RunOutcome, Stream};
use crate::cancel::CancelToken;

/// Grace period between SIGTERM and SIGKILL when cancelling.
const KILL_GRACE: Duration = Duration::from_millis(300);

/// Production [`CommandRunner`]. Optionally streams each output line on `tx`.
pub struct ShellRunner {
    shell: String,
    tx: Option<mpsc::Sender<OutputLine>>,
}

impl ShellRunner {
    /// A runner that only accumulates output (no live streaming).
    pub fn new(shell: String) -> Self {
        ShellRunner { shell, tx: None }
    }

    /// A runner that also emits each line on `tx` as it arrives.
    pub fn with_sender(shell: String, tx: mpsc::Sender<OutputLine>) -> Self {
        ShellRunner {
            shell,
            tx: Some(tx),
        }
    }

    async fn exec(&self, script: &str, cancel: CancelToken) -> RunOutcome {
        let mut child = match Command::new(&self.shell)
            .arg("-c")
            .arg(script)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                return RunOutcome {
                    exit: None,
                    stdout: String::new(),
                    stderr: format!("failed to spawn shell: {e}"),
                    cancelled: false,
                };
            }
        };

        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");
        // With process_group(0) the child is its own group leader, so the group
        // id equals the child pid; signalling -pid hits the whole group.
        let child_pid = child.id();

        let stdout_reader = spawn_reader(stdout, Stream::Stdout, self.tx.clone());
        let stderr_reader = spawn_reader(stderr, Stream::Stderr, self.tx.clone());

        let wait_fut = child.wait();
        tokio::pin!(wait_fut);

        let (cancelled, status) = tokio::select! {
            res = &mut wait_fut => (false, res.ok()),
            _ = cancel.cancelled() => {
                if let Some(pid) = child_pid {
                    let group = Pid::from_raw(-(pid as i32));
                    let _ = signal::kill(group, Signal::SIGTERM);
                    tokio::time::sleep(KILL_GRACE).await;
                    let _ = signal::kill(group, Signal::SIGKILL);
                }
                let _ = wait_fut.as_mut().await;
                (true, None)
            }
        };

        let stdout = stdout_reader.await.unwrap_or_default();
        let stderr = stderr_reader.await.unwrap_or_default();
        let exit = status.and_then(|s| s.code());

        RunOutcome {
            exit,
            stdout,
            stderr,
            cancelled,
        }
    }
}

#[async_trait]
impl CommandRunner for ShellRunner {
    async fn run(&self, spec: &CommandSpec, cancel: CancelToken) -> RunOutcome {
        let script = build_script(spec);
        self.exec(&script, cancel).await
    }

    async fn run_shell(&self, script: &str, cancel: CancelToken) -> RunOutcome {
        self.exec(script, cancel).await
    }
}

/// Read a child pipe line-by-line: accumulate the full text and, if a sender is
/// present, emit each line (newline trimmed) as an [`OutputLine`].
fn spawn_reader<R>(
    reader: R,
    stream: Stream,
    tx: Option<mpsc::Sender<OutputLine>>,
) -> tokio::task::JoinHandle<String>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buf = BufReader::new(reader);
        let mut acc = String::new();
        let mut line = String::new();
        loop {
            line.clear();
            match buf.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    acc.push_str(&line);
                    if let Some(tx) = &tx {
                        let text = line.trim_end_matches(['\n', '\r']).to_string();
                        let _ = tx.send(OutputLine { stream, text }).await;
                    }
                }
                Err(_) => break,
            }
        }
        acc
    })
}

/// POSIX single-quote a token: wrap in `'...'`, escaping embedded quotes as
/// `'\''`. Safe against spaces, globs, and injection.
fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Turn a [`CommandSpec`] into a single safely-quoted shell command line.
fn build_script(spec: &CommandSpec) -> String {
    let mut parts = Vec::with_capacity(spec.argv.len() + 1);
    parts.push(shell_quote(&spec.tool));
    for arg in &spec.argv {
        parts.push(shell_quote(arg));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel::CancelToken;
    use std::time::Duration;

    #[test]
    fn shell_quote_wraps_and_escapes_single_quotes() {
        assert_eq!(shell_quote("simple"), "'simple'");
        assert_eq!(shell_quote("with space"), "'with space'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn build_script_quotes_every_token() {
        let spec = CommandSpec {
            tool: "grep".to_string(),
            argv: vec!["-e".to_string(), "a b".to_string()],
        };
        assert_eq!(build_script(&spec), "'grep' '-e' 'a b'");
    }

    #[tokio::test]
    #[ignore = "spawns a real subprocess"]
    async fn shell_runner_runs_echo() {
        let runner = ShellRunner::new("/bin/sh".to_string());
        let out = runner
            .run_shell("echo hello", CancelToken::new())
            .await;
        assert_eq!(out.exit, Some(0));
        assert_eq!(out.stdout.trim_end(), "hello");
        assert!(!out.cancelled);
    }

    #[tokio::test]
    #[ignore = "spawns a real subprocess"]
    async fn shell_runner_streams_lines_on_tx() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let runner = ShellRunner::with_sender("/bin/sh".to_string(), tx);
        let out = runner
            .run_shell("printf 'a\\nb\\n'", CancelToken::new())
            .await;
        assert_eq!(out.exit, Some(0));

        let mut lines = Vec::new();
        while let Ok(line) = rx.try_recv() {
            assert_eq!(line.stream, Stream::Stdout);
            lines.push(line.text);
        }
        assert_eq!(lines, vec!["a".to_string(), "b".to_string()]);
    }

    #[tokio::test]
    #[ignore = "spawns a real subprocess"]
    async fn shell_runner_cancel_kills_the_process_group() {
        let runner = ShellRunner::new("/bin/sh".to_string());
        let cancel = CancelToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            trigger.cancel();
        });

        let out = runner.run_shell("sleep 30", cancel).await;
        assert!(out.cancelled);
        assert_eq!(out.exit, None);
    }
}
