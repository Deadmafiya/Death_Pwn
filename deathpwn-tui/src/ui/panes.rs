//! Isolated widget execution engines for both upper panels and the prompt box.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::ui::filebrowser::{ClickAction, ClickItem};
use crate::ui::theme;

/// Renders the Left Column (Tactical Telemetry) container.
pub fn render_telemetry(f: &mut Frame, area: Rect, app: &mut App) {
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

    let ip_row_y = area.y + 2; // block border + blank pad
    let dir_row_y = area.y + 3;
    app.clickable_items.push(ClickItem {
        action: ClickAction::CopyToClipboard { text: app.local_ip.clone() },
        row_y: ip_row_y,
        col_range: None,
    });
    app.clickable_items.push(ClickItem {
        action: ClickAction::CopyToClipboard { text: app.current_dir.clone() },
        row_y: dir_row_y,
        col_range: None,
    });

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
            " deathPWN ",
            theme::label_style(),
        ))
        .title_alignment(Alignment::Left)
        .bg(theme::PITCH_BLACK);

    let mut render_lines = app.output.clone();

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

    let prompt_line_idx = text_height.saturating_sub(1);
    let relative_y = prompt_line_idx.saturating_sub(scroll as usize);
    if relative_y < inner_height {
        let cursor_x = area.x + 1 + 2 + app.cursor_pos as u16;
        let cursor_y = area.y + 1 + relative_y as u16;
        f.set_cursor_position((cursor_x, cursor_y));
    }
}

/// Renders the lower file browser bar showing current directory contents with icons — horizontally scrollable.
pub fn render_filebar(f: &mut Frame, area: Rect, app: &mut App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border_style())
        .title(Span::styled(
            format!("  {}  ", app.current_dir),
            theme::label_style(),
        ))
        .bg(theme::PITCH_BLACK);

    let inner = block.inner(area);
    app.filebar_origin_x = inner.x;
    app.filebar_row_y = inner.y;

    let mut row0_entries = Vec::new();
    let mut row1_entries = Vec::new();
    for (i, entry) in app.file_entries.iter().enumerate() {
        if i % 2 == 0 {
            row0_entries.push(entry);
        } else {
            row1_entries.push(entry);
        }
    }

    let mut row0_width: u16 = 0;
    for entry in &row0_entries {
        let full = format!("{} {} ", entry.icon, entry.name);
        row0_width += full.chars().count() as u16;
    }

    let mut row1_width: u16 = 0;
    for entry in &row1_entries {
        let full = format!("{} {} ", entry.icon, entry.name);
        row1_width += full.chars().count() as u16;
    }

    let total_max_width = row0_width.max(row1_width);
    let max_scroll = total_max_width.saturating_sub(inner.width);
    app.filebar_max_scroll = max_scroll;
    if app.filebar_scroll > max_scroll {
        app.filebar_scroll = max_scroll;
    }

    let scroll_offset = app.filebar_scroll;
    let visible_limit = scroll_offset + inner.width;

    let mut row0_spans: Vec<Span> = Vec::new();
    let mut current_char_idx0: u16 = 0;
    for entry in &row0_entries {
        let icon_color = if entry.is_dir {
            theme::CYBER_CYAN
        } else {
            theme::TOXIC_ACID_GREEN
        };

        let full = format!("{} {} ", entry.icon, entry.name);
        let full_chars: Vec<char> = full.chars().collect();
        let full_w = full_chars.len() as u16;

        let entry_start = current_char_idx0;
        let entry_end = current_char_idx0 + full_w;

        let path = format!("{}/{}", app.current_dir.trim_end_matches('/'), entry.name);
        let action = if entry.is_dir {
            ClickAction::NavigateDir { path }
        } else {
            ClickAction::OpenFile { path }
        };
        app.clickable_items.push(ClickItem {
            action,
            row_y: inner.y,
            col_range: Some((entry_start, entry_end.saturating_sub(1))),
        });

        if entry_end > scroll_offset && entry_start < visible_limit {
            let slice_start = if scroll_offset > entry_start {
                (scroll_offset - entry_start) as usize
            } else {
                0
            };
            let slice_end = if visible_limit < entry_end {
                (visible_limit - entry_start) as usize
            } else {
                full_chars.len()
            };

            if slice_start < slice_end && slice_start < full_chars.len() {
                let visible_str: String = full_chars[slice_start..slice_end.min(full_chars.len())].iter().collect();
                row0_spans.push(Span::styled(
                    visible_str,
                    Style::default().fg(icon_color).bg(theme::PITCH_BLACK),
                ));
            }
        }
        current_char_idx0 += full_w;
    }

    let mut row1_spans: Vec<Span> = Vec::new();
    let mut current_char_idx1: u16 = 0;
    let row1_y = inner.y + 1;

    for entry in &row1_entries {
        let icon_color = if entry.is_dir {
            theme::CYBER_CYAN
        } else {
            theme::TOXIC_ACID_GREEN
        };

        let full = format!("{} {} ", entry.icon, entry.name);
        let full_chars: Vec<char> = full.chars().collect();
        let full_w = full_chars.len() as u16;

        let entry_start = current_char_idx1;
        let entry_end = current_char_idx1 + full_w;

        let path = format!("{}/{}", app.current_dir.trim_end_matches('/'), entry.name);
        let action = if entry.is_dir {
            ClickAction::NavigateDir { path }
        } else {
            ClickAction::OpenFile { path }
        };
        app.clickable_items.push(ClickItem {
            action,
            row_y: row1_y,
            col_range: Some((entry_start, entry_end.saturating_sub(1))),
        });

        if entry_end > scroll_offset && entry_start < visible_limit {
            let slice_start = if scroll_offset > entry_start {
                (scroll_offset - entry_start) as usize
            } else {
                0
            };
            let slice_end = if visible_limit < entry_end {
                (visible_limit - entry_start) as usize
            } else {
                full_chars.len()
            };

            if slice_start < slice_end && slice_start < full_chars.len() {
                let visible_str: String = full_chars[slice_start..slice_end.min(full_chars.len())].iter().collect();
                row1_spans.push(Span::styled(
                    visible_str,
                    Style::default().fg(icon_color).bg(theme::PITCH_BLACK),
                ));
            }
        }
        current_char_idx1 += full_w;
    }

    let lines = if inner.height > 1 {
        vec![Line::from(row0_spans), Line::from(row1_spans)]
    } else {
        vec![Line::from(row0_spans)]
    };

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(theme::PITCH_BLACK));
    f.render_widget(paragraph, area);
}

/// Renders the new "Discovered Target Matrix" section below Tactical Telemetry.
pub fn render_relations(f: &mut Frame, area: Rect, app: &mut App) {
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
            app.clickable_items.push(ClickItem {
                action: ClickAction::ToggleTarget { ip: target.ip.clone() },
                row_y,
                col_range: None,
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
                        app.clickable_items.push(ClickItem {
                            action: ClickAction::CopyToClipboard {
                                text: ports_str.clone(),
                            },
                            row_y,
                            col_range: None,
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
                            app.clickable_items.push(ClickItem {
                                action: ClickAction::CopyToClipboard {
                                    text: url.clone(),
                                },
                                row_y,
                                col_range: None,
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
                            app.clickable_items.push(ClickItem {
                                action: ClickAction::CopyToClipboard {
                                    text: path.clone(),
                                },
                                row_y,
                                col_range: None,
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
