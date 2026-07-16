# deathPWN v1 — Design Spec

> Natural-language-driven terminal for offensive security. Raw English in,
> real command output out. Terminal-first, no persona. Rust.

This spec turns the vision in `GOAL.md` into locked, buildable decisions. It is
the authority for the implementation plan. Where this spec and `GOAL.md`
disagree, this spec governs; where this spec is silent, `GOAL.md` governs.

---

## 1. Locked decisions (from brainstorm)

| # | Area | Decision |
|---|------|----------|
| 1 | Terminal model | **Full-screen TUI** via `ratatui` + `crossterm`, raw mode, redraw loop |
| 2 | Pipeline shape | **4 discrete sequential stages**, each with its own schema + cache seam |
| 3 | Search backend | **`SearchProvider` trait** + DuckDuckGo **HTML-scrape** impl; degrades when thin |
| 4 | Cache | **Exact normalized key** (intent + normalized params); no embeddings |
| 5 | Test boundaries | **Traits + fakes** for every non-deterministic edge; real impls behind `#[ignore]` integration tests |
| 6 | Step 0 + exec | Resolve *and* run through the user's **`$SHELL -c`** (aliases/builtins/functions count) |
| 7 | Process control | **`tokio`** runtime; children in **own process group**; signal-to-group on Ctrl+C / Ctrl+X |
| 8 | Layout | **Cargo workspace**: `deathpwn-core` (lib, all logic + traits) + `deathpwn-tui` (bin, ratatui + tokio wiring) |

These are non-negotiable for v1. The plan builds exactly this.

---

## 2. Workspace & module topology

```
deathpwn/                      (Cargo workspace root)
├── Cargo.toml                 (workspace members)
├── deathpwn-core/             (library crate — no ratatui, no #[tokio::main])
│   └── src/
│       ├── lib.rs             re-exports; Engine facade
│       ├── error.rs           DeathpwnError (thiserror), Result alias
│       ├── config.rs          Config loaded from env (providers, shell, caps)
│       ├── schema/            typed stage structs (serde), parse-or-fail
│       ├── detector/          Step 0: $SHELL-based command-vs-raw resolution
│       ├── providers/         AiProvider trait, OpenAI-compat client, FailoverClient
│       ├── search/            SearchProvider trait, DuckDuckGo scrape impl
│       ├── pipeline/          Stage 1 understand · 2 retrieve · 3 plan · 4 render
│       ├── exec/              CommandRunner trait, process-group runner, feedback loop, installer
│       ├── goal/              GoalContext, goal-achieved check, step cap
│       ├── session/           SessionState (targets/hosts/ports/services/findings), artifacts log
│       ├── cache/             PlanCache (exact normalized key)
│       └── engine.rs          orchestrates detector → pipeline → exec → session
└── deathpwn-tui/              (binary crate — ratatui + tokio runtime)
    └── src/
        ├── main.rs            #[tokio::main]; builds Config + Engine, runs App
        ├── app.rs             App state, event loop, key handling (Ctrl+C/Ctrl+X)
        └── ui.rs              ratatui widgets: input, output pane, status bar
```

`deathpwn-core` is `#![forbid(unsafe_code)]`, has no terminal or async-main
dependency, and is fully unit-testable with fakes. `deathpwn-tui` owns the
tokio runtime, the terminal, and all rendering.

**Async boundary:** trait methods that do I/O (AI calls, search, command exec)
are `async` and return `Pin<Box<dyn Future>>` via `#[async_trait]`. Core logic
that is pure stays sync.

---

## 3. Configuration (`config.rs`)

Loaded once at startup from environment. No hardcoded paths. Missing required
vars → hard error at boot with a clear message naming the missing var.

| Env var | Meaning | Required | Default |
|---------|---------|----------|---------|
| `DEATHPWN_PROVIDER_A_URL` | Primary base URL (OpenAI-compat `/chat/completions`) | yes | — |
| `DEATHPWN_PROVIDER_A_KEY` | Primary API key | yes | — |
| `DEATHPWN_PROVIDER_A_MODEL` | Primary model name | yes | — |
| `DEATHPWN_PROVIDER_B_URL` | Fallback base URL | yes | — |
| `DEATHPWN_PROVIDER_B_KEY` | Fallback API key | yes | — |
| `DEATHPWN_PROVIDER_B_MODEL` | Fallback model name | yes | — |
| `DEATHPWN_SHELL` | Shell for Step 0 + exec | no | `$SHELL`, else `/bin/sh` |
| `DEATHPWN_MAX_GOAL_STEPS` | Goal-loop safety cap | no | `12` |
| `DEATHPWN_MAX_CORRECTIONS` | Self-corrections per command | no | `2` |
| `DEATHPWN_ARTIFACTS_DIR` | Session artifacts root | no | `${XDG_DATA_HOME:-~/.local/share}/deathpwn` |
| `DEATHPWN_HTTP_TIMEOUT_SECS` | Per-call HTTP timeout | no | `30` |

