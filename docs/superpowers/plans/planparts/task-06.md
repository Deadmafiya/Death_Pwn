### Task 6: detector — Step 0 (command-vs-raw resolution)

**Files:**
- Create: `deathpwn-core/src/detector/mod.rs` (core crate; `InputKind`, `Detector`, unit tests in a `#[cfg(test)] mod tests` in the same file)
- Modify: `deathpwn-core/src/lib.rs` (core crate; add `pub mod detector;`)
- Modify: `deathpwn-core/Cargo.toml` (core crate; add `shell-words` dependency)
- Test: inline `#[cfg(test)] mod tests` in `deathpwn-core/src/detector/mod.rs`

**Interfaces:**

- Consumes (from Task 7 — `deathpwn-core/src/exec/` — author against these exact signatures, Task 7 lands first):
  - `struct RunOutcome { exit: Option<i32>, stdout: String, stderr: String, cancelled: bool }`
  - `struct CommandSpec { tool: String, argv: Vec<String> }` (part of the trait surface; not called directly here)
  - `#[derive(Clone)] struct CancelToken(/* … */)` with `fn CancelToken::new() -> CancelToken`, `fn cancel(&self)`, `fn is_cancelled(&self) -> bool`, and an async `cancelled()` future. Task 6 uses `CancelToken::new()` only (a fresh, un-cancelled token for the `command -v` probe).
  - `#[async_trait] trait CommandRunner: Send + Sync { async fn run(&self, spec: &CommandSpec, cancel: CancelToken) -> RunOutcome; async fn run_shell(&self, script: &str, cancel: CancelToken) -> RunOutcome; }` — Task 6 calls only `run_shell`.
  - test-support (from Task 7, re-exported for cross-task use): `struct FakeCommandRunner` scripting `input → RunOutcome`. Task 6 relies on this builder surface: `FakeCommandRunner::new()` and `.on_shell(script: &str, outcome: RunOutcome) -> Self` (exact-match map on the `run_shell` script argument); any unmatched `run_shell` call returns the miss default `RunOutcome { exit: Some(127), stdout: String::new(), stderr: String::new(), cancelled: false }` (POSIX "command not found"). This is the "scripted by token→exit" fake named in the manifest.

- Produces (later tasks — Task 15 `engine.rs` — rely on these):
  - `enum InputKind { DirectCommand, RawInput }` (`#[derive(Debug, Clone, Copy, PartialEq, Eq)]`)
  - `struct Detector<R: CommandRunner> { runner: R, shell: String }`
  - `fn Detector::<R>::new(runner: R, shell: String) -> Detector<R>`
  - `fn Detector::<R>::shell(&self) -> &str`
  - `async fn Detector::<R>::classify(&self, line: &str) -> InputKind` — empty/whitespace-only line → `RawInput`; otherwise extract the leading token via `shell_words`, run `command -v -- <token>` through `runner.run_shell`; exit `0` → `DirectCommand`, anything else → `RawInput`.

---

#### Cycle 1 — enum, constructor, and empty/whitespace default

- [ ] **Step 1: Add the `shell-words` dependency**

  Edit `deathpwn-core/Cargo.toml`, adding to the existing `[dependencies]` table:

  ```toml
  [dependencies]
  shell-words = "1.1"
  ```

  (`tokio` with the `macros` and `rt` features is already a `[dev-dependencies]` entry from Task 3 and provides `#[tokio::test]`; no change needed there.)

- [ ] **Step 2: Write the failing test** — register the module and add the first tests.

  Edit `deathpwn-core/src/lib.rs`, adding the module declaration alongside the other `pub mod` lines:

  ```rust
  pub mod detector;
  ```

  Create `deathpwn-core/src/detector/mod.rs` with only the test module (the referenced types do not exist yet, so this must fail to compile):

  ```rust
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
  ```

- [ ] **Step 3: Run test to verify it fails**

  ```
  cargo test -p deathpwn-core detector
  ```

  Expected: fails to compile — `error[E0432]: unresolved import 'super::...'` / `cannot find type 'Detector' in this scope` and `cannot find type 'InputKind' in this scope` (neither type is defined yet).

