//! Smart in-line highlighting for raw command output.
//!
//! Scans each line with compiled regexes to detect and colorize information-bearing
//! patterns: IP addresses, MAC addresses, ports, URLs, file paths, and status keywords.
//! Uses overlap resolution so broader patterns (URLs) consume narrower embedded
//! patterns (IPs, ports) rather than producing conflicting colored spans.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use regex::bytes::Regex as BytesRegex;
use std::sync::LazyLock;

use super::theme;

// ── Highlight styles ─────────────────────────────────────────────────

fn cyan() -> Style {
    Style::default()
        .fg(theme::PITCH_BLACK)
        .bg(theme::CYBER_CYAN)
        .add_modifier(Modifier::BOLD)
}

fn cyan_underline() -> Style {
    cyan().add_modifier(Modifier::UNDERLINED)
}

fn green() -> Style {
    Style::default()
        .fg(theme::PITCH_BLACK)
        .bg(theme::TOXIC_ACID_GREEN)
        .add_modifier(Modifier::BOLD)
}

fn red() -> Style {
    Style::default()
        .fg(theme::PITCH_BLACK)
        .bg(theme::HIGH_EXPLOSIVE_RED)
        .add_modifier(Modifier::BOLD)
}

fn purple() -> Style {
    Style::default()
        .fg(theme::PITCH_BLACK)
        .bg(theme::PURPLE_HIGHLIGHT)
        .add_modifier(Modifier::BOLD)
}

fn yellow() -> Style {
    Style::default()
        .fg(theme::PITCH_BLACK)
        .bg(theme::WARNING_YELLOW)
        .add_modifier(Modifier::BOLD)
}

// ── Debug flag ────────────────────────────────────────────────────────

/// Set to true to prepend a visible `[»]` marker on every highlighted line,
/// confirming the code path is active. Set to false for normal operation.
const DEBUG_MARKER: bool = false;

// ── Match record ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Match {
    start: usize,
    end: usize,
    style: Style,
    priority: u8,
}

// ── Compiled patterns ─────────────────────────────────────────────────

/// Priority 1 — URLs: http/https/ftp followed by non-whitespace.
static URL_RE: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(r#"(?i)\b(?:https?|ftp)://[^\s<>"{}|\\^`\[\]]+"#).unwrap()
});

/// Priority 2 — IPv6: full 8-group (7 colons) or compressed (contains ::).
static IPV6_RE: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(concat!(
        r"(?i)(?:",
        r"(?:[0-9a-f]{1,4}:){7}[0-9a-f]{1,4}",
        r"|",
        r"[0-9a-f:]*::[0-9a-f:.]*",
        r")",
        r"(?:/\d{1,3})?",
    )).unwrap()
});

/// Priority 3 — IPv4 dotted quad, optional CIDR suffix.
static IPV4_RE: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(concat!(
        r"\b(?:25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9]?[0-9])\.",
        r"(?:25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9]?[0-9])\.",
        r"(?:25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9]?[0-9])\.",
        r"(?:25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9]?[0-9])",
        r"(?:/\d{1,2})?",
    )).unwrap()
});

/// Priority 4 — MAC: colon, dash, or dot-separated hex.
static MAC_RE: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(concat!(
        r"(?i)(?:[0-9a-f]{2}[:-]){5}[0-9a-f]{2}|",
        r"(?:[0-9a-f]{4}\.){2}[0-9a-f]{4}",
    )).unwrap()
});

/// Priority 5 — File paths: starts with /, ./, ../, or ~/.
/// Excludes paths ending in a bare colon. Requires meaningful content
/// after the final slash (more than just a protocol name like /tcp).
static PATH_RE: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(r"(?:~|\.{0,2})/(?:[^\s(){}\[\]<>]+/)*[^\s(){}\[\]<>:]+").unwrap()
});

/// Priority 6 — Ports with context: 22/tcp, :443, port 80, port 22/tcp.
/// Ordered longest-first so "port 22/tcp" is one unit.
static PORT_RE: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(concat!(
        r"(?i)(?:port|s port)\s+\d{1,5}/(?:tcp|udp|sctp)\b|",
        r"(?:port|s port)\s+\d{1,5}\b|",
        r"\d{1,5}/(?:tcp|udp|sctp)\b|",
        r":\d{1,5}\b",
    )).unwrap()
});

