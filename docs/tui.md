# TUI Frontend

**Crate:** `deathpwn-tui`
**Binary:** `deathPWN`

Built with **ratatui** + **crossterm**. Runs on tokio async runtime.

## Main Loop

```
tokio::select! {
    key = key_rx.recv()       → App::handle_key()   → may send Job to engine
    event = event_rx.recv()   → App::on_event()     → updates UI state
    _ = sleep(80ms)           → redraws spinner animation
}
```

Two background tasks:

| Task | Runtime | Purpose |
|------|---------|---------|
| Key input thread | `std::thread::spawn` | Polls crossterm for key events, sends `KeyEvent` via `mpsc` |
| Engine task | `tokio::spawn` | Receives `Job { line, cancel_token }`, calls `Engine::handle_line()`, streams `EngineEvent`s back |

## Layout (4 Panes)

```
┌──activity──────┬──output──────────────────────────────────────────────┐
│── THINKING ──  │$ nmap -sV 10.0.0.5                                    │
│  understanding │PORT   STATE SERVICE                                    │
│── THINKING ──  │22/tcp open  ssh                                       │
│  retrieving    │80/tcp open  http                                       │
│── THINKING ──  │                                                        │
│  planning      │── analysis ────────────────────────────────────────   │
│── RUNNING ──   │Open Ports                                    Severity │
│  scan services │22/tcp (ssh)                                  Info     │
│── ANALYZING ── │80/tcp (http)                                 Info     │
├────────────────┴───────────────────────────────────────────────────────┤
│ ⠋ executing command...   target: 10.0.0.5  steps: 3  provider: model-a │
├────────────────────────────────────────────────────────────────────────┤
│ > enumerate the web server on 10.0.0.5                                  │
└────────────────────────────────────────────────────────────────────────┘
```

- **Left 20%** — "activity" log pane showing phase banners (THINKING, RUNNING, ANALYZING, EVALUATING)
- **Right 80%** — Terminal output with optional 60/40 analysis split when `Stage4Render` is present
- **Status bar** — animated spinner with phase, target, steps, provider
- **Input line** — bordered 3-line input box

## Status Bar

The status bar shows an animated braille spinner (`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`) during active pipeline execution. When idle, it shows a static `◼ ready`. Phase colors:

| Phase | Color |
|-------|-------|
| `Classifying` / `Understanding` / `Retrieving` / `Planning` | Blue |
| `Executing` | Yellow |
| `Rendering` / `GoalChecking` | Magenta |
| `Installing` | Cyan |
| `Correcting` | Light Red |
| `Idle` | Dark Gray |

## Key Bindings

| Key | Action |
|-----|--------|
| `Enter` | Submit input as a `Job` to the engine |
| `Ctrl+C` | Cancel current running command (flips CancelToken) |
| `Ctrl+X` | Cancel command + drain remaining chain, fresh prompt |
| `Ctrl+D` | Quit immediately (always, even with text in input) |
| `Esc` | Quit (on empty input) |
| `PageUp` / `PageDown` | Scroll output buffer |

## Engine Events

| Event | Payload | UI Action |
|-------|---------|-----------|
| `Output` | `OutputLine { stream, text }` | Route to log pane (Banner) or output pane (Stdout/Stderr) |
| `Rendered` | `Stage4Render` | Display structured analysis in output pane |
| `Error` | `String` | Red bold error in output pane |
| `Progress` | `target, step` | Update status bar target and step count |
| `PhaseChange` | `Phase` | Update status bar phase label and color |
| `Done` | - | Reset to idle state |

## App State (`app.rs`)

```
App {
  input: String,                       // current input buffer
  output: Vec<Line>,                   // terminal output (right pane)
  log_lines: Vec<Line>,                // activity log (left pane)
  status: StatusBar,                   // phase, target, steps, spinner
  scroll: u16,                         // PageUp/PageDown offset
  should_quit: bool,
  running: bool,
  cancel: CancelToken,
  current_render: Option<Stage4Render>, // current analysis pane content
}
```
