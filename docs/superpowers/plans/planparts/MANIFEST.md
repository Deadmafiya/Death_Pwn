# deathPWN v1 — Task Manifest (interface contract)

16 TDD tasks. Every signature below is authoritative and copied from the spec.
Task authors must use these EXACT names/types so cross-task interfaces line up.

Crate: `deathpwn-core` (lib) unless noted `deathpwn-tui` (bin).
`#![forbid(unsafe_code)]` in core. `#[async_trait]` for async traits.

---

## Task 1 — Workspace skeleton + error + config
Files: `Cargo.toml` (workspace), `deathpwn-core/Cargo.toml`, `deathpwn-core/src/lib.rs`,
`deathpwn-core/src/error.rs`, `deathpwn-core/src/config.rs`, `deathpwn-tui/Cargo.toml`,
`deathpwn-tui/src/main.rs` (placeholder that builds).
Produces:
- `enum DeathpwnError` (thiserror): `Config(String)`, `Provider(String)`, `Search(String)`,
  `Exec(String)`, `Schema(String)`, `Cache(String)`, `Io(#[from] std::io::Error)`, `Cancelled`.
- `type Result<T> = std::result::Result<T, DeathpwnError>;`
- `struct Config { provider_a: ProviderConfig, provider_b: ProviderConfig, shell: String,
  max_goal_steps: u32, max_corrections: u32, artifacts_dir: PathBuf, http_timeout_secs: u64 }`
- `struct ProviderConfig { url: String, key: String, model: String }`
- `Config::from_env() -> Result<Config>` (validates required vars; error names the missing var).
Deps: thiserror. Workspace resolver = "2", edition 2021.

## Task 2 — schema/ (all stage structs)
Files: `deathpwn-core/src/schema/mod.rs` (+ submodules if desired).
Consumes: nothing.
Produces (all `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]`,
enums `#[serde(rename_all = "snake_case")]`):
- `struct Stage1Understanding { intent: String, params: IntentParams, mode: Mode, goal_summary: String }`
- `struct IntentParams { target: Option<String>, ports: Option<String>, url: Option<String>, extra: BTreeMap<String,String> }`
- `enum Mode { SingleCommand, GoalCompletion }`
- `struct Stage2Knowledge { theory: String, candidates: Vec<CandidateCommand> }`
- `struct CandidateCommand { tool: String, argv: Vec<String>, purpose: String }`
- `struct Stage3Plan { commands: Vec<PlannedCommand> }`
- `struct PlannedCommand { tool: String, argv: Vec<String>, purpose: String, depends_on_prev: bool }`
- `struct Stage4Render { sections: Vec<RenderSection> }`
- `struct RenderSection { title: String, kind: SectionKind, body: RenderBody }`
- `enum SectionKind { Table, KeyValue, Text, Findings }`
- `enum RenderBody { Table { headers: Vec<String>, rows: Vec<Vec<String>> }, KeyValue(Vec<(String,String)>), Text(String), Findings(Vec<FindingItem>) }`
  (use `#[serde(tag="kind")]` or untagged — author picks; round-trip test must pass)
- `struct FindingItem { severity: String, title: String, detail: String }`
- `enum FailureClass { NotFound, BenignEmpty, FixableUsage, Transient, Fatal }`
- `struct ExecFailureVerdict { class: FailureClass, corrected_argv: Option<Vec<String>> }`
- `struct GoalVerdict { achieved: bool, reason: String, next_step_hint: Option<String> }`
Deps: serde (derive), serde_json. Tests: round-trip + malformed-JSON rejection per struct.

## Task 3 — providers: AiProvider trait + Clock + OpenAiClient
Files: `deathpwn-core/src/providers/mod.rs`, `.../ai.rs`, `.../openai.rs`, `deathpwn-core/src/clock.rs`.
Consumes: DeathpwnError.
Produces:
- `struct ChatRequest { system: String, user: String, temperature: f32 }`
- `enum ProviderError { Network(String), Timeout, Http { status: u16 }, RateLimit, Decode(String) }`
- `#[async_trait] trait AiProvider: Send + Sync { async fn complete(&self, req: &ChatRequest) -> std::result::Result<String, ProviderError>; fn label(&self) -> &str; }`
- `trait Clock: Send + Sync { fn now_ms(&self) -> u64; }` + `struct SystemClock;` impl.
- `struct OpenAiClient { /* base url, key, model, http client, label */ }` impl `AiProvider`
  (reqwest POST `{base}/chat/completions`, bearer, parse `choices[0].message.content`).
