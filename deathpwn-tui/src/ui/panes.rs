//! Isolated widget execution engines for both upper panels and the prompt box.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::ui::theme;

/// Renders the Left Column (Tactical Telemetry) container.
pub fn render_telemetry(f: &mut Frame, area: Rect, app: &App) {
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
            Span::styled(" IP       │ ", theme::label_style()),
            Span::styled(&app.local_ip, Style::default().fg(theme::TERMINAL_SILVER)),
        ]),
        Line::from(vec![
            Span::styled(" DIR      │ ", theme::label_style()),
            Span::styled(
                &app.current_dir,
                Style::default().fg(theme::TERMINAL_SILVER),
            ),
        ]),
        Line::from(vec![
            Span::styled(" ENGINE   │ ", theme::label_style()),
            Span::styled(
                app.status.provider.clone(),
                Style::default().fg(theme::TERMINAL_SILVER),
            ),
        ]),
        Line::from(vec![
            Span::styled(" STATUS   │ ", theme::label_style()),
            Span::styled(
                status_text,
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
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
        .title(Span::styled(
            " INTERACTIVE TERMINAL CONSOLE ",
            theme::label_style(),
        ))
        .title_alignment(Alignment::Left)
        .bg(theme::PITCH_BLACK);

    let mut render_lines = app.output.clone();

    // Add the active prompt line at the bottom
    let prompt_line = Line::from(vec![
        Span::styled(
            "> ",
            Style::default()
                .fg(theme::TOXIC_ACID_GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(app.input.clone()),
    ]);
    render_lines.push(prompt_line);

    let text_height = render_lines.len();
    let inner_height = area.height.saturating_sub(2) as usize;

    let scroll = if text_height > inner_height {
        let max_scroll = (text_height - inner_height) as u16;
        app.scroll.min(max_scroll)
    } else {
        0
    };

    let paragraph = Paragraph::new(render_lines)
        .block(block)
        .style(theme::text_style())
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    f.render_widget(paragraph, area);

    if text_height > 0 {
        let prompt_line_idx = text_height - 1;
        let relative_y = prompt_line_idx.saturating_sub(scroll as usize);
        if relative_y < inner_height {
            let cursor_x = area.x + 1 + 2 + app.cursor_pos as u16;
            let cursor_y = area.y + 1 + relative_y as u16;
            f.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

/// Renders the lower terminal input row. Sets cursor position for the blinking cursor.
pub fn render_input(f: &mut Frame, area: Rect, _app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border_style())
        .title(Span::styled(
            " COMMAND INTERACTION ENTRY ",
            theme::label_style(),
        ))
        .bg(theme::PITCH_BLACK);

    let paragraph = Paragraph::new("").block(block);
    f.render_widget(paragraph, area);
}

/// Renders the new "Discovered Target Matrix" section below Tactical Telemetry.
pub fn render_relations(f: &mut Frame, area: Rect, app: &mut App) {
    app.clickable_items.clear();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border_style())
        .title(Span::styled(
            " DISCOVERED TARGET MATRIX ",
            theme::label_style(),
        ))
        .title_alignment(Alignment::Left)
        .bg(theme::PITCH_BLACK);

    let mut lines = Vec::new();
    lines.push(Line::from("")); // padding

    if app.targets.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "  No targets discovered yet.",
            Style::default().fg(Color::DarkGray),
        )]));
    } else {
        let mut current_line_offset = 1;

        for target in &app.targets {
            let row_y = area.y + 1 + current_line_offset;
            app.clickable_items.push(crate::app::MatrixClickItem {
                text_to_copy: target.ip.clone(),
                target_ip: Some(target.ip.clone()),
                row_y,
            });

            let arrow = if target.expanded { "▼ " } else { "▶ " };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {}", arrow),
                    Style::default()
                        .fg(theme::TOXIC_ACID_GREEN)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    target.ip.clone(),
                    Style::default()
                        .fg(theme::TERMINAL_SILVER)
                        .add_modifier(Modifier::UNDERLINED),
                ),
            ]));
            current_line_offset += 1;

            if target.expanded {
                let has_related = !target.ports.is_empty()
                    || !target.urls.is_empty()
                    || !target.filepaths.is_empty();
                if !has_related {
                    lines.push(Line::from(vec![Span::styled(
                        "    nothing related have dicovered.",
                        Style::default().fg(Color::DarkGray),
                    )]));
                    current_line_offset += 1;
                } else {
                    if !target.ports.is_empty() {
                        let ports_str = target
                            .ports
                            .iter()
                            .map(|p| p.to_string())
                            .collect::<Vec<_>>()
                            .join(", ");
                        let row_y = area.y + 1 + current_line_offset;
                        app.clickable_items.push(crate::app::MatrixClickItem {
                            text_to_copy: ports_str.clone(),
                            target_ip: None,
                            row_y,
                        });
                        lines.push(Line::from(vec![
                            Span::styled("    PORTS: ", Style::default().fg(theme::CYBER_CYAN)),
                            Span::styled(ports_str, Style::default().fg(theme::TERMINAL_SILVER)),
                        ]));
                        current_line_offset += 1;
                    }
                    if !target.urls.is_empty() {
                        lines.push(Line::from(vec![Span::styled(
                            "    URLs:",
                            Style::default().fg(theme::CYBER_CYAN),
                        )]));
                        current_line_offset += 1;
                        for url in &target.urls {
                            let row_y = area.y + 1 + current_line_offset;
                            app.clickable_items.push(crate::app::MatrixClickItem {
                                text_to_copy: url.clone(),
                                target_ip: None,
                                row_y,
                            });
                            lines.push(Line::from(vec![Span::styled(
                                format!("      - {}", url),
                                Style::default().fg(theme::TERMINAL_SILVER),
                            )]));
                            current_line_offset += 1;
                        }
                    }
                    if !target.filepaths.is_empty() {
                        lines.push(Line::from(vec![Span::styled(
                            "    PAYLOADS:",
                            Style::default().fg(theme::CYBER_CYAN),
                        )]));
                        current_line_offset += 1;
                        for path in &target.filepaths {
                            let row_y = area.y + 1 + current_line_offset;
                            app.clickable_items.push(crate::app::MatrixClickItem {
                                text_to_copy: path.clone(),
                                target_ip: None,
                                row_y,
                            });
                            lines.push(Line::from(vec![Span::styled(
                                format!("      - {}", path),
                                Style::default().fg(theme::TERMINAL_SILVER),
                            )]));
                            current_line_offset += 1;
                        }
                    }
                }
            }
        }
    }

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);
}
