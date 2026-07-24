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

## Layout (3 Top Panes + Bottom File Browser Bar)

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
│ FILE BROWSER BAR (height: 4, 2-row horizontal scrollable grid)   │
└──────────────────────────────────────────────────────────────────┘
```

- **Vertical split:** Main workspace (`Min(1)`) + File Browser bar (`Length(4)`)
- **Main workspace horizontal split:** Console 60% (`Ratio(3,5)`) + Right panel 40% (`Ratio(2,5)`)
- **Right panel vertical split:** Telemetry (`Length(7)`) + Target Matrix (`Min(1)`)

### Interactive Terminal Console (left, 60%)

- Interactive terminal layout displaying combined output history and active typing prompt.
- `Paragraph` widget containing scrollback history + prompt line `> <input>` at the bottom.
- Active cursor rendered dynamically on the prompt line relative to the current scroll offset.
- `Stdout` lines → Gray base (smart highlighted), `Stderr` → Red base (smart highlighted), `Banner` (command echo) → Cyan Bold (skips smart highlight).
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

### File Browser Bar (bottom, 4 lines)

- 2-row horizontal scrollable grid showing all files and directories in the shell's current working directory.
- Features Nerd Font file/directory icons (`icon_for_entry`) mapped by extension (`.rs`, `.py`, `.sh`, `.json`, `.md`, `.nse`, etc.).
- **Left-click on directory**: Changes current working directory (`NavigateDir`).
- **Left-click on file**: Opens the interactive **Popup File Editor** (`OpenFile`).

### Popup File Editor (`ui::popup`)

Overlay modal for viewing and editing files directly inside the TUI:
- **Title Bar**: Shows file path and `[modified]` indicator when edits are unsaved.
- **Editing Capabilities**: Standard character insertion, backspace, delete, newline, and arrow key cursor movement.
- **Undo / Redo System**: Snapshot-based undo/redo stack (`popup.undo_stack` / `popup.redo_stack`) supporting up to 256 state history steps.
- **Controls**: `Ctrl+S` or `[Save]` button writes to file; `Ctrl+Z` undoes; `Ctrl+Y` redoes; `Esc` or `[Exit]` button closes editor.
- **Mouse support**: Click on buttons (`[Save]`, `[Exit]`, `[Undo]`, `[Redo]`) or click any line/column in the text area to position the cursor.

## Smart Output Text Highlighting (`ui::highlight`)

Every incoming `EngineEvent::Output` line passes through `ui::highlight::highlight_line(text, base_style)` to apply in-line colorization for key pentesting entities before pushing to the output buffer.

### Pattern Priority & Styling

| Priority | Pattern | Description | Style |
|:---:|---------|-------------|-------|
| **1** | `URL_RE` | `http://`, `https://`, `ftp://` URLs | Cyber Cyan Bold Underlined (`#00D7FF`) |
| **2** | `IPV6_RE` | Full or compressed IPv6 addresses & CIDRs | Cyber Cyan Bold (`#00D7FF`) |
| **3** | `IPV4_RE` | Dotted quad IPv4 addresses & CIDRs | Cyber Cyan Bold (`#00D7FF`) |
| **4** | `MAC_RE` | Colon, dash, or dot-separated MAC addresses | Purple Highlight Bold (`#AF5FFF`) |
| **5** | `PATH_RE` | Absolute, relative, or home file paths (`/`, `./`, `../`, `~/`) | Toxic Acid Green Bold (`#00FF66`) |
| **6** | `PORT_RE` | Contextual ports (`22/tcp`, `:8080`, `port 443`) | Toxic Acid Green Bold (`#00FF66`) |
| **7** | `STATUS_GOOD_RE` | Good keywords (`open`, `up`, `success`, `alive`, `running`, `accepted`, `filtered`) | Toxic Acid Green Bold (`#00FF66`) |
| **7** | `STATUS_BAD_RE` | Bad keywords (`closed`, `down`, `failed`, `error`, `dead`, `stopped`, `refused`, `denied`, `timed out`, `unreachable`) | High Explosive Red Bold (`#FF3333`) |
| **7** | `STATUS_WARN_RE` | Warning keywords (`warning`, `warn`, `caution`, `deprecated`, `notice`) | Warning Yellow Bold (`#FFD700`) |

### Overlap Resolution

