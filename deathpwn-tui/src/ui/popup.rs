//! Popup file editor with undo/redo, save/exit, and mouse support.

use std::io;

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::ui::theme;

pub enum ArrowDir {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone)]
pub struct PopupState {
    pub file_path: String,
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub scroll_row: usize,
    pub scroll_col: usize,
    pub dirty: bool,
    pub undo_stack: Vec<Vec<String>>,
    pub redo_stack: Vec<Vec<String>>,
}

pub fn popup_area(full: Rect) -> Rect {
    let w = ((full.width as f32) * 0.85_f32) as u16;
    let h = ((full.height as f32) * 0.85_f32) as u16;
    let x = full.x + (full.width.saturating_sub(w)) / 2;
    let y = full.y + (full.height.saturating_sub(h)) / 2;
    Rect { x, y, width: w.max(20), height: h.max(10) }
}

pub fn button_area(popup: Rect) -> Rect {
    Rect {
        x: popup.x + 2,
        y: popup.y + popup.height.saturating_sub(5),
        width: popup.width.saturating_sub(4),
        height: 3,
    }
}

pub fn text_area(popup: Rect) -> Rect {
    Rect {
        x: popup.x + 2,
        y: popup.y + 2,
        width: popup.width.saturating_sub(4),
        height: popup.height.saturating_sub(7),
    }
}

pub fn load_file(path: &str) -> io::Result<PopupState> {
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<String> = if content.is_empty() {
        vec![String::new()]
    } else {
        content.lines().map(|l| l.to_string()).collect()
    };
    Ok(PopupState {
        file_path: path.to_string(),
        lines,
        cursor_row: 0,
        cursor_col: 0,
        scroll_row: 0,
        scroll_col: 0,
        dirty: false,
        undo_stack: Vec::new(),
        redo_stack: Vec::new(),
    })
}

pub fn snapshot_before(popup: &mut PopupState) {
    popup.undo_stack.push(popup.lines.clone());
    popup.redo_stack.clear();
    if popup.undo_stack.len() > 256 {
        popup.undo_stack.remove(0);
    }
}

pub fn insert_char(popup: &mut PopupState, ch: char) {
    snapshot_before(popup);
    if popup.cursor_col > popup.lines[popup.cursor_row].len() {
        popup.cursor_col = popup.lines[popup.cursor_row].len();
    }
    popup.lines[popup.cursor_row].insert(popup.cursor_col, ch);
    popup.cursor_col = popup.cursor_col.saturating_add(1);
    popup.dirty = true;
}

pub fn backspace(popup: &mut PopupState) {
    snapshot_before(popup);
    if popup.cursor_col > 0 {
        let col = popup.cursor_col - 1;
        popup.lines[popup.cursor_row].remove(col);
        popup.cursor_col = col;
    } else if popup.cursor_row > 0 {
        let rest = std::mem::take(&mut popup.lines[popup.cursor_row]);
        popup.lines.remove(popup.cursor_row);
        let prev_len = popup.lines[popup.cursor_row - 1].len();
        popup.lines[popup.cursor_row - 1].push_str(&rest);
        popup.cursor_row -= 1;
        popup.cursor_col = prev_len;
    }
    popup.dirty = true;
}

pub fn delete_char(popup: &mut PopupState) {
    snapshot_before(popup);
    let line = &mut popup.lines[popup.cursor_row];
    if popup.cursor_col < line.len() {
        line.remove(popup.cursor_col);
    } else if popup.cursor_row + 1 < popup.lines.len() {
        let next = popup.lines.remove(popup.cursor_row + 1);
        popup.lines[popup.cursor_row].push_str(&next);
    }
    popup.dirty = true;
}

pub fn newline(popup: &mut PopupState) {
    snapshot_before(popup);
    let rest: String = popup.lines[popup.cursor_row]
        .drain(popup.cursor_col..)
        .collect();
    popup.lines.insert(popup.cursor_row + 1, rest);
    popup.cursor_row += 1;
    popup.cursor_col = 0;
    popup.dirty = true;
}