`Config` is a plain struct; a `Config::from_env()` constructor does the reading
and validation. Tests build `Config` directly.

---

## 4. Schemas (`schema/`)

Every AI stage has a strict typed response struct with `#[derive(Deserialize)]`.
"Validated" = the provider's JSON `content` field parses into the struct via
`serde_json::from_str`; any failure is a validation error that triggers
failover (§7). The AI is instructed to emit *only* JSON matching the schema.

- **`Stage1Understanding`**: `intent: String`, `params: IntentParams`
  (`target: Option<String>`, `ports: Option<String>`, `url: Option<String>`,
  `extra: BTreeMap<String,String>`), `mode: Mode` (`SingleCommand` |
  `GoalCompletion`), `goal_summary: String`.
- **`Stage2Knowledge`**: `theory: String`, `candidates: Vec<CandidateCommand>`
  where `CandidateCommand { tool: String, argv: Vec<String>, purpose: String }`.
- **`Stage3Plan`**: `commands: Vec<PlannedCommand>` where
  `PlannedCommand { tool, argv: Vec<String>, purpose: String,
  depends_on_prev: bool }`.
- **`Stage4Render`**: `sections: Vec<RenderSection>` where
  `RenderSection { title: String, kind: SectionKind, body: RenderBody }`;
  `SectionKind` ∈ `{ Table, KeyValue, Text, Findings }`; `RenderBody` is an
  enum carrying the shape each kind needs (rows+headers for `Table`, etc.).
- **Failure classification** (§6): `FailureClass` ∈ `{ NotFound, BenignEmpty,
  FixableUsage, Transient, Fatal }`, produced by an exec-AI call that returns
  `ExecFailureVerdict { class: FailureClass, corrected_argv: Option<Vec<String>> }`.
- **Goal check** (§8): `GoalVerdict { achieved: bool, reason: String,
  next_step_hint: Option<String> }`.

All enums use `#[serde(rename_all = "snake_case")]`. Each schema module has
round-trip and rejection unit tests.

---

## 5. Step 0 detector (`detector/`)

**Purpose:** decide *command* vs *raw input* the way a shell would, without a
wordlist.

**Interface:** `Detector::classify(&self, line: &str) -> InputKind` where
`InputKind ∈ { DirectCommand, RawInput }`.

**Mechanism:** extract the leading token (respecting quotes via `shell_words`).
Resolve it against the configured shell using
`$SHELL -c 'command -v -- <token>'`:
- exit 0 → token resolves (executable in `$PATH`, builtin, function, or alias) →
  `DirectCommand`.
- non-zero → `RawInput`.

Edge cases handled explicitly:
- empty / whitespace-only line → `RawInput` (nothing to run).
- leading token is an absolute/relative path that exists and is executable →
  `DirectCommand` (covered by `command -v`).
- a line that *parses* as a shell construct but whose command is unknown (e.g.
  `foobar | baz`) → the leading token drives the decision; `foobar` unknown →
  `RawInput`.

`command -v` is the resolver so aliases and shell functions defined in the
user's shell init are honored. The detector takes a `CommandRunner` (the same
trait used for execution, §6) so it is testable with a fake that maps known
tokens → exit 0.

---

## 6. Execution (`exec/`)

**`CommandRunner` trait** — the single OS-process boundary:

```rust
#[async_trait]
trait CommandRunner: Send + Sync {
    /// Run `argv` (already split) via the shell, in its own process group.
    async fn run(&self, spec: &CommandSpec, cancel: CancelToken) -> RunOutcome;
    /// Convenience: run a shell string (used by detector's `command -v`).
    async fn run_shell(&self, script: &str, cancel: CancelToken) -> RunOutcome;
}
```

`RunOutcome { exit: Option<i32>, stdout: String, stderr: String, cancelled: bool }`.
`cancelled: true` means a cancel signal fired (Ctrl+C). `exit: None` +
`cancelled` distinguishes user-abort from a real exit code.

**Real impl (`ShellRunner`):** spawns `$SHELL -c <script>` via
`tokio::process::Command` with `process_group(0)` (new group). Cancellation
sends `SIGTERM` then `SIGKILL` to the *group* so child trees die. stdout/stderr
are captured (streaming to the UI is handled at the TUI layer via a channel;
the trait returns the accumulated result plus emits lines on a provided
`mpsc::Sender<OutputLine>`).

