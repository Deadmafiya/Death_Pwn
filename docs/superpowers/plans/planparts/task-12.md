### Task 12: pipeline: Stage 2 Retrieve

Stage 2 turns a validated `Stage1Understanding` into a `Stage2Knowledge` (a theory
plus concrete candidate commands). It builds a web-search query from the intent and
params, runs it through the injected `SearchProvider`, then feeds the results (or an
explicit "no results, use your own knowledge" note) plus the intent to the AI through
the `FailoverClient`, validating the reply into `Stage2Knowledge`. Search-thin input
degrades gracefully rather than failing (spec §1 decision 3, §8 Stage 2).

**Files:**
- Create: `deathpwn-core/src/pipeline/retrieve.rs` (core crate)
- Modify: `deathpwn-core/src/pipeline/mod.rs` (core crate — add `mod retrieve;` + re-exports; created in Task 11)
- Test: unit tests live in a `#[cfg(test)] mod tests` at the bottom of `deathpwn-core/src/pipeline/retrieve.rs` (Rust convention; manifest names no separate test file)

**Interfaces:**
- Consumes:
  - `struct Stage1Understanding { intent: String, params: IntentParams, mode: Mode, goal_summary: String }` (Task 2)
  - `struct IntentParams { target: Option<String>, ports: Option<String>, url: Option<String>, extra: BTreeMap<String,String> }` (Task 2)
  - `struct Stage2Knowledge { theory: String, candidates: Vec<CandidateCommand> }` (Task 2)
  - `struct CandidateCommand { tool: String, argv: Vec<String>, purpose: String }` (Task 2)
  - `struct ChatRequest { system: String, user: String, temperature: f32 }` (Task 3)
  - `struct FailoverClient { a: Arc<dyn AiProvider>, b: Arc<dyn AiProvider>, clock: Arc<dyn Clock> }` with `async fn complete_validated<T, F>(&self, req: &ChatRequest, validate: F) -> Result<T> where F: Fn(&str) -> std::result::Result<T, String>` (Task 4)
  - `#[async_trait] trait SearchProvider: Send + Sync { async fn search(&self, query: &str) -> Result<Vec<SearchResult>>; }` and `struct SearchResult { title: String, url: String, snippet: String }` (Task 5)
  - `type Result<T> = std::result::Result<T, DeathpwnError>;` (Task 1)
  - test-support fakes re-exported from earlier tasks: `FakeAiProvider` (Task 3, `crate::providers::FakeAiProvider`), `FakeClock` (Task 3, `crate::clock::FakeClock`), `FakeSearchProvider` (Task 5, `crate::search::FakeSearchProvider`)
- Produces:
  - `struct Retrieve { ai: FailoverClient, search: Arc<dyn SearchProvider> }` with `pub fn new(ai: FailoverClient, search: Arc<dyn SearchProvider>) -> Self` and `pub async fn run(&self, u: &Stage1Understanding) -> Result<Stage2Knowledge>`
  - `pub fn build_query(u: &Stage1Understanding) -> String`

**Dependencies:** No new crate dependencies. This task uses `serde_json` (Task 2), `async-trait` (Task 3), and the `[dev-dependencies]` `tokio = { version = "1", features = ["macros", "rt"] }` already present from Task 3. Before starting, confirm that dev-dependency exists in `deathpwn-core/Cargo.toml`; if a prior task already added it, no change is needed.

---

