# Pipeline

The 4-stage AI pipeline transforms natural language into executed commands with structured output.

## Stage 1 — Understand

**File:** `deathpwn-core/src/pipeline/understand.rs`

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

**File:** `deathpwn-core/src/pipeline/retrieve.rs`

Converts understanding into executable knowledge via web search:

1. Build a DuckDuckGo search query from intent + params
   - Format: `"{intent} {target} kali OR blackarch pentest command"`
2. Fetch DDG results (HTML scrape)
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

**File:** `deathpwn-core/src/pipeline/plan.rs`

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

**File:** `deathpwn-core/src/pipeline/render.rs`

After command execution, AI formats raw stdout/stderr/exit code into structured display:

```
Stage4Render {
  summary: "Found 3 open ports: 22, 80, 443",
  sections: [
    RenderBody::Table { title: "Open Ports", columns: [...], rows: [...] },
    RenderBody::KeyValue { title: "Server Info", entries: [...] },
    RenderBody::Findings { items: [{ severity: High, description: "..." }] },
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
| Critical | Red |
| High | Light Red |
| Medium | Yellow |
| Low | Green |
| Info | Cyan |
