//! Core orchestration layer managing terminal frames, 2:3 screen distributions,
//! and parsing structures for chronological inline text mapping.

use deathpwn_core::schema::{RenderBody, Stage4Render};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::app::App;

pub mod panes;
pub mod theme;

/// Builds layout constraints and draws nested widgets sequentially.
pub fn draw(f: &mut Frame, app: &mut App) {
    let screen_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // Main workspace
            Constraint::Length(3), // Input boundary block
        ])
        .split(f.area());

    // Enforce exact layout constraint: Upper large boxes split at a clean 3:2 spatial ratio.
    let upper_workspace_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(3, 5), // Left: Live Output Console (60%)
            Constraint::Ratio(2, 5), // Right: Tactical Telemetry (40%)
        ])
        .split(screen_chunks[0]);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7), // Tactical Telemetry
            Constraint::Min(1),    // Discovered Target Matrix
        ])
        .split(upper_workspace_chunks[1]);

    panes::render_console(f, upper_workspace_chunks[0], app);
    panes::render_telemetry(f, right_chunks[0], app);
    panes::render_relations(f, right_chunks[1], app);
    panes::render_input(f, screen_chunks[1], app);
}

/// Maps engine analysis configurations into text lines for chronological output stream ingestion.
pub fn stage4_to_lines(render: &Stage4Render) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for section in &render.sections {
        lines.push(Line::from(Span::styled(
            format!("// ANALYSIS SECTION: {}", section.title.to_uppercase()),
            Style::default()
                .fg(theme::CYBER_CYAN)
                .add_modifier(Modifier::BOLD),
        )));

        match &section.body {
            RenderBody::Text(text) => {
                for l in text.lines() {
                    lines.push(Line::from(Span::styled(l.to_string(), theme::text_style())));
                }
            }
            RenderBody::KeyValue(pairs) => {
                for (k, v) in pairs {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {}: ", k), Style::default().fg(theme::CYBER_CYAN)),
                        Span::styled(v.clone(), theme::text_style()),
                    ]));
                }
            }
            RenderBody::Table { headers, rows } => {
                lines.push(Line::from(Span::styled(
                    format!("  {}", headers.join(" │ ")),
                    Style::default()
                        .fg(theme::MATTE_OBSIDIAN)
                        .add_modifier(Modifier::BOLD),
                )));
                for row in rows {
                    lines.push(Line::from(Span::styled(
                        format!("  {}", row.join(" │ ")),
                        theme::text_style(),
                    )));
                }
            }
            RenderBody::Findings(items) => {
                for item in items {
                    let color = match item.severity.to_ascii_lowercase().as_str() {
                        "critical" | "high" => theme::HIGH_EXPLOSIVE_RED,
                        "medium" => ratatui::style::Color::Yellow,
                        "low" => theme::TOXIC_ACID_GREEN,
                        _ => theme::CYBER_CYAN,
                    };

                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("  [{}] ", item.severity.to_uppercase()),
                            Style::default().fg(color).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(item.title.clone(), Style::default().fg(color)),
                    ]));

                    if !item.detail.is_empty() {
                        lines.push(Line::from(Span::styled(
                            format!("    └─ {}", item.detail),
                            theme::text_style(),
                        )));
                    }
                }
            }
        }
        lines.push(Line::from(""));
    }
    lines
}
