### Task 16: TUI (deathpwn-tui)

The binary crate: a full-screen `ratatui` + `crossterm` terminal that plumbs
key events into the core `Engine` and draws the `EngineEvent`s it streams back.
The TUI is deliberately thin — no business logic. `App` owns the UI state and a
synchronous `handle_key` method (so it is unit-testable without a real
terminal); `ui.rs` maps state and `Stage4Render` sections onto widgets; `main.rs`
owns the tokio runtime, builds `Config` + `Engine`, and runs the redraw loop
with two channels: one input thread → UI, one UI → engine task.

**Files:**
- Create: `deathpwn-tui/Cargo.toml`  (tui crate — dependencies)
- Create: `deathpwn-tui/src/main.rs`  (tui crate — `#[tokio::main]`, wiring, event loop)
- Create: `deathpwn-tui/src/app.rs`  (tui crate — `App`, `StatusBar`, `Job`, key handling)
- Create: `deathpwn-tui/src/ui.rs`  (tui crate — `draw`, `render_section`, `stage4_to_lines`)
- Test: unit smoke test lives in `#[cfg(test)] mod tests` inside `app.rs` (drives `App::handle_key` directly; never enters raw mode — deterministic, no real terminal)

**New dependencies:** `ratatui`, `crossterm`, `tokio` (with runtime/macros/sync/time
features), and the `deathpwn-core` path dependency. `async-trait` is **not**
needed here: the smoke test's "fake engine" is just the receiving end of the job
channel (no trait is implemented in this crate), so there is no async trait to
annotate.

**Interfaces:**

- Consumes (exact signatures from earlier tasks — do not re-type):
  - `struct Config { provider_a: ProviderConfig, provider_b: ProviderConfig, shell: String, max_goal_steps: u32, max_corrections: u32, artifacts_dir: PathBuf, http_timeout_secs: u64 }`, `struct ProviderConfig { url: String, key: String, model: String }`, `Config::from_env() -> Result<Config>` (Task 1)
  - `type Result<T> = std::result::Result<T, DeathpwnError>;` with `DeathpwnError::Io(#[from] std::io::Error)` (Task 1) — lets `?` absorb crossterm/`io` errors in `main`
  - `struct Stage4Render { sections: Vec<RenderSection> }`, `struct RenderSection { title: String, kind: SectionKind, body: RenderBody }`, `enum SectionKind { Table, KeyValue, Text, Findings }`, `enum RenderBody { Table { headers: Vec<String>, rows: Vec<Vec<String>> }, KeyValue(Vec<(String,String)>), Text(String), Findings(Vec<FindingItem>) }`, `struct FindingItem { severity: String, title: String, detail: String }` (Task 2)
  - `struct OutputLine { stream: Stream, text: String }`, `enum Stream { Stdout, Stderr }`, `#[derive(Clone)] struct CancelToken` with `fn cancel(&self)` / `fn is_cancelled(&self) -> bool` / async `cancelled()` and `CancelToken::new()` (Task 7)
  - `enum EngineEvent { Output(OutputLine), Rendered(Stage4Render), Error(String), Done }`, `struct Engine<R: CommandRunner>` with `impl<R: CommandRunner> Engine<R> { async fn handle_line(&mut self, line: &str, tx: mpsc::Sender<EngineEvent>, cancel: CancelToken) -> Result<()> }` (Task 15)
  - Wiring only (Tasks 3–14 constructors — see assumptions box): `OpenAiClient`, `SystemClock`/`Clock`, `FailoverClient`, `DuckDuckGoSearch`/`SearchProvider`, `AiProvider`, `ShellRunner`, `Detector`, `Understand`, `Retrieve`, `Plan`, `Render`, `FeedbackLoop`, `SessionState`, `PlanCache`, `Artifacts`

