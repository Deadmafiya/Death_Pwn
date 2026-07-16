### Task 10: cache: PlanCache

**Files:**
- Create: `deathpwn-core/src/cache/mod.rs` (core crate — all cache logic + unit tests)
- Edit: `deathpwn-core/src/lib.rs` (core crate — add `pub mod cache;` module declaration)
- Test: unit tests live in a `#[cfg(test)] mod tests` inside `deathpwn-core/src/cache/mod.rs` (Rust convention; manifest specifies no separate test file)

**Interfaces:**
- Consumes (from Task 2 `schema/`):
  - `struct IntentParams { target: Option<String>, ports: Option<String>, url: Option<String>, extra: BTreeMap<String, String> }`
  - `struct Stage3Plan { commands: Vec<PlannedCommand> }`
  - `struct PlannedCommand { tool: String, argv: Vec<String>, purpose: String, depends_on_prev: bool }` (used only by the tests to build a plan value)
- Produces (relied on by Task 13 `pipeline/plan.rs`):
  - `fn normalize_intent(intent: &str) -> String` — lowercase, trim, collapse whitespace
  - `fn normalize_params(p: &IntentParams) -> String` — sorted `key=val` over `target`/`ports`/`url`/`extra`
  - `struct PlanCache { map: HashMap<String, Stage3Plan> }` with:
    - `fn new() -> PlanCache`
    - `fn key(intent: &str, params: &IntentParams) -> String` = `normalize_intent + "|" + normalize_params`
    - `fn get(&self, intent: &str, params: &IntentParams) -> Option<&Stage3Plan>`
    - `fn put(&mut self, intent: &str, params: &IntentParams, plan: Stage3Plan)`

**Dependencies added this task:** none. `HashMap` and `BTreeMap` come from `std::collections`; `IntentParams`/`Stage3Plan` already exist from Task 2. No new lines in any `Cargo.toml`.

---

#### Cycle 1 — `normalize_intent`

- [ ] **Step 1: Write the failing test.** Wire the module into the crate and create the cache file with a test that pins the normalization contract (lowercase, trim, collapse internal whitespace runs to a single space).

  Add the module declaration to `deathpwn-core/src/lib.rs` alongside the other `pub mod` lines:

  ```rust
  pub mod cache;
  ```

  Create `deathpwn-core/src/cache/mod.rs`:

  ```rust
  //! Plan cache: exact normalized-key lookup for Stage 3 plans.
  //!
  //! Key = `normalize_intent(intent) + "|" + normalize_params(params)`.
  //! Equivalent phrasings collide; different parameters never do.

  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn normalize_intent_lowercases_trims_and_collapses_whitespace() {
          assert_eq!(normalize_intent("  Scan   PORT  "), "scan port");
          assert_eq!(normalize_intent("SCAN PORT"), "scan port");
          assert_eq!(
              normalize_intent("scan\tport\non\thost"),
              "scan port on host"
          );
      }
  }
  ```

- [ ] **Step 2: Run test to verify it fails.**
  Command: `cargo test -p deathpwn-core cache`
  Expected: fails to compile — `error[E0425]: cannot find function \`normalize_intent\` in this scope`.

- [ ] **Step 3: Implement `normalize_intent`.** Add the imports and function to the top of `deathpwn-core/src/cache/mod.rs` (above the `#[cfg(test)]` module):

  ```rust
  use std::collections::{BTreeMap, HashMap};

  use crate::schema::{IntentParams, Stage3Plan};

  /// Lowercase, trim, and collapse internal whitespace runs to single spaces.
  pub fn normalize_intent(intent: &str) -> String {
      intent
          .trim()
          .to_lowercase()
          .split_whitespace()
          .collect::<Vec<_>>()
          .join(" ")
  }
  ```

  Note: `IntentParams`, `Stage3Plan`, `BTreeMap`, and `HashMap` are imported now because the next two cycles use them; they compile cleanly here since `IntentParams`/`Stage3Plan`/`HashMap` are consumed by later steps in this same file.

