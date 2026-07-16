### Task 9: session — SessionState + Artifacts

**Files:**
- Create: `deathpwn-core/src/session/mod.rs` (core crate — `Target`, `Finding`, `SessionState`, module wiring)
- Create: `deathpwn-core/src/session/artifacts.rs` (core crate — `Artifacts`)
- Edit: `deathpwn-core/src/lib.rs` (core crate — add `pub mod session;`)
- Edit: `deathpwn-core/Cargo.toml` (core crate — add `tempfile` dev-dependency)
- Test: unit tests live in a `#[cfg(test)] mod tests` inside `session/mod.rs` (SessionState) and inside `session/artifacts.rs` (Artifacts), per Rust convention.

**Interfaces:**
- Consumes:
  - `struct RunOutcome { exit: Option<i32>, stdout: String, stderr: String, cancelled: bool }` — from Task 7, path `crate::exec::RunOutcome`.
  - `trait Clock: Send + Sync { fn now_ms(&self) -> u64; }` — from Task 3, path `crate::clock::Clock`.
  - `struct FakeClock` — Task 3 test-support fake (available under `#[cfg(any(test, feature="test-support"))]`), path `crate::clock::FakeClock`. Assumed constructor `FakeClock::new(now_ms: u64)` whose `now_ms()` returns that fixed value.
  - `type Result<T> = std::result::Result<T, DeathpwnError>;` with `DeathpwnError::Io(#[from] std::io::Error)` — from Task 1, path `crate::error::Result` / `crate::error::DeathpwnError`.
- Produces:
  - `struct Target { value: String }` (host or url).
  - `struct Finding { severity: String, title: String, detail: String }`.
  - `struct SessionState { targets: Vec<Target>, hosts: Vec<String>, ports_by_host: BTreeMap<String, Vec<u16>>, services: Vec<String>, findings: Vec<Finding>, command_log: Vec<String> }` (fields private, exposed via getters) with:
    - `fn new() -> Self`
    - `fn add_target(&mut self, target: Target)`
    - `fn record_command(&mut self, command: &str)`
    - `fn add_finding(&mut self, finding: Finding)`
    - `fn add_service(&mut self, service: &str)`
    - `fn add_ports(&mut self, host: &str, ports: Vec<u16>)`
    - getters: `fn targets(&self) -> &[Target]`, `fn hosts(&self) -> &[String]`, `fn ports_by_host(&self) -> &BTreeMap<String, Vec<u16>>`, `fn services(&self) -> &[String]`, `fn findings(&self) -> &[Finding]`, `fn command_log(&self) -> &[String]`
  - `struct Artifacts { root: PathBuf, session_dir: PathBuf }` with:
    - `fn open(root: PathBuf, clock: &dyn Clock) -> Result<Artifacts>` (session dir = `root/<now_ms>`)
    - `fn write_output(&self, index: usize, outcome: &RunOutcome) -> Result<PathBuf>`
    - getters: `fn session_dir(&self) -> &Path`, `fn root(&self) -> &Path`
  - Re-export `pub use artifacts::Artifacts;` from `session/mod.rs`.

---

- [ ] **Step 1: Prep — deps + module wiring.**
  Add the dev-dependency to `deathpwn-core/Cargo.toml`:
  ```toml
  [dev-dependencies]
  tempfile = "3"
  ```
  Add the module to `deathpwn-core/src/lib.rs` (place next to the other `pub mod` declarations):
  ```rust
  pub mod session;
  ```
  Create `deathpwn-core/src/session/mod.rs` with only the submodule declaration (compiles cleanly; the re-export is added once `Artifacts` exists in Step 14):
  ```rust
  pub mod artifacts;
  ```
  Create an empty `deathpwn-core/src/session/artifacts.rs` (a doc comment only — an empty module compiles):
  ```rust
  //! Per-session artifact directory and command-output persistence.
  ```
  Verify it still builds:
  ```sh
  cargo build -p deathpwn-core
  ```
  Expected: builds clean (empty `session` module, no new symbols yet).