- Produces (this crate's public surface):
  - `struct App { input: String, output: Vec<Line<'static>>, status: StatusBar, scroll: u16, /* + runtime plumbing */ }` with `App::new(cmd_tx: mpsc::Sender<Job>, status: StatusBar) -> Self`, `fn handle_key(&mut self, key: KeyEvent)`, `fn on_event(&mut self, event: EngineEvent)`
  - `struct StatusBar { target: Option<String>, steps: u32, provider: String }` with `StatusBar::new(provider: impl Into<String>)`
  - `struct Job { line: String, cancel: CancelToken }` (unit of work sent UI → engine task)
  - `fn draw(f: &mut Frame, app: &App)` (top-level layout)
  - `fn render_section(f: &mut Frame, area: Rect, section: &Stage4Render)` — deterministic `SectionKind` → widget mapping, fixed severity palette
  - `fn stage4_to_lines(render: &Stage4Render) -> Vec<Line<'static>>` — the pure mapping `render_section` renders and `App` accumulates

> **Upstream constructor assumptions** (natural `new`/factory fns from Tasks
> 3–14; if a task named one differently, adjust only the call site in `main.rs`
> — `App`/`ui` are unaffected): `OpenAiClient::new(url, key, model, timeout_secs, label)`,
> `SystemClock` (unit struct), `FailoverClient::new(a, b, clock)`,
> `DuckDuckGoSearch::new(timeout_secs)`, `ShellRunner::new(shell, tx: Option<mpsc::Sender<OutputLine>>)`,
> `Detector::new(runner, shell)`, `Understand::new(ai)`, `Retrieve::new(ai, search)`,
> `Plan::new(ai)`, `Render::new(ai)`, `FeedbackLoop::new(runner, ai, max_corrections)`,
> `SessionState::new()`, `PlanCache::new()`, `Artifacts::open(root, clock: &dyn Clock)`,
> `Engine::new(detector, understand, retrieve, plan, render, feedback, session, cache, artifacts, ai, config)`.
> `Config`/`ProviderConfig` have public fields (spec §3: "Tests build `Config` directly").

---

#### Cycle A — crate + Cargo dependencies

- [ ] **Step 1: Create the crate manifest.** Write `deathpwn-tui/Cargo.toml` with the exact dependency set this task introduces. The binary is named `deathpwn`.

```toml
[package]
name = "deathpwn-tui"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "deathpwn"
path = "src/main.rs"

[dependencies]
deathpwn-core = { path = "../deathpwn-core" }
ratatui = "0.29"
crossterm = "0.28"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time"] }
```

---

#### Cycle B — `App` state + key handling (the smoke test)

- [ ] **Step 2: Write the failing smoke test.** Create a stub `deathpwn-tui/src/main.rs` so the module is declared, and create `deathpwn-tui/src/app.rs` containing *only* the test below (the types come in Step 4). The test constructs `App` with a fake engine (just the receiving half of the job channel), pumps a scripted key sequence through `App::handle_key`, and asserts on resulting state — no raw mode, no pixels, fully deterministic.

`deathpwn-tui/src/main.rs` (stub):

```rust
mod app;

fn main() {}
```

`deathpwn-tui/src/app.rs` (test only for now):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use tokio::sync::mpsc;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    #[test]
    fn scripted_keys_drive_app_state() {
        // "Fake engine": we hold the Job receiver instead of a real Engine task.
        let (job_tx, mut job_rx) = mpsc::channel::<Job>(16);
        let mut app = App::new(job_tx, StatusBar::new("gpt-4o-mini"));

        // Type "id" then Enter -> a job is submitted, input cleared, running set.
        app.handle_key(key(KeyCode::Char('i')));
        app.handle_key(key(KeyCode::Char('d')));
        assert_eq!(app.input, "id");

        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.input, "", "Enter must clear the input line");
        assert!(app.running, "submitting a line marks a command running");
        let job = job_rx.try_recv().expect("a job was submitted to the engine");
        assert_eq!(job.line, "id");

        // Ctrl+C cancels the running command's token (shared with the engine).
        assert!(!app.cancel.is_cancelled());
        app.handle_key(ctrl(KeyCode::Char('c')));
        assert!(app.cancel.is_cancelled(), "Ctrl+C cancels the running command");
        assert!(job.cancel.is_cancelled(), "engine shares the same cancel token");

        // PageDown then PageUp move the scroll offset and return to start.
        let before = app.scroll;
        app.handle_key(key(KeyCode::PageDown));
        assert!(app.scroll > before, "PageDown scrolls down");
        app.handle_key(key(KeyCode::PageUp));
        assert_eq!(app.scroll, before, "PageUp scrolls back up");

        // Ctrl+X cancels and drains the pending chain, returning to a fresh prompt.
        app.handle_key(key(KeyCode::Char('z'))); // some in-flight typing
        app.handle_key(ctrl(KeyCode::Char('x')));
        assert_eq!(app.input, "", "Ctrl+X returns to a fresh prompt");
        assert!(!app.running, "Ctrl+X drains the running chain");

        // Engine events append output; Done clears the running flag.
        app.handle_key(key(KeyCode::Char('a')));
        app.handle_key(key(KeyCode::Enter));
        assert!(app.running);
        let out_len = app.output.len();
        app.on_event(EngineEvent::Output(OutputLine {
            stream: Stream::Stdout,
            text: "hello\nworld".to_string(),
        }));
        assert_eq!(app.output.len(), out_len + 2, "each stdout line becomes a Line");
        app.on_event(EngineEvent::Done);
        assert!(!app.running, "EngineEvent::Done clears the running flag");

        // Ctrl+D with text present does NOT quit; on empty input it quits.
        app.handle_key(key(KeyCode::Char('y')));
        app.handle_key(ctrl(KeyCode::Char('d')));
        assert!(!app.should_quit, "Ctrl+D with text present does not quit");
        app.input.clear();
        app.handle_key(ctrl(KeyCode::Char('d')));
        assert!(app.should_quit, "Ctrl+D on empty input quits");

        // Esc on empty input also quits (fresh app to isolate the flag).
        let (job_tx2, _rx2) = mpsc::channel::<Job>(16);
        let mut app2 = App::new(job_tx2, StatusBar::new("gpt-4o-mini"));
        app2.handle_key(key(KeyCode::Esc));
        assert!(app2.should_quit, "Esc on empty input quits");
    }
}
```

- [ ] **Step 3: Run the test to verify it fails.** `cargo test -p deathpwn-tui scripted_keys_drive_app_state`. Expected: fails to compile — `cannot find type App in this scope` (also `StatusBar`, `Job`, `EngineEvent`, `OutputLine`, `Stream` unresolved), because `app.rs` has no implementation yet.

- [ ] **Step 4: Implement `app.rs`.** Write the full state + key handling *above* the existing test module in `deathpwn-tui/src/app.rs`:

```rust
//! App state and synchronous key handling for the deathpwn TUI.
//!
//! `handle_key` is deliberately synchronous and side-effect-light (it mutates
//! state, cancels tokens, and `try_send`s jobs) so it can be unit-tested by
//! pumping a scripted key sequence — no terminal, no async runtime required.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use tokio::sync::mpsc;

