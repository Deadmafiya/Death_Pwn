//! Real subprocess runner: runs commands in a single, persistent background shell process,
//! preserving working directory, environment variables, and shell state.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;

use super::{CommandRunner, CommandSpec, OutputLine, RunOutcome, Stream};
use crate::cancel::CancelToken;

/// Grace period between SIGTERM and SIGKILL when cancelling.
const KILL_GRACE: Duration = Duration::from_millis(300);

struct PersistentSession {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    stderr: BufReader<tokio::process::ChildStderr>,
    pid: u32,
}

impl PersistentSession {
    fn spawn(shell: &str) -> std::io::Result<Self> {
        let mut child = Command::new(shell)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()?;

        let stdin = child.stdin.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "Failed to open stdin")
        })?;
        let stdout = BufReader::new(child.stdout.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "Failed to open stdout")
        })?);
        let stderr = BufReader::new(child.stderr.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "Failed to open stderr")
        })?);
        let pid = child.id().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "Failed to get child PID")
        })?;

        Ok(PersistentSession {
            child,
            stdin,
            stdout,
            stderr,
            pid,
        })
    }
}

/// Production [`CommandRunner`]. Optionally streams each output line on `tx`.
#[derive(Clone)]
pub struct ShellRunner {
    shell: String,
    tx: Option<mpsc::Sender<OutputLine>>,
    session: Arc<Mutex<Option<PersistentSession>>>,
}

impl ShellRunner {
    /// A runner that only accumulates output (no live streaming).
    pub fn new(shell: String) -> Self {
        ShellRunner {
            shell,
            tx: None,
            session: Arc::new(Mutex::new(None)),
        }
    }

    /// A runner that also emits each line on `tx` as it arrives.
    pub fn with_sender(shell: String, tx: mpsc::Sender<OutputLine>) -> Self {
        ShellRunner {
            shell,
            tx: Some(tx),
            session: Arc::new(Mutex::new(None)),
        }
    }

    async fn exec(&self, script: &str, cancel: CancelToken) -> RunOutcome {
        let mut session_guard = self.session.lock().await;

        let session = if let Some(ref mut s) = *session_guard {
            if let Ok(Some(_)) = s.child.try_wait() {
                match PersistentSession::spawn(&self.shell) {
                    Ok(new_s) => {
                        *s = new_s;
                        s
                    }
                    Err(e) => {
                        return RunOutcome {
                            exit: None,
                            stdout: String::new(),
                            stderr: format!("failed to restart shell: {e}"),
                            cancelled: false,
                        };
                    }
                }
            } else {
                s
            }
        } else {
            match PersistentSession::spawn(&self.shell) {
                Ok(new_s) => {
                    *session_guard = Some(new_s);
                    session_guard.as_mut().unwrap()
                }
                Err(e) => {
                    return RunOutcome {
                        exit: None,
                        stdout: String::new(),
                        stderr: format!("failed to spawn shell: {e}"),
                        cancelled: false,
                    };
                }
            }
        };

        // Write user command followed by stdout/stderr sentinels
        let run_cmd = format!(
            "{}\necho \"==DEATHPWN_STDOUT_DONE== $?\"\necho \"==DEATHPWN_STDERR_DONE==\" >&2\n",
            script
        );
        if let Err(e) = session.stdin.write_all(run_cmd.as_bytes()).await {
            return RunOutcome {
                exit: None,
                stdout: String::new(),
                stderr: format!("failed to write to shell stdin: {e}"),
                cancelled: false,
            };
        }
        if let Err(e) = session.stdin.flush().await {
            return RunOutcome {
                exit: None,
                stdout: String::new(),
                stderr: format!("failed to flush shell stdin: {e}"),
                cancelled: false,
            };
        }

        let stdout_reader = &mut session.stdout;
        let stderr_reader = &mut session.stderr;
        let tx_clone = self.tx.clone();
        let tx_clone2 = self.tx.clone();

        let run_fut = async move {
            let stdout_task = async move {
                let mut accumulated_stdout = String::new();
                let mut exit_code = None;
                let mut line = String::new();
                loop {
                    line.clear();
                    match stdout_reader.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
                            if line.contains("==DEATHPWN_STDOUT_DONE==") {
                                if let Some(pos) = line.find("==DEATHPWN_STDOUT_DONE==") {
                                    let rest = &line[pos + "==DEATHPWN_STDOUT_DONE==".len()..];
                                    if let Ok(code) = rest.trim().parse::<i32>() {
                                        exit_code = Some(code);
                                    }
                                    let prefix = &line[..pos];
                                    if !prefix.is_empty() {
                                        accumulated_stdout.push_str(prefix);
                                        if let Some(ref tx) = tx_clone {
                                            let text =
                                                prefix.trim_end_matches(['\n', '\r']).to_string();
                                            let _ = tx
                                                .send(OutputLine {
                                                    stream: Stream::Stdout,
                                                    text,
                                                })
                                                .await;
                                        }
                                    }
                                }
                                break;
                            }
                            accumulated_stdout.push_str(&line);
                            if let Some(ref tx) = tx_clone {
                                let text = line.trim_end_matches(['\n', '\r']).to_string();
                                let _ = tx
                                    .send(OutputLine {
                                        stream: Stream::Stdout,
                                        text,
                                    })
                                    .await;
                            }
                        }
                        Err(_) => break,
                    }
                }
                (accumulated_stdout, exit_code)
            };