**Feedback loop (`FeedbackLoop`)** wraps `CommandRunner` and implements §4 of
GOAL.md:
1. **availability check** — `command -v <tool>`; on miss →
2. **auto-install** — one AI call → BlackArch install command
   (`pacman`/AUR/`go install`) → run it → retry original.
3. **run** the command.
4. **non-zero handling** — feed exit + stderr to the exec-AI, get a
   `ExecFailureVerdict`:
   - `NotFound` → route to install loop (not counted as a correction).
   - `BenignEmpty` → success-with-empty; report, no retry.
   - `FixableUsage` → apply `corrected_argv`, retry (counts toward cap).
   - `Transient` → retry once (counts toward cap).
   - `Fatal` → report, stop.
   Cap = `DEATHPWN_MAX_CORRECTIONS` (default 2). Every attempt logged.

`FeedbackLoop` takes the `AiProvider` (for classify/install) and `CommandRunner`
by trait, so it is fully unit-testable with fakes: a fake runner scripted to
return a bad-flag error then success, a fake AI returning a `FixableUsage`
verdict, asserts the retry happens and the cap holds.

---

## 7. Providers & failover (`providers/`)

**`AiProvider` trait:**

```rust
#[async_trait]
trait AiProvider: Send + Sync {
    async fn complete(&self, req: &ChatRequest) -> Result<String, ProviderError>;
    fn label(&self) -> &str;
}
```

`ChatRequest { system: String, user: String, temperature: f32 }`. Returns the
model's raw text `content`.

**`OpenAiClient`** — `reqwest` POST to `{base}/chat/completions`, bearer key,
parses the standard `choices[0].message.content`. Maps network/timeout/5xx/
429 → `ProviderError` variants.

**`FailoverClient`** — holds Provider A + Provider B (both `AiProvider`) and a
`validate: Fn(&str) -> Result<T, _>` closure supplied by the caller (the stage's
schema parser). Algorithm (GOAL.md §8):
1. call A → validate. Success → return.
2. on A error **or** validation failure → call B (same request) → validate.
3. B success → return; B failure → return an aggregated error.
4. log each attempt (provider label, latency, outcome). Latency uses an injected
   `Clock` trait (fake in tests — no real `Instant`).
5. **Optional circuit breaker** (stretch, behind a flag): after N consecutive
   A-failures, skip straight to B for a cooldown. Off by default in v1.

Unit tests use two fake providers scripted for {A ok}, {A err → B ok},
{A bad-json → B ok}, {both fail}.

---

## 8. Pipeline (`pipeline/`)

Each stage is a struct holding a `FailoverClient` (or the shared AI wrapper) and
exposing one async method. Stages are pure orchestration over the AI + schema.

- **Stage 1 `Understand`**: input `raw: &str` + `&SessionState` → prompt →
  `Stage1Understanding`. Builds the initial `GoalContext`.
- **Stage 2 `Retrieve`**: input the understanding → builds a search query →
  `SearchProvider::search` → feeds results + intent to AI → `Stage2Knowledge`.
  If search returns thin/empty results, the prompt says so and the model relies
  on its own knowledge (graceful degrade, §1 decision 3).
- **Stage 3 `Plan`**: input understanding + knowledge + session → `Stage3Plan`
  (one command for `SingleCommand`, ordered chain for `GoalCompletion`).
- **Stage 4 `Render`**: input a command's `RunOutcome` (+ intent) → `Stage4Render`.
  The TUI turns `Stage4Render` into ratatui widgets deterministically.

**Cache seam:** Stage 1→3 results are cached by the plan cache (§10). Stage 4 is
not cached (output is data-dependent).

---

## 9. Goal loop (`goal/` + `engine.rs`)

`GoalContext { goal_summary: String, mode: Mode, steps_taken: u32,
history: Vec<StepRecord> }`, threaded through every stage.

Engine loop for `GoalCompletion`:
1. Plan next command(s) (Stage 3) using current context.
2. Execute via `FeedbackLoop`; record outcome into session + context.
3. **Goal-achieved check** — AI call → `GoalVerdict`. `achieved` → stop, render
   summary. else → loop.
4. **Safety cap** — stop unconditionally when `steps_taken >=
   DEATHPWN_MAX_GOAL_STEPS`, regardless of the verdict.

`SingleCommand` mode runs steps 1–2 once and renders; no goal loop.

Testable with fakes: a fake AI scripted to return `achieved: false` twice then
`true` asserts the loop runs 3 rounds; a fake stuck on `false` asserts the cap
halts it.