- [ ] **Step 2: Write the failing test — SessionState core (targets, command log, dedup).**
  Append to `deathpwn-core/src/session/mod.rs`:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn records_targets_commands_and_dedups() {
          let mut s = SessionState::new();
          assert!(s.targets().is_empty());
          assert!(s.command_log().is_empty());

          s.add_target(Target { value: "192.168.1.1".to_string() });
          s.add_target(Target { value: "192.168.1.1".to_string() }); // duplicate ignored
          s.add_target(Target { value: "http://example.com".to_string() });

          s.record_command("nmap -sV 192.168.1.1");
          s.record_command("gobuster dir -u http://example.com");

          assert_eq!(s.targets().len(), 2);
          assert_eq!(s.targets()[0].value, "192.168.1.1");
          assert_eq!(
              s.command_log(),
              &[
                  "nmap -sV 192.168.1.1".to_string(),
                  "gobuster dir -u http://example.com".to_string(),
              ]
          );
      }
  }
  ```

- [ ] **Step 3: Run test to verify it fails.**
  ```sh
  cargo test -p deathpwn-core records_targets_commands_and_dedups
  ```
  Expected: fails to compile — `cannot find type SessionState in this scope` / `cannot find type Target in this scope`.

- [ ] **Step 4: Implement SessionState core.**
  Replace the full contents of `deathpwn-core/src/session/mod.rs` with:
  ```rust
  use std::collections::BTreeMap;

  pub mod artifacts;

  /// A scan/attack target — either a host or a URL.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct Target {
      pub value: String,
  }

  /// A security finding surfaced during a session.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct Finding {
      pub severity: String,
      pub title: String,
      pub detail: String,
  }

  /// Accumulated knowledge about the current session. Mutated after each
  /// execution and read by pipeline stages so follow-ups ("scan those ports")
  /// resolve without re-stating the target.
  #[derive(Debug, Clone, Default)]
  pub struct SessionState {
      targets: Vec<Target>,
      hosts: Vec<String>,
      ports_by_host: BTreeMap<String, Vec<u16>>,
      services: Vec<String>,
      findings: Vec<Finding>,
      command_log: Vec<String>,
  }

  impl SessionState {
      pub fn new() -> Self {
          Self::default()
      }

      pub fn add_target(&mut self, target: Target) {
          if !self.targets.iter().any(|t| t.value == target.value) {
              self.targets.push(target);
          }
      }

      pub fn record_command(&mut self, command: &str) {
          self.command_log.push(command.to_string());
      }

      pub fn targets(&self) -> &[Target] {
          &self.targets
      }

      pub fn hosts(&self) -> &[String] {
          &self.hosts
      }

      pub fn ports_by_host(&self) -> &BTreeMap<String, Vec<u16>> {
          &self.ports_by_host
      }

      pub fn services(&self) -> &[String] {
          &self.services
      }

      pub fn findings(&self) -> &[Finding] {
          &self.findings
      }

      pub fn command_log(&self) -> &[String] {
          &self.command_log
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn records_targets_commands_and_dedups() {
          let mut s = SessionState::new();
          assert!(s.targets().is_empty());
          assert!(s.command_log().is_empty());

          s.add_target(Target { value: "192.168.1.1".to_string() });
          s.add_target(Target { value: "192.168.1.1".to_string() }); // duplicate ignored
          s.add_target(Target { value: "http://example.com".to_string() });

          s.record_command("nmap -sV 192.168.1.1");
          s.record_command("gobuster dir -u http://example.com");

          assert_eq!(s.targets().len(), 2);
          assert_eq!(s.targets()[0].value, "192.168.1.1");
          assert_eq!(
              s.command_log(),
              &[
                  "nmap -sV 192.168.1.1".to_string(),
                  "gobuster dir -u http://example.com".to_string(),
              ]
          );
      }
  }
  ```

- [ ] **Step 5: Run test to verify it passes.**
  ```sh
  cargo test -p deathpwn-core records_targets_commands_and_dedups
  ```
  Expected: PASS (`test session::tests::records_targets_commands_and_dedups ... ok`).

- [ ] **Step 6: Commit.**
  ```sh
  git add deathpwn-core/Cargo.toml deathpwn-core/src/lib.rs deathpwn-core/src/session/mod.rs deathpwn-core/src/session/artifacts.rs
  git commit -m "feat(deathpwn): add SessionState with targets and command log"
  ```

- [ ] **Step 7: Write the failing test — ports, services, findings (merge + dedup + sort).**
  Add a second `#[test]` inside the existing `mod tests` in `deathpwn-core/src/session/mod.rs`:
  ```rust
  #[test]
  fn tracks_ports_findings_and_services() {
      let mut s = SessionState::new();

      s.add_ports("10.0.0.5", vec![80, 22, 80]); // duplicate 80 collapses
      s.add_ports("10.0.0.5", vec![443]);        // merges into existing host
      s.add_service("http");
      s.add_service("http"); // duplicate ignored
      s.add_finding(Finding {
          severity: "high".to_string(),
          title: "Anonymous FTP".to_string(),
          detail: "vsftpd allows anonymous login".to_string(),
      });

      assert_eq!(s.hosts(), &["10.0.0.5".to_string()]);
      assert_eq!(s.ports_by_host().get("10.0.0.5"), Some(&vec![22u16, 80, 443]));
      assert_eq!(s.services(), &["http".to_string()]);
      assert_eq!(s.findings().len(), 1);
      assert_eq!(s.findings()[0].severity, "high");
  }
  ```

- [ ] **Step 8: Run test to verify it fails.**
  ```sh
  cargo test -p deathpwn-core tracks_ports_findings_and_services
  ```
  Expected: fails to compile — `no method named add_ports found for struct SessionState` (and `add_service`, `add_finding`).

- [ ] **Step 9: Implement the mutators.**
  Replace the full contents of `deathpwn-core/src/session/mod.rs` with (adds `add_finding`, `add_service`, `add_ports`; keeps both tests):
  ```rust
  use std::collections::BTreeMap;

  pub mod artifacts;

  /// A scan/attack target — either a host or a URL.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct Target {
      pub value: String,
  }

  /// A security finding surfaced during a session.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct Finding {
      pub severity: String,
      pub title: String,
      pub detail: String,
  }

  /// Accumulated knowledge about the current session. Mutated after each
  /// execution and read by pipeline stages so follow-ups ("scan those ports")
  /// resolve without re-stating the target.
  #[derive(Debug, Clone, Default)]
  pub struct SessionState {
      targets: Vec<Target>,
      hosts: Vec<String>,
      ports_by_host: BTreeMap<String, Vec<u16>>,
      services: Vec<String>,
      findings: Vec<Finding>,
      command_log: Vec<String>,
  }

  impl SessionState {
      pub fn new() -> Self {
          Self::default()
      }

      pub fn add_target(&mut self, target: Target) {
          if !self.targets.iter().any(|t| t.value == target.value) {
              self.targets.push(target);
          }
      }

      pub fn record_command(&mut self, command: &str) {
          self.command_log.push(command.to_string());
      }

      pub fn add_finding(&mut self, finding: Finding) {
          self.findings.push(finding);
      }

      pub fn add_service(&mut self, service: &str) {
          if !self.services.iter().any(|s| s == service) {
              self.services.push(service.to_string());
          }
      }

      pub fn add_ports(&mut self, host: &str, ports: Vec<u16>) {
          if !self.hosts.iter().any(|h| h == host) {
              self.hosts.push(host.to_string());
          }
          let entry = self.ports_by_host.entry(host.to_string()).or_default();
          for port in ports {
              if !entry.contains(&port) {
                  entry.push(port);
              }
          }
          entry.sort_unstable();
      }

      pub fn targets(&self) -> &[Target] {
          &self.targets
      }

      pub fn hosts(&self) -> &[String] {
          &self.hosts
      }

      pub fn ports_by_host(&self) -> &BTreeMap<String, Vec<u16>> {
          &self.ports_by_host
      }

      pub fn services(&self) -> &[String] {
          &self.services
      }

      pub fn findings(&self) -> &[Finding] {
          &self.findings
      }

      pub fn command_log(&self) -> &[String] {
          &self.command_log
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn records_targets_commands_and_dedups() {
          let mut s = SessionState::new();
          assert!(s.targets().is_empty());
          assert!(s.command_log().is_empty());

          s.add_target(Target { value: "192.168.1.1".to_string() });
          s.add_target(Target { value: "192.168.1.1".to_string() }); // duplicate ignored
          s.add_target(Target { value: "http://example.com".to_string() });

          s.record_command("nmap -sV 192.168.1.1");
          s.record_command("gobuster dir -u http://example.com");

          assert_eq!(s.targets().len(), 2);
          assert_eq!(s.targets()[0].value, "192.168.1.1");
          assert_eq!(
              s.command_log(),
              &[
                  "nmap -sV 192.168.1.1".to_string(),
                  "gobuster dir -u http://example.com".to_string(),
              ]
          );
      }

      #[test]
      fn tracks_ports_findings_and_services() {
          let mut s = SessionState::new();

          s.add_ports("10.0.0.5", vec![80, 22, 80]); // duplicate 80 collapses
          s.add_ports("10.0.0.5", vec![443]);        // merges into existing host
          s.add_service("http");
          s.add_service("http"); // duplicate ignored
          s.add_finding(Finding {
              severity: "high".to_string(),
              title: "Anonymous FTP".to_string(),
              detail: "vsftpd allows anonymous login".to_string(),
          });

          assert_eq!(s.hosts(), &["10.0.0.5".to_string()]);
          assert_eq!(s.ports_by_host().get("10.0.0.5"), Some(&vec![22u16, 80, 443]));
          assert_eq!(s.services(), &["http".to_string()]);
          assert_eq!(s.findings().len(), 1);
          assert_eq!(s.findings()[0].severity, "high");
      }
  }
  ```

- [ ] **Step 10: Run tests to verify they pass.**
  ```sh
  cargo test -p deathpwn-core session::
  ```
  Expected: PASS — both `records_targets_commands_and_dedups` and `tracks_ports_findings_and_services` are ok.

- [ ] **Step 11: Commit.**
  ```sh
  git add deathpwn-core/src/session/mod.rs
  git commit -m "feat(deathpwn): track ports, services, and findings in SessionState"
  ```

- [ ] **Step 12: Write the failing test — Artifacts::open creates a clock-named session dir.**
  Add a `#[cfg(test)] mod tests` to `deathpwn-core/src/session/artifacts.rs`:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::clock::FakeClock;

      #[test]
      fn open_creates_session_dir_named_by_clock() {
          let tmp = tempfile::tempdir().unwrap();
          let clock = FakeClock::new(1_700_000_000_000);

          let artifacts = Artifacts::open(tmp.path().to_path_buf(), &clock).unwrap();

          let expected = tmp.path().join("1700000000000");
          assert_eq!(artifacts.session_dir(), expected.as_path());
          assert!(expected.is_dir());
      }
  }
  ```

- [ ] **Step 13: Run test to verify it fails.**
  ```sh
  cargo test -p deathpwn-core open_creates_session_dir_named_by_clock
  ```
  Expected: fails to compile — `cannot find type Artifacts in this scope` / `failed to resolve: Artifacts`.

- [ ] **Step 14: Implement Artifacts + open, and re-export from the module.**
  Replace the full contents of `deathpwn-core/src/session/artifacts.rs` with:
  ```rust
  //! Per-session artifact directory and command-output persistence.

  use std::fs;
  use std::path::{Path, PathBuf};

  use crate::clock::Clock;
  use crate::error::Result;

  /// Per-session artifact directory. Each command's raw output is written to a
  /// numbered file under `session_dir` for later review/export.
  #[derive(Debug, Clone)]
  pub struct Artifacts {
      root: PathBuf,
      session_dir: PathBuf,
  }

  impl Artifacts {
      /// Open (creating if needed) a session directory at `root/<now_ms>`.
      /// The timestamp comes from the injected clock so tests are deterministic.
      pub fn open(root: PathBuf, clock: &dyn Clock) -> Result<Artifacts> {
          let session_dir = root.join(clock.now_ms().to_string());
          fs::create_dir_all(&session_dir)?;
          Ok(Artifacts { root, session_dir })
      }

      /// Absolute path to this session's directory.
      pub fn session_dir(&self) -> &Path {
          &self.session_dir
      }

      /// Root artifacts directory shared across sessions.
      pub fn root(&self) -> &Path {
          &self.root
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::clock::FakeClock;

      #[test]
      fn open_creates_session_dir_named_by_clock() {
          let tmp = tempfile::tempdir().unwrap();
          let clock = FakeClock::new(1_700_000_000_000);

          let artifacts = Artifacts::open(tmp.path().to_path_buf(), &clock).unwrap();

          let expected = tmp.path().join("1700000000000");
          assert_eq!(artifacts.session_dir(), expected.as_path());
          assert!(expected.is_dir());
      }
  }
  ```
  Add the re-export to `deathpwn-core/src/session/mod.rs` immediately below `pub mod artifacts;`:
  ```rust
  pub use artifacts::Artifacts;
  ```

- [ ] **Step 15: Run test to verify it passes.**
  ```sh
  cargo test -p deathpwn-core open_creates_session_dir_named_by_clock
  ```
  Expected: PASS (`test session::artifacts::tests::open_creates_session_dir_named_by_clock ... ok`).

- [ ] **Step 16: Commit.**
  ```sh
  git add deathpwn-core/src/session/artifacts.rs deathpwn-core/src/session/mod.rs
  git commit -m "feat(deathpwn): open clock-named session artifact directory"
  ```

- [ ] **Step 17: Write the failing test — write_output persists a numbered file with both streams.**
  Add a second `#[test]` (and the `RunOutcome` import) to the existing `mod tests` in `deathpwn-core/src/session/artifacts.rs`:
  ```rust
  #[test]
  fn write_output_persists_numbered_file_with_streams() {
      use crate::exec::RunOutcome;

      let tmp = tempfile::tempdir().unwrap();
      let clock = FakeClock::new(1_700_000_000_000);
      let artifacts = Artifacts::open(tmp.path().to_path_buf(), &clock).unwrap();

      let outcome = RunOutcome {
          exit: Some(0),
          stdout: "PORT 80 open\n".to_string(),
          stderr: "warning: slow\n".to_string(),
          cancelled: false,
      };

      let path = artifacts.write_output(1, &outcome).unwrap();

      assert_eq!(path, artifacts.session_dir().join("1.txt"));
      let contents = std::fs::read_to_string(&path).unwrap();
      assert!(contents.contains("exit: 0"));
      assert!(contents.contains("cancelled: false"));
      assert!(contents.contains("PORT 80 open"));
      assert!(contents.contains("warning: slow"));
  }
  ```

- [ ] **Step 18: Run test to verify it fails.**
  ```sh
  cargo test -p deathpwn-core write_output_persists_numbered_file_with_streams
  ```
  Expected: fails to compile — `no method named write_output found for struct Artifacts`.

- [ ] **Step 19: Implement write_output.**
  Replace the full contents of `deathpwn-core/src/session/artifacts.rs` with (adds `write_output`, the `std::io::Write` and `RunOutcome` imports; keeps both tests):
  ```rust
  //! Per-session artifact directory and command-output persistence.

  use std::fs;
  use std::io::Write;
  use std::path::{Path, PathBuf};

  use crate::clock::Clock;
  use crate::error::Result;
  use crate::exec::RunOutcome;

  /// Per-session artifact directory. Each command's raw output is written to a
  /// numbered file under `session_dir` for later review/export.
  #[derive(Debug, Clone)]
  pub struct Artifacts {
      root: PathBuf,
      session_dir: PathBuf,
  }

  impl Artifacts {
      /// Open (creating if needed) a session directory at `root/<now_ms>`.
      /// The timestamp comes from the injected clock so tests are deterministic.
      pub fn open(root: PathBuf, clock: &dyn Clock) -> Result<Artifacts> {
          let session_dir = root.join(clock.now_ms().to_string());
          fs::create_dir_all(&session_dir)?;
          Ok(Artifacts { root, session_dir })
      }

      /// Absolute path to this session's directory.
      pub fn session_dir(&self) -> &Path {
          &self.session_dir
      }

      /// Root artifacts directory shared across sessions.
      pub fn root(&self) -> &Path {
          &self.root
      }

      /// Write one command's captured output to `session_dir/<index>.txt`,
      /// returning the path written. Includes exit code, cancellation flag, and
      /// both output streams.
      pub fn write_output(&self, index: usize, outcome: &RunOutcome) -> Result<PathBuf> {
          let path = self.session_dir.join(format!("{index}.txt"));
          let mut file = fs::File::create(&path)?;
          let exit = match outcome.exit {
              Some(code) => code.to_string(),
              None => "none".to_string(),
          };
          writeln!(file, "exit: {exit}")?;
          writeln!(file, "cancelled: {}", outcome.cancelled)?;
          writeln!(file, "--- stdout ---")?;
          file.write_all(outcome.stdout.as_bytes())?;
          writeln!(file, "\n--- stderr ---")?;
          file.write_all(outcome.stderr.as_bytes())?;
          Ok(path)
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::clock::FakeClock;

      #[test]
      fn open_creates_session_dir_named_by_clock() {
          let tmp = tempfile::tempdir().unwrap();
          let clock = FakeClock::new(1_700_000_000_000);

          let artifacts = Artifacts::open(tmp.path().to_path_buf(), &clock).unwrap();

          let expected = tmp.path().join("1700000000000");
          assert_eq!(artifacts.session_dir(), expected.as_path());
          assert!(expected.is_dir());
      }

      #[test]
      fn write_output_persists_numbered_file_with_streams() {
          let tmp = tempfile::tempdir().unwrap();
          let clock = FakeClock::new(1_700_000_000_000);
          let artifacts = Artifacts::open(tmp.path().to_path_buf(), &clock).unwrap();

          let outcome = RunOutcome {
              exit: Some(0),
              stdout: "PORT 80 open\n".to_string(),
              stderr: "warning: slow\n".to_string(),
              cancelled: false,
          };

          let path = artifacts.write_output(1, &outcome).unwrap();

          assert_eq!(path, artifacts.session_dir().join("1.txt"));
          let contents = std::fs::read_to_string(&path).unwrap();
          assert!(contents.contains("exit: 0"));
          assert!(contents.contains("cancelled: false"));
          assert!(contents.contains("PORT 80 open"));
          assert!(contents.contains("warning: slow"));
      }
  }
  ```

- [ ] **Step 20: Run tests to verify they pass.**
  ```sh
  cargo test -p deathpwn-core session::artifacts::
  ```
  Expected: PASS — both `open_creates_session_dir_named_by_clock` and `write_output_persists_numbered_file_with_streams` are ok.

- [ ] **Step 21: Commit.**
  ```sh
  git add deathpwn-core/src/session/artifacts.rs
  git commit -m "feat(deathpwn): persist command output to numbered artifact files"
  ```

- [ ] **Step 22: Final — full module verification + wrap-up commit.**
  Run the whole session module and confirm the crate still builds clean:
  ```sh
  cargo test -p deathpwn-core session::
  cargo build -p deathpwn-core
  ```
  Expected: all four session tests pass; `deathpwn-core` builds with no errors (note: `#![forbid(unsafe_code)]` holds — no `unsafe` introduced). If `git status` shows any residual staged changes (e.g. the `Cargo.lock` update from adding `tempfile`), commit them:
  ```sh
  git add Cargo.lock
  git commit -m "chore(deathpwn): lock tempfile dev-dependency for session tests"
  ```