use deathpwn_core::cancel::CancelToken;
use deathpwn_core::engine::EngineEvent;
use deathpwn_core::exec::Stream;
use deathpwn_core::schema::Stage4Render;

use crate::ui;

/// Lines scrolled per PageUp / PageDown.
const PAGE: u16 = 10;

/// One unit of work sent from the UI to the engine task: the raw input line
/// plus the cancel token the UI keeps a clone of (so Ctrl+C reaches the child).
pub struct Job {
    pub line: String,
    pub cancel: CancelToken,
}

/// The bottom status bar: current target, goal step count, active provider.
pub struct StatusBar {
    pub target: Option<String>,
    pub steps: u32,
    pub provider: String,
}

impl StatusBar {
    pub fn new(provider: impl Into<String>) -> Self {
        Self {
            target: None,
            steps: 0,
            provider: provider.into(),
        }
    }

    /// Render the status bar as a single styled line.
    pub fn line(&self) -> Line<'static> {
        let target = self.target.clone().unwrap_or_else(|| "-".to_string());
        Line::from(vec![
            Span::styled(" target: ", Style::default().fg(Color::DarkGray)),
            Span::styled(target, Style::default().fg(Color::Cyan)),
            Span::raw("  "),
            Span::styled("steps: ", Style::default().fg(Color::DarkGray)),
            Span::styled(self.steps.to_string(), Style::default().fg(Color::Yellow)),
            Span::raw("  "),
            Span::styled("provider: ", Style::default().fg(Color::DarkGray)),
            Span::styled(self.provider.clone(), Style::default().fg(Color::Green)),
        ])
    }
}

