### Task 7: exec — CommandRunner trait + ShellRunner + CancelToken

Foundational OS-process boundary. Everything that touches a real subprocess goes
through the `CommandRunner` trait defined here; the detector (Task 6) and the
feedback loop (Task 8) both consume this trait, so the exact signatures below are
load-bearing. This task lands the trait, the concrete `ShellRunner` (own process
group, SIGTERM→SIGKILL cancellation, optional line streaming), the `CancelToken`
primitive, and the `FakeCommandRunner` test-support double every downstream task
builds on.

**Files:**
- Create: `deathpwn-core/src/cancel.rs`  (core crate — `CancelToken`)
- Create: `deathpwn-core/src/exec/mod.rs`  (core crate — types, `CommandRunner` trait, `FakeCommandRunner`)
- Create: `deathpwn-core/src/exec/runner.rs`  (core crate — `ShellRunner` + shell-quoting helper)
- Modify: `deathpwn-core/Cargo.toml`  (add tokio / async-trait / nix deps)
- Modify: `deathpwn-core/src/lib.rs`  (declare + re-export the new modules)
- Test: unit tests live in `#[cfg(test)] mod tests` inside `cancel.rs`, `exec/mod.rs`, and `exec/runner.rs` (Rust convention; manifest specifies no separate test file). Real-subprocess tests in `runner.rs` are `#[ignore]`.

**Interfaces:**
- Consumes: nothing (foundational — this is the first exec task; only depends on the crate skeleton from Task 1).
- Produces (exact signatures downstream tasks rely on):
  - `struct CommandSpec { tool: String, argv: Vec<String> }`
  - `struct RunOutcome { exit: Option<i32>, stdout: String, stderr: String, cancelled: bool }`
  - `struct OutputLine { stream: Stream, text: String }`
  - `enum Stream { Stdout, Stderr }`
  - `#[derive(Clone)] struct CancelToken` with `fn new() -> Self`, `fn cancel(&self)`, `fn is_cancelled(&self) -> bool`, and `async fn cancelled(&self)`
  - `#[async_trait] trait CommandRunner: Send + Sync { async fn run(&self, spec: &CommandSpec, cancel: CancelToken) -> RunOutcome; async fn run_shell(&self, script: &str, cancel: CancelToken) -> RunOutcome; }`
  - `struct ShellRunner { shell: String, tx: Option<mpsc::Sender<OutputLine>> }` implementing `CommandRunner` (spawns `$SHELL -c <script>` via `tokio::process::Command` with `.process_group(0)`; SIGTERM→SIGKILL to the group on cancel; streams lines on `tx` if present)
  - test-support: `struct FakeCommandRunner` (scripted `RunOutcome` sequence, records inputs) implementing `CommandRunner`, gated `#[cfg(any(test, feature = "test-support"))]` and re-exported

---

#### Cycle A — dependencies + `CancelToken`

- [ ] **Step 1: Add the crate dependencies** — edit `deathpwn-core/Cargo.toml`. Append these exact lines under `[dependencies]` (keep the `thiserror` line from Task 1), and add the `test-support` feature so downstream tasks can pull in the fakes:

```toml
[dependencies]
thiserror = "1"
async-trait = "0.1"
tokio = { version = "1", features = ["process", "io-util", "sync", "rt", "time", "macros"] }
nix = { version = "0.29", features = ["signal", "process"] }

[features]
test-support = []
```

- [ ] **Step 2: Write the failing test** — create `deathpwn-core/src/cancel.rs` with only the test module for now (the type does not exist yet, so it must fail to compile):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn new_token_is_not_cancelled() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
    }

    #[tokio::test]
    async fn cancel_sets_flag_and_wakes_a_waiter() {
        let token = CancelToken::new();
        let waiter = token.clone();
        let handle = tokio::spawn(async move {
            waiter.cancelled().await;
        });

        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());

        // The awaiting future must complete once cancel() fires.
        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("cancelled() should resolve after cancel()")
            .expect("waiter task should not panic");
    }

    #[tokio::test]
    async fn cancelled_returns_immediately_when_already_cancelled() {
        let token = CancelToken::new();
        token.cancel();
        // Must resolve without blocking even though no waiter was registered first.
        tokio::time::timeout(Duration::from_millis(100), token.cancelled())
            .await
            .expect("already-cancelled token should resolve immediately");
    }

    #[tokio::test]
    async fn clones_share_state() {
        let a = CancelToken::new();
        let b = a.clone();
        a.cancel();
        assert!(b.is_cancelled());
    }
}
```

- [ ] **Step 3: Run test to verify it fails** — `cargo test -p deathpwn-core cancel`. Expected: **fails to compile** — `cannot find type CancelToken in this scope` / `CancelToken not found` (the type is not defined yet).

- [ ] **Step 4: Implement** — replace the contents of `deathpwn-core/src/cancel.rs` with the full implementation plus the test module. `CancelToken` wraps an `AtomicBool` + `Notify`; `cancelled()` registers the waiter (via `enable()`) *before* re-checking the flag so a racing `cancel()` cannot be missed:

```rust
//! Cooperative cancellation primitive shared across the exec boundary.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::Notify;

