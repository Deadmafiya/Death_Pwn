# Architecture

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      deathPWN Binary                      │
│                                                           │
│  ┌──────────┐   mpsc    ┌──────────────┐   mpsc   ┌────┐ │
│  │ Key Input │ ────────→ │   App (TUI)  │ ←────── │ UI │ │
│  │  Thread   │ KeyEvent  │  (app.rs)    │ Engine  │    │ │
│  └──────────┘            └──────┬───────┘ Event    └────┘ │
│                                  │                         │
│                          tokio::spawn                      │
│                                  │                         │
│                          ┌───────▼────────┐               │
│                          │  Engine Task    │               │
│                          │  (engine.rs)    │               │
│                          └───────┬────────┘               │
│                                  │                         │
│         ┌────────────────────────┼──────────────────┐     │
│         │                        │                   │     │
│  ┌──────▼──────┐  ┌──────────────▼──┐  ┌───────────▼──┐ │
│  │  Detector   │  │  Providers     │  │  Execution   │ │
│  │  (Step 0)   │  │  (Failover)    │  │  (ShellRunner)│ │
│  └─────────────┘  └────────────────┘  └──────────────┘ │
│                                                            │
│  ┌──────────────┐  ┌──────────────┐  ┌───────────────┐   │
│  │   Search     │  │   Schema     │  │  Cancel       │   │
│  │  (DDG)       │  │  (Types)     │  │  Token        │   │
│  └──────────────┘  └──────────────┘  └───────────────┘   │
└───────────────────────────────────────────────────────────┘
```

## Entry Point: `handle_line()`

Everything starts in `Engine::handle_line(line, tx, cancel)`:

```
User Input
    │
    ▼
[Detector]  ── DirectCommand? ──→ ShellRunner::run_streaming() → Done
    │
    │ (NL input)
    ▼
FailoverClient::complete_validated() → clean_command()
    │
    ▼
ShellRunner::run_streaming() → Done
```

The current engine uses a **simplified single-stage flow**. Schema types for the full 4-stage pipeline (Understand → Retrieve → Plan → Render) exist in `schema/mod.rs`, but the stage runners are not yet wired into the Engine.

## Crate Dependency Graph

```
deathpwn-tui ───→ deathpwn-core
  (binary)          (library)
      │                  │
      ▼                  ▼
  ratatui          reqwest, tokio,
  crossterm        serde, shell-words,
  tokio            scraper, nix, tracing
```

## Core Modules

| Module | Path | Purpose |
|--------|------|---------|
| Engine | `engine.rs` | Main orchestrator — dispatches input, runs AI resolution, streams output |
| Detector | `detector/mod.rs` | Step 0: `command -v` probe to classify command vs NL |
| Schema | `schema/mod.rs` | All structured data types for AI pipeline stages (Stage1–4, FeedbackLoop, GoalVerdict) |
| Execution | `exec/runner.rs` | `ShellRunner` — persistent shell process, streaming output, cancellation |
| Execution | `exec/feedback.rs` | `FeedbackLoop` — availability check, auto-install, ai-driven argv correction (**built, not yet wired**) |
| Execution | `exec/installer.rs` | AI-resolved BlackArch install commands |
| Providers | `providers/openai.rs` | `OpenAiClient` — OpenAI-compatible HTTP client |
| Providers | `providers/failover.rs` | `FailoverClient` — dual-provider with schema validation fallback |
| Providers | `providers/ai.rs` | `AiProvider` trait + `ChatRequest` + `ProviderError` |
| Search | `search/ddg.rs` | DuckDuckGo HTML scrape search (**built, not yet wired**) |
| Search | `search/mod.rs` | `SearchProvider` trait + `SearchResult` |
| Cancel | `cancel.rs` | `CancelToken` — cooperative async cancellation |
| Config | `config.rs` | Environment-based configuration, preference loading |
| Clock | `clock.rs` | Wall-clock abstraction (injectable) |

## TUI Modules

| Module | Path | Purpose |
|--------|------|---------|
| App | `app.rs` | UI state, key bindings, event dispatch, text scraping, mouse handling, file bar state, popup editor |
| UI | `ui/mod.rs` | Layout orchestration (3:2 split + filebar + popup overlay), `Stage4Render` → ratatui `Line` conversion |
| Panes | `ui/panes.rs` | 4 widget renderers: console (with embedded prompt), telemetry, target matrix, file browser bar |
| Highlight | `ui/highlight.rs` | Smart output highlighter: regex pattern matcher (`URL`, `IPv6`, `IPv4`, `MAC`, `PATH`, `PORT`, `STATUS`), priority overlap filter, span builder |
| Filebrowser | `ui/filebrowser.rs` | Directory entry representations, Nerd Font icon mapper (`icon_for_entry`), click actions (`NavigateDir`, `OpenFile`, `ToggleTarget`, `CopyToClipboard`) |
| Popup | `ui/popup.rs` | Modal popup file editor state (`PopupState`), undo/redo stack (256 levels), scroll clamping, rendering, mouse hit testing |
| Theme | `ui/theme.rs` | Color palette (BLACKARCH_VOID: Obsidian, Toxic Green, Cyber Cyan, Explosive Red, Purple, Yellow) and style helpers |

## TUI Layout

```
┌──────────────────────────────────────┬──────────────────────────┐
│                                      │ TACTICAL TELEMETRY       │
│        LIVE OUTPUT CONSOLE           │ (7 lines fixed)          │
│            (60% width)              │ IP, DIR, ENGINE, STATUS   │
│                                      │ + animated spinner        │
│                                      ├──────────────────────────┤
│                                      │ DISCOVERED TARGET MATRIX │
│                                      │ (remaining space)        │
├──────────────────────────────────────┴──────────────────────────┤
│ FILE BROWSER BAR (height: 4, 2-row horizontal scrollable grid)   │
└──────────────────────────────────────────────────────────────────┘
```

## Data Flow: Current Implementation

```
"scan port 80 on 10.0.0.5"
  │
  ▼ Detector
  command -v scan → 127 (not found) → classify as RawInput
  │
  ▼ Engine (Phase::Thinking)
  FailoverClient::complete_validated() → try provider A → OK
  clean_command("```sh\nnmap -p 80 10.0.0.5\n```") → CommandSpec { tool: "nmap", argv: ["-p", "80", "10.0.0.5"] }
  │
  ▼ Engine (Phase::Executing)
  ShellRunner::run_streaming() → live OutputLine events
  │
  ▼ App::on_event()
  scrape_text() → extract IPs/ports/URLs/paths → Target Matrix
  ui::highlight::highlight_line(text, base_style) → styled spans → output buffer → Redraw