---

## 10. Session, cache, artifacts (`session/`, `cache/`)

**`SessionState`**: `targets: Vec<Target>`, `hosts`, `ports_by_host`,
`services`, `findings: Vec<Finding>`, plus a running command log. Mutated after
each execution; read by Stages 1 & 3 so follow-ups ("scan those ports") resolve
without re-stating the target. Pure struct, unit-tested.

**`PlanCache`**: key = `normalize(intent) + "|" + normalize(params)`.
`normalize` lowercases, trims, and sorts param keys so equivalent phrasings
collide but **different parameters do not** (the `.1.1` vs `.1.2` rule from
GOAL.md §7 is a required test). In-memory `HashMap` for v1. Stores the
`Stage3Plan`. Hit requires intent **and** params match.

**Artifacts**: per-session directory under `DEATHPWN_ARTIFACTS_DIR/<timestamp>/`;
each command's raw stdout/stderr written to a numbered file. Minimal — enough to
review/export later. The timestamp comes from an injected `Clock` so tests don't
touch the real clock or filesystem (a fake writes to a temp dir).

---

## 11. TUI (`deathpwn-tui/`)

- **Panes:** top output pane (scrollable command output + rendered sections),
  a status/session bar (current target, step count, provider in use), a bottom
  input line.
- **Event loop:** `crossterm` events on a tokio task; engine work on another;
  they communicate over `mpsc`. Redraw on each event or output line.
- **Keys:** `Enter` submit; `Ctrl+C` → cancel the running command (send cancel
  token; engine notifies the acting AI the command was user-stopped, per
  GOAL.md §6); `Ctrl+X` → cancel command **and** drain any pending chain, return
  to a fresh prompt; `PageUp/PageDown` scroll; `Ctrl+D`/`Esc` on empty input
  quits.
- Rendering of `Stage4Render` sections → ratatui `Table`/`Paragraph`/styled
  spans, one deterministic mapping per `SectionKind`. Severity/finding coloring
  is a fixed palette.
- The TUI is thin: no business logic, only event↔engine plumbing and drawing.
  Not unit-tested (integration-smoke only); all logic lives in core.

---

## 12. Error handling

`DeathpwnError` (thiserror) with variants: `Config`, `Provider`, `Search`,
`Exec`, `Schema`, `Cache`, `Io`, `Cancelled`. Core returns `Result<_,
DeathpwnError>`. The engine converts unrecoverable errors into a rendered error
section rather than crashing the TUI. Failover and the feedback loop absorb the
*expected* failures; `DeathpwnError` is for the rest.

---

## 13. Testing strategy

- **Unit tests, deterministic, no network/FS/clock:** every core module, using
  fakes for `AiProvider`, `SearchProvider`, `CommandRunner`, `Clock`. This is
  the bulk of the suite and the TDD driver for every task.
- **Fakes live in `deathpwn-core` behind `#[cfg(test)]`** (or a `test-support`
  module) so every task can build the fakes it needs.
- **Integration tests (`#[ignore]`):** real `OpenAiClient` against a live
  endpoint, real DDG scrape, real `ShellRunner` running `echo`. Run manually,
  never in the default `cargo test`.
- **TUI:** a single smoke test that constructs the app with a fake engine and
  pumps a scripted key sequence; no assertions on pixels.

Every task in the plan is TDD: failing test → implement → green → refactor.

---

## 14. Non-goals (v1) — inherited from GOAL.md §10

No persona/chit-chat. No scope/authorization gate. No exploitation/wireless
tools (recon + web + general shell only). BlackArch-only. No cost/token
budgeting. No deterministic per-tool parsers (Stage 4 stays LLM-assisted). No
embeddings in the cache. Circuit breaker is off by default.

---

## 15. Build order (informs the plan)

1. Workspace skeleton + `error` + `config` + fakes scaffolding.
2. `schema/` (all stage structs + tests).
3. `providers/` (`AiProvider`, `OpenAiClient`, `FailoverClient` + `Clock`).
4. `search/` (`SearchProvider` + DDG scrape).
5. `detector/` (Step 0).
6. `exec/` (`CommandRunner` + `ShellRunner` + `FeedbackLoop` + installer).
7. `session/` + `cache/`.
8. `pipeline/` (Stages 1–4).
9. `goal/` + `engine.rs` (wire everything; goal loop).
10. `deathpwn-tui/` (ratatui app, keys, rendering).

Each numbered item is one or more plan tasks, ordered so every task builds on
green predecessors and nothing is stubbed longer than necessary.