            let stderr_task = async move {
                let mut accumulated_stderr = String::new();
                let mut line = String::new();
                loop {
                    line.clear();
                    match stderr_reader.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
                            if line.contains("==DEATHPWN_STDERR_DONE==") {
                                if let Some(pos) = line.find("==DEATHPWN_STDERR_DONE==") {
                                    let prefix = &line[..pos];
                                    if !prefix.is_empty() {
                                        accumulated_stderr.push_str(prefix);
                                        if let Some(ref tx) = tx_clone2 {
                                            let text =
                                                prefix.trim_end_matches(['\n', '\r']).to_string();
                                            let _ = tx
                                                .send(OutputLine {
                                                    stream: Stream::Stderr,
                                                    text,
                                                })
                                                .await;
                                        }
                                    }
                                }
                                break;
                            }
                            accumulated_stderr.push_str(&line);
                            if let Some(ref tx) = tx_clone2 {
                                let text = line.trim_end_matches(['\n', '\r']).to_string();
                                let _ = tx
                                    .send(OutputLine {
                                        stream: Stream::Stderr,
                                        text,
                                    })
                                    .await;
                            }
                        }
                        Err(_) => break,
                    }
                }
                accumulated_stderr
            };

            tokio::join!(stdout_task, stderr_task)
        };

        tokio::pin!(run_fut);

        let shell_pid = session.pid;
        let mut cancelled = false;
        let mut final_output = None;

        tokio::select! {
            res = run_fut.as_mut() => {
                final_output = Some(res);
            }
            _ = cancel.cancelled() => {
                cancelled = true;
                let descendants = get_all_descendant_pids(shell_pid);
                for pid in descendants {
                    let group = Pid::from_raw(-(pid as i32));
                    let _ = signal::kill(group, Signal::SIGTERM);
                    let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
                }

                tokio::time::sleep(KILL_GRACE).await;

                let descendants = get_all_descendant_pids(shell_pid);
                for pid in descendants {
                    let group = Pid::from_raw(-(pid as i32));
                    let _ = signal::kill(group, Signal::SIGKILL);
                    let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
                }

                if let Ok(res) = tokio::time::timeout(Duration::from_secs(2), run_fut.as_mut()).await {
                    final_output = Some(res);
                }
            }
        }

        let ((accumulated_stdout, exit_code), accumulated_stderr) =
            final_output.unwrap_or_else(|| ((String::new(), None), String::new()));

        RunOutcome {
            exit: if cancelled { None } else { exit_code },
            stdout: accumulated_stdout,
            stderr: accumulated_stderr,
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

    async fn run_streaming(
        &self,
        spec: &CommandSpec,
        tx: mpsc::Sender<OutputLine>,
        cancel: CancelToken,
    ) -> RunOutcome {
        let script = build_script(spec);
        let runner = ShellRunner {
            shell: self.shell.clone(),
            tx: Some(tx),
            session: self.session.clone(),
        };
        runner.exec(&script, cancel).await
    }

    async fn run_shell(&self, script: &str, cancel: CancelToken) -> RunOutcome {
        self.exec(script, cancel).await
    }

    async fn get_cwd(&self) -> Option<std::path::PathBuf> {
        let guard = self.session.lock().await;
        if let Some(ref session) = *guard {
            std::fs::read_link(format!("/proc/{}/cwd", session.pid)).ok()
        } else {
            std::env::current_dir().ok()
        }
    }

    async fn write_stdin(&self, input: &str) -> std::io::Result<()> {
        let mut guard = self.session.lock().await;
        if let Some(ref mut session) = *guard {
            use tokio::io::AsyncWriteExt;
            session.stdin.write_all(input.as_bytes()).await?;
            session.stdin.flush().await?;
        }
        Ok(())
    }
}

fn get_all_descendant_pids(parent_pid: u32) -> Vec<u32> {
    let mut descendants = Vec::new();
    let mut parents_to_check = vec![parent_pid];

    while !parents_to_check.is_empty() {
        let mut next_parents = Vec::new();
        if let Ok(entries) = std::fs::read_dir("/proc") {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_dir() {
                        let name = entry.file_name();
                        if let Some(name_str) = name.to_str() {
                            if let Ok(pid) = name_str.parse::<u32>() {
                                if let Ok(stat) =
                                    std::fs::read_to_string(format!("/proc/{}/stat", pid))
                                {
                                    if let Some(last_paren) = stat.rfind(')') {
                                        let rest = &stat[last_paren + 1..];
                                        let mut parts = rest.split_whitespace();
                                        let _state = parts.next();
                                        if let Some(ppid_str) = parts.next() {
                                            if let Ok(ppid) = ppid_str.parse::<u32>() {
                                                if parents_to_check.contains(&ppid) {
                                                    descendants.push(pid);
                                                    next_parents.push(pid);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        parents_to_check = next_parents;
    }
    descendants
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
        let out = runner.run_shell("echo hello", CancelToken::new()).await;
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
