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
use deathpwn_core::engine::Phase;
use deathpwn_core::exec::Stream;
use deathpwn_core::schema::Stage4Render;

use crate::ui;

/// Lines scrolled per PageUp / PageDown.
const PAGE: u16 = 10;

/// Spinner animation frames (braille-based, smooth rotation).
const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Map a Phase color key to a ratatui Color.
fn phase_color(phase: &Phase) -> Color {
    match phase.color_key() {
        "darkgray" => Color::DarkGray,
        "blue" => Color::Blue,
        "yellow" => Color::Yellow,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "lightred" => Color::LightRed,
        _ => Color::Gray,
    }
}

/// One unit of work sent from the UI to the engine task: the raw input line
/// plus the cancel token the UI keeps a clone of (so Ctrl+C reaches the child).
pub struct Job {
    pub line: String,
    pub cancel: CancelToken,
}

/// The bottom status bar: current target, goal step count, active provider,
/// current pipeline phase with animated spinner (only when running).
pub struct StatusBar {
    pub target: Option<String>,
    pub steps: u32,
    pub provider: String,
    pub phase: Phase,
    pub spinner_tick: usize,
    pub running: bool,
}

impl StatusBar {
    pub fn new(provider: impl Into<String>) -> Self {
        Self {
            target: None,
            steps: 0,
            provider: provider.into(),
            phase: Phase::Idle,
            spinner_tick: 0,
            running: false,
        }
    }

    /// Advance the spinner animation frame.
    pub fn tick(&mut self) {
        self.spinner_tick = self.spinner_tick.wrapping_add(1);
    }

    /// Render the status bar as a single styled line.
    pub fn line(&self) -> Line<'static> {
        let target = self.target.clone().unwrap_or_else(|| "-".to_string());
        let color = phase_color(&self.phase);

        let phase_part: Span = if self.running {
            let frame = SPINNER[self.spinner_tick % SPINNER.len()];
            Span::styled(
                format!("{} {}", frame, self.phase.label()),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled("◼ ready", Style::default().fg(Color::DarkGray))
        };

        Line::from(vec![
            phase_part,
            Span::raw("  "),
            Span::styled("target:", Style::default().fg(Color::White)),
            Span::raw(" "),
            Span::styled(target, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled("steps:", Style::default().fg(Color::White)),
            Span::raw(" "),
            Span::styled(self.steps.to_string(), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled("provider:", Style::default().fg(Color::White)),
            Span::raw(" "),
            Span::styled(self.provider.clone(), Style::default().fg(Color::Green)),
        ])
    }
}

/// All UI state. `output` is the terminal output pane; `log_lines` is the
/// left-side activity log (phase banners); `current_render` holds the most
/// recent structured `Stage4Render`.
pub struct App {
    pub input: String,
    pub output: Vec<Line<'static>>,
    #[allow(dead_code)]
    pub log_lines: Vec<Line<'static>>,
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
            log_lines: Vec::new(),
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
                self.should_quit = true;
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
                let (style, target) = match line.stream {
                    Stream::Stdout => (Style::default().fg(Color::Gray), &mut self.output),
                    Stream::Stderr => (Style::default().fg(Color::Red), &mut self.output),
                    Stream::Banner => (
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                        &mut self.output,
                    ),
                };
                for text in line.text.lines() {
                    if text.trim().is_empty() {
                        continue;
                    }
                    target.push(Line::from(Span::styled(text.to_string(), style)));
                }
            }
            EngineEvent::Rendered(render) => {
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
            EngineEvent::Progress { target, step } => {
                if let Some(target) = target {
                    self.status.target = Some(target);
                }
                self.status.steps = step;
            }
            EngineEvent::PhaseChange(phase) => {
                self.status.phase = phase;
            }
            EngineEvent::Done => {
                self.running = false;
                self.status.running = false;
                self.status.phase = Phase::Idle;
            }
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
        self.status.running = true;
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
        self.status.running = false;
        self.status.phase = Phase::Idle;
        self.input.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use deathpwn_core::exec::{OutputLine, Stream};
    use tokio::sync::mpsc;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    #[test]
    fn scripted_keys_drive_app_state() {
        let (job_tx, mut job_rx) = mpsc::channel::<Job>(16);
        let mut app = App::new(job_tx, StatusBar::new("gpt-4o-mini"));

        app.handle_key(key(KeyCode::Char('i')));
        app.handle_key(key(KeyCode::Char('d')));
        assert_eq!(app.input, "id");

        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.input, "", "Enter must clear the input line");
        assert!(app.running, "submitting a line marks a command running");
        let job = job_rx.try_recv().expect("a job was submitted to the engine");
        assert_eq!(job.line, "id");

        assert!(!app.cancel.is_cancelled());
        app.handle_key(ctrl(KeyCode::Char('c')));
        assert!(app.cancel.is_cancelled(), "Ctrl+C cancels the running command");
        assert!(job.cancel.is_cancelled(), "engine shares the same cancel token");

        let before = app.scroll;
        app.handle_key(key(KeyCode::PageDown));
        assert!(app.scroll > before, "PageDown scrolls down");
        app.handle_key(key(KeyCode::PageUp));
        assert_eq!(app.scroll, before, "PageUp scrolls back up");

        app.handle_key(key(KeyCode::Char('z')));
        app.handle_key(ctrl(KeyCode::Char('x')));
        assert_eq!(app.input, "", "Ctrl+X returns to a fresh prompt");
        assert!(!app.running, "Ctrl+X drains the running chain");

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

        app.handle_key(key(KeyCode::Char('y')));
        app.handle_key(ctrl(KeyCode::Char('d')));
        assert!(app.should_quit, "Ctrl+D quits immediately, even with text");

        let (job_tx2, _rx2) = mpsc::channel::<Job>(16);
        let mut app2 = App::new(job_tx2, StatusBar::new("gpt-4o-mini"));
        app2.handle_key(key(KeyCode::Esc));
        assert!(app2.should_quit, "Esc on empty input quits");
    }

    #[test]
    fn progress_event_updates_status_bar() {
        let (job_tx, _rx) = mpsc::channel::<Job>(16);
        let mut app = App::new(job_tx, StatusBar::new("gpt-4o-mini"));
        assert_eq!(app.status.target, None);
        assert_eq!(app.status.steps, 0);

        app.on_event(EngineEvent::Progress {
            target: Some("10.0.0.5".to_string()),
            step: 1,
        });
        assert_eq!(app.status.target.as_deref(), Some("10.0.0.5"));
        assert_eq!(app.status.steps, 1);

        // A later progress event with no target keeps the last known target but
        // advances the step count.
        app.on_event(EngineEvent::Progress {
            target: None,
            step: 2,
        });
        assert_eq!(app.status.target.as_deref(), Some("10.0.0.5"));
        assert_eq!(app.status.steps, 2);
    }
}