#[derive(Debug)]
struct Inner {
    cancelled: AtomicBool,
    notify: Notify,
}

/// A cheap, clonable cancellation handle. All clones share one state; calling
/// [`CancelToken::cancel`] on any clone flips the flag and wakes every task
/// currently awaiting [`CancelToken::cancelled`].
#[derive(Clone, Debug)]
pub struct CancelToken(Arc<Inner>);

impl CancelToken {
    /// Create a fresh, not-yet-cancelled token.
    pub fn new() -> Self {
        CancelToken(Arc::new(Inner {
            cancelled: AtomicBool::new(false),
            notify: Notify::new(),
        }))
    }

    /// Request cancellation. Idempotent. Wakes all current waiters.
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        self.0.notify.notify_waiters();
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    /// Resolve as soon as cancellation is requested. Returns immediately if the
    /// token is already cancelled.
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }

        let notified = self.0.notify.notified();
        tokio::pin!(notified);
        // Register this waiter before re-checking the flag: if cancel() ran
        // between our first check and here, notify_waiters() would otherwise
        // have found no waiter and we would block forever.
        notified.as_mut().enable();

        if self.is_cancelled() {
            return;
        }

        notified.await;
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        CancelToken::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn new_token_is_not_cancelled() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
    }

    #[tokio::test]
    async fn cancel_sets_flag_and_wakes_a_waiter() {
        let token = CancelToken::new();
        let waiter = token.clone();
        let handle = tokio::spawn(async move {
            waiter.cancelled().await;
        });

        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());

        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("cancelled() should resolve after cancel()")
            .expect("waiter task should not panic");
    }

    #[tokio::test]
    async fn cancelled_returns_immediately_when_already_cancelled() {
        let token = CancelToken::new();
        token.cancel();
        tokio::time::timeout(Duration::from_millis(100), token.cancelled())
            .await
            .expect("already-cancelled token should resolve immediately");
    }

    #[tokio::test]
    async fn clones_share_state() {
        let a = CancelToken::new();
        let b = a.clone();
        a.cancel();
        assert!(b.is_cancelled());
    }
}
```

  Then declare the module in `deathpwn-core/src/lib.rs`. Add these lines alongside the existing Task 1 module declarations (keep `#![forbid(unsafe_code)]` at the very top of the file):

```rust
pub mod cancel;

pub use cancel::CancelToken;
```

- [ ] **Step 5: Run test to verify it passes** — `cargo test -p deathpwn-core cancel`. Expected: **PASS** — 4 tests pass (`new_token_is_not_cancelled`, `cancel_sets_flag_and_wakes_a_waiter`, `cancelled_returns_immediately_when_already_cancelled`, `clones_share_state`).

- [ ] **Step 6: Commit** — `git add deathpwn-core/Cargo.toml deathpwn-core/src/cancel.rs deathpwn-core/src/lib.rs` && `git commit -m "feat(deathpwn): add CancelToken cooperative cancellation primitive"`.

---

#### Cycle B — exec types, `CommandRunner` trait, and `FakeCommandRunner`

- [ ] **Step 7: Write the failing test** — create `deathpwn-core/src/exec/mod.rs` with the type/trait skeleton absent and only the test module present, so it fails to compile. (Write the whole file now with just the tests; the impl comes in Step 9.)

```rust
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
        fake.push(RunOutcome {
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

        let b = fake.run_shell("command -v -- nmap", CancelToken::new()).await;
        assert_eq!(b.exit, Some(1));
        assert_eq!(b.stderr, "boom");

        // Inputs are recorded so consumers (detector, feedback loop) can assert.
        let calls = fake.calls();
        assert_eq!(calls[0], "nmap -sV host");
        assert_eq!(calls[1], "command -v -- nmap");
    }

    #[tokio::test]
    async fn fake_defaults_to_success_when_script_exhausted() {
        let fake = FakeCommandRunner::new();
        let out = fake.run_shell("command -v -- ls", CancelToken::new()).await;
        assert_eq!(out.exit, Some(0));
        assert!(!out.cancelled);
        assert_eq!(out.stdout, "");
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
```

- [ ] **Step 8: Run test to verify it fails** — `cargo test -p deathpwn-core exec::`. Expected: **fails to compile** — `cannot find type FakeCommandRunner` / `CommandSpec` / `RunOutcome` / `OutputLine` / `Stream` in this scope (none defined yet).

- [ ] **Step 9: Implement** — replace the contents of `deathpwn-core/src/exec/mod.rs` with the full types, the trait, and the gated `FakeCommandRunner`, keeping the same test module:

