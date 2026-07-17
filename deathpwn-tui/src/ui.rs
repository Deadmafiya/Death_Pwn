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

/// Draw the whole UI: left log pane (1/5) + right terminal output (4/5),
/// status bar, and the input line.
pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // main area (log + output)
            Constraint::Length(1), // status bar
            Constraint::Length(3), // input line (bordered)
        ])
        .split(f.area());

    // Draw terminal output (with optional analysis split)
    match &app.current_render {
        Some(render) => {
            let out_panes = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(chunks[0]);
            draw_console(f, out_panes[0], app);
            render_section(f, out_panes[1], render);
        }
        None => draw_console(f, chunks[0], app),
    }

    let status_bg = if app.running {
        Color::Rgb(20, 30, 50)
    } else {
        Color::Rgb(15, 15, 25)
    };
    let status = Paragraph::new(app.status.line())
        .block(Block::default().style(Style::default().bg(status_bg)));
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