```

## Planned / Not Yet Wired

These components exist as code in the repository but are not yet integrated into the Engine:

| Component | Location | Status |
|-----------|----------|--------|
| FeedbackLoop | `deathpwn-core/src/exec/feedback.rs` | Fully built: availability check, auto-install, failure classification, argv correction. Not wired into Engine (Engine calls ShellRunner directly). |
| DuckDuckGo Search | `deathpwn-core/src/search/ddg.rs` | `DuckDuckGoSearch` client works. Not used by Engine. |
| Goal Completion Loop | Schema in `schema/mod.rs` | `GoalVerdict` type exists. Goal loop state machine not built. |
| 4-Stage Pipeline | Schema in `schema/mod.rs` | All `Stage1–4` and `RenderBody` types defined. Stage runners not built. |
| Plan Cache | — | CLI args (`--cache`/`--no-cache`) implemented. In-memory cache not wired. |

## Execution: ShellRunner

`ShellRunner` maintains a persistent background shell process across commands:

1. Shell process spawned with piped stdin/stdout/stderr
2. Each command written to stdin between sentinel delimiters
3. Stdout/stderr read concurrently until sentinels (`==DEATHPWN_STDOUT_DONE==`, `==DEATHPWN_STDERR_DONE==`)
4. Live streaming: each line forwarded via `mpsc::Sender<OutputLine>` before the command completes
5. Cancellation: `tokio::select!` checks `CancelToken`; sends SIGTERM to process group, escalates to SIGKILL after 300ms

## Execution: FeedbackLoop (planned)

When wired into Engine, every command will run through `FeedbackLoop::run()`:

```
1. Availability: command -v <tool>
   └─ Not found → AI resolves install command → run installer → retry
2. Execute: ShellRunner spawns subprocess in own process group
3. Classify exit:
   ├─ exit 0 → ok, proceed
   ├─ exit ≠0 → AI classifies failure:
   │   ├─ NotFound → install + retry (capped)
   │   ├─ BenignEmpty → report, no retry
   │   ├─ FixableUsage → AI corrects argv → retry (max 2 corrections)
   │   ├─ Transient → retry once
   │   └─ Fatal → report cleanly
4. Cancel: CancelToken checked via tokio::select!
```

## Provider Failover

Every AI call uses `FailoverClient::complete_validated()`:

```
1. Try Provider A
2. Validate response against the provided clean/parse function
3. If API error OR validation failure → immediate failover to Provider B
4. Both fail → errors aggregated and returned
```

## Configuration

See [configuration.md](./configuration.md) for the full environment variable reference. Key additions beyond the provider vars:

- `DEATHPWN_PREFERENCE_FILE` — path to `preference.json` for command overrides
- `DEATHPWN_DISABLE_CACHE` — disable plan cache (set by `--no-cache` CLI flag)
- `DEATHPWN_DISABLE_HISTORY` — disable history (set by `--history off` CLI flag)

## Artifacts

Every command's raw output is written to:

```
$XDG_DATA_HOME/deathpwn/<ISO8601-timestamp>/<step-index>.txt
```

Configurable via `DEATHPWN_ARTIFACTS_DIR`.
