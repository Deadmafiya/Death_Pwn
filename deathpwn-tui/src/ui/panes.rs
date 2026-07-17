//! Isolated widget execution engines for both upper panels and the prompt box.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use ratatui::style::Stylize;

use crate::app::App;
use crate::ui::theme;

/// Renders the Left Column (Tactical Telemetry) container.
pub fn render_telemetry(f: &mut Frame, area: Rect, app: &App) {
    let target = app.status.target.as_deref().unwrap_or("-");
    let steps = app.status.steps.to_string();

    let status_color = if app.running {
        theme::TOXIC_ACID_GREEN
    } else {
        theme::MATTE_OBSIDIAN
    };

    let spinner_frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let status_text = if app.running {
        let frame = spinner_frames[app.status.spinner_tick % spinner_frames.len()];
        format!("{} {}", frame, app.status.phase.label().to_uppercase())
    } else {
        "◼ IDLE".to_string()
    };

    let telemetry_lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(" TARGET   │ ", theme::label_style()),
            Span::styled(target, Style::default().fg(theme::TERMINAL_SILVER)),
        ]),
        Line::from(vec![
            Span::styled(" ENGINE   │ ", theme::label_style()),
            Span::styled(app.status.provider.clone(), Style::default().fg(theme::TERMINAL_SILVER)),
        ]),
        Line::from(vec![
            Span::styled(" STEPS    │ ", theme::label_style()),
            Span::styled(steps, Style::default().fg(theme::TERMINAL_SILVER)),
        ]),
        Line::from(vec![
            Span::styled(" STATUS   │ ", theme::label_style()),
            Span::styled(status_text, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border_style())
        .title(Span::styled(" TACTICAL TELEMETRY ", theme::label_style()))
        .title_alignment(Alignment::Left)
        .bg(theme::PITCH_BLACK);

    let paragraph = Paragraph::new(telemetry_lines).block(block);
    f.render_widget(paragraph, area);
}

/// Renders the Right Column (Live Output Console) container.
pub fn render_console(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border_style())
        .title(Span::styled(" LIVE OUTPUT CONSOLE ", theme::label_style()))
        .title_alignment(Alignment::Left)
        .bg(theme::PITCH_BLACK);

    let paragraph = Paragraph::new(app.output.clone())
        .block(block)
        .style(theme::text_style())
        .wrap(Wrap { trim: false })
        .scroll((app.scroll, 0));

    f.render_widget(paragraph, area);
}

/// Renders the lower terminal input row. Sets cursor position for the blinking cursor.
pub fn render_input(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border_style())
        .title(Span::styled(" COMMAND INTERACTION ENTRY ", theme::label_style()))
        .bg(theme::PITCH_BLACK);

    let input_line = Line::from(vec![
        Span::styled("> ", Style::default().fg(theme::TOXIC_ACID_GREEN).add_modifier(Modifier::BOLD)),
        Span::raw(app.input.clone()),
    ]);

    let paragraph = Paragraph::new(input_line).block(block);
    f.render_widget(paragraph, area);

    let cursor_x = area.x + 1 + 2 + app.cursor_pos as u16;
    let cursor_y = area.y + 1;
    f.set_cursor_position((cursor_x, cursor_y));
}
