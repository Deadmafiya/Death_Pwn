//! App state and synchronous key handling for the deathpwn TUI.
//!
//! `handle_key` is deliberately synchronous and side-effect-light (it mutates
//! state, cancels tokens, and `try_send`s jobs) so it can be unit-tested by
//! pumping a scripted key sequence — no terminal, no async runtime required.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use tokio::sync::mpsc;

use deathpwn_core::cancel::CancelToken;
use deathpwn_core::engine::EngineEvent;
use deathpwn_core::engine::Phase;
use deathpwn_core::exec::Stream;
use deathpwn_core::schema::Stage4Render;

use crate::ui;
use crate::ui::filebrowser::{self, ClickAction, ClickItem, FileEntry};
use crate::ui::popup::{self, PopupState, PopupHit};

/// Lines scrolled per PageUp / PageDown.
const PAGE: u16 = 10;

/// One unit of work sent from the UI to the engine task: the raw input line
/// plus the cancel token the UI keeps a clone of (so Ctrl+C reaches the child).
pub struct Job {
    pub line: String,
    pub cancel: CancelToken,
    pub resolve_only: bool,
}

/// Status bar state shared with the telemetry pane.
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
}

#[derive(Debug, Clone)]
pub struct DiscoveredTarget {
    pub ip: String,
    pub ports: Vec<u16>,
    pub urls: Vec<String>,
    pub filepaths: Vec<String>,
    pub expanded: bool,
}

/// All UI state.
pub struct App {
    pub input: String,
    pub cursor_pos: usize,
    pub output: Vec<Line<'static>>,
    pub status: StatusBar,
    pub scroll: u16,
    pub should_quit: bool,
    pub should_reload: bool,
    pub running: bool,
    pub cancel: CancelToken,
    pub current_render: Option<Stage4Render>,
    pub targets: Vec<DiscoveredTarget>,
    pub active_scrape_ip: Option<String>,
    pub clickable_items: Vec<ClickItem>,
    pub local_ip: String,
    pub current_dir: String,
    pub file_entries: Vec<FileEntry>,
    pub popup: Option<PopupState>,
    pub term_size: (u16, u16),
    pub filebar_scroll: u16,
    pub filebar_max_scroll: u16,
    pub filebar_origin_x: u16,
    pub filebar_row_y: u16,
    pub filebar_drag_active: bool,
    pub filebar_drag_start_x: u16,
    pub filebar_drag_last_x: u16,
    pub filebar_drag_delta: u16,
    cmd_tx: mpsc::Sender<Job>,
    stdin_tx: mpsc::Sender<String>,
    history: Vec<String>,
    history_idx: Option<usize>,
}

impl App {
    pub fn new(
        cmd_tx: mpsc::Sender<Job>,
        stdin_tx: mpsc::Sender<String>,
        status: StatusBar,
    ) -> Self {
        let output = Vec::new();
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "-".to_string());