/// All UI state. `output` is the scrollback console; `current_render` holds the
/// most recent structured `Stage4Render` shown in its own pane.
pub struct App {
    pub input: String,
    pub output: Vec<Line<'static>>,
    pub status: StatusBar,
    pub scroll: u16,
    pub should_quit: bool,
    pub running: bool,
    pub cancel: CancelToken,
    pub current_render: Option<Stage4Render>,
    cmd_tx: mpsc::Sender<Job>,
}

impl App {
    pub fn new(cmd_tx: mpsc::Sender<Job>, status: StatusBar) -> Self {
        Self {
            input: String::new(),
            output: Vec::new(),
            status,
            scroll: 0,
            should_quit: false,
            running: false,
            cancel: CancelToken::new(),
            current_render: None,
            cmd_tx,
        }
    }

    /// Handle one key press. Pure state mutation plus non-blocking channel/token
    /// side effects — safe to call from tests without a runtime.
    pub fn handle_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (key.code, ctrl) {
            (KeyCode::Enter, _) => self.submit(),
            (KeyCode::Char('c'), true) => self.cancel_running(),
            (KeyCode::Char('x'), true) => self.cancel_and_drain(),
            (KeyCode::Char('d'), true) => {
                if self.input.is_empty() {
                    self.should_quit = true;
                }
            }
            (KeyCode::Esc, _) => {
                if self.input.is_empty() {
                    self.should_quit = true;
                }
            }
            (KeyCode::PageUp, _) => self.scroll = self.scroll.saturating_sub(PAGE),
            (KeyCode::PageDown, _) => self.scroll = self.scroll.saturating_add(PAGE),
            (KeyCode::Backspace, _) => {
                self.input.pop();
            }
            (KeyCode::Char(c), false) => self.input.push(c),
            _ => {}
        }
    }

    /// Apply an event streamed from the engine.
    pub fn on_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::Output(line) => {
                let style = match line.stream {
                    Stream::Stdout => Style::default().fg(Color::Gray),
                    Stream::Stderr => Style::default().fg(Color::Red),
                };
                for text in line.text.lines() {
                    self.output
                        .push(Line::from(Span::styled(text.to_string(), style)));
                }
            }
            EngineEvent::Rendered(render) => {
                // Also fold the structured render into the scrollback so history
                // is preserved, and keep it as the live pane's current view.
                self.output.extend(ui::stage4_to_lines(&render));
                self.current_render = Some(render);
            }
            EngineEvent::Error(msg) => {
                self.output.push(Line::from(Span::styled(
                    format!("error: {msg}"),
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::BOLD),
                )));
            }
            EngineEvent::Done => self.running = false,
        }
    }

    /// Submit the current input line as a job for the engine task.
    fn submit(&mut self) {
        if self.input.trim().is_empty() {
            return;
        }
        let line = std::mem::take(&mut self.input);
        let cancel = CancelToken::new();
        self.cancel = cancel.clone();
        self.running = true;
        let job = Job { line, cancel };
        // Non-blocking: the engine task drains jobs. A full queue drops input
        // rather than stalling the UI thread.
        let _ = self.cmd_tx.try_send(job);
    }

    /// Ctrl+C: cancel the currently running command via its token.
    fn cancel_running(&mut self) {
        if self.running {
            self.cancel.cancel();
        }
    }

    /// Ctrl+X: cancel the running command AND abandon the rest of the chain,
    /// returning to a fresh prompt.
    fn cancel_and_drain(&mut self) {
        self.cancel.cancel();
        self.running = false;
        self.input.clear();
    }
}
```

- [ ] **Step 5: Run the test to verify it passes.** `cargo test -p deathpwn-tui scripted_keys_drive_app_state`. Expected: `test app::tests::scripted_keys_drive_app_state ... ok` — `1 passed`.

- [ ] **Step 6: Commit.** `git add deathpwn-tui/Cargo.toml deathpwn-tui/src/main.rs deathpwn-tui/src/app.rs && git commit -m "feat(deathpwn): TUI App state + key handling with smoke test"`.

---

#### Cycle C — `ui.rs` rendering (`draw`, `render_section`, `stage4_to_lines`)

- [ ] **Step 7: Implement `ui.rs`.** Create `deathpwn-tui/src/ui.rs` with the deterministic `SectionKind` → widget mapping and fixed severity palette, then add `mod ui;` to `deathpwn-tui/src/main.rs` (below `mod app;`).

```rust
//! Widget rendering: the three-pane layout, the structured `Stage4Render`
//! renderer (`render_section`), and its pure line-mapping helper
//! (`stage4_to_lines`). Every `SectionKind` maps to one deterministic widget
//! shape; finding severities use a fixed color palette.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use deathpwn_core::schema::{RenderBody, Stage4Render};