- test-support: `struct FakeAiProvider` (scriptable responses/errors) + `struct FakeClock`
  in a `#[cfg(any(test, feature="test-support"))]` module re-exported for other tasks.
Deps: async-trait, reqwest (json), serde_json, tokio (for tests). OpenAiClient real HTTP test is `#[ignore]`.

## Task 4 — providers: FailoverClient
Files: `deathpwn-core/src/providers/failover.rs`.
Consumes: AiProvider, Clock, ChatRequest, ProviderError.
Produces:
- `struct FailoverClient { a: Arc<dyn AiProvider>, b: Arc<dyn AiProvider>, clock: Arc<dyn Clock> }`
- `impl FailoverClient { async fn complete_validated<T, F>(&self, req: &ChatRequest, validate: F) -> Result<T>
   where F: Fn(&str) -> std::result::Result<T, String> }`
  A→validate; on A error OR validation-fail → B→validate; both fail → DeathpwnError::Provider(aggregated).
  Log each attempt (label, latency via clock, outcome). Circuit breaker OFF (not in v1).
Tests (fakes from Task 3): {A ok}, {A err→B ok}, {A bad-json→B ok}, {both fail}.

## Task 5 — search: SearchProvider trait + DDG scrape
Files: `deathpwn-core/src/search/mod.rs`, `.../ddg.rs`.
Consumes: DeathpwnError.
Produces:
- `struct SearchResult { title: String, url: String, snippet: String }`
- `#[async_trait] trait SearchProvider: Send + Sync { async fn search(&self, query: &str) -> Result<Vec<SearchResult>>; }`
- `struct DuckDuckGoSearch { /* http client */ }` impl (scrape `https://html.duckduckgo.com/html/?q=`).
- `fn parse_ddg_html(html: &str) -> Vec<SearchResult>` (pure, unit-tested against a fixture string).
- test-support: `struct FakeSearchProvider` (canned results) re-exported.
Deps: reqwest, scraper (HTML). Real network test `#[ignore]`. Pure parser test uses embedded fixture.

## Task 6 — detector: Step 0
Files: `deathpwn-core/src/detector/mod.rs`.
Consumes: CommandRunner trait (from Task 7 — see note), RunOutcome, CancelToken.
NOTE: detector depends on CommandRunner. To avoid a cycle, Task 7 lands first for the trait;
detector only needs the trait + `run_shell`. Author against the Task 7 signatures below.
Produces:
- `enum InputKind { DirectCommand, RawInput }`
- `struct Detector<R: CommandRunner> { runner: R, shell: String }`
- `impl Detector { async fn classify(&self, line: &str) -> InputKind }`
  empty/whitespace → RawInput; else leading token via `shell_words`, run
  `command -v -- <token>` through `runner.run_shell`; exit 0 → DirectCommand else RawInput.
Deps: shell_words. Tests use FakeCommandRunner scripted by token→exit.

## Task 7 — exec: CommandRunner trait + ShellRunner + CancelToken
Files: `deathpwn-core/src/exec/mod.rs`, `.../runner.rs`, `deathpwn-core/src/cancel.rs`.
Consumes: nothing (foundational).
Produces:
- `struct CommandSpec { tool: String, argv: Vec<String> }`
- `struct RunOutcome { exit: Option<i32>, stdout: String, stderr: String, cancelled: bool }`
- `struct OutputLine { stream: Stream, text: String }` + `enum Stream { Stdout, Stderr }`
- `#[derive(Clone)] struct CancelToken(/* Arc<Notify> or tokio CancellationToken */)` with
  `fn cancel(&self)`, `fn is_cancelled(&self) -> bool`, and an async `cancelled()` future.
- `#[async_trait] trait CommandRunner: Send + Sync {
    async fn run(&self, spec: &CommandSpec, cancel: CancelToken) -> RunOutcome;
    async fn run_shell(&self, script: &str, cancel: CancelToken) -> RunOutcome; }`
- `struct ShellRunner { shell: String, tx: Option<mpsc::Sender<OutputLine>> }` impl:
  spawn `$SHELL -c <script>` via `tokio::process::Command` `.process_group(0)`, capture out/err,
  emit lines on tx if present; on cancel send SIGTERM then SIGKILL to the group.
