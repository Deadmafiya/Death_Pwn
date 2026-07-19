# TUI Frontend

**Crate:** `deathpwn-tui`
**Binary:** `deathPWN`

Built with **ratatui 0.29** + **crossterm 0.28**. Runs on tokio async multi-thread runtime.

## CLI Arguments

```
deathPWN [OPTIONS] [RAW_QUERY]

Options:
  --no-cache, --disable-cache  Disable in-memory command caching
  --cache, --enable-cache      Enable in-memory command caching
  --clear-history              Delete all command output artifacts and exit
  --history on|off|clear       Enable, disable, or clear command history
  -h, --help                   Print help information
```

### CLI Mode (One-liner Execution)

When a `[RAW_QUERY]` is provided, deathPWN skips the TUI entirely and runs in
**CLI mode** — the query is resolved via the AI engine, then the resulting
command is executed inline in the current terminal with stdout/stderr streamed
directly. The process exits with the command's exit code.

```bash
# Natural language → resolve → execute → exit
deathPWN scan all open ports on 10.10.10.5
deathPWN list all docker containers
deathPWN find suid binaries on this machine

# Without arguments → launches the full interactive TUI
deathPWN
```

CLI mode workflow:
1. **Resolve** — The raw query is sent through the AI pipeline (`resolve_only`)
   to produce a concrete shell command.
2. **Display** — The resolved command is printed to the terminal before execution.
3. **Execute** — The command is run via `ShellRunner` with real-time stdout/stderr streaming.
4. **Exit** — The process exits with the command's exit code (non-zero on failure).

## Main Loop

The event loop multiplexes three sources via `tokio::select!`:

```
loop {
    terminal.draw(|f| ui::draw(f, &mut app));
    
    tokio::select! {
        crossterm_rx.recv() → handle_key / handle_mouse
        event_rx.recv()    → on_event (engine output)
        sleep(80ms)        → advance spinner animation frame
    }
}
```

| Signal | Source | Handler |
|--------|--------|---------|
| Key/Mouse events | Dedicated OS thread polling `crossterm::event::poll()` at 100ms | `App::handle_key()` / `App::handle_mouse()` |
| Engine events | `tokio::spawn` engine task via `mpsc` channel | `App::on_event()` |
| Spinner timer | `tokio::time::sleep(80ms)` | Re-draws frame to advance braille animation |

**Reload:** The entire engine setup runs inside a retry loop. When `Ctrl+R` is pressed, the current run is torn down, `.env` is re-parsed, providers and engine are rebuilt, and the inner event loop restarts fresh.

## Layout (3 Panes + Right Sidebar)

```
┌──────────────────────────────────────┬──────────────────────────┐
│                                      │  TACTICAL TELEMETRY       │
│        INTERACTIVE TERMINAL CONSOLE  │  (height: 7 lines)       │
│            (Ratio 3:5)              │  IP, DIR, ENGINE, STATUS  │
│                                      ├──────────────────────────┤
│                                      │  DISCOVERED TARGET        │
│                                      │  MATRIX                  │
│                                      │  (Min: remaining space)  │
├──────────────────────────────────────┴──────────────────────────┤
│ COMMAND INTERACTION ENTRY (height: 3)                            │
└──────────────────────────────────────────────────────────────────┘
```

- **Vertical split:** Main workspace (`Min(1)`) + Input bar (`Length(3)`)
- **Main workspace horizontal split:** Console 60% (`Ratio(3,5)`) + Right panel 40% (`Ratio(2,5)`)
- **Right panel vertical split:** Telemetry (`Length(7)`) + Target Matrix (`Min(1)`)

### Interactive Terminal Console (left, 60%)

- Interactive terminal layout displaying combined output history and active typing prompt.
- `Paragraph` widget containing scrollback history + prompt line `> <input>` at the bottom.
- Active cursor rendered dynamically on the prompt line relative to the current scroll offset.
- `Stdout` lines → Gray, `Stderr` → Red, `Banner` (command echo) → Cyan Bold.
- `Stage4Render` output converted to styled lines (see `ui::stage4_to_lines()`).
- Auto-scrolls (pins view to bottom) when new output arrives or a command is submitted.

### Tactical Telemetry (right top, 7 lines)

Four-line status readout:

| Line | Label | Source |
|------|-------|--------|
| `IP` | Machine's external IP | `ip route get 1.1.1.1` at startup |
| `DIR` | Shell current directory | `EngineEvent::Cwd` from engine |
| `ENGINE` | AI model name | `Config::provider_a.model` |
| `STATUS` | Animated spinner + phase or `◼ IDLE` | `EngineEvent::PhaseChange` |

**Spinner states:**
- Running → Braille cycle (`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` at 10 frames) + phase label in green
- Idle → `◼ IDLE` in dark gray

### Discovered Target Matrix (right bottom, remaining space)

Interactive tree of targets discovered via text scraping. Each target shows:

```
▶ 192.168.1.5           (collapsed — left-click to expand)
▼ 10.0.0.5              (expanded)
    PORTS: 22, 80, 443
    URLS: http://10.0.0.5:80/
    PAYLOADS: /etc/passwd, /usr/share/nmap/scripts/http-enum.nse
```