pub fn move_cursor(popup: &mut PopupState, dir: ArrowDir) {
    match dir {
        ArrowDir::Left => {
            if popup.cursor_col > 0 {
                popup.cursor_col -= 1;
            } else if popup.cursor_row > 0 {
                popup.cursor_row -= 1;
                popup.cursor_col = popup.lines[popup.cursor_row].len();
            }
        }
        ArrowDir::Right => {
            if popup.cursor_col < popup.lines[popup.cursor_row].len() {
                popup.cursor_col += 1;
            } else if popup.cursor_row + 1 < popup.lines.len() {
                popup.cursor_row += 1;
                popup.cursor_col = 0;
            }
        }
        ArrowDir::Up => {
            popup.cursor_row = popup.cursor_row.saturating_sub(1);
            if popup.cursor_col > popup.lines[popup.cursor_row].len() {
                popup.cursor_col = popup.lines[popup.cursor_row].len();
            }
        }
        ArrowDir::Down => {
            if popup.cursor_row + 1 < popup.lines.len() {
                popup.cursor_row += 1;
                if popup.cursor_col > popup.lines[popup.cursor_row].len() {
                    popup.cursor_col = popup.lines[popup.cursor_row].len();
                }
            }
        }
    }
    clamp_scroll(popup);
}

fn clamp_scroll(popup: &mut PopupState) {
    if popup.cursor_row < popup.scroll_row {
        popup.scroll_row = popup.cursor_row;
    }
    let visible_lines = 10usize;
    if popup.cursor_row >= popup.scroll_row + visible_lines {
        popup.scroll_row = popup.cursor_row.saturating_sub(visible_lines.saturating_sub(1));
    }
    if popup.cursor_col < popup.scroll_col {
        popup.scroll_col = popup.cursor_col;
    }
    let visible_cols = 40usize;
    if popup.cursor_col >= popup.scroll_col + visible_cols {
        popup.scroll_col = popup.cursor_col.saturating_sub(visible_cols.saturating_sub(1));
    }
}

pub fn undo(popup: &mut PopupState) {
    if let Some(prev) = popup.undo_stack.pop() {
        popup.redo_stack.push(std::mem::replace(&mut popup.lines, prev));
        popup.dirty = true;
        clamp_cursor(popup);
    }
}

pub fn redo(popup: &mut PopupState) {
    if let Some(next) = popup.redo_stack.pop() {
        popup.undo_stack.push(std::mem::replace(&mut popup.lines, next));
        popup.dirty = true;
        clamp_cursor(popup);
    }
}

fn clamp_cursor(popup: &mut PopupState) {
    if popup.lines.is_empty() {
        popup.lines.push(String::new());
    }
    if popup.cursor_row >= popup.lines.len() {
        popup.cursor_row = popup.lines.len().saturating_sub(1);
    }
    let line_len = popup.lines[popup.cursor_row].len();
    if popup.cursor_col > line_len {
        popup.cursor_col = line_len;
    }
    clamp_scroll(popup);
}

pub fn save(popup: &PopupState) -> io::Result<()> {
    std::fs::write(&popup.file_path, popup.lines.join("\n") + "\n")
}