- [ ] **Step 4: Run test to verify it passes.**
  Command: `cargo test -p deathpwn-core cache`
  Expected: PASS — `test cache::tests::normalize_intent_lowercases_trims_and_collapses_whitespace ... ok`.

- [ ] **Step 5: Commit.**
  `git add deathpwn-core/src/lib.rs deathpwn-core/src/cache/mod.rs && git commit -m "feat(deathpwn): add normalize_intent for plan cache keys"`

---

#### Cycle 2 — `normalize_params`

- [ ] **Step 6: Write the failing test.** Add a test-only `params` helper and two tests covering the sorted `key=val` encoding and the omission of `None` fields. Insert these into the `mod tests` block in `deathpwn-core/src/cache/mod.rs`:

  ```rust
      use crate::schema::{PlannedCommand, Stage3Plan};
      use std::collections::BTreeMap;

      fn params(
          target: Option<&str>,
          ports: Option<&str>,
          url: Option<&str>,
          extra: &[(&str, &str)],
      ) -> IntentParams {
          let mut map = BTreeMap::new();
          for (k, v) in extra {
              map.insert(k.to_string(), v.to_string());
          }
          IntentParams {
              target: target.map(|s| s.to_string()),
              ports: ports.map(|s| s.to_string()),
              url: url.map(|s| s.to_string()),
              extra: map,
          }
      }

      #[test]
      fn normalize_params_sorts_key_value_pairs_and_normalizes_values() {
          let p = params(
              Some("192.168.1.1"),
              Some("22,80"),
              None,
              &[("scheme", "TCP"), ("aggr", "T4")],
          );
          // Keys sorted alphabetically: aggr, ports, scheme, target. Values lowercased.
          assert_eq!(
              normalize_params(&p),
              "aggr=t4&ports=22,80&scheme=tcp&target=192.168.1.1"
          );
      }

      #[test]
      fn normalize_params_ignores_none_fields() {
          let p = params(Some("host"), None, None, &[]);
          assert_eq!(normalize_params(&p), "target=host");
      }
  ```

  (The `PlannedCommand`/`Stage3Plan` imports are used by Cycle 3's tests; adding them now keeps the single test module coherent.)

- [ ] **Step 7: Run test to verify it fails.**
  Command: `cargo test -p deathpwn-core cache`
  Expected: fails to compile — `error[E0425]: cannot find function \`normalize_params\` in this scope`.

- [ ] **Step 8: Implement `normalize_params`.** Add the function to `deathpwn-core/src/cache/mod.rs` directly after `normalize_intent`:

  ```rust
  /// Deterministic, sorted `key=value` encoding of the intent parameters.
  ///
  /// `target`/`ports`/`url` are emitted as fixed keys when present; every entry
  /// in `extra` is included. Keys are sorted (via `BTreeMap`) so equivalent
  /// parameter sets encode identically, and values are normalized like the
  /// intent so casing/whitespace differences collide.
  pub fn normalize_params(p: &IntentParams) -> String {
      let mut parts: BTreeMap<String, String> = BTreeMap::new();
      if let Some(target) = &p.target {
          parts.insert("target".to_string(), normalize_intent(target));
      }
      if let Some(ports) = &p.ports {
          parts.insert("ports".to_string(), normalize_intent(ports));
      }
      if let Some(url) = &p.url {
          parts.insert("url".to_string(), normalize_intent(url));
      }
      for (k, v) in &p.extra {
          parts.insert(normalize_intent(k), normalize_intent(v));
      }
      parts
          .iter()
          .map(|(k, v)| format!("{k}={v}"))
          .collect::<Vec<_>>()
          .join("&")
  }
  ```

- [ ] **Step 9: Run test to verify it passes.**
  Command: `cargo test -p deathpwn-core cache`
  Expected: PASS — `normalize_params_sorts_key_value_pairs_and_normalizes_values ... ok` and `normalize_params_ignores_none_fields ... ok`.

- [ ] **Step 10: Commit.**
  `git add deathpwn-core/src/cache/mod.rs && git commit -m "feat(deathpwn): add normalize_params for plan cache keys"`

---

#### Cycle 3 — `PlanCache` (key format, hit, and the required miss)

- [ ] **Step 11: Write the failing tests.** Add a `sample_plan` helper and three tests: the key format, an equivalent-phrasing hit, and the **required** `192.168.1.1` vs `192.168.1.2` miss. Insert into the `mod tests` block in `deathpwn-core/src/cache/mod.rs`:

  ```rust
      fn sample_plan(tool: &str) -> Stage3Plan {
          Stage3Plan {
              commands: vec![PlannedCommand {
                  tool: tool.to_string(),
                  argv: vec!["-p".to_string(), "80".to_string()],
                  purpose: "scan".to_string(),
                  depends_on_prev: false,
              }],
          }
      }

      #[test]
      fn key_is_normalized_intent_pipe_normalized_params() {
          let p = params(Some("192.168.1.1"), Some("80"), None, &[]);
          assert_eq!(
              PlanCache::key("  Scan PORT ", &p),
              "scan port|ports=80&target=192.168.1.1"
          );
      }

      #[test]
      fn same_intent_and_params_hit_even_with_different_phrasing() {
          let mut cache = PlanCache::new();
          let p = params(Some("192.168.1.1"), Some("80"), None, &[]);
          cache.put("scan port", &p, sample_plan("nmap"));

          // Case + whitespace differences must still resolve to the same entry.
          let got = cache.get("  SCAN   port ", &p);
          assert!(got.is_some());
          assert_eq!(got.unwrap().commands[0].tool, "nmap");
      }

      #[test]
      fn different_target_must_miss() {
          let mut cache = PlanCache::new();
          let p1 = params(Some("192.168.1.1"), None, None, &[]);
          let p2 = params(Some("192.168.1.2"), None, None, &[]);
          cache.put("scan port on 192.168.1.1", &p1, sample_plan("nmap"));

          // Required rule (GOAL.md §7 / manifest): .1.1 vs .1.2 must NOT collide.
          assert!(cache.get("scan port on 192.168.1.2", &p2).is_none());
      }
  ```

- [ ] **Step 12: Run test to verify it fails.**
  Command: `cargo test -p deathpwn-core cache`
  Expected: fails to compile — `error[E0433]: failed to resolve: use of undeclared type \`PlanCache\`` (and `no function or associated item named \`new\``).

- [ ] **Step 13: Implement `PlanCache`.** Add the struct and its impl to `deathpwn-core/src/cache/mod.rs` after `normalize_params`:

  ```rust
  /// In-memory exact-match cache of Stage 3 plans, keyed by normalized
  /// intent + params. No embeddings; a hit requires intent AND params to match.
  #[derive(Debug, Default)]
  pub struct PlanCache {
      map: HashMap<String, Stage3Plan>,
  }

  impl PlanCache {
      pub fn new() -> Self {
          Self {
              map: HashMap::new(),
          }
      }

      /// Compose the cache key: `normalize_intent + "|" + normalize_params`.
      pub fn key(intent: &str, params: &IntentParams) -> String {
          format!(
              "{}|{}",
              normalize_intent(intent),
              normalize_params(params)
          )
      }

      pub fn get(&self, intent: &str, params: &IntentParams) -> Option<&Stage3Plan> {
          self.map.get(&Self::key(intent, params))
      }

      pub fn put(&mut self, intent: &str, params: &IntentParams, plan: Stage3Plan) {
          self.map.insert(Self::key(intent, params), plan);
      }
  }
  ```

- [ ] **Step 14: Run test to verify it passes.**
  Command: `cargo test -p deathpwn-core cache`
  Expected: PASS — all cache tests green, including `different_target_must_miss ... ok`. Confirm the full core suite still builds with `cargo test -p deathpwn-core` (expected: PASS, no regressions).

- [ ] **Step 15: Commit (final).**
  `git add deathpwn-core/src/cache/mod.rs && git commit -m "feat(deathpwn): add PlanCache with exact normalized-key lookup"`