        Self {
            input: String::new(),
            cursor_pos: 0,
            output,
            status,
            scroll: 0,
            should_quit: false,
            should_reload: false,
            running: false,
            cancel: CancelToken::new(),
            current_render: None,
            targets: Vec::new(),
            active_scrape_ip: None,
            clickable_items: Vec::new(),
            local_ip: Self::get_local_ip(),
            current_dir: cwd.clone(),
            file_entries: filebrowser::refresh_file_list(&cwd),
            popup: None,
            term_size: (80, 24),
            filebar_scroll: 0,
            filebar_max_scroll: 0,
            filebar_origin_x: 0,
            filebar_row_y: 0,
            filebar_drag_active: false,
            filebar_drag_start_x: 0,
            filebar_drag_last_x: 0,
            filebar_drag_delta: 0,
            cmd_tx,
            stdin_tx,
            history: Vec::new(),
            history_idx: None,
        }
    }

    /// Handle one key press.
    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.popup.is_some() {
            self.handle_popup_key(key);
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match (key.code, ctrl, alt) {
            (KeyCode::Tab, true, _) | (KeyCode::Tab, _, true) | (KeyCode::BackTab, _, _) => {
                self.submit_resolve_only();
            }
            (KeyCode::Enter, _, _) => self.submit(),
            (KeyCode::Char('c'), true, _) => self.cancel_running(),
            (KeyCode::Char('x'), true, _) => self.cancel_and_drain(),
            (KeyCode::Char('d'), true, _) => {
                self.should_quit = true;
            }
            (KeyCode::Char('r'), true, _) => {
                self.should_reload = true;
            }
            (KeyCode::Esc, _, _) => {
                if self.input.is_empty() {
                    self.should_quit = true;
                }
            }
            (KeyCode::Left, true, _) | (KeyCode::Left, _, true) => {
                self.filebar_scroll = self.filebar_scroll.saturating_sub(6);
            }
            (KeyCode::Right, true, _) | (KeyCode::Right, _, true) => {
                self.filebar_scroll = (self.filebar_scroll + 6).min(self.filebar_max_scroll);
            }
            (KeyCode::Left, false, false) => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                }
            }
            (KeyCode::Right, false, false) => {
                if self.cursor_pos < self.input.len() {
                    self.cursor_pos += 1;
                }
            }
            (KeyCode::Up, _, _) => self.history_prev(),
            (KeyCode::Down, _, _) => self.history_next(),
            (KeyCode::Home, _, _) => {
                self.cursor_pos = 0;
            }
            (KeyCode::End, _, _) => {
                self.cursor_pos = self.input.len();
            }
            (KeyCode::PageUp, _, _) => self.scroll = self.scroll.saturating_sub(PAGE),
            (KeyCode::PageDown, _, _) => self.scroll = self.scroll.saturating_add(PAGE),
            (KeyCode::Backspace, _, _) => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                    self.input.remove(self.cursor_pos);
                }
            }
            (KeyCode::Delete, _, _) => {
                if self.cursor_pos < self.input.len() {
                    self.input.remove(self.cursor_pos);
                }
            }
            (KeyCode::Char(c), false, false) => {
                self.input.insert(self.cursor_pos, c);
                self.cursor_pos += 1;
                self.scroll = u16::MAX;
            }
            _ => {}
        }
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let idx = match self.history_idx {
            None => self.history.len() - 1,
            Some(0) => return,
            Some(i) => i - 1,
        };
        self.history_idx = Some(idx);
        self.input = self.history[idx].clone();
        self.cursor_pos = self.input.len();
    }

    fn history_next(&mut self) {
        match self.history_idx {
            None => return,
            Some(i) if i + 1 >= self.history.len() => {
                self.history_idx = None;
                self.input.clear();
                self.cursor_pos = 0;
            }
            Some(i) => {
                let idx = i + 1;
                self.history_idx = Some(idx);
                self.input = self.history[idx].clone();
                self.cursor_pos = self.input.len();
            }
        }
    }

    fn handle_popup_key(&mut self, key: KeyEvent) {
        let popup = self.popup.as_mut().unwrap();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (key.code, ctrl) {
            (KeyCode::Esc, _) => { self.popup = None; }
            (KeyCode::Char('s'), true) => {
                let _ = popup::save(popup);
                popup.dirty = false;
            }
            (KeyCode::Char('z'), true) => popup::undo(popup),
            (KeyCode::Char('y'), true) => popup::redo(popup),
            (KeyCode::Left, _) => popup::move_cursor(popup, popup::ArrowDir::Left),
            (KeyCode::Right, _) => popup::move_cursor(popup, popup::ArrowDir::Right),
            (KeyCode::Up, _) => popup::move_cursor(popup, popup::ArrowDir::Up),
            (KeyCode::Down, _) => popup::move_cursor(popup, popup::ArrowDir::Down),
            (KeyCode::Backspace, _) => popup::backspace(popup),
            (KeyCode::Delete, _) => popup::delete_char(popup),
            (KeyCode::Enter, _) => popup::newline(popup),
            (KeyCode::Home, _) => { popup.cursor_col = 0; }
            (KeyCode::End, _) => {
                let len = popup.lines[popup.cursor_row].len();
                popup.cursor_col = len;
            }
            (KeyCode::PageUp, _) => {
                popup.scroll_row = popup.scroll_row.saturating_sub(10);
            }
            (KeyCode::PageDown, _) => {
                popup.scroll_row = popup.scroll_row.saturating_add(10);
            }
            (KeyCode::Char(c), false) => popup::insert_char(popup, c),
            _ => {}
        }
    }

    /// Apply an event streamed from the engine.
    pub fn on_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::Output(line) => {
                self.scrape_text(&line.text);
                let base_style = match line.stream {
                    Stream::Stdout => Style::default().fg(Color::Gray),
                    Stream::Stderr => Style::default().fg(Color::Red),
                    Stream::Banner => Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                };
                for text in line.text.lines() {
                    if matches!(line.stream, Stream::Banner) {
                        self.output
                            .push(Line::from(Span::styled(text.to_string(), base_style)));
                    } else {
                        self.output
                            .push(ui::highlight::highlight_line(text, base_style));
                    }
                }
                self.scroll = u16::MAX;
            }
            EngineEvent::Rendered(render) => {
                self.output.push(Line::from(""));
                self.output.push(Line::from(Span::styled(
                    "⚡ [AI ANALYSIS INGESTED INTO LOG MATRIX] ─────────────────────────────",
                    Style::default()
                        .fg(Color::Rgb(0, 255, 102))
                        .add_modifier(Modifier::BOLD),
                )));
                self.output.push(Line::from(""));

                self.output.extend(ui::stage4_to_lines(&render));

                self.output.push(Line::from(Span::styled(
                    "──────────────────────────────────────────────────────────────────────",
                    Style::default().fg(Color::Rgb(38, 38, 38)),
                )));

                self.current_render = Some(render);

                self.scroll = u16::MAX;
            }
            EngineEvent::Error(msg) => {
                self.output.push(Line::from(Span::styled(
                    format!("error: {msg}"),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )));
                self.scroll = u16::MAX;
            }
            EngineEvent::Progress { target, step } => {
                if let Some(target) = target {
                    let ips = find_ips(&target);
                    if !ips.is_empty() {
                        self.active_scrape_ip = Some(ips[0].clone());
                        if !self.targets.iter().any(|t| t.ip == ips[0]) {
                            self.targets.push(DiscoveredTarget {
                                ip: ips[0].clone(),
                                ports: Vec::new(),
                                urls: Vec::new(),
                                filepaths: Vec::new(),
                                expanded: false,
                            });
                        }
                    }
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
            EngineEvent::Cwd(cwd) => {
                self.current_dir = cwd.clone();
                self.file_entries = filebrowser::refresh_file_list(&cwd);
            }
            EngineEvent::Resolved(resolved) => {
                self.input = resolved;
                self.cursor_pos = self.input.len();
                self.running = false;
                self.status.running = false;
                self.status.phase = Phase::Idle;
            }
        }
    }

    fn get_local_ip() -> String {
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg("ip route get 1.1.1.1 | awk '{print $7; exit}'")
            .output();
        match output {
            Ok(out) => {
                let ip = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if ip.is_empty() {
                    "-".to_string()
                } else {
                    ip
                }
            }
            Err(_) => "-".to_string(),
        }
    }

    fn submit(&mut self) {
        if self.running {
            let line = std::mem::take(&mut self.input);
            self.history.push(line.clone());
            self.history_idx = None;
            let prompt_line = Line::from(vec![
                Span::styled(
                    "> ",
                    Style::default()
                        .fg(Color::Rgb(0, 255, 102))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(line.clone()),
            ]);
            self.output.push(prompt_line);
            self.scroll = u16::MAX;
            self.cursor_pos = 0;

            let _ = self.stdin_tx.try_send(format!("{}\n", line));
            return;
        }

        if self.input.trim().is_empty() {
            return;
        }
        let line = std::mem::take(&mut self.input);

        let trimmed = line.trim();
        if trimmed == "clear" || trimmed == "cls" {
            self.history.push(line);
            self.history_idx = None;
            self.output.clear();
            self.scroll = 0;
            self.cursor_pos = 0;
            return;
        }

        self.history.push(line.clone());
        self.history_idx = None;
        self.scrape_text(&line);
        self.cursor_pos = 0;

        let cancel = CancelToken::new();
        self.cancel = cancel.clone();
        self.running = true;
        self.status.running = true;
        let job = Job {
            line,
            cancel,
            resolve_only: false,
        };
        let _ = self.cmd_tx.try_send(job);
    }

    fn submit_resolve_only(&mut self) {
        if self.running || self.input.trim().is_empty() {
            return;
        }
        let line = self.input.clone();
        let cancel = CancelToken::new();
        self.cancel = cancel.clone();
        self.running = true;
        self.status.running = true;
        let job = Job {
            line,
            cancel,
            resolve_only: true,
        };
        let _ = self.cmd_tx.try_send(job);
    }

    /// Ctrl+C: cancel the currently running command via its token.
    fn cancel_running(&mut self) {
        if self.running {
            self.cancel.cancel();
        }
    }

    /// Ctrl+X: cancel the running command AND return to a fresh prompt.
    fn cancel_and_drain(&mut self) {
        self.cancel.cancel();
        self.running = false;
        self.status.running = false;
        self.status.phase = Phase::Idle;
        self.input.clear();
        self.cursor_pos = 0;
    }

    fn trigger_click_action(&mut self, row: u16, col: u16) {
        let adjusted_col = (col as i32 - self.filebar_origin_x as i32 + self.filebar_scroll as i32).max(0) as u16;
        if let Some(item) = self.clickable_items.iter().find(|i| {
            i.row_y == row && (i.col_range.is_none() ||
                i.col_range.map_or(false, |(c1, c2)| adjusted_col >= c1 && adjusted_col <= c2))
        }) {
            match &item.action {
                ClickAction::ToggleTarget { ip } => {
                    if let Some(t) = self.targets.iter_mut().find(|t| &t.ip == ip) {
                        t.expanded = !t.expanded;
                    }
                }
                ClickAction::NavigateDir { path } => {
                    self.input = format!("cd {}", shell_words::quote(path));
                    self.submit();
                }
                ClickAction::OpenFile { path } => {
                    match popup::load_file(path) {
                        Ok(ps) => self.popup = Some(ps),
                        Err(_) => {}
                    }
                }
                ClickAction::CopyToClipboard { text } => {
                    copy_to_clipboard(text);
                }
            }
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        if let Some(ref popup_st) = self.popup {
            let full = ratatui::layout::Rect {
                x: 0, y: 0,
                width: self.term_size.0,
                height: self.term_size.1,
            };
            let pr = popup::popup_area(full);
            match popup::popup_hit_test(popup_st, pr, mouse.row, mouse.column) {
                PopupHit::Save => {
                    let mut popup = self.popup.take().unwrap();
                    let _ = popup::save(&popup);
                    popup.dirty = false;
                    self.popup = Some(popup);
                }
                PopupHit::Exit => { self.popup = None; }
                PopupHit::Undo => {
                    let mut popup = self.popup.take().unwrap();
                    popup::undo(&mut popup);
                    self.popup = Some(popup);
                }
                PopupHit::Redo => {
                    let mut popup = self.popup.take().unwrap();
                    popup::redo(&mut popup);
                    self.popup = Some(popup);
                }
                PopupHit::Text { row, col } => {
                    let mut popup = self.popup.take().unwrap();
                    popup.cursor_row = row;
                    popup.cursor_col = col.min(
                        popup.lines.get(row).map(|l| l.len()).unwrap_or(0)
                    );
                    self.popup = Some(popup);
                }
                PopupHit::None => {}
            }
            return;
        }

        let row = mouse.row;
        let col = mouse.column;
        let filebar_y = if self.filebar_row_y > 0 {
            self.filebar_row_y.saturating_sub(1)
        } else {
            self.term_size.1.saturating_sub(3)
        };
        let is_in_filebar_region = row >= filebar_y;
        let shift = mouse.modifiers.contains(KeyModifiers::SHIFT);

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if is_in_filebar_region {
                    self.filebar_drag_active = true;
                    self.filebar_drag_start_x = col;
                    self.filebar_drag_last_x = col;
                    self.filebar_drag_delta = 0;
                } else {
                    self.filebar_drag_active = false;
                    self.trigger_click_action(row, col);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.filebar_drag_active {
                    let dx = self.filebar_drag_last_x as i32 - col as i32;
                    self.filebar_drag_delta = self.filebar_drag_delta.saturating_add(dx.unsigned_abs() as u16);
                    self.filebar_drag_last_x = col;
                    if dx != 0 {
                        let new_scroll = (self.filebar_scroll as i32 + dx)
                            .clamp(0, self.filebar_max_scroll as i32) as u16;
                        self.filebar_scroll = new_scroll;
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.filebar_drag_active {
                    self.filebar_drag_active = false;
                    if self.filebar_drag_delta <= 1 {
                        self.trigger_click_action(row, self.filebar_drag_start_x);
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                if let Some(item) = self.clickable_items.iter().find(|i| i.row_y == row) {
                    let text = match &item.action {
                        ClickAction::ToggleTarget { ip } => ip.clone(),
                        ClickAction::NavigateDir { path }
                        | ClickAction::OpenFile { path } => path.clone(),
                        ClickAction::CopyToClipboard { text } => text.clone(),
                    };
                    copy_to_clipboard(&text);
                }
            }
            MouseEventKind::ScrollDown => {
                if is_in_filebar_region || shift {
                    self.filebar_scroll = (self.filebar_scroll + 6).min(self.filebar_max_scroll);
                } else {
                    self.scroll = self.scroll.saturating_add(3);
                }
            }
            MouseEventKind::ScrollUp => {
                if is_in_filebar_region || shift {
                    self.filebar_scroll = self.filebar_scroll.saturating_sub(6);
                } else {
                    self.scroll = self.scroll.saturating_sub(3);
                }
            }
            MouseEventKind::ScrollRight => {
                self.filebar_scroll = (self.filebar_scroll + 6).min(self.filebar_max_scroll);
            }
            MouseEventKind::ScrollLeft => {
                self.filebar_scroll = self.filebar_scroll.saturating_sub(6);
            }
            _ => {}
        }
    }

    pub fn scrape_text(&mut self, text: &str) {
        for line in text.lines() {
            let is_interface_line = line.contains("Interface:")
                || line.contains("listening on")
                || line.contains("ip link");
            let ips = if is_interface_line {
                Vec::new()
            } else {
                find_ips(line)
            };
            for ip in &ips {
                self.active_scrape_ip = Some(ip.clone());
                if !self.targets.iter().any(|t| &t.ip == ip) {
                    self.targets.push(DiscoveredTarget {
                        ip: ip.clone(),
                        ports: Vec::new(),
                        urls: Vec::new(),
                        filepaths: Vec::new(),
                        expanded: true,
                    });
                }
            }

            if self.active_scrape_ip.is_none() {
                if let Some(ref status_t) = self.status.target {
                    let status_ips = find_ips(status_t);
                    if !status_ips.is_empty() {
                        let ip = status_ips[0].clone();
                        self.active_scrape_ip = Some(ip.clone());
                        if !self.targets.iter().any(|t| &t.ip == &ip) {
                            self.targets.push(DiscoveredTarget {
                                ip: ip.clone(),
                                ports: Vec::new(),
                                urls: Vec::new(),
                                filepaths: Vec::new(),
                                expanded: true,
                            });
                        }
                    }
                }
            }

            let ports = extract_ports(line);
            let urls = find_urls(line);
            let filepaths = find_filepaths(line);

            let has_items = !ports.is_empty() || !urls.is_empty() || !filepaths.is_empty();

            if has_items {
                let current_ip = match self.active_scrape_ip {
                    Some(ref ip) => ip.clone(),
                    None => {
                        if let Some(first_target) = self.targets.first() {
                            first_target.ip.clone()
                        } else {
                            let fallback = "General Target".to_string();
                            self.targets.push(DiscoveredTarget {
                                ip: fallback.clone(),
                                ports: Vec::new(),
                                urls: Vec::new(),
                                filepaths: Vec::new(),
                                expanded: true,
                            });
                            self.active_scrape_ip = Some(fallback.clone());
                            fallback
                        }
                    }
                };

                if let Some(t) = self.targets.iter_mut().find(|t| &t.ip == &current_ip) {
                    for p in ports {
                        if !t.ports.contains(&p) {
                            t.ports.push(p);
                        }
                    }
                    for u in urls {
                        if !t.urls.contains(&u) {
                            t.urls.push(u);
                        }
                    }
                    for f in filepaths {
                        if !t.filepaths.contains(&f) {
                            t.filepaths.push(f);
                        }
                    }
                }
            }
        }
    }
}

fn find_ips(text: &str) -> Vec<String> {
    let mut ips = Vec::new();
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i].is_ascii_digit() {
            let start = i;
            let mut end = start;

            while end < len {
                let b = bytes[end];
                if b.is_ascii_digit() || b == b'.' {
                    end += 1;
                } else {
                    break;
                }
            }

            let candidate = &text[start..end];
            let parts: Vec<&str> = candidate.split('.').collect();
            if parts.len() == 4
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.parse::<u8>().is_ok())
            {
                let prev_ok = start == 0 || !text.as_bytes()[start - 1].is_ascii_alphanumeric();
                let next_ok = end >= len
                    || !text.as_bytes()[end].is_ascii_alphanumeric()
                    || text.as_bytes()[end] == b'/'
                    || text.as_bytes()[end] == b':';
                if prev_ok && next_ok {
                    let ip = candidate.to_string();
                    if !ips.contains(&ip) {
                        ips.push(ip);
                    }
                }
            }
            i = end;
        } else {
            i += 1;
        }
    }
    ips
}

fn extract_ports(text: &str) -> Vec<u16> {
    let mut ports = Vec::new();

    let ips = find_ips(text);
    for ip in &ips {
        if let Some(pos) = text.find(ip) {
            let after = &text[pos + ip.len()..];
            if after.starts_with(':') {
                let rest = &after[1..];
                let port_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(p) = port_str.parse::<u16>() {
                    if p > 0 && !ports.contains(&p) {
                        ports.push(p);
                    }
                }
            }
        }
    }

    for word in text.split_whitespace() {
        let cleaned = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '/');
        if let Some(idx) = cleaned.find("/tcp") {
            let prefix = &cleaned[..idx];
            let num: String = prefix.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(p) = num.parse::<u16>() {
                if p > 0 && !ports.contains(&p) {
                    ports.push(p);
                }
            }
        }
        if let Some(idx) = cleaned.find("/udp") {
            let prefix = &cleaned[..idx];
            let num: String = prefix.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(p) = num.parse::<u16>() {
                if p > 0 && !ports.contains(&p) {
                    ports.push(p);
                }
            }
        }
    }

    if text.contains("-p") {
        let words: Vec<&str> = text.split_whitespace().collect();
        for i in 0..words.len() {
            if words[i] == "-p" && i + 1 < words.len() {
                for p in parse_port_list(words[i + 1]) {
                    if !ports.contains(&p) {
                        ports.push(p);
                    }
                }
            } else if words[i].starts_with("-p") && words[i].len() > 2 {
                for p in parse_port_list(&words[i][2..]) {
                    if !ports.contains(&p) {
                        ports.push(p);
                    }
                }
            }
        }
    }

    ports
}