pub fn render_popup(f: &mut Frame, popup_rect: Rect, popup: &PopupState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::CYBER_CYAN))
        .title(Span::styled(
            format!(" EDITING: {} ", popup.file_path),
            Style::default().fg(theme::CYBER_CYAN).add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center)
        .bg(Color::Rgb(8, 8, 16));

    let bg = Color::Rgb(8, 8, 16);
    let fg = Color::Rgb(216, 216, 216);

    let ta = text_area(popup_rect);
    let visible_h = ta.height as usize;

    let mut text_lines: Vec<Line> = Vec::new();
    for i in 0..visible_h {
        let line_idx = popup.scroll_row + i;
        if line_idx >= popup.lines.len() {
            text_lines.push(Line::from(Span::styled("~", Style::default().fg(Color::Rgb(60, 60, 60)))));
        } else {
            let content = &popup.lines[line_idx];
            let col_start = popup.scroll_col.min(content.len());
            let visible = &content[col_start..];
            let line_num = format!("{:>4} ", line_idx + 1);
            let line_style = if line_idx == popup.cursor_row {
                Style::default().fg(fg).bg(Color::Rgb(48, 48, 64))
            } else {
                Style::default().fg(fg)
            };
            text_lines.push(Line::from(vec![
                Span::styled(line_num, Style::default().fg(Color::Rgb(80, 80, 80))),
                Span::styled(visible.to_string(), line_style),
            ]));
        }
    }

    let text_para = Paragraph::new(text_lines).bg(bg);
    f.render_widget(text_para, ta);

    // Buttons
    let ba = button_area(popup_rect);
    let btn_bg = Color::Rgb(32, 32, 48);
    let btn_active = Style::default().fg(Color::Rgb(0, 215, 255)).bg(btn_bg).add_modifier(Modifier::BOLD);

    let dirty_str = if popup.dirty { " [modified] " } else { "" };

    let buttons = Line::from(vec![
        Span::styled("[Save] ", btn_active),
        Span::styled("[Exit] ", btn_active),
        Span::styled("[Undo] ", btn_active),
        Span::styled("[Redo] ", btn_active),
        Span::styled(dirty_str, Style::default().fg(Color::Rgb(255, 180, 0)).bg(btn_bg)),
    ]);

    let status = Line::from(Span::styled(
        format!("Ln {}, Col {}  |  Ctrl+S:Save  Ctrl+Z:Undo  Ctrl+Y:Redo  Esc:Exit",
            popup.cursor_row + 1, popup.cursor_col + 1),
        Style::default().fg(Color::Rgb(100, 100, 100)).bg(btn_bg),
    ));

    let bar_para = Paragraph::new(vec![buttons, status]).block(
        Block::default().bg(btn_bg).borders(Borders::TOP).border_style(Style::default().fg(Color::Rgb(60, 60, 60)))
    );
    f.render_widget(bar_para, ba);

    let block_para = Paragraph::new("").block(block.clone()).bg(bg);
    f.render_widget(block_para, popup_rect);

    // Cursor
    let cursor_line_idx = popup.cursor_row.saturating_sub(popup.scroll_row);
    if cursor_line_idx < visible_h {
        let cursor_screen_col = popup.cursor_col.saturating_sub(popup.scroll_col);
        let cx = ta.x + 5 + cursor_screen_col as u16;
        let cy = ta.y + cursor_line_idx as u16;
        if cx < ta.x + ta.width && cy < ta.y + ta.height {
            f.set_cursor_position((cx, cy));
        }
    }
}

pub fn popup_hit_test(popup: &PopupState, popup_rect: Rect, row: u16, col: u16) -> PopupHit {
    let ba = button_area(popup_rect);
    if row == ba.y + 1 {
        let base_col = popup_rect.x + 2;
        if col >= base_col && col < base_col + 7 { return PopupHit::Save; }
        let base_col = base_col + 8;
        if col >= base_col && col < base_col + 7 { return PopupHit::Exit; }
        let base_col = base_col + 8;
        if col >= base_col && col < base_col + 7 { return PopupHit::Undo; }
        let base_col = base_col + 8;
        if col >= base_col && col < base_col + 7 { return PopupHit::Redo; }
    }
    let ta = text_area(popup_rect);
    if col >= ta.x && col < ta.x + ta.width && row >= ta.y && row < ta.y + ta.height {
        let rel_row = (row - ta.y) as usize + popup.scroll_row;
        let rel_col = (col.saturating_sub(ta.x + 5) as usize) + popup.scroll_col;
        return PopupHit::Text { row: rel_row.min(popup.lines.len().saturating_sub(1)), col: rel_col };
    }
    PopupHit::None
}

#[derive(Debug, Clone)]
pub enum PopupHit {
    Save,
    Exit,
    Undo,
    Redo,
    Text { row: usize, col: usize },
    None,
}
