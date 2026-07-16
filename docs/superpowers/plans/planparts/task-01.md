### Task 1: Workspace skeleton + error + config

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `deathpwn-core/Cargo.toml` (core crate manifest)
- Create: `deathpwn-core/src/lib.rs` (core crate root; re-exports)
- Create: `deathpwn-core/src/error.rs` (core crate — `DeathpwnError`, `Result`)
- Create: `deathpwn-core/src/config.rs` (core crate — `Config`, `ProviderConfig`)
- Create: `deathpwn-tui/Cargo.toml` (tui crate manifest)
- Create: `deathpwn-tui/src/main.rs` (tui crate — placeholder binary that builds)
- Test: unit tests live in `#[cfg(test)] mod tests` inside `deathpwn-core/src/error.rs` and `deathpwn-core/src/config.rs` (Rust convention; no separate test files this task).

**Interfaces:**
- Consumes: nothing (foundational task; no earlier tasks exist).
- Produces (later tasks depend on these EXACT names/types):
  - `enum DeathpwnError` (thiserror) with variants `Config(String)`, `Provider(String)`, `Search(String)`, `Exec(String)`, `Schema(String)`, `Cache(String)`, `Io(#[from] std::io::Error)`, `Cancelled`.
  - `type Result<T> = std::result::Result<T, DeathpwnError>;`
  - `struct ProviderConfig { url: String, key: String, model: String }`
  - `struct Config { provider_a: ProviderConfig, provider_b: ProviderConfig, shell: String, max_goal_steps: u32, max_corrections: u32, artifacts_dir: PathBuf, http_timeout_secs: u64 }`
  - `Config::from_env() -> Result<Config>` (reads + validates env; error names the missing/invalid var).

---

- [ ] **Step 1: Scaffold the workspace and add deps** — create the four manifests, the core crate root, and the placeholder binary. This introduces the only dependency this task needs (`thiserror`) and sets `resolver = "2"`, `edition = "2021"`. No logic yet; the crates must simply compile.

  `Cargo.toml` (workspace root):
  ```toml
  [workspace]
  resolver = "2"
  members = ["deathpwn-core", "deathpwn-tui"]
  ```

  `deathpwn-core/Cargo.toml`:
  ```toml
  [package]
  name = "deathpwn-core"
  version = "0.1.0"
  edition = "2021"

  [dependencies]
  thiserror = "1"
  ```

  `deathpwn-core/src/lib.rs`:
  ```rust
  #![forbid(unsafe_code)]

  //! deathpwn-core: natural-language offensive-security terminal (library crate).
  //! All logic and traits live here; the crate has no terminal or async-main deps.
  ```

  `deathpwn-tui/Cargo.toml`:
  ```toml
  [package]
  name = "deathpwn-tui"
  version = "0.1.0"
  edition = "2021"

  [dependencies]
  deathpwn-core = { path = "../deathpwn-core" }
  ```

  `deathpwn-tui/src/main.rs` (placeholder that builds and proves core is linkable):
  ```rust
  //! deathpwn-tui: terminal front-end (placeholder until Task 16).

  // Force the core crate to be linked so the workspace wiring is proven now.
  use deathpwn_core as _;

  fn main() {
      println!("deathpwn (placeholder) — workspace build OK");
  }
  ```

- [ ] **Step 2: Verify the skeleton compiles** — command:
  ```
  cargo build --workspace
  ```
  Expected: PASS. Both `deathpwn-core` and `deathpwn-tui` compile cleanly (only `thiserror` and the path dep are pulled in). No warnings that fail the build.

- [ ] **Step 3: Commit the skeleton**
  ```
  git add Cargo.toml deathpwn-core/Cargo.toml deathpwn-core/src/lib.rs deathpwn-tui/Cargo.toml deathpwn-tui/src/main.rs
  git commit -m "feat(deathpwn): scaffold cargo workspace (core lib + tui bin)"
  ```

---

