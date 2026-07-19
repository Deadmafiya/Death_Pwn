# Pipeline

> ⚠️ **Design Reference** — The full 4-stage pipeline described below is the **planned architecture**. The current Engine (`engine.rs`) uses a simplified single-stage flow: `FailoverClient::complete_validated()` → `clean_command()` → `ShellRunner::run_streaming()`.  
> **Schema types** for all 4 stages are defined in `deathpwn-core/src/schema/mod.rs`. Stage runners are not yet built.

The 4-stage AI pipeline transforms natural language into executed commands with structured output.

## Implementation Status

| Stage | Schema Type | Stage Runner | Wired into Engine |
|-------|-------------|:---:|:---:|
| Stage 1 — Understand | `Stage1Understanding` ✅ | ❌ | ❌ |
| Stage 2 — Retrieve | `Stage2Knowledge` ✅ | ❌ | ❌ |
| Stage 3 — Plan | `Stage3Plan` ✅ | ❌ | ❌ |
| Stage 4 — Render | `Stage4Render` ✅ | ❌ | ❌ |
| FeedbackLoop | `ExecFailureVerdict` ✅ | ✅ (exec/feedback.rs) | ❌ |
| GoalCompletion | `GoalVerdict` ✅ | ❌ | ❌ |
| DDG Search | `SearchResult` ✅ | ✅ (search/ddg.rs) | ❌ |

## Stage 1 — Understand

**Schema:** `Stage1Understanding` in `deathpwn-core/src/schema/mod.rs`  
**Stage runner:** Not yet built (would live in `pipeline/understand.rs`)

Takes raw user input + session state and sends to AI. Returns a validated struct:

```
Raw input: "enumerate web server on 10.0.0.5"
Session state: { targets: [...], hosts: {...}, findings: [...] }
  │
  ▼
AI → Stage1Understanding {
  intent: "web_enumeration",
  params: { target: "10.0.0.5", ... },
  mode: SingleCommand | GoalCompletion,
  goal_summary: "Enumerate web services on 10.0.0.5"
}
```

The session state is critical — it enables follow-ups like "scan those ports" where the AI resolves "those" from accumulated state without restating the target.

## Stage 2 — Knowledge Retrieval

**Schema:** `Stage2Knowledge` in `deathpwn-core/src/schema/mod.rs`  
**Search:** DuckDuckGo HTML scraper exists in `deathpwn-core/src/search/ddg.rs` (not yet wired)  
**Stage runner:** Not yet built (would live in `pipeline/retrieve.rs`)

Converts understanding into executable knowledge via web search:

1. Build a DuckDuckGo search query from intent + params
   - Format: `"{intent} {target} kali OR blackarch pentest command"`
2. Fetch DDG results (HTML scrape via `parse_ddg_html()`)
3. Feed results (or "use your own knowledge" fallback if empty) to AI
4. AI produces:

```
Stage2Knowledge {
  theory: "Port scanning discovers open services...",
  candidates: [
    { tool: "nmap", argv: ["-sV", "-p-", "10.0.0.5"], reason: "..." },
    { tool: "whatweb", argv: ["http://10.0.0.5:80"], reason: "..." },
  ]
}
```

## Stage 3 — Planning

**Schema:** `Stage3Plan` in `deathpwn-core/src/schema/mod.rs`  
**Stage runner:** Not yet built (would live in `pipeline/plan.rs`)

Converts understanding + knowledge + session context into a concrete execution plan:

```
Stage3Plan {
  step: 0,
  intent: "web_enumeration",
  commands: [
    PlannedCommand { tool: "nmap", argv: [...], purpose: "...", depends_on_prev: false },
    PlannedCommand { tool: "whatweb", argv: [...], purpose: "...", depends_on_prev: true },
  ],
  theory: "..."
}
```

**Caching:** Plans are cached by normalized key (`normalize_intent + "|" + normalize_params`) for SingleCommand mode. Goal completion uses `Plan::next_step()` which is *uncached* — it receives execution history + goal-check hints to advance the loop.

**`next_step()`** — Used inside the goal loop:
- Takes full `GoalContext` (all previous steps + outcomes)
- Takes the `next_step_hint` from the previous GoalCheck
- Produces the next batch of commands to run
- Deliberately uncached — each call must produce different output as the loop progresses

## Stage 4 — Render

**Schema:** `Stage4Render` in `deathpwn-core/src/schema/mod.rs`  
**TUI integration:** `ui::stage4_to_lines()` in `deathpwn-tui/src/ui/mod.rs` converts `Stage4Render` into ratatui `Line`s (active)  
**Stage runner:** Not yet built (would live in `pipeline/render.rs`)

After command execution, AI formats raw stdout/stderr/exit code into structured display:

```
Stage4Render {
  sections: [
    RenderSection {
      title: "Open Ports",
      kind: Table,
      body: Table { headers: ["Port", "Service"], rows: [["22/tcp", "ssh"], ["80/tcp", "http"]] }
    },
    RenderSection {
      title: "Findings",
      kind: Findings,
      body: Findings([{ severity: "high", title: "...", detail: "..." }])
    },
  ]
}
```

**Never cached** — output always changes per execution.

### RenderBody Variants

| Variant | Purpose |
|---------|---------|
| `Table` | Tabular data (port scans, dir listings) |
| `KeyValue` | Key-value pairs (server info, headers) |
| `Text` | Free-text blocks |
| `Findings` | Security findings with severity (Critical, High, Medium, Low, Info) |

### Severity Color Mapping (in TUI)

| Severity | Color |
|----------|-------|
| Critical / High | Red (`#FF3333`) |
| Medium | Yellow |
| Low | Green (`#00FF66`) |
| Info / default | Cyan (`#00D7FF`) |

## Feedback Loop (built, not yet wired)

**Location:** `deathpwn-core/src/exec/feedback.rs`

When wired into Engine, `FeedbackLoop::run()` will handle every command execution:

1. **Availability check**: `command -v <tool>` — if missing, AI resolves BlackArch install command
2. **Execute**: ShellRunner with CancellationToken safety
3. **Classify failure**: On non-zero exit, AI classifies as `NotFound`, `BenignEmpty`, `FixableUsage`, `Transient`, or `Fatal`
4. **React**:
   - `NotFound` → auto-install then retry
   - `FixableUsage` → apply corrected argv then retry (capped at `max_corrections`)
   - `BenignEmpty` / `Fatal` → stop
   - `Transient` → retry unchanged

## Goal Completion Loop (schema exists, not yet wired)

**Schema:** `GoalVerdict` in `deathpwn-core/src/schema/mod.rs`

When wired, this loop iterates until the goal is achieved or max steps (12) reached:

```
Goal Loop:
  ├─ Plan (next_step) → AI + history → [commands]
  ├─ FeedbackLoop → execute commands
  ├─ Render → structured output
  ├─ GoalCheck → AI → GoalVerdict { achieved, reason, next_step_hint }
  └─ repeat or Done
```