```rust
//! Execution boundary: the single trait through which every real OS process is
//! run, plus the value types crossing that boundary.

pub mod runner;

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
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::{CommandRunner, CommandSpec, RunOutcome};
    use crate::cancel::CancelToken;

    /// A scriptable [`CommandRunner`] double. Returns pushed outcomes in FIFO
    /// order (falling back to a success outcome when exhausted) and records the
    /// textual form of every input so consumers can assert on what was run.
    #[derive(Default)]
    pub struct FakeCommandRunner {
        scripted: Mutex<VecDeque<RunOutcome>>,
        calls: Mutex<Vec<String>>,
    }

    impl FakeCommandRunner {
        pub fn new() -> Self {
            Self::default()
        }

        /// Pre-load outcomes to be returned in order.
        pub fn with_outcomes(outcomes: Vec<RunOutcome>) -> Self {
            let runner = Self::new();
            for o in outcomes {
                runner.push(o);
            }
            runner
        }

        /// Queue one outcome for the next call.
        pub fn push(&self, outcome: RunOutcome) {
            self.scripted.lock().unwrap().push_back(outcome);
        }

        /// The textual inputs recorded so far, in call order.
        pub fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn next(&self, recorded: String) -> RunOutcome {
            self.calls.lock().unwrap().push(recorded);
            self.scripted
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| RunOutcome {
                    exit: Some(0),
                    stdout: String::new(),
                    stderr: String::new(),
                    cancelled: false,
                })
        }
    }

    #[async_trait]
    impl CommandRunner for FakeCommandRunner {
        async fn run(&self, spec: &CommandSpec, _cancel: CancelToken) -> RunOutcome {
            let recorded = std::iter::once(spec.tool.clone())
                .chain(spec.argv.iter().cloned())
                .collect::<Vec<_>>()
                .join(" ");
            self.next(recorded)
        }

        async fn run_shell(&self, script: &str, _cancel: CancelToken) -> RunOutcome {
            self.next(script.to_string())
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
        fake.push(RunOutcome {
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

        let b = fake.run_shell("command -v -- nmap", CancelToken::new()).await;
        assert_eq!(b.exit, Some(1));
        assert_eq!(b.stderr, "boom");

        let calls = fake.calls();
        assert_eq!(calls[0], "nmap -sV host");
        assert_eq!(calls[1], "command -v -- nmap");
    }

    #[tokio::test]
    async fn fake_defaults_to_success_when_script_exhausted() {
        let fake = FakeCommandRunner::new();
        let out = fake.run_shell("command -v -- ls", CancelToken::new()).await;
        assert_eq!(out.exit, Some(0));
        assert!(!out.cancelled);
        assert_eq!(out.stdout, "");
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
```

  Then declare and re-export the module in `deathpwn-core/src/lib.rs` (add alongside the `cancel` lines from Step 4):

```rust
pub mod exec;

pub use exec::{CommandRunner, CommandSpec, OutputLine, RunOutcome, ShellRunner, Stream};
```

- [ ] **Step 10: Run test to verify it passes** — `cargo test -p deathpwn-core exec::`. Expected: **PASS** — `fake_returns_scripted_outcomes_in_order`, `fake_defaults_to_success_when_script_exhausted`, and `output_line_carries_stream_and_text` all pass.

- [ ] **Step 11: Commit** — `git add deathpwn-core/src/exec/mod.rs deathpwn-core/src/lib.rs` && `git commit -m "feat(deathpwn): add CommandRunner trait, exec types, and FakeCommandRunner"`.

---

#### Cycle C — `ShellRunner` (shell quoting + real process group + cancellation)

- [ ] **Step 12: Write the failing test** — create `deathpwn-core/src/exec/runner.rs` with only the test module present. The pure `shell_quote` test runs by default; the three subprocess tests are `#[ignore]` so default `cargo test` stays deterministic (spec §13).

```rust
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
```

- [ ] **Step 13: Run test to verify it fails** — `cargo test -p deathpwn-core exec::runner`. Expected: **fails to compile** — `cannot find function shell_quote` / `build_script`, `cannot find type ShellRunner` in this scope (nothing implemented yet).

- [ ] **Step 14: Implement** — replace the contents of `deathpwn-core/src/exec/runner.rs` with the full `ShellRunner`. It spawns `$SHELL -c <script>` in a new process group, streams lines on `tx` when present, and on cancel sends SIGTERM then (after a grace window) SIGKILL to the whole group so child trees die. Keep the test module from Step 12.

```rust
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
```

- [ ] **Step 15: Run test to verify it passes** — `cargo test -p deathpwn-core exec::runner`. Expected: **PASS** — `shell_quote_wraps_and_escapes_single_quotes` and `build_script_quotes_every_token` pass; the three subprocess tests report as **ignored** (default run stays deterministic).

- [ ] **Step 16: Run the ignored integration tests to verify real execution** — `cargo test -p deathpwn-core exec::runner -- --ignored`. Expected: **PASS** — `shell_runner_runs_echo` (captures `hello`, exit 0), `shell_runner_streams_lines_on_tx` (emits `a`, `b` on the channel), and `shell_runner_cancel_kills_the_process_group` (cancels the `sleep`, `cancelled == true`, `exit == None`) all pass on a unix host with `/bin/sh`.

- [ ] **Step 17: Commit** — `git add deathpwn-core/src/exec/runner.rs` && `git commit -m "feat(deathpwn): add ShellRunner with process-group cancellation and line streaming"`.