- [ ] **Step 4: Write the failing test (error type)** — declare the module in `lib.rs`, then create `error.rs` containing ONLY the test module (the types it references do not exist yet). The tests assert real behavior: the `#[from]` io conversion, that `Config` display carries its message, and that the `Result` alias is usable.

  Update `deathpwn-core/src/lib.rs`:
  ```rust
  #![forbid(unsafe_code)]

  //! deathpwn-core: natural-language offensive-security terminal (library crate).
  //! All logic and traits live here; the crate has no terminal or async-main deps.

  pub mod error;
  ```

  Create `deathpwn-core/src/error.rs`:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn io_error_converts_via_from() {
          let io = std::io::Error::new(std::io::ErrorKind::NotFound, "nope");
          let err: DeathpwnError = io.into();
          match err {
              DeathpwnError::Io(inner) => {
                  assert_eq!(inner.kind(), std::io::ErrorKind::NotFound);
              }
              other => panic!("expected Io variant, got {other:?}"),
          }
      }

      #[test]
      fn config_display_includes_message() {
          let err = DeathpwnError::Config("missing DEATHPWN_PROVIDER_A_URL".to_string());
          assert!(err.to_string().contains("DEATHPWN_PROVIDER_A_URL"));
      }

      #[test]
      fn result_alias_carries_deathpwn_error() {
          fn ok() -> Result<u32> {
              Ok(7)
          }
          fn cancelled() -> Result<u32> {
              Err(DeathpwnError::Cancelled)
          }
          assert_eq!(ok().unwrap(), 7);
          assert!(matches!(cancelled(), Err(DeathpwnError::Cancelled)));
      }
  }
  ```

- [ ] **Step 5: Run test to verify it fails** — command:
  ```
  cargo test -p deathpwn-core error
  ```
  Expected: FAILS TO COMPILE. Errors like `cannot find type 'DeathpwnError' in this scope` and `cannot find type 'Result'` (the alias) — the module is declared but the types are not implemented yet.

- [ ] **Step 6: Implement `error.rs` and re-export from `lib.rs`** — add the full enum + alias above the existing test module, and re-export the types from the crate root.

  `deathpwn-core/src/error.rs` (test module unchanged, shown in full for clarity):
  ```rust
  use thiserror::Error;

  /// Top-level error for deathpwn-core. Failover and the feedback loop absorb the
  /// *expected* failures; this type is for the rest.
  #[derive(Debug, Error)]
  pub enum DeathpwnError {
      #[error("config error: {0}")]
      Config(String),

      #[error("provider error: {0}")]
      Provider(String),

      #[error("search error: {0}")]
      Search(String),

      #[error("exec error: {0}")]
      Exec(String),

      #[error("schema error: {0}")]
      Schema(String),

      #[error("cache error: {0}")]
      Cache(String),

      #[error("io error: {0}")]
      Io(#[from] std::io::Error),

      #[error("operation cancelled")]
      Cancelled,
  }

  /// Crate-wide result alias.
  pub type Result<T> = std::result::Result<T, DeathpwnError>;

  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn io_error_converts_via_from() {
          let io = std::io::Error::new(std::io::ErrorKind::NotFound, "nope");
          let err: DeathpwnError = io.into();
          match err {
              DeathpwnError::Io(inner) => {
                  assert_eq!(inner.kind(), std::io::ErrorKind::NotFound);
              }
              other => panic!("expected Io variant, got {other:?}"),
          }
      }

      #[test]
      fn config_display_includes_message() {
          let err = DeathpwnError::Config("missing DEATHPWN_PROVIDER_A_URL".to_string());
          assert!(err.to_string().contains("DEATHPWN_PROVIDER_A_URL"));
      }

      #[test]
      fn result_alias_carries_deathpwn_error() {
          fn ok() -> Result<u32> {
              Ok(7)
          }
          fn cancelled() -> Result<u32> {
              Err(DeathpwnError::Cancelled)
          }
          assert_eq!(ok().unwrap(), 7);
          assert!(matches!(cancelled(), Err(DeathpwnError::Cancelled)));
      }
  }
  ```

  `deathpwn-core/src/lib.rs`:
  ```rust
  #![forbid(unsafe_code)]

  //! deathpwn-core: natural-language offensive-security terminal (library crate).
  //! All logic and traits live here; the crate has no terminal or async-main deps.

  pub mod error;

  pub use error::{DeathpwnError, Result};
  ```

- [ ] **Step 7: Run test to verify it passes** — command:
  ```
  cargo test -p deathpwn-core error
  ```
  Expected: PASS. `3 passed; 0 failed` (`io_error_converts_via_from`, `config_display_includes_message`, `result_alias_carries_deathpwn_error`).

- [ ] **Step 8: Commit the error type**
  ```
  git add deathpwn-core/src/error.rs deathpwn-core/src/lib.rs
  git commit -m "feat(deathpwn): add DeathpwnError enum and Result alias"
  ```

---

- [ ] **Step 9: Write the failing test (config)** — declare the `config` module in `lib.rs`, then create `config.rs` containing ONLY the test module. Tests drive the whole surface at once so they all fail before implementation: required-var reading, missing-var error naming the var, defaults, shell precedence, numeric/path overrides, XDG artifacts default, and numeric parse failure. Tests exercise a private `from_lookup(get)` helper so they stay deterministic and never touch (or race on) the real process environment.

  Update `deathpwn-core/src/lib.rs`:
  ```rust
  #![forbid(unsafe_code)]

  //! deathpwn-core: natural-language offensive-security terminal (library crate).
  //! All logic and traits live here; the crate has no terminal or async-main deps.

  pub mod config;
  pub mod error;

  pub use error::{DeathpwnError, Result};
  ```

  Create `deathpwn-core/src/config.rs`:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use std::collections::HashMap;
      use std::path::PathBuf;

      fn all_required() -> HashMap<String, String> {
          let mut m = HashMap::new();
          m.insert("DEATHPWN_PROVIDER_A_URL".into(), "https://a.example/v1".into());
          m.insert("DEATHPWN_PROVIDER_A_KEY".into(), "key-a".into());
          m.insert("DEATHPWN_PROVIDER_A_MODEL".into(), "model-a".into());
          m.insert("DEATHPWN_PROVIDER_B_URL".into(), "https://b.example/v1".into());
          m.insert("DEATHPWN_PROVIDER_B_KEY".into(), "key-b".into());
          m.insert("DEATHPWN_PROVIDER_B_MODEL".into(), "model-b".into());
          m
      }

      fn lookup(m: HashMap<String, String>) -> impl Fn(&str) -> Option<String> {
          move |k| m.get(k).cloned()
      }

      #[test]
      fn reads_required_provider_vars() {
          let cfg = Config::from_lookup(lookup(all_required())).unwrap();
          assert_eq!(cfg.provider_a.url, "https://a.example/v1");
          assert_eq!(cfg.provider_a.key, "key-a");
          assert_eq!(cfg.provider_a.model, "model-a");
          assert_eq!(cfg.provider_b.url, "https://b.example/v1");
          assert_eq!(cfg.provider_b.key, "key-b");
          assert_eq!(cfg.provider_b.model, "model-b");
      }

      #[test]
      fn missing_required_var_names_it() {
          let mut m = all_required();
          m.remove("DEATHPWN_PROVIDER_B_MODEL");
          let err = Config::from_lookup(lookup(m)).unwrap_err();
          match err {
              DeathpwnError::Config(msg) => {
                  assert!(
                      msg.contains("DEATHPWN_PROVIDER_B_MODEL"),
                      "error should name the missing var, was: {msg}"
                  );
              }
              other => panic!("expected Config error, got {other:?}"),
          }
      }

      #[test]
      fn applies_defaults_when_optional_absent() {
          let cfg = Config::from_lookup(lookup(all_required())).unwrap();
          assert_eq!(cfg.shell, "/bin/sh");
          assert_eq!(cfg.max_goal_steps, 12);
          assert_eq!(cfg.max_corrections, 2);
          assert_eq!(cfg.http_timeout_secs, 30);
          // No DEATHPWN_ARTIFACTS_DIR / XDG_DATA_HOME / HOME in the lookup.
          assert_eq!(cfg.artifacts_dir, PathBuf::from(".local/share/deathpwn"));
      }

      #[test]
      fn shell_prefers_deathpwn_then_shell_env() {
          let mut m = all_required();
          m.insert("SHELL".into(), "/usr/bin/zsh".into());
          let cfg = Config::from_lookup(lookup(m.clone())).unwrap();
          assert_eq!(cfg.shell, "/usr/bin/zsh");

          m.insert("DEATHPWN_SHELL".into(), "/usr/bin/fish".into());
          let cfg = Config::from_lookup(lookup(m)).unwrap();
          assert_eq!(cfg.shell, "/usr/bin/fish");
      }

      #[test]
      fn overrides_numeric_and_artifacts_path() {
          let mut m = all_required();
          m.insert("DEATHPWN_MAX_GOAL_STEPS".into(), "5".into());
          m.insert("DEATHPWN_MAX_CORRECTIONS".into(), "3".into());
          m.insert("DEATHPWN_HTTP_TIMEOUT_SECS".into(), "45".into());
          m.insert("DEATHPWN_ARTIFACTS_DIR".into(), "/tmp/dp".into());
          let cfg = Config::from_lookup(lookup(m)).unwrap();
          assert_eq!(cfg.max_goal_steps, 5);
          assert_eq!(cfg.max_corrections, 3);
          assert_eq!(cfg.http_timeout_secs, 45);
          assert_eq!(cfg.artifacts_dir, PathBuf::from("/tmp/dp"));
      }

      #[test]
      fn xdg_data_home_used_for_artifacts_default() {
          let mut m = all_required();
          m.insert("XDG_DATA_HOME".into(), "/xdg".into());
          let cfg = Config::from_lookup(lookup(m)).unwrap();
          assert_eq!(cfg.artifacts_dir, PathBuf::from("/xdg/deathpwn"));
      }

      #[test]
      fn bad_numeric_is_config_error_naming_var() {
          let mut m = all_required();
          m.insert("DEATHPWN_MAX_GOAL_STEPS".into(), "not-a-number".into());
          let err = Config::from_lookup(lookup(m)).unwrap_err();
          match err {
              DeathpwnError::Config(msg) => {
                  assert!(
                      msg.contains("DEATHPWN_MAX_GOAL_STEPS"),
                      "error should name the bad var, was: {msg}"
                  );
              }
              other => panic!("expected Config error, got {other:?}"),
          }
      }
  }
  ```