Matches are collected and sorted by starting offset, priority rank (lower numbers win), and match length. When patterns overlap (e.g. an IPv4 or port embedded inside a URL), the higher-priority match consumes the entire range, discarding overlapping sub-matches to guarantee clean, untorn spans. Unmatched line segments inherit the stream's default base style (`base_style`).

## Key Bindings

| Key | Action |
|-----|--------|
| `Enter` | Submit input line as a `Job` to the engine |
| `Ctrl+Tab` / `Alt+Tab` / `Shift+Tab` | Resolve raw input using the AI and replace the prompt in-place without executing it |
| `Ctrl+C` | Cancel current running command (flips CancelToken → SIGTERM to process group) |
| `Ctrl+X` | Cancel running command AND clear input, return to fresh prompt |
| `Ctrl+D` | Quit immediately (even with text in input) |
| `Ctrl+R` | Reload config (.env re-parsed, providers/engine rebuilt) |
| `Ctrl+S` | Save current file (when Popup Editor is open) |
| `Ctrl+Z` | Undo edit (when Popup Editor is open) |
| `Ctrl+Y` | Redo edit (when Popup Editor is open) |
| `Esc` | Close Popup Editor / Quit TUI (when input is empty) |
| `←` / `→` | Move cursor in input / Popup Editor |
| `↑` / `↓` | Move cursor up/down (in Popup Editor) |
| `Home` / `End` | Jump to start/end of input |
| `PageUp` / `PageDown` | Scroll output console by 10 lines |
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
| `Output` | `OutputLine { stream, text }` | Scrape text for IPs/ports/URLs/filepaths; run through `highlight_line()`; push styled lines to output buffer |
| `Rendered` | `Stage4Render` | Insert "AI ANALYSIS INGESTED" divider; convert via `stage4_to_lines()`; store in `current_render` |
| `Error` | `String` | Red bold "error: {msg}" in output |
| `Progress` | `{ target, step }` | Update `active_scrape_ip`, create `DiscoveredTarget`, update status bar |
| `PhaseChange` | `Phase` | Update `status.phase` |
| `Done` | — | Clear running flags, set phase to `Idle` |
| `Cwd` | `String` | Update `current_dir` in telemetry pane and refresh file bar |

## Theme (BLACKARCH_VOID)

Palette defined in `deathpwn-tui/src/ui/theme.rs`:

| Constant | Hex | Use |
|----------|-----|-----|
| `PITCH_BLACK` | `#000000` | All pane backgrounds |
| `MATTE_OBSIDIAN` | `#262626` | Borders, idle status, table headers |
| `TOXIC_ACID_GREEN` | `#00FF66` | Spinner, running status, prompt, matrix arrows, low-severity findings, path/port highlights |
| `CYBER_CYAN` | `#00D7FF` | Labels, section titles, analysis headers, IP/URL highlights |
| `TERMINAL_SILVER` | `#D8D8D8` | Body text, telemetry values |
| `HIGH_EXPLOSIVE_RED` | `#FF3333` | Critical/high severity findings, errors, stderr, bad status highlights |
| `PURPLE_HIGHLIGHT` | `#AF5FFF` | MAC address highlights |
| `WARNING_YELLOW` | `#FFD700` | Warning status highlights, popup unsaved modified tag |

## App State (`app.rs`)

```rust
App {
    input: String,                           // current input buffer
    cursor_pos: usize,                       // cursor position in input
    output: Vec<Line<'static>>,              // scrollback buffer (smart highlighted)
    status: StatusBar,                       // phase, target, steps, provider, spinner
    scroll: u16,                             // PageUp/PageDown scroll offset
    should_quit: bool,                       // exit flag
    should_reload: bool,                     // Ctrl+R reload flag
    running: bool,                           // is a job in flight?
    cancel: CancelToken,                     // current job's cancel token
    current_render: Option<Stage4Render>,    // last AI-rendered analysis
    targets: Vec<DiscoveredTarget>,          // scraped targets
    active_scrape_ip: Option<String>,        // current IP context
    clickable_items: Vec<ClickItem>,         // populated each frame for mouse clicks
    file_entries: Vec<FileEntry>,            // current directory entries with icons
    filebar_scroll: u16,                     // horizontal scroll offset for file bar
    popup: Option<PopupState>,               // active popup file editor state
    local_ip: String,                        // machine's IP
    current_dir: String,                     // shell cwd
    cmd_tx: mpsc::Sender<Job>,               // channel to engine
}
```

