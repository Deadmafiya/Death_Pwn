//! System-wide color parameters and element styles reflecting the
//! BLACKARCH_VOID core hacker-terminal theme.

use ratatui::style::{Color, Modifier, Style};

// Raw Palette Hex Constants
pub const PITCH_BLACK: Color = Color::Rgb(0, 0, 0);
pub const MATTE_OBSIDIAN: Color = Color::Rgb(38, 38, 38); // #262626
pub const TOXIC_ACID_GREEN: Color = Color::Rgb(0, 255, 102); // #00FF66
pub const CYBER_CYAN: Color = Color::Rgb(0, 215, 255); // #00D7FF
pub const TERMINAL_SILVER: Color = Color::Rgb(216, 216, 216); // #D8D8D8
pub const HIGH_EXPLOSIVE_RED: Color = Color::Rgb(255, 51, 51); // #FF3333

/// Returns the operational terminal theme settings for window frameworks.
pub fn border_style() -> Style {
    Style::default().fg(MATTE_OBSIDIAN).bg(PITCH_BLACK)
}

/// Base styling applied directly to standard stdout output blocks.
pub fn text_style() -> Style {
    Style::default().fg(TERMINAL_SILVER).bg(PITCH_BLACK)
}

/// Highlight rule used to isolate operational metadata keys.
pub fn label_style() -> Style {
    Style::default().fg(CYBER_CYAN).add_modifier(Modifier::BOLD)
}
