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
│  │  Detector   │  │  Pipeline       │  │  Execution   │ │
│  │  (Step 0)   │  │  (4 stages)     │  │  + Feedback  │ │
│  └─────────────┘  └────────────────┘  └──────────────┘ │
│                                                            │
│  ┌──────────────┐  ┌──────────────┐  ┌───────────────┐   │
│  │  Providers   │  │   Search     │  │  Session +    │   │
│  │  (Failover)  │  │  (DDG)       │  │  Cache        │   │
│  └──────────────┘  └──────────────┘  └───────────────┘   │
└───────────────────────────────────────────────────────────┘
```

## Entry Point: `handle_line()`

Everything starts in `Engine::handle_line(line, session, cancel)`:

```
User Input
    │
    ▼
[Detector]  ── raw command? ──→ exec_direct() → stream output → Done
    │
    │ (NL input)
    ▼
[Understand] → [Retrieve] → {SingleCommand | GoalCompletion}
                                │
                    ┌───────────┴───────────┐
                    ▼                       ▼
              Plan once            Goal Loop
              Execute               ├─ Plan (next_step, uncached)
              Render                ├─ FeedbackLoop (execute)
              Done                  ├─ Render
                                    ├─ GoalCheck
                                    └─ repeat or Done
```

## Crate Dependency Graph

```
deathpwn-tui ───→ deathpwn-core
  (binary)          (library)
      │                  │
      ▼                  ▼
  ratatui          reqwest, tokio,
  crossterm        serde, schemars,
  tokio            shell-words
```

## Core Modules

| Module | Path | Purpose |
|--------|------|---------|
| Engine | `engine.rs` | Main orchestrator — dispatches input, runs pipeline & goal loop |
| Detector | `detector/mod.rs` | Step 0: `command -v` check to classify command vs NL |
| Pipeline | `pipeline/` | 4-stage AI pipeline (Understand, Retrieve, Plan, Render) |
| Execution | `exec/` | Command running via `ShellRunner`, feedback loop, installer |
| Providers | `providers/` | AI provider trait, OpenAI client, dual-provider failover |
| Search | `search/` | Web search trait + DuckDuckGo HTML scraper |
| Session | `session/` | Accumulated state (targets, ports, services, findings) |
| Cache | `cache/mod.rs` | In-memory plan cache (normalized key lookup) |
| Goal | `goal/mod.rs` | GoalContext for goal-completion loop state |
| Schema | `schema/mod.rs` | All structured data types for AI responses |
| Cancel | `cancel.rs` | `CancelToken` — cooperative async cancellation |
| Config | `config.rs` | Environment-based configuration |

## Data Flow: Single Command

```
"scan port 80 on 10.0.0.5"
  │
  ▼ Stage 1 — Understand
  AI → { intent: "port_scan", params: { target: "10.0.0.5", ports: "80" }, mode: SingleCommand }
  │
  ▼ Stage 2 — Retrieve
  DDG search → AI → { theory: "...", candidates: [nmap, nc] }
  │
  ▼ Stage 3 — Plan
  AI → [ { tool: "nmap", argv: ["-p", "80", "10.0.0.5"] } ]
  │  (cached by normalized intent+params for future identical requests)
  ▼ FeedbackLoop
  which nmap ✓ → run nmap -p 80 10.0.0.5 → exit 0
  │
  ▼ Stage 4 — Render
  AI → Stage4Render { sections: [ Table of open ports ] }
  │
  ▼ UI streams Output events + Rendered event + Done
```

## Data Flow: Goal Completion

```
"enumerate the web server on 10.0.0.5"
  │
  ▼ Stage 1 → mode: GoalCompletion, goal_summary: "Enumerate web services on 10.0.0.5"
  ▼ Stage 2 → candidate commands for web enumeration
  │
  ▼ Goal Loop (max 12 steps):
  │
  ├─ Plan (next_step) → AI + history → [nmap -sV -p- 10.0.0.5]
  ├─ FeedbackLoop → run nmap → exit 0
  ├─ Render → table of open ports
  ├─ GoalCheck → AI → { achieved: false, hint: "run whatweb on port 80" }
  │
  ├─ Plan (next_step) → AI + history + hint → [whatweb 10.0.0.5:80]
  ├─ FeedbackLoop → run whatweb → exit 0
  ├─ Render → detected technologies
  ├─ GoalCheck → AI → { achieved: true }
  │
  └─ Done
```

## Execution Feedback Loop

Every command runs through `FeedbackLoop::run()`:

```
1. Availability: command -v <tool>
   └─ Not found → AI resolves install command → run installer → retry
2. Execute: ShellRunner spawns subprocess in own process group
3. Classify exit:
   ├─ exit 0 → ok, proceed to render
   ├─ exit ≠0 → AI classifies failure:
   │   ├─ NotFound → install + retry (capped)
   │   ├─ BenignEmpty → report, no retry
   │   ├─ FixableUsage → AI corrects argv → retry (max 2 corrections)
   │   ├─ Transient → retry once
   │   └─ Fatal → report cleanly
4. Cancel: CancelToken checked via tokio::select! — SIGTERM → 300ms → SIGKILL
```

## Provider Failover

Every AI call uses `FailoverClient::complete_validated()`:

```
1. Try Provider A
2. Validate response → parse against strict JSON schema
3. If API error OR schema mismatch → immediate failover to Provider B
4. Both fail → DeathpwnError::Provider
```

## Session State

`SessionState` accumulates across a session:

- `targets`: Vec of known target IPs/hostnames
- `hosts`: map of host → (ports, services)
- `findings`: Vec of discovered findings with severity
- `command_log`: full history of executed commands

This state is fed back into Stage 1 (Understand) so follow-up commands like "scan those ports" resolve without restating the target.

## Plan Cache

In-memory exact-match cache for Stage 3 plans. Key format:

```
normalize_intent(intent) + "|" + normalize_params(params)
```

- Different params (different target IP) never collide
- Only used in SingleCommand mode — goal loop bypasses cache (always uses `next_step()`)
- No TTL, lives for the session duration

## Artifacts

Every command's raw output is written to:

```
$XDG_DATA_HOME/deathpwn/<ISO8601-timestamp>/<step-index>.txt
```

Configurable via `DEATHPWN_ARTIFACTS_DIR`.