/// Priority 7 — Good status keywords.
static STATUS_GOOD_RE: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(r"(?i)\b(open|up|success|listening|alive|running|accepted|filtered)\b").unwrap()
});

/// Priority 7 — Bad status keywords.
static STATUS_BAD_RE: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(r"(?i)\b(closed|down|failed|error|dead|stopped|refused|denied|timed\s*out|unreachable)\b").unwrap()
});

/// Priority 7 — Warning status keywords.
static STATUS_WARN_RE: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(r"(?i)\b(warning|warn|caution|deprecated|notice)\b").unwrap()
});

// ── Public API ────────────────────────────────────────────────────────

/// Highlight information-bearing patterns in a single line of command output.
///
/// `base_style` is the default style for unhighlighted portions (e.g., gray for
/// stdout, red for stderr). Banner lines should skip this function entirely.
pub fn highlight_line(text: &str, base_style: Style) -> Line<'static> {
    let mut matches: Vec<Match> = Vec::new();

    collect_matches(text, &URL_RE, cyan_underline(), 1, &mut matches);
    collect_matches(text, &IPV6_RE, cyan(), 2, &mut matches);
    collect_matches(text, &IPV4_RE, cyan(), 3, &mut matches);
    collect_matches(text, &MAC_RE, purple(), 4, &mut matches);
    collect_matches(text, &PATH_RE, green(), 5, &mut matches);
    collect_matches(text, &PORT_RE, green(), 6, &mut matches);
    collect_matches(text, &STATUS_GOOD_RE, green(), 7, &mut matches);
    collect_matches(text, &STATUS_BAD_RE, red(), 7, &mut matches);
    collect_matches(text, &STATUS_WARN_RE, yellow(), 7, &mut matches);

    if matches.is_empty() {
        if DEBUG_MARKER {
            return Line::from(vec![
                Span::styled("[»]", Style::default().fg(theme::TOXIC_ACID_GREEN)),
                Span::styled(text.to_string(), base_style),
            ]);
        }
        return Line::from(Span::styled(text.to_string(), base_style));
    }

    // Sort: start pos → priority (lower wins) → longer match first
    matches.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then_with(|| a.priority.cmp(&b.priority))
            .then_with(|| b.end.cmp(&a.end))
    });

    // Filter overlapping matches
    let mut kept: Vec<Match> = Vec::new();
    for m in matches {
        if let Some(last) = kept.last() {
            if m.start < last.end {
                continue;
            }
        }
        kept.push(m);
    }

    // Build spans: alternating base_style → highlight → base_style → ...
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut cursor = 0;

    for m in &kept {
        if m.start > cursor {
            spans.push(Span::styled(
                text[cursor..m.start].to_string(),
                base_style,
            ));
        }
        spans.push(Span::styled(text[m.start..m.end].to_string(), m.style));
        cursor = m.end;
    }

    if cursor < text.len() {
        spans.push(Span::styled(text[cursor..].to_string(), base_style));
    }

    if DEBUG_MARKER {
        let mut full = vec![Span::styled(
            "[»]",
            Style::default().fg(theme::TOXIC_ACID_GREEN),
        )];
        full.extend(spans);
        return Line::from(full);
    }

    Line::from(spans)
}