- [ ] **Step 1: Write the failing test for `build_query`** — add the test module skeleton and the first behavior to `deathpwn-core/src/pipeline/retrieve.rs`.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{IntentParams, Mode, Stage1Understanding};
    use std::collections::BTreeMap;

    fn sample_understanding() -> Stage1Understanding {
        Stage1Understanding {
            intent: "scan for open ports".to_string(),
            params: IntentParams {
                target: Some("192.168.1.1".to_string()),
                ports: Some("1-1000".to_string()),
                url: None,
                extra: BTreeMap::new(),
            },
            mode: Mode::SingleCommand,
            goal_summary: "enumerate open ports on host".to_string(),
        }
    }

    #[test]
    fn build_query_includes_intent_and_target() {
        let u = sample_understanding();
        let q = build_query(&u);
        assert!(q.contains("scan for open ports"), "query missing intent: {q}");
        assert!(q.contains("192.168.1.1"), "query missing target: {q}");
    }
}
```

- [ ] **Step 2: Run test to verify it fails** — `cargo test -p deathpwn-core retrieve`. Expected: fails to compile — `error[E0432]: unresolved import` / `cannot find function build_query in this scope` (neither `retrieve.rs`'s items nor the module wiring exist yet).

- [ ] **Step 3: Implement `build_query` and wire the module** — create the top of `deathpwn-core/src/pipeline/retrieve.rs` with imports and the pure query builder, and register the module.

Create `deathpwn-core/src/pipeline/retrieve.rs` (top portion, above the `#[cfg(test)]` block from Step 1):

```rust
use std::sync::Arc;

use crate::error::Result;
use crate::providers::failover::FailoverClient;
use crate::providers::ChatRequest;
use crate::schema::{Stage1Understanding, Stage2Knowledge};
use crate::search::{SearchProvider, SearchResult};

/// Sampling temperature for the retrieval stage. Low, since we want stable,
/// grounded candidate commands rather than creative variety.
const RETRIEVE_TEMPERATURE: f32 = 0.2;

/// System prompt: pins the model to emit ONLY JSON matching `Stage2Knowledge`.
const RETRIEVE_SYSTEM: &str = "You are the retrieval stage of an offensive-security \
assistant. Given an operator intent and web search results about relevant tooling and \
techniques, produce a concise theory and a list of concrete candidate commands. Respond \
with ONLY a JSON object matching this schema: {\"theory\": string, \"candidates\": \
[{\"tool\": string, \"argv\": [string], \"purpose\": string}]}. Emit no prose outside \
the JSON.";

/// Build the web-search query for Stage 2 from the understanding.
pub fn build_query(u: &Stage1Understanding) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(u.intent.trim().to_string());
    if let Some(target) = u.params.target.as_deref() {
        let target = target.trim();
        if !target.is_empty() {
            parts.push(target.to_string());
        }
    }
    if let Some(url) = u.params.url.as_deref() {
        let url = url.trim();
        if !url.is_empty() {
            parts.push(url.to_string());
        }
    }
    parts.push("kali OR blackarch command usage example".to_string());
    parts.join(" ")
}
```

Add to `deathpwn-core/src/pipeline/mod.rs` (alongside the `understand` wiring from Task 11):

```rust
mod retrieve;

pub use retrieve::{build_query, Retrieve};
```

- [ ] **Step 4: Run test to verify it passes** — `cargo test -p deathpwn-core retrieve`. Expected: `test pipeline::retrieve::tests::build_query_includes_intent_and_target ... ok`.

- [ ] **Step 5: Commit** — `git add deathpwn-core/src/pipeline/retrieve.rs deathpwn-core/src/pipeline/mod.rs` && `git commit -m "feat(deathpwn): add Stage 2 retrieve query builder"`

---

- [ ] **Step 6: Write the failing test for graceful-degrade prompt** — the prompt fed to the AI must differ between "results present" and "no results". Add these tests inside the same `mod tests`.