**Mouse interactions:**
- **Left-click** on target IP → toggles expand/collapse
- **Right-click** on any item (IP, port, URL, filepath) → copies to clipboard via OSC 52 escape sequence + fallback chain: `xclip` → `xsel --clipboard` → `wl-copy`

### Command Interaction Entry (bottom, 3 lines)

- Bordered block titled `COMMAND INTERACTION ENTRY`.
- Left empty/inactive as the interactive terminal prompt is hosted inside the console pane.

## Key Bindings

| Key | Action |
|-----|--------|
| `Enter` | Submit input line as a `Job` to the engine |
| `Ctrl+Tab` / `Alt+Tab` / `Shift+Tab` | Resolve raw input using the AI and replace the prompt in-place without executing it |
| `Ctrl+C` | Cancel current running command (flips CancelToken → SIGTERM to process group) |
| `Ctrl+X` | Cancel running command AND clear input, return to fresh prompt |
| `Ctrl+D` | Quit immediately (even with text in input) |
| `Ctrl+R` | Reload config (.env re-parsed, providers/engine rebuilt) |
| `Esc` | Quit (only when input is empty) |
| `←` / `→` | Move cursor in input |
| `Home` / `End` | Jump to start/end of input |
| `PageUp` / `PageDown` | Scroll output by 10 lines |
| `Backspace` | Delete char before cursor |
| `Delete` | Delete char at cursor |
| Printable chars | Insert at cursor position |

## Text Scraping

Every `EngineEvent::Output` line and the user's own submitted input is scanned for:

| Type | Pattern | Method |
|------|---------|--------|
| **IPs** | `\d+\.\d+\.\d+\.\d+` with boundary checks (skips interface lines like "Interface:", "listening on", "ip link") | `find_ips()` |
| **Ports** | `ip:port` notation, `/tcp`/`/udp` suffixes (nmap style), `-p` flag arguments (comma-separated + ranges) | `extract_ports()` |
| **URLs** | `http://`/`https://` prefixed tokens (filters out tool docs domains: github.com, nmap.org, etc.) | `find_urls()` |
| **Filepaths** | Tokens ending in common extensions or starting with `/`, `./`, `../` (excludes URLs and IPs) | `find_filepaths()` |

Discovered items are associated with a context IP:
1. If `active_scrape_ip` is set (from `EngineEvent::Progress` or a recently seen IP), items bind to that target
2. Otherwise falls back to the status bar target or creates a "General Target"

## Clipboard

`copy_to_clipboard()` uses a layered approach:

1. **OSC 52** escape sequence (`\x1b]52;c;<base64>\x07`) — terminal-native clipboard
2. **OSC 9** notification (`\x1b]9;Text copied to clipboard\x07`) — Ghostty/kitty notification
3. **Fallback chain**: `xclip -selection clipboard` → `xsel --clipboard --input` → `wl-copy`

Includes a hand-rolled base64 encoder (no external crate dependency).

## Engine Events

| Event | Payload | UI Action |
|-------|---------|-----------|
| `Output` | `OutputLine { stream, text }` | Scrape text for IPs/ports/URLs/filepaths; push styled lines to output buffer |
| `Rendered` | `Stage4Render` | Insert "AI ANALYSIS INGESTED" divider; convert via `stage4_to_lines()`; store in `current_render` |
| `Error` | `String` | Red bold "error: {msg}" in output |
| `Progress` | `{ target, step }` | Update `active_scrape_ip`, create `DiscoveredTarget`, update status bar |
| `PhaseChange` | `Phase` | Update `status.phase` |
| `Done` | — | Clear running flags, set phase to `Idle` |
| `Cwd` | `String` | Update `current_dir` in telemetry pane |

## Theme (BLACKARCH_VOID)

6-color palette defined in `deathpwn-tui/src/ui/theme.rs`:

| Constant | Hex | Use |
|----------|-----|-----|
| `PITCH_BLACK` | `#000000` | All pane backgrounds |
| `MATTE_OBSIDIAN` | `#262626` | Borders, idle status, table headers |
| `TOXIC_ACID_GREEN` | `#00FF66` | Spinner, running status, prompt, matrix arrows, low-severity findings |
| `CYBER_CYAN` | `#00D7FF` | Labels, section titles, analysis headers |
| `TERMINAL_SILVER` | `#D8D8D8` | Body text, telemetry values |
| `HIGH_EXPLOSIVE_RED` | `#FF3333` | Critical/high severity findings, errors, stderr |

## App State (`app.rs`)

```rust
App {
    input: String,                           // current input buffer
    cursor_pos: usize,                       // cursor position in input
    output: Vec<Line<'static>>,              // scrollback buffer
    status: StatusBar,                       // phase, target, steps, provider, spinner
    scroll: u16,                             // PageUp/PageDown scroll offset
    should_quit: bool,                       // exit flag
    should_reload: bool,                     // Ctrl+R reload flag
    running: bool,                           // is a job in flight?
    cancel: CancelToken,                     // current job's cancel token
    current_render: Option<Stage4Render>,    // last AI-rendered analysis
    targets: Vec<DiscoveredTarget>,          // scraped targets
    active_scrape_ip: Option<String>,        // current IP context
    clickable_items: Vec<MatrixClickItem>,   // populated each frame for mouse
    local_ip: String,                        // machine's IP
    current_dir: String,                     // shell cwd
    cmd_tx: mpsc::Sender<Job>,               // channel to engine
}
```