- [ ] **Step 10: Run test to verify it fails** — command:
  ```
  cargo test -p deathpwn-core config
  ```
  Expected: FAILS TO COMPILE. Errors like `failed to resolve: use of undeclared type 'Config'` and `cannot find type 'DeathpwnError' in this scope` — `config.rs` declares only tests; `Config`, `ProviderConfig`, and `from_lookup` do not exist yet.

- [ ] **Step 11: Implement `config.rs` and re-export from `lib.rs`** — add the structs, the public `from_env`, the testable private `from_lookup`, and the numeric/path helpers above the existing test module.

  `deathpwn-core/src/config.rs` (test module unchanged from Step 9; implementation prepended):
  ```rust
  use std::path::PathBuf;

  use crate::error::{DeathpwnError, Result};

  /// One OpenAI-compatible provider endpoint.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct ProviderConfig {
      pub url: String,
      pub key: String,
      pub model: String,
  }

  /// Runtime configuration, loaded once at startup from the environment.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct Config {
      pub provider_a: ProviderConfig,
      pub provider_b: ProviderConfig,
      pub shell: String,
      pub max_goal_steps: u32,
      pub max_corrections: u32,
      pub artifacts_dir: PathBuf,
      pub http_timeout_secs: u64,
  }

  impl Config {
      /// Load and validate configuration from the process environment.
      /// Missing required vars are a hard error naming the offending var.
      pub fn from_env() -> Result<Config> {
          Self::from_lookup(|key| std::env::var(key).ok())
      }

      /// Core loader parameterized over a variable lookup. Kept private and used
      /// by tests so they never mutate the global, racy process environment.
      fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Config> {
          let required = |name: &str| -> Result<String> {
              match get(name) {
                  Some(v) if !v.is_empty() => Ok(v),
                  _ => Err(DeathpwnError::Config(format!(
                      "missing required env var: {name}"
                  ))),
              }
          };

          let provider_a = ProviderConfig {
              url: required("DEATHPWN_PROVIDER_A_URL")?,
              key: required("DEATHPWN_PROVIDER_A_KEY")?,
              model: required("DEATHPWN_PROVIDER_A_MODEL")?,
          };
          let provider_b = ProviderConfig {
              url: required("DEATHPWN_PROVIDER_B_URL")?,
              key: required("DEATHPWN_PROVIDER_B_KEY")?,
              model: required("DEATHPWN_PROVIDER_B_MODEL")?,
          };

          let shell = get("DEATHPWN_SHELL")
              .filter(|s| !s.is_empty())
              .or_else(|| get("SHELL").filter(|s| !s.is_empty()))
              .unwrap_or_else(|| "/bin/sh".to_string());

          let max_goal_steps = parse_or_default(&get, "DEATHPWN_MAX_GOAL_STEPS", 12u32)?;
          let max_corrections = parse_or_default(&get, "DEATHPWN_MAX_CORRECTIONS", 2u32)?;
          let http_timeout_secs = parse_or_default(&get, "DEATHPWN_HTTP_TIMEOUT_SECS", 30u64)?;

          let artifacts_dir = resolve_artifacts_dir(&get);

          Ok(Config {
              provider_a,
              provider_b,
              shell,
              max_goal_steps,
              max_corrections,
              artifacts_dir,
              http_timeout_secs,
          })
      }
  }

  /// Parse an optional numeric env var, falling back to `default` when unset/empty.
  /// A present-but-unparseable value is a config error that names the var.
  fn parse_or_default<T>(
      get: &impl Fn(&str) -> Option<String>,
      name: &str,
      default: T,
  ) -> Result<T>
  where
      T: std::str::FromStr,
  {
      match get(name) {
          Some(raw) if !raw.is_empty() => raw.parse::<T>().map_err(|_| {
              DeathpwnError::Config(format!("invalid value for {name}: {raw:?}"))
          }),
          _ => Ok(default),
      }
  }

  /// Resolve the artifacts root: explicit override, else `$XDG_DATA_HOME/deathpwn`,
  /// else `$HOME/.local/share/deathpwn`, else a relative `.local/share/deathpwn`.
  fn resolve_artifacts_dir(get: &impl Fn(&str) -> Option<String>) -> PathBuf {
      if let Some(dir) = get("DEATHPWN_ARTIFACTS_DIR").filter(|s| !s.is_empty()) {
          return PathBuf::from(dir);
      }
      if let Some(xdg) = get("XDG_DATA_HOME").filter(|s| !s.is_empty()) {
          return PathBuf::from(xdg).join("deathpwn");
      }
      if let Some(home) = get("HOME").filter(|s| !s.is_empty()) {
          return PathBuf::from(home).join(".local/share/deathpwn");
      }
      PathBuf::from(".local/share/deathpwn")
  }

  #[cfg(test)]
  mod tests {
      use super::*;
      use std::collections::HashMap;
      use std::path::PathBuf;

      fn all_required() -> HashMap<String, String> {
          let mut m = HashMap::new();
          m.insert("DEATHPWN_PROVIDER_A_URL".into(), "https://a.example/v1".into());
          m.insert("DEATHPWN_PROVIDER_A_KEY".into(), "key-a".into());
          m.insert("DEATHPWN_PROVIDER_A_MODEL".into(), "model-a".into());
          m.insert("DEATHPWN_PROVIDER_B_URL".into(), "https://b.example/v1".into());
          m.insert("DEATHPWN_PROVIDER_B_KEY".into(), "key-b".into());
          m.insert("DEATHPWN_PROVIDER_B_MODEL".into(), "model-b".into());
          m
      }

      fn lookup(m: HashMap<String, String>) -> impl Fn(&str) -> Option<String> {
          move |k| m.get(k).cloned()
      }

      #[test]
      fn reads_required_provider_vars() {
          let cfg = Config::from_lookup(lookup(all_required())).unwrap();
          assert_eq!(cfg.provider_a.url, "https://a.example/v1");
          assert_eq!(cfg.provider_a.key, "key-a");
          assert_eq!(cfg.provider_a.model, "model-a");
          assert_eq!(cfg.provider_b.url, "https://b.example/v1");
          assert_eq!(cfg.provider_b.key, "key-b");
          assert_eq!(cfg.provider_b.model, "model-b");
      }

      #[test]
      fn missing_required_var_names_it() {
          let mut m = all_required();
          m.remove("DEATHPWN_PROVIDER_B_MODEL");
          let err = Config::from_lookup(lookup(m)).unwrap_err();
          match err {
              DeathpwnError::Config(msg) => {
                  assert!(
                      msg.contains("DEATHPWN_PROVIDER_B_MODEL"),
                      "error should name the missing var, was: {msg}"
                  );
              }
              other => panic!("expected Config error, got {other:?}"),
          }
      }

      #[test]
      fn applies_defaults_when_optional_absent() {
          let cfg = Config::from_lookup(lookup(all_required())).unwrap();
          assert_eq!(cfg.shell, "/bin/sh");
          assert_eq!(cfg.max_goal_steps, 12);
          assert_eq!(cfg.max_corrections, 2);
          assert_eq!(cfg.http_timeout_secs, 30);
          // No DEATHPWN_ARTIFACTS_DIR / XDG_DATA_HOME / HOME in the lookup.
          assert_eq!(cfg.artifacts_dir, PathBuf::from(".local/share/deathpwn"));
      }

      #[test]
      fn shell_prefers_deathpwn_then_shell_env() {
          let mut m = all_required();
          m.insert("SHELL".into(), "/usr/bin/zsh".into());
          let cfg = Config::from_lookup(lookup(m.clone())).unwrap();
          assert_eq!(cfg.shell, "/usr/bin/zsh");

          m.insert("DEATHPWN_SHELL".into(), "/usr/bin/fish".into());
          let cfg = Config::from_lookup(lookup(m)).unwrap();
          assert_eq!(cfg.shell, "/usr/bin/fish");
      }

      #[test]
      fn overrides_numeric_and_artifacts_path() {
          let mut m = all_required();
          m.insert("DEATHPWN_MAX_GOAL_STEPS".into(), "5".into());
          m.insert("DEATHPWN_MAX_CORRECTIONS".into(), "3".into());
          m.insert("DEATHPWN_HTTP_TIMEOUT_SECS".into(), "45".into());
          m.insert("DEATHPWN_ARTIFACTS_DIR".into(), "/tmp/dp".into());
          let cfg = Config::from_lookup(lookup(m)).unwrap();
          assert_eq!(cfg.max_goal_steps, 5);
          assert_eq!(cfg.max_corrections, 3);
          assert_eq!(cfg.http_timeout_secs, 45);
          assert_eq!(cfg.artifacts_dir, PathBuf::from("/tmp/dp"));
      }

      #[test]
      fn xdg_data_home_used_for_artifacts_default() {
          let mut m = all_required();
          m.insert("XDG_DATA_HOME".into(), "/xdg".into());
          let cfg = Config::from_lookup(lookup(m)).unwrap();
          assert_eq!(cfg.artifacts_dir, PathBuf::from("/xdg/deathpwn"));
      }

      #[test]
      fn bad_numeric_is_config_error_naming_var() {
          let mut m = all_required();
          m.insert("DEATHPWN_MAX_GOAL_STEPS".into(), "not-a-number".into());
          let err = Config::from_lookup(lookup(m)).unwrap_err();
          match err {
              DeathpwnError::Config(msg) => {
                  assert!(
                      msg.contains("DEATHPWN_MAX_GOAL_STEPS"),
                      "error should name the bad var, was: {msg}"
                  );
              }
              other => panic!("expected Config error, got {other:?}"),
          }
      }
  }
  ```

  `deathpwn-core/src/lib.rs`:
  ```rust
  #![forbid(unsafe_code)]

  //! deathpwn-core: natural-language offensive-security terminal (library crate).
  //! All logic and traits live here; the crate has no terminal or async-main deps.

  pub mod config;
  pub mod error;

  pub use config::{Config, ProviderConfig};
  pub use error::{DeathpwnError, Result};
  ```

- [ ] **Step 12: Run test to verify it passes** — command:
  ```
  cargo test -p deathpwn-core config
  ```
  Expected: PASS. `7 passed; 0 failed` (`reads_required_provider_vars`, `missing_required_var_names_it`, `applies_defaults_when_optional_absent`, `shell_prefers_deathpwn_then_shell_env`, `overrides_numeric_and_artifacts_path`, `xdg_data_home_used_for_artifacts_default`, `bad_numeric_is_config_error_naming_var`).

- [ ] **Step 13: Verify the whole workspace builds and all core tests pass** — commands:
  ```
  cargo build --workspace
  cargo test -p deathpwn-core
  ```
  Expected: PASS. Workspace compiles; core test run reports `10 passed; 0 failed` (3 error + 7 config). Default `cargo test` stays deterministic (no network/FS/env access in this task).

- [ ] **Step 14: Final commit**
  ```
  git add deathpwn-core/src/config.rs deathpwn-core/src/lib.rs
  git commit -m "feat(deathpwn): add Config + ProviderConfig with env loading and validation"
  ```