use crate::app::App;

/// Draw the whole UI: output pane (console + optional structured render),
/// status bar, and the input line.
pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // output pane
            Constraint::Length(1), // status bar
            Constraint::Length(3), // input line (bordered)
        ])
        .split(f.area());

    match &app.current_render {
        Some(render) => {
            let panes = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(chunks[0]);
            draw_console(f, panes[0], app);
            render_section(f, panes[1], render);
        }
        None => draw_console(f, chunks[0], app),
    }

    let status = Paragraph::new(app.status.line()).style(Style::default().bg(Color::Black));
    f.render_widget(status, chunks[1]);

    let input = Paragraph::new(Line::from(vec![
        Span::styled(
            "> ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(app.input.clone()),
    ]))
    .block(Block::default().borders(Borders::ALL).title("input"));
    f.render_widget(input, chunks[2]);
}

/// Scrollable console of accumulated output lines.
fn draw_console(f: &mut Frame, area: Rect, app: &App) {
    let para = Paragraph::new(app.output.clone())
        .block(Block::default().borders(Borders::ALL).title("output"))
        .wrap(Wrap { trim: false })
        .scroll((app.scroll, 0));
    f.render_widget(para, area);
}

/// Render a structured `Stage4Render` into `area` as a bordered paragraph.
pub fn render_section(f: &mut Frame, area: Rect, section: &Stage4Render) {
    let para = Paragraph::new(stage4_to_lines(section))
        .block(Block::default().borders(Borders::ALL).title("analysis"))
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

/// Deterministic mapping from a `Stage4Render` to styled lines. Each
/// `RenderBody` variant (mirroring `SectionKind`) has exactly one shape.
pub fn stage4_to_lines(render: &Stage4Render) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for section in &render.sections {
        lines.push(Line::from(Span::styled(
            section.title.clone(),
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        )));
        match &section.body {
            RenderBody::Text(text) => {
                for l in text.lines() {
                    lines.push(Line::from(l.to_string()));
                }
            }
            RenderBody::KeyValue(pairs) => {
                for (k, v) in pairs {
                    lines.push(Line::from(vec![
                        Span::styled(format!("{k}: "), Style::default().fg(Color::Cyan)),
                        Span::raw(v.clone()),
                    ]));
                }
            }
            RenderBody::Table { headers, rows } => {
                lines.push(Line::from(Span::styled(
                    headers.join(" | "),
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )));
                for row in rows {
                    lines.push(Line::from(row.join(" | ")));
                }
            }
            RenderBody::Findings(items) => {
                for item in items {
                    let color = severity_color(&item.severity);
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("[{}] ", item.severity.to_uppercase()),
                            Style::default().fg(color).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(item.title.clone(), Style::default().fg(color)),
                    ]));
                    if !item.detail.is_empty() {
                        lines.push(Line::from(Span::raw(format!("    {}", item.detail))));
                    }
                }
            }
        }
        lines.push(Line::from("")); // blank separator between sections
    }
    lines
}