fn parse_port_list(s: &str) -> Vec<u16> {
    let mut ports = Vec::new();
    for part in s.split(',') {
        let trimmed = part.trim_matches(|c: char| !c.is_numeric() && c != '-');
        if let Ok(port) = trimmed.parse::<u16>() {
            if port > 0 {
                ports.push(port);
            }
        } else if trimmed.contains('-') {
            let range: Vec<&str> = trimmed.split('-').collect();
            if range.len() == 2 {
                if let (Ok(start), Ok(end)) = (range[0].parse::<u16>(), range[1].parse::<u16>()) {
                    for p in start..=end {
                        if p > 0 {
                            ports.push(p);
                        }
                    }
                }
            }
        }
    }
    ports
}

fn find_urls(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let tool_domains = [
        "github.com",
        "gitlab.com",
        "gnu.org",
        "nmap.org",
        "kali.org",
        "blackarch.org",
        "sourceforge.net",
        "royhills",
    ];
    for word in text.split_whitespace() {
        let trimmed = word.trim_matches(|c: char| {
            c == '"'
                || c == '\''
                || c == '<'
                || c == '>'
                || c == '('
                || c == ')'
                || c == ','
                || c == ';'
        });
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            let is_tool_doc = tool_domains.iter().any(|d| trimmed.contains(d));
            if !is_tool_doc && !urls.contains(&trimmed.to_string()) {
                urls.push(trimmed.to_string());
            }
        }
    }
    urls
}