```rust
    #[test]
    fn prompt_embeds_results_when_present() {
        let u = sample_understanding();
        let results = vec![SearchResult {
            title: "nmap cheat sheet".to_string(),
            url: "https://example.com/nmap".to_string(),
            snippet: "nmap -p- scans all 65535 ports".to_string(),
        }];
        let req = build_request(&u, &results);
        assert_eq!(req.system, RETRIEVE_SYSTEM);
        assert_eq!(req.temperature, RETRIEVE_TEMPERATURE);
        assert!(req.user.contains("nmap cheat sheet"), "prompt missing result title: {}", req.user);
        assert!(req.user.contains("scan for open ports"), "prompt missing intent");
        assert!(!req.user.contains("No search results were found"));
    }

    #[test]
    fn prompt_degrades_when_no_results() {
        let u = sample_understanding();
        let req = build_request(&u, &[]);
        assert!(req.user.contains("No search results were found"), "prompt missing degrade note: {}", req.user);
        assert!(req.user.contains("your own knowledge"), "prompt missing self-knowledge instruction");
    }

    #[test]
    fn prompt_differs_with_and_without_results() {
        let u = sample_understanding();
        let with = build_request(
            &u,
            &[SearchResult {
                title: "nmap cheat sheet".to_string(),
                url: "https://example.com/nmap".to_string(),
                snippet: "nmap -p- scans all ports".to_string(),
            }],
        );
        let without = build_request(&u, &[]);
        assert_ne!(with.user, without.user, "graceful-degrade prompt must differ");
    }
```

- [ ] **Step 7: Run test to verify it fails** — `cargo test -p deathpwn-core retrieve`. Expected: fails to compile — `cannot find function build_request in this scope`.

- [ ] **Step 8: Implement `build_request`** — add the private prompt builder to `deathpwn-core/src/pipeline/retrieve.rs`, below `build_query`.

```rust
/// Build the `ChatRequest` for Stage 2. When `results` is empty the user prompt
/// explicitly tells the model no search results were found and to rely on its own
/// knowledge — the graceful-degrade path (spec §8 Stage 2).
fn build_request(u: &Stage1Understanding, results: &[SearchResult]) -> ChatRequest {
    let mut user = String::new();
    user.push_str(&format!("Operator intent: {}\n", u.intent));
    user.push_str(&format!("Goal summary: {}\n", u.goal_summary));
    if let Some(target) = u.params.target.as_deref() {
        user.push_str(&format!("Target: {}\n", target));
    }
    if let Some(ports) = u.params.ports.as_deref() {
        user.push_str(&format!("Ports: {}\n", ports));
    }
    if let Some(url) = u.params.url.as_deref() {
        user.push_str(&format!("URL: {}\n", url));
    }
    for (k, v) in &u.params.extra {
        user.push_str(&format!("{k}: {v}\n"));
    }
    user.push('\n');
    if results.is_empty() {
        user.push_str(
            "No search results were found. Rely on your own knowledge of offensive-security \
             tooling to propose candidate commands.\n",
        );
    } else {
        user.push_str("Search results:\n");
        for (i, r) in results.iter().enumerate() {
            user.push_str(&format!("{}. {}\n   {}\n   {}\n", i + 1, r.title, r.url, r.snippet));
        }
    }
    ChatRequest {
        system: RETRIEVE_SYSTEM.to_string(),
        user,
        temperature: RETRIEVE_TEMPERATURE,
    }
}
```

- [ ] **Step 9: Run test to verify it passes** — `cargo test -p deathpwn-core retrieve`. Expected: `prompt_embeds_results_when_present`, `prompt_degrades_when_no_results`, and `prompt_differs_with_and_without_results` all `... ok`.

- [ ] **Step 10: Commit** — `git add deathpwn-core/src/pipeline/retrieve.rs` && `git commit -m "feat(deathpwn): add Stage 2 retrieve prompt builder with search degrade"`

---

- [ ] **Step 11: Write the failing test for `Retrieve::run`** — end-to-end through `FakeSearchProvider` + `FailoverClient(FakeAiProvider, FakeAiProvider, FakeClock)`, for both the results and empty-results paths. Add to `mod tests`.