fn collect_matches(text: &str, re: &BytesRegex, style: Style, priority: u8, out: &mut Vec<Match>) {
    for m in re.find_iter(text.as_bytes()) {
        out.push(Match {
            start: m.start(),
            end: m.end(),
            style,
            priority,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn default_style() -> Style {
        Style::default()
    }

    fn span_content(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect::<String>()
    }

    fn assert_span(spans: &[Span], content: &str, bg: Color) {
        let found = spans.iter().find(|s| s.content == content);
        assert!(
            found.is_some(),
            "span with content '{content}' not found; spans: {spans:?}"
        );
        assert_eq!(found.unwrap().style.bg, Some(bg));
    }

    #[test]
    fn ipv4_highlighted() {
        let line = highlight_line("Host 192.168.1.1 is up", default_style());
        assert_span(&line.spans, "192.168.1.1", theme::CYBER_CYAN);
    }

    #[test]
    fn ipv4_with_cidr() {
        let line = highlight_line("net 10.0.0.0/24", default_style());
        assert_span(&line.spans, "10.0.0.0/24", theme::CYBER_CYAN);
    }

    #[test]
    fn ipv6_full() {
        let line = highlight_line("fe80::1ff:fe23:4567:890a", default_style());
        assert_span(&line.spans, "fe80::1ff:fe23:4567:890a", theme::CYBER_CYAN);
    }

    #[test]
    fn ipv6_loopback() {
        let line = highlight_line("Addr ::1 port 80", default_style());
        assert_span(&line.spans, "::1", theme::CYBER_CYAN);
    }

    #[test]
    fn mac_colon_format() {
        let line = highlight_line("MAC: aa:bb:cc:dd:ee:ff", default_style());
        assert_span(&line.spans, "aa:bb:cc:dd:ee:ff", theme::PURPLE_HIGHLIGHT);
    }

    #[test]
    fn mac_dash_format() {
        let line = highlight_line("00-11-22-33-44-55", default_style());
        assert_span(&line.spans, "00-11-22-33-44-55", theme::PURPLE_HIGHLIGHT);
    }

    #[test]
    fn mac_dot_format() {
        let line = highlight_line("aabb.ccdd.eeff", default_style());
        assert_span(&line.spans, "aabb.ccdd.eeff", theme::PURPLE_HIGHLIGHT);
    }

    #[test]
    fn mac_not_confused_with_ipv6() {
        let line = highlight_line("ether aa:bb:cc:dd:ee:ff", default_style());
        assert_span(&line.spans, "aa:bb:cc:dd:ee:ff", theme::PURPLE_HIGHLIGHT);
        let cyan_spans: Vec<_> = line
            .spans
            .iter()
            .filter(|s| s.style.bg == Some(theme::CYBER_CYAN))
            .collect();
        assert!(
            cyan_spans.is_empty(),
            "MAC incorrectly matched as IPv6: {line:?}"
        );
    }

    #[test]
    fn url_highlighted() {
        let line = highlight_line("http://example.com/path", default_style());
        assert_span(&line.spans, "http://example.com/path", theme::CYBER_CYAN);
    }

    #[test]
    fn url_has_underline() {
        let line = highlight_line("http://x.com", default_style());
        let url_span = line
            .spans
            .iter()
            .find(|s| s.content == "http://x.com")
            .unwrap();
        assert!(url_span.style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn url_consumes_embedded_ip() {
        let line = highlight_line("Scan http://10.0.0.1:8080/ now", default_style());
        assert_span(&line.spans, "http://10.0.0.1:8080/", theme::CYBER_CYAN);
        let ip_spans: Vec<_> = line
            .spans
            .iter()
            .filter(|s| s.content == "10.0.0.1")
            .collect();
        assert!(ip_spans.is_empty(), "IPv4 leaked inside URL: {line:?}");
    }

    #[test]
    fn port_with_slash() {
        let line = highlight_line("22/tcp open", default_style());
        assert_span(&line.spans, "22/tcp", theme::TOXIC_ACID_GREEN);
    }

    #[test]
    fn port_with_colon() {
        let line = highlight_line("listen :8080", default_style());
        assert_span(&line.spans, ":8080", theme::TOXIC_ACID_GREEN);
    }

    #[test]
    fn port_with_keyword() {
        let line = highlight_line("on port 443", default_style());
        assert_span(&line.spans, "port 443", theme::TOXIC_ACID_GREEN);
    }

    #[test]
    fn port_with_protocol_after_keyword() {
        let line = highlight_line("listening on port 22/tcp", default_style());
        assert_span(&line.spans, "port 22/tcp", theme::TOXIC_ACID_GREEN);
    }

    #[test]
    fn bare_number_not_highlighted_as_port() {
        let line = highlight_line("pid 443 started", default_style());
        let port_spans: Vec<_> = line
            .spans
            .iter()
            .filter(|s| s.content.contains("443"))
            .collect();
        assert!(
            port_spans.is_empty()
                || port_spans
                    .iter()
                    .all(|s| s.style.bg != Some(theme::TOXIC_ACID_GREEN)),
            "bare 443 incorrectly highlighted: {line:?}"
        );
    }

    #[test]
    fn file_absolute_path() {
        let line = highlight_line("found /usr/bin/nmap", default_style());
        assert_span(&line.spans, "/usr/bin/nmap", theme::TOXIC_ACID_GREEN);
    }

    #[test]
    fn file_relative_path() {
        let line = highlight_line("run ./script.sh", default_style());
        assert_span(&line.spans, "./script.sh", theme::TOXIC_ACID_GREEN);
    }

    #[test]
    fn file_home_path() {
        let line = highlight_line("config ~/.config/hack", default_style());
        assert_span(&line.spans, "~/.config/hack", theme::TOXIC_ACID_GREEN);
    }

    #[test]
    fn status_open_green() {
        let line = highlight_line("22/tcp open", default_style());
        assert_span(&line.spans, "open", theme::TOXIC_ACID_GREEN);
    }

    #[test]
    fn status_closed_red() {
        let line = highlight_line("80/tcp closed", default_style());
        assert_span(&line.spans, "closed", theme::HIGH_EXPLOSIVE_RED);
    }

    #[test]
    fn status_filtered_green() {
        let line = highlight_line("443/tcp filtered", default_style());
        assert_span(&line.spans, "filtered", theme::TOXIC_ACID_GREEN);
    }

    #[test]
    fn status_warning_yellow() {
        let line = highlight_line("warning: unsafe", default_style());
        assert_span(&line.spans, "warning", theme::WARNING_YELLOW);
    }

    #[test]
    fn status_word_boundary() {
        let line = highlight_line("reopened", default_style());
        let open_spans: Vec<_> = line.spans.iter().filter(|s| s.content == "open").collect();
        assert!(
            open_spans.is_empty(),
            "'reopened' incorrectly matched 'open': {line:?}"
        );
    }

    #[test]
    fn no_matches_single_span() {
        let line = highlight_line("nothing interesting here", default_style());
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "nothing interesting here");
    }

    #[test]
    fn empty_line_single_span() {
        let line = highlight_line("", default_style());
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "");
    }

    #[test]
    fn multiple_matches_non_overlapping() {
        let line = highlight_line(
            "Host 10.0.0.1 MAC aa:bb:cc:dd:ee:ff port 22/tcp open",
            default_style(),
        );
        assert_span(&line.spans, "10.0.0.1", theme::CYBER_CYAN);
        assert_span(&line.spans, "aa:bb:cc:dd:ee:ff", theme::PURPLE_HIGHLIGHT);
        assert_span(&line.spans, "port 22/tcp", theme::TOXIC_ACID_GREEN);
        assert_span(&line.spans, "open", theme::TOXIC_ACID_GREEN);
    }

    #[test]
    fn span_content_reconstructs_original() {
        let original = "Host 192.168.1.1 is up and http://10.0.0.1:8080/path works";
        let line = highlight_line(original, default_style());
        assert_eq!(span_content(&line.spans), original);
    }

    #[test]
    fn ipv6_consumes_inner_ipv4() {
        let line = highlight_line("::ffff:192.168.1.1", default_style());
        assert_span(&line.spans, "::ffff:192.168.1.1", theme::CYBER_CYAN);
    }

    #[test]
    fn base_style_preserved_on_unmatched_portions() {
        let base = Style::default().fg(Color::Gray);
        let line = highlight_line("prefix 10.0.0.1 suffix", base);
        assert_eq!(line.spans[0].content, "prefix ");
        assert_eq!(line.spans[0].style.fg, Some(Color::Gray));
        assert_eq!(line.spans[2].content, " suffix");
        assert_eq!(line.spans[2].style.fg, Some(Color::Gray));
    }
}