- test-support: `struct FakeCommandRunner` (script: input→RunOutcome sequence) re-exported.
Deps: tokio (process, sync), async-trait, nix (kill/signals, process group). ShellRunner real test `#[ignore]` (runs `echo`).

## Task 8 — exec: FeedbackLoop + installer
Files: `deathpwn-core/src/exec/feedback.rs`, `.../installer.rs`.
Consumes: CommandRunner, AiProvider, CommandSpec, RunOutcome, CancelToken, ExecFailureVerdict, FailureClass, Config.
Produces:
- `struct FeedbackLoop<R: CommandRunner> { runner: R, ai: Arc<dyn AiProvider>, max_corrections: u32 }`
- `struct AttemptLog { argv: Vec<String>, exit: Option<i32>, note: String }`
- `struct FeedbackOutcome { outcome: RunOutcome, attempts: Vec<AttemptLog> }`
- `impl FeedbackLoop { async fn run(&self, spec: &CommandSpec, cancel: CancelToken) -> Result<FeedbackOutcome> }`
  1. availability `command -v tool`; miss → installer (AI → install cmd → run) then retry (not a correction).
  2. run. 3. non-zero → AI classify → ExecFailureVerdict:
     NotFound→install loop; BenignEmpty→report ok; FixableUsage→apply corrected_argv, retry (counts);
     Transient→retry once (counts); Fatal→stop. Cap = max_corrections (default 2). Log every attempt.
Tests: fake runner (bad-flag then ok) + fake AI (FixableUsage) asserts retry + cap.

## Task 9 — session: SessionState
Files: `deathpwn-core/src/session/mod.rs`, `.../artifacts.rs`.
Consumes: RunOutcome, Clock.
Produces:
- `struct Target { value: String }` (host or url)
- `struct Finding { severity: String, title: String, detail: String }`
- `struct SessionState { targets: Vec<Target>, hosts: Vec<String>, ports_by_host: BTreeMap<String,Vec<u16>>,
   services: Vec<String>, findings: Vec<Finding>, command_log: Vec<String> }`
  methods: `new()`, `add_target`, `record_command(&str)`, `add_finding`, `add_ports(host, ports)`, getters.
- `struct Artifacts { root: PathBuf, session_dir: PathBuf }` with
  `Artifacts::open(root, clock: &dyn Clock) -> Result<Artifacts>` (dir = root/<now_ms>),
  `fn write_output(&self, index: usize, outcome: &RunOutcome) -> Result<PathBuf>`.
Tests: session mutation/read; artifacts with FakeClock into a tempdir.
Deps: tempfile (dev).

## Task 10 — cache: PlanCache
Files: `deathpwn-core/src/cache/mod.rs`.
Consumes: Stage3Plan, IntentParams.
Produces:
- `fn normalize_intent(intent: &str) -> String` (lowercase, trim, collapse ws)
- `fn normalize_params(p: &IntentParams) -> String` (sorted key=val; target/ports/url/extra)
- `struct PlanCache { map: HashMap<String, Stage3Plan> }` with
  `key(intent, params) -> String` = `normalize_intent + "|" + normalize_params`,
  `get(intent, params) -> Option<&Stage3Plan>`, `put(intent, params, plan)`.
Tests: same intent+params hits; **`scan port on 192.168.1.1` vs `...1.2` MUST miss** (required).

## Task 11 — pipeline: Stage 1 Understand
Files: `deathpwn-core/src/pipeline/mod.rs`, `.../understand.rs`.
Consumes: FailoverClient, SessionState, Stage1Understanding, GoalContext (from Task 15 — define minimal here).
NOTE: GoalContext is produced in Task 15. To break the cycle, Task 11 defines
`GoalContext` in `goal` module? No — Task 15 owns it. Instead Stage 1 returns Stage1Understanding only;
GoalContext construction happens in engine (Task 15). Stage 1 does NOT reference GoalContext.
Produces:
- `struct Understand { ai: FailoverClient }` with
  `async fn run(&self, raw: &str, session: &SessionState) -> Result<Stage1Understanding>`
  (builds system+user prompt embedding session summary; validates via serde into Stage1Understanding).
- `fn session_summary(session: &SessionState) -> String` (compact context string).
Tests: fake failover (via 2 fake AIs) returns canned JSON → asserts parsed struct + that session appears in prompt.