/// Fixed severity palette (case-insensitive).
fn severity_color(severity: &str) -> Color {
    match severity.to_ascii_lowercase().as_str() {
        "critical" => Color::Red,
        "high" => Color::LightRed,
        "medium" => Color::Yellow,
        "low" => Color::Green,
        "info" | "informational" => Color::Cyan,
        _ => Color::Gray,
    }
}
```

- [ ] **Step 8: Build to verify `ui.rs` compiles.** `cargo build -p deathpwn-tui`. Expected: compiles cleanly. `app.rs` now resolves `crate::ui::stage4_to_lines`; `ui.rs` resolves `crate::app::App`. (The still-stub `main.rs` yields no errors.)

- [ ] **Step 9: Commit.** `git add deathpwn-tui/src/ui.rs deathpwn-tui/src/main.rs && git commit -m "feat(deathpwn): TUI widget rendering and Stage4Render mapping"`.

---

#### Cycle D — `main.rs` wiring + event loop

- [ ] **Step 10: Implement `main.rs`.** Replace the stub with the full runtime: build `Config` and the `Engine`, spawn the engine task and the input thread, then run the redraw/event loop. This is plumbing only — every decision lives in core.

```rust
//! deathpwn-tui: the ratatui front end. Owns the tokio runtime, the terminal,
//! and all rendering. No business logic — it plumbs crossterm key events into
//! the core `Engine` and draws the `EngineEvent`s streamed back.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use deathpwn_core::cache::PlanCache;
use deathpwn_core::clock::{Clock, SystemClock};
use deathpwn_core::config::Config;
use deathpwn_core::detector::Detector;
use deathpwn_core::engine::{Engine, EngineEvent};
use deathpwn_core::error::Result;
use deathpwn_core::exec::{FeedbackLoop, ShellRunner};
use deathpwn_core::pipeline::{Plan, Render, Retrieve, Understand};
use deathpwn_core::providers::{AiProvider, FailoverClient, OpenAiClient};
use deathpwn_core::search::{DuckDuckGoSearch, SearchProvider};
use deathpwn_core::session::{Artifacts, SessionState};

mod app;
mod ui;

use app::{App, Job, StatusBar};