- [ ] **Step 4: Implement** — the enum, the struct, its constructor/accessor, and a conservative `classify` that only decides the empty case (non-empty defaults to `RawInput` until Cycle 2 adds resolution).

  Prepend to `deathpwn-core/src/detector/mod.rs`, above the `#[cfg(test)] mod tests` block:

  ```rust
  //! Step 0 detector: decide *command* vs *raw natural-language input* the way a
  //! shell would, without a wordlist. The leading token is resolved against the
  //! user's shell via `command -v` so aliases, functions, and builtins count.

  use crate::exec::{CancelToken, CommandRunner};

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
  ```

- [ ] **Step 5: Run test to verify it passes**

  ```
  cargo test -p deathpwn-core detector
  ```

  Expected: PASS — `empty_line_is_raw_input`, `whitespace_only_line_is_raw_input`, and `detector_exposes_configured_shell` all green (3 passed).

- [ ] **Step 6: Commit**

  ```
  git add deathpwn-core/Cargo.toml deathpwn-core/src/lib.rs deathpwn-core/src/detector/mod.rs
  git commit -m "feat(deathpwn): add Step 0 detector skeleton with empty-input handling"
  ```

---

#### Cycle 2 — resolve the leading token via `command -v`

- [ ] **Step 7: Write the failing test** — add resolution behaviors: known command → `DirectCommand`, unknown leading token → `RawInput`, the leading token (not the whole line) drives the decision through a pipe, quoted tokens are resolved with correct shell-quoting, and unbalanced quotes fall back to `RawInput`.

  Add these tests inside the existing `#[cfg(test)] mod tests` block in `deathpwn-core/src/detector/mod.rs`, and extend its imports to `use crate::exec::{FakeCommandRunner, RunOutcome};`:

  ```rust
      #[tokio::test]
      async fn known_command_is_direct_command() {
          let runner = FakeCommandRunner::new().on_shell(
              "command -v -- nmap",
              RunOutcome {
                  exit: Some(0),
                  stdout: "/usr/bin/nmap\n".to_string(),
                  stderr: String::new(),
                  cancelled: false,
              },
          );
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
          let runner = FakeCommandRunner::new().on_shell(
              "command -v -- baz",
              RunOutcome {
                  exit: Some(0),
                  stdout: String::new(),
                  stderr: String::new(),
                  cancelled: false,
              },
          );
          let detector = Detector::new(runner, "/bin/sh".to_string());
          assert_eq!(
              detector.classify("foobar | baz").await,
              InputKind::RawInput
          );
      }

      #[tokio::test]
      async fn quoted_leading_token_is_shell_quoted_before_resolution() {
          let runner = FakeCommandRunner::new().on_shell(
              "command -v -- 'my scanner'",
              RunOutcome {
                  exit: Some(0),
                  stdout: String::new(),
                  stderr: String::new(),
                  cancelled: false,
              },
          );
          let detector = Detector::new(runner, "/bin/sh".to_string());
          assert_eq!(
              detector.classify("'my scanner' --fast").await,
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
  ```

- [ ] **Step 8: Run test to verify it fails**

  ```
  cargo test -p deathpwn-core detector
  ```

  Expected: compiles, but `known_command_is_direct_command` and `quoted_leading_token_is_shell_quoted_before_resolution` FAIL with `assertion 'left == right' failed: left: RawInput, right: DirectCommand` — the conservative Cycle 1 `classify` returns `RawInput` for every non-empty line and never calls the runner.

- [ ] **Step 9: Implement** — replace `classify` with the full resolver: split with `shell_words`, take the leading non-empty token, shell-quote it, probe `command -v -- <token>` through `run_shell`, and map exit `0` → `DirectCommand`.

  Replace the entire `classify` method body in `deathpwn-core/src/detector/mod.rs` with:

  ```rust
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
  ```

- [ ] **Step 10: Run test to verify it passes**

  ```
  cargo test -p deathpwn-core detector
  ```

  Expected: PASS — all 8 detector tests green (`empty_line_is_raw_input`, `whitespace_only_line_is_raw_input`, `detector_exposes_configured_shell`, `known_command_is_direct_command`, `unknown_leading_token_is_raw_input`, `leading_token_drives_decision_across_pipe`, `quoted_leading_token_is_shell_quoted_before_resolution`, `unbalanced_quotes_are_raw_input`).

- [ ] **Step 11: Commit**

  ```
  git add deathpwn-core/src/detector/mod.rs
  git commit -m "feat(deathpwn): resolve Step 0 leading token via command -v"
  ```