fn find_filepaths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let exts = [
        ".py", ".sh", ".exe", ".txt", ".bin", ".elf", ".json", ".toml", ".php", ".pl", ".rb",
        ".js", ".nse", ".c", ".cpp", ".go",
    ];
    for word in text.split_whitespace() {
        let trimmed = word.trim_matches(|c: char| {
            c == '"'
                || c == '\''
                || c == '<'
                || c == '>'
                || c == '('
                || c == ')'
                || c == ','
                || c == ';'
        });
        let has_ext = exts.iter().any(|ext| trimmed.ends_with(ext));
        let has_path_prefix =
            trimmed.starts_with('/') || trimmed.starts_with("./") || trimmed.starts_with("../");
        if has_ext || has_path_prefix {
            if !trimmed.starts_with("http://")
                && !trimmed.starts_with("https://")
                && !trimmed.starts_with('-')
                && find_ips(trimmed).is_empty()
            {
                let path = trimmed.to_string();
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
    }
    paths
}

fn copy_to_clipboard(text: &str) {
    use std::io::Write;
    use std::process::Stdio;

    let b64 = base64_encode(text.as_bytes());
    let osc52 = format!("\x1b]52;c;{}\x07", b64);
    let _ = std::io::stdout().write_all(osc52.as_bytes());
    let osc9 = "\x1b]9;Text copied to clipboard\x07";
    let _ = std::io::stdout().write_all(osc9.as_bytes());
    let _ = std::io::stdout().flush();

    let mut child = std::process::Command::new("xclip")
        .arg("-selection")
        .arg("clipboard")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    if child.is_err() {
        child = std::process::Command::new("xsel")
            .arg("--clipboard")
            .arg("--input")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }

    if child.is_err() {
        child = std::process::Command::new("wl-copy")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }

    if let Ok(mut process) = child {
        if let Some(mut stdin) = process.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
    }
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut buf = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i];
        let b1 = if i + 1 < data.len() { data[i + 1] } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] } else { 0 };

        let triple = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);

        buf.push(CHARS[((triple >> 18) & 63) as usize] as char);
        buf.push(CHARS[((triple >> 12) & 63) as usize] as char);
        if i + 1 < data.len() {
            buf.push(CHARS[((triple >> 6) & 63) as usize] as char);
        } else {
            buf.push('=');
        }
        if i + 2 < data.len() {
            buf.push(CHARS[(triple & 63) as usize] as char);
        } else {
            buf.push('=');
        }
        i += 3;
    }
    buf
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
        let (stdin_tx, _stdin_rx) = mpsc::channel::<String>(16);
        let mut app = App::new(job_tx, stdin_tx, StatusBar::new("gpt-4o-mini"));

        app.handle_key(key(KeyCode::Char('i')));
        app.handle_key(key(KeyCode::Char('d')));
        assert_eq!(app.input, "id");

        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.input, "", "Enter must clear the input line");
        assert!(app.running, "submitting a line marks a command running");
        let job = job_rx
            .try_recv()
            .expect("a job was submitted to the engine");
        assert_eq!(job.line, "id");

        assert!(!app.cancel.is_cancelled());
        app.handle_key(ctrl(KeyCode::Char('c')));
        assert!(
            app.cancel.is_cancelled(),
            "Ctrl+C cancels the running command"
        );
        assert!(
            job.cancel.is_cancelled(),
            "engine shares the same cancel token"
        );

        let before = app.scroll;
        app.handle_key(key(KeyCode::PageUp));
        assert!(app.scroll < before, "PageUp scrolls up");
        app.handle_key(key(KeyCode::PageDown));
        assert_eq!(app.scroll, before, "PageDown scrolls back down");

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
        assert_eq!(
            app.output.len(),
            out_len + 2,
            "each stdout line becomes a Line"
        );
        app.on_event(EngineEvent::Done);
        assert!(!app.running, "EngineEvent::Done clears the running flag");

        app.handle_key(key(KeyCode::Char('y')));
        app.handle_key(ctrl(KeyCode::Char('d')));
        assert!(app.should_quit, "Ctrl+D quits immediately, even with text");

        let (job_tx_r, _rx_r) = mpsc::channel::<Job>(16);
        let (stdin_tx_r, _stdin_rx_r) = mpsc::channel::<String>(16);
        let mut app_r = App::new(job_tx_r, stdin_tx_r, StatusBar::new("gpt-4o-mini"));
        app_r.handle_key(ctrl(KeyCode::Char('r')));
        assert!(app_r.should_reload, "Ctrl+R sets should_reload to true");

        let (job_tx2, _rx2) = mpsc::channel::<Job>(16);
        let (stdin_tx2, _stdin_rx2) = mpsc::channel::<String>(16);
        let mut app2 = App::new(job_tx2, stdin_tx2, StatusBar::new("gpt-4o-mini"));
        app2.handle_key(key(KeyCode::Esc));
        assert!(app2.should_quit, "Esc on empty input quits");
    }

    #[test]
    fn progress_event_updates_status_bar() {
        let (job_tx, _rx) = mpsc::channel::<Job>(16);
        let (stdin_tx, _stdin_rx) = mpsc::channel::<String>(16);
        let mut app = App::new(job_tx, stdin_tx, StatusBar::new("gpt-4o-mini"));
        assert_eq!(app.status.target, None);
        assert_eq!(app.status.steps, 0);

        app.on_event(EngineEvent::Progress {
            target: Some("10.0.0.5".to_string()),
            step: 1,
        });
        assert_eq!(app.status.target.as_deref(), Some("10.0.0.5"));
        assert_eq!(app.status.steps, 1);

        app.on_event(EngineEvent::Progress {
            target: None,
            step: 2,
        });
        assert_eq!(app.status.target.as_deref(), Some("10.0.0.5"));
        assert_eq!(app.status.steps, 2);
    }

    #[test]
    fn test_scraping_ips_ports_urls_filepaths() {
        let (job_tx, _rx) = mpsc::channel::<Job>(16);
        let (stdin_tx, _stdin_rx) = mpsc::channel::<String>(16);
        let mut app = App::new(job_tx, stdin_tx, StatusBar::new("gpt-4o-mini"));

        app.scrape_text("nmap -p 80,443 192.168.1.5");
        assert_eq!(app.targets.len(), 1);
        assert_eq!(app.targets[0].ip, "192.168.1.5");
        assert_eq!(app.targets[0].ports, vec![80, 443]);

        app.scrape_text("gobuster dir -u http://192.168.1.5:8080/ -w /tmp/wordlist.txt");
        assert_eq!(app.targets[0].ports, vec![80, 443, 8080]);
        assert_eq!(app.targets[0].urls, vec!["http://192.168.1.5:8080/"]);
        assert_eq!(app.targets[0].filepaths, vec!["/tmp/wordlist.txt"]);
    }

    #[test]
    fn test_clear_command_clears_output() {
        let (job_tx, _rx) = mpsc::channel::<Job>(16);
        let (stdin_tx, _stdin_rx) = mpsc::channel::<String>(16);
        let mut app = App::new(job_tx, stdin_tx, StatusBar::new("gpt-4o-mini"));

        app.output.push(Line::from("some old output"));
        app.input = "clear".to_string();
        app.submit();

        assert!(
            app.output.is_empty(),
            "clear command must clear output lines"
        );
        assert_eq!(app.scroll, 0, "clear command must reset scroll to 0");
    }

    #[test]
    fn test_tab_resolves_without_executing() {
        let (job_tx, mut job_rx) = mpsc::channel::<Job>(16);
        let (stdin_tx, _stdin_rx) = mpsc::channel::<String>(16);
        let mut app = App::new(job_tx, stdin_tx, StatusBar::new("gpt-4o-mini"));

        app.input = "python command to print 2+2".to_string();
        app.submit_resolve_only();

        let job = job_rx.try_recv().expect("resolve only job submitted");
        assert!(job.resolve_only, "job must have resolve_only flag set");
        assert_eq!(job.line, "python command to print 2+2");

        app.on_event(EngineEvent::Resolved("python3 -c 'print(2+2)'".to_string()));
        assert_eq!(app.input, "python3 -c 'print(2+2)'");
        assert!(!app.running, "running flag must be cleared");
    }

    #[test]
    fn test_stdin_sent_when_running() {
        let (job_tx, _job_rx) = mpsc::channel::<Job>(16);
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(16);
        let mut app = App::new(job_tx, stdin_tx, StatusBar::new("gpt-4o-mini"));

        app.running = true;
        app.input = "some interactive input".to_string();
        app.submit();

        assert_eq!(app.input, "");
        assert_eq!(app.cursor_pos, 0);

        let sent = stdin_rx.try_recv().expect("stdin input sent");
        assert_eq!(sent, "some interactive input\n");
    }

    #[test]
    fn test_filebar_trackpad_scroll_and_drag_swipe() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

        let (job_tx, _job_rx) = mpsc::channel::<Job>(16);
        let (stdin_tx, _stdin_rx) = mpsc::channel::<String>(16);
        let mut app = App::new(job_tx, stdin_tx, StatusBar::new("gpt-4o-mini"));
        app.filebar_row_y = 22;
        app.filebar_max_scroll = 50;

        // 1. Trackpad ScrollRight / ScrollLeft
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollRight,
            column: 10,
            row: 22,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.filebar_scroll, 6);

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollLeft,
            column: 10,
            row: 22,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.filebar_scroll, 0);

        // 2. Trackpad / wheel scroll in filebar region (row 22) scrolls filebar
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 10,
            row: 22,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.filebar_scroll, 6);

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 10,
            row: 22,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.filebar_scroll, 0);

        // 3. Alt+Right / Alt+Left keyboard scrolling
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
        assert_eq!(app.filebar_scroll, 6);

        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT));
        assert_eq!(app.filebar_scroll, 0);

        // 3. Trackpad / wheel scroll in console region (row 5) scrolls console vertically
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.scroll, 3);
        assert_eq!(app.filebar_scroll, 0);

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.scroll, 0);
        assert_eq!(app.filebar_scroll, 0);

        // 3. Click hold & swipe drag
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 30,
            row: 22,
            modifiers: KeyModifiers::NONE,
        });
        assert!(app.filebar_drag_active);

        // Drag left (col 30 -> 20, dx = +10) -> content scrolls right (+10)
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 20,
            row: 22,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.filebar_scroll, 10);

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 20,
            row: 22,
            modifiers: KeyModifiers::NONE,
        });
        assert!(!app.filebar_drag_active);
        assert_eq!(app.filebar_scroll, 10);
    }
}