## Task 12 — pipeline: Stage 2 Retrieve
Files: `deathpwn-core/src/pipeline/retrieve.rs`.
Consumes: FailoverClient, SearchProvider, Stage1Understanding, Stage2Knowledge.
Produces:
- `struct Retrieve { ai: FailoverClient, search: Arc<dyn SearchProvider> }` with
  `async fn run(&self, u: &Stage1Understanding) -> Result<Stage2Knowledge>`
  (build query from intent/params; search; if empty → prompt notes "no results, use own knowledge";
  feed results+intent to AI → Stage2Knowledge).
- `fn build_query(u: &Stage1Understanding) -> String`.
Tests: fake search {results} and {empty} both → fake AI canned → asserts graceful-degrade prompt differs.

## Task 13 — pipeline: Stage 3 Plan
Files: `deathpwn-core/src/pipeline/plan.rs`.
Consumes: FailoverClient, PlanCache, Stage1Understanding, Stage2Knowledge, SessionState, Stage3Plan.
Produces:
- `struct Plan { ai: FailoverClient }` with
  `async fn run(&self, u: &Stage1Understanding, k: &Stage2Knowledge, session: &SessionState, cache: &mut PlanCache) -> Result<Stage3Plan>`
  (cache lookup by (intent, params); miss → AI → Stage3Plan → cache.put; hit → return clone).
Tests: cache miss calls AI; second identical call hits cache (AI NOT called again — fake asserts call count).

## Task 14 — pipeline: Stage 4 Render
Files: `deathpwn-core/src/pipeline/render.rs`.
Consumes: FailoverClient, RunOutcome, Stage4Render, Stage1Understanding.
Produces:
- `struct Render { ai: FailoverClient }` with
  `async fn run(&self, u: &Stage1Understanding, outcome: &RunOutcome) -> Result<Stage4Render>`
  (feed intent + stdout/stderr/exit to AI → Stage4Render; NOT cached).
Tests: fake AI canned Stage4Render JSON → parsed; malformed → failover → error.

## Task 15 — goal + engine
Files: `deathpwn-core/src/goal/mod.rs`, `deathpwn-core/src/engine.rs`.
Consumes: EVERYTHING — Detector, Understand, Retrieve, Plan, Render, FeedbackLoop, SessionState,
Artifacts, PlanCache, GoalVerdict, Mode, Config, CancelToken.
Produces:
- `struct StepRecord { command: String, summary: String }`
- `struct GoalContext { goal_summary: String, mode: Mode, steps_taken: u32, history: Vec<StepRecord> }`
- `struct Engine<R: CommandRunner> { detector, understand, retrieve, plan, render, feedback,
   session, cache, artifacts, ai (for goal check), config }`
- `enum EngineEvent { Output(OutputLine), Rendered(Stage4Render), Error(String), Done }` (streamed via mpsc)
- `impl Engine { async fn handle_line(&mut self, line: &str, tx: mpsc::Sender<EngineEvent>, cancel: CancelToken) -> Result<()> }`
  Step 0 detect → DirectCommand: run via FeedbackLoop, render, done.
  RawInput: Stage1→2→3; SingleCommand: exec+render once. GoalCompletion: loop
  {plan next, exec, record, goal-check AI→GoalVerdict; achieved→render summary; cap at max_goal_steps}.
- `async fn goal_check(&self, ctx: &GoalContext) -> Result<GoalVerdict>`.
Tests: fake AI scripted achieved=false×2 then true → 3 rounds; stuck-false → cap halts.

## Task 16 — TUI (deathpwn-tui)
Files: `deathpwn-tui/src/main.rs`, `deathpwn-tui/src/app.rs`, `deathpwn-tui/src/ui.rs`.
Consumes: Engine, EngineEvent, OutputLine, Stage4Render, CancelToken, Config.
Produces:
- `struct App { input: String, output: Vec<Line>, status: StatusBar, scroll: u16 }`
- event loop: crossterm events on one tokio task, engine on another, mpsc between.
- keys: Enter submit; Ctrl+C cancel running cmd; Ctrl+X cancel+drain chain; PageUp/Down scroll;
  Ctrl+D/Esc on empty input quits.
- `fn render_section(f, area, section: &Stage4Render)` deterministic SectionKind→widget mapping,
  fixed severity palette.
Tests: one smoke test — construct App with a fake engine, pump scripted keys, assert no panic + state.
Deps: ratatui, crossterm, tokio.