#[tokio::main]
async fn main() -> Result<()> {
    // ---- build config + engine (all core wiring) -------------------------
    let config = Config::from_env()?;
    let provider_label = config.provider_a.model.clone();

    let provider_a: Arc<dyn AiProvider> = Arc::new(OpenAiClient::new(
        config.provider_a.url.clone(),
        config.provider_a.key.clone(),
        config.provider_a.model.clone(),
        config.http_timeout_secs,
        "provider-a",
    ));
    let provider_b: Arc<dyn AiProvider> = Arc::new(OpenAiClient::new(
        config.provider_b.url.clone(),
        config.provider_b.key.clone(),
        config.provider_b.model.clone(),
        config.http_timeout_secs,
        "provider-b",
    ));
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let search: Arc<dyn SearchProvider> =
        Arc::new(DuckDuckGoSearch::new(config.http_timeout_secs));

    let detector = Detector::new(
        ShellRunner::new(config.shell.clone(), None),
        config.shell.clone(),
    );
    let understand = Understand::new(FailoverClient::new(
        provider_a.clone(),
        provider_b.clone(),
        clock.clone(),
    ));
    let retrieve = Retrieve::new(
        FailoverClient::new(provider_a.clone(), provider_b.clone(), clock.clone()),
        search.clone(),
    );
    let plan = Plan::new(FailoverClient::new(
        provider_a.clone(),
        provider_b.clone(),
        clock.clone(),
    ));
    let render = Render::new(FailoverClient::new(
        provider_a.clone(),
        provider_b.clone(),
        clock.clone(),
    ));
    let feedback = FeedbackLoop::new(
        ShellRunner::new(config.shell.clone(), None),
        provider_a.clone(),
        config.max_corrections,
    );
    let engine_ai =
        FailoverClient::new(provider_a.clone(), provider_b.clone(), clock.clone());

    let session = SessionState::new();
    let cache = PlanCache::new();
    let artifacts = Artifacts::open(config.artifacts_dir.clone(), clock.as_ref())?;

    let mut engine = Engine::new(
        detector, understand, retrieve, plan, render, feedback, session, cache, artifacts,
        engine_ai, config,
    );

    // ---- channels: input thread -> UI, UI -> engine task, engine -> UI ---
    let (job_tx, mut job_rx) = mpsc::channel::<Job>(64);
    let (event_tx, mut event_rx) = mpsc::channel::<EngineEvent>(1024);
    let (key_tx, mut key_rx) = mpsc::channel::<KeyEvent>(64);

    // Engine task: process one submitted line at a time.
    tokio::spawn(async move {
        while let Some(job) = job_rx.recv().await {
            let _ = engine
                .handle_line(&job.line, event_tx.clone(), job.cancel)
                .await;
        }
    });

    // Input thread: blocking crossterm reads forwarded onto an mpsc channel.
    thread::spawn(move || loop {
        match event::poll(Duration::from_millis(100)) {
            Ok(true) => {
                if let Ok(Event::Key(key)) = event::read() {
                    if key_tx.blocking_send(key).is_err() {
                        break;
                    }
                }
            }
            Ok(false) => {}
            Err(_) => break,
        }
    });

    // ---- terminal setup ---------------------------------------------------
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(job_tx, StatusBar::new(provider_label));

    // ---- redraw / event loop ---------------------------------------------
    let result: Result<()> = loop {
        if let Err(e) = terminal.draw(|f| ui::draw(f, &app)) {
            break Err(e.into());
        }
        if app.should_quit {
            break Ok(());
        }
        tokio::select! {
            maybe_key = key_rx.recv() => {
                match maybe_key {
                    Some(key) if key.kind == KeyEventKind::Press => app.handle_key(key),
                    Some(_) => {}
                    None => break Ok(()),
                }
            }
            maybe_event = event_rx.recv() => {
                if let Some(engine_event) = maybe_event {
                    app.on_event(engine_event);
                }
            }
        }
    };

    // ---- teardown ---------------------------------------------------------
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result
}
```

- [ ] **Step 11: Build and re-run the smoke test to verify green.** `cargo build -p deathpwn-tui && cargo test -p deathpwn-tui`. Expected: the binary compiles and links; `test app::tests::scripted_keys_drive_app_state ... ok`, `1 passed`. (Wiring `main.rs` did not disturb the `App`/`ui` units.)

- [ ] **Step 12: Commit.** `git add deathpwn-tui/src/main.rs && git commit -m "feat(deathpwn): TUI runtime wiring and crossterm/engine event loop"`.
