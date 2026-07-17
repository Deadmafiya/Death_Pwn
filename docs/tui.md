# TUI Frontend

**Crate:** `deathpwn-tui`
**Binary:** `deathPWN`

Built with **ratatui** + **crossterm**. Runs on tokio async runtime.

## Main Loop

```
tokio::select! {
    key = key_rx.recv()     → App::handle_key()   → may send Job to engine
    event = event_rx.recv() → App::on_event()     → updates UI state
}
```

Two background tasks:

| Task | Runtime | Purpose |
|------|---------|---------|
| Key input thread | `std::thread::spawn` | Polls crossterm for key events, sends `KeyEvent` via `mpsc` |
| Engine task | `tokio::spawn` | Receives `Job { line, cancel_token }`, calls `Engine::handle_line()`, streams `EngineEvent`s back |

## Layout (3 Panes)

```
┌──────────────────────────────────────────┐
│  Console Output (scrollable)              │
│  [exec] nmap -p 80 10.0.0.5              │
│  PORT   STATE SERVICE                     │
│  80/tcp open  http                        │
│  ...                                      │
│                                           │
│  ── Analysis ─────────────────────────── │
│  Open Ports                     Severity  │
│  80/tcp (http)                  Info      │
│                                           │
├──────────────────────────────────────────┤
│ Status: step 2/12 | Provider: A | ...    │
├──────────────────────────────────────────┤
│ > enumerate the web server on 10.0.0.5   │
│                                           │
│                                           │
└──────────────────────────────────────────┘
```

When a `Stage4Render` is present, the output pane splits 60/40:
- **Top (60%):** Raw command output
- **Bottom (40%):** Structured "Analysis" pane — rendered from `stage4_to_lines()`

## Key Bindings

| Key | Action |
|-----|--------|
| `Enter` | Submit input as a `Job` to the engine |
| `Ctrl+C` | Cancel current running command (flips CancelToken, engine reports cancelled outcome) |
| `Ctrl+X` | Cancel command + drain remaining chain, get fresh prompt |
| `Ctrl+D` | Quit (on empty input) |
| `Esc` | Quit (on empty input) |
| `PageUp` / `PageDown` | Scroll output buffer |

## Engine Events

The UI listens for these events from the engine task:

| Event | Payload | UI Action |
|-------|---------|-----------|
| `Output` | `String` | Append to scrollback buffer |
| `Rendered` | `Stage4Render` | Store for analysis pane rendering |
| `Progress` | `step: usize, total: usize` | Update status bar |
| `Done` | - | Clear cancel token, return to idle |
| `Cancelled` | - | Report cancelled in output |

## App State (`app.rs`)

```
App {
  input: String,                     // current input buffer
  output_lines: Vec<String>,         // scrollback buffer
  scroll_offset: usize,              // for PageUp/PageDown
  status_text: String,               // status bar line
  current_render: Option<Stage4Render>, // for analysis pane
  cancel_token: Option<CancelToken>, // active cancellation handle
  step: usize,                       // current step in plan/goal loop
  total_steps: usize,                // total steps
  provider_label: String,            // "A" or "B"
}
```

`handle_key()` is synchronous and side-effect-light — unit-testable with scripted key sequences.