```rust
    use crate::clock::FakeClock;
    use crate::providers::FakeAiProvider;
    use crate::search::FakeSearchProvider;

    fn failover_returning(canned: &str) -> FailoverClient {
        // Provider A succeeds with the canned JSON; B is present but never reached.
        let a = Arc::new(FakeAiProvider::with_script("A", vec![Ok(canned.to_string())]));
        let b = Arc::new(FakeAiProvider::with_script("B", vec![]));
        let clock = Arc::new(FakeClock::fixed(0));
        FailoverClient::new(a, b, clock)
    }

    #[tokio::test]
    async fn run_returns_parsed_knowledge_with_results() {
        let canned = r#"{"theory":"scan ports with nmap","candidates":[{"tool":"nmap","argv":["-p-","192.168.1.1"],"purpose":"full port scan"}]}"#;
        let ai = failover_returning(canned);
        let search = Arc::new(FakeSearchProvider::new(vec![SearchResult {
            title: "nmap".to_string(),
            url: "https://example.com".to_string(),
            snippet: "port scanner".to_string(),
        }]));
        let retrieve = Retrieve::new(ai, search);

        let k = retrieve.run(&sample_understanding()).await.unwrap();
        assert_eq!(k.theory, "scan ports with nmap");
        assert_eq!(k.candidates.len(), 1);
        assert_eq!(k.candidates[0].tool, "nmap");
        assert_eq!(
            k.candidates[0].argv,
            vec!["-p-".to_string(), "192.168.1.1".to_string()]
        );
        assert_eq!(k.candidates[0].purpose, "full port scan");
    }

    #[tokio::test]
    async fn run_returns_parsed_knowledge_when_search_empty() {
        let canned = r#"{"theory":"use own knowledge of nmap","candidates":[]}"#;
        let ai = failover_returning(canned);
        let search = Arc::new(FakeSearchProvider::new(vec![]));
        let retrieve = Retrieve::new(ai, search);

        let k = retrieve.run(&sample_understanding()).await.unwrap();
        assert_eq!(k.theory, "use own knowledge of nmap");
        assert!(k.candidates.is_empty());
    }
```

- [ ] **Step 12: Run test to verify it fails** — `cargo test -p deathpwn-core retrieve`. Expected: fails to compile — `cannot find type Retrieve in this scope` / `no function or associated item named new` (the struct and `run` do not exist yet).

- [ ] **Step 13: Implement `Retrieve`** — add the struct, constructor, and `run` to `deathpwn-core/src/pipeline/retrieve.rs`, below `build_request`.

```rust
/// Stage 2 of the pipeline: retrieve. Turns a Stage 1 understanding into
/// candidate commands, grounded in web search when available.
pub struct Retrieve {
    ai: FailoverClient,
    search: Arc<dyn SearchProvider>,
}

impl Retrieve {
    pub fn new(ai: FailoverClient, search: Arc<dyn SearchProvider>) -> Self {
        Self { ai, search }
    }

    /// Build a query, search the web, then ask the AI (with failover + schema
    /// validation) for a `Stage2Knowledge`. Empty search results degrade
    /// gracefully via the prompt built in `build_request`.
    pub async fn run(&self, u: &Stage1Understanding) -> Result<Stage2Knowledge> {
        let query = build_query(u);
        let results = self.search.search(&query).await?;
        let req = build_request(u, &results);
        self.ai
            .complete_validated(&req, |content: &str| {
                serde_json::from_str::<Stage2Knowledge>(content).map_err(|e| e.to_string())
            })
            .await
    }
}
```

- [ ] **Step 14: Run test to verify it passes** — `cargo test -p deathpwn-core retrieve`. Expected: `run_returns_parsed_knowledge_with_results ... ok` and `run_returns_parsed_knowledge_when_search_empty ... ok`, with all earlier retrieve tests still green. Also run `cargo build -p deathpwn-core` to confirm `#![forbid(unsafe_code)]` and the module wiring compile clean.

- [ ] **Step 15: Commit** — `git add deathpwn-core/src/pipeline/retrieve.rs` && `git commit -m "feat(deathpwn): implement Stage 2 Retrieve pipeline stage"`
