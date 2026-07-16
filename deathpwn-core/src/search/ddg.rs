use async_trait::async_trait;
use scraper::{Html, Selector};

use crate::error::{DeathpwnError, Result};

use super::{SearchProvider, SearchResult};

const DDG_ENDPOINT: &str = "https://html.duckduckgo.com/html/";

/// DuckDuckGo HTML-scrape search provider. The caller supplies a configured
/// `reqwest::Client` (timeout + user-agent set at wiring time).
pub struct DuckDuckGoSearch {
    client: reqwest::Client,
}

impl DuckDuckGoSearch {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl SearchProvider for DuckDuckGoSearch {
    async fn search(&self, query: &str) -> Result<Vec<SearchResult>> {
        let resp = self
            .client
            .get(DDG_ENDPOINT)
            .query(&[("q", query)])
            .send()
            .await
            .map_err(|e| DeathpwnError::Search(format!("ddg request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(DeathpwnError::Search(format!("ddg http status {status}")));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| DeathpwnError::Search(format!("ddg body read failed: {e}")))?;

        Ok(parse_ddg_html(&body))
    }
}

/// Pure parser over the DuckDuckGo HTML results page. No I/O — unit-testable
/// against a fixture string.
pub fn parse_ddg_html(html: &str) -> Vec<SearchResult> {
    let doc = Html::parse_document(html);

    // Static selectors: `.parse` cannot fail for these literals.
    let result_sel = Selector::parse("div.result").expect("valid selector");
    let title_sel = Selector::parse("a.result__a").expect("valid selector");
    let snippet_sel = Selector::parse(".result__snippet").expect("valid selector");

    let mut out = Vec::new();

    for res in doc.select(&result_sel) {
        let title_el = match res.select(&title_sel).next() {
            Some(el) => el,
            None => continue,
        };

        let title = collapse_ws(&title_el.text().collect::<String>());
        let href = title_el.value().attr("href").unwrap_or("");
        let url = decode_ddg_href(href);

        let snippet = res
            .select(&snippet_sel)
            .next()
            .map(|el| collapse_ws(&el.text().collect::<String>()))
            .unwrap_or_default();

        if title.is_empty() && url.is_empty() {
            continue;
        }

        out.push(SearchResult { title, url, snippet });
    }

    out
}

/// Trim and collapse internal runs of whitespace into single spaces.
fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// DuckDuckGo wraps real URLs in a redirect:
/// `//duckduckgo.com/l/?uddg=<percent-encoded-real-url>&rut=...`.
/// Extract and percent-decode the `uddg` param; fall back to normalizing a
/// protocol-relative href, else return the href unchanged.
fn decode_ddg_href(href: &str) -> String {
    if let Some(pos) = href.find("uddg=") {
        let rest = &href[pos + "uddg=".len()..];
        let encoded = rest.split('&').next().unwrap_or(rest);
        return percent_decode(encoded);
    }

    if let Some(stripped) = href.strip_prefix("//") {
        return format!("https://{stripped}");
    }

    href.to_string()
}

/// Minimal application/x-www-form-urlencoded percent-decoder. Enough for DDG's
/// `uddg` values; avoids pulling in an extra crate.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push((hi << 4) | lo);
                    i += 3;
                }
                _ => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }

    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DDG_FIXTURE: &str = r##"<!DOCTYPE html>
<html><body>
<div class="results">
  <div class="result results_links results_links_deep web-result">
    <div class="links_main">
      <h2 class="result__title">
        <a rel="nofollow" class="result__a"
           href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fnmap.org%2Fbook%2Fman.html&amp;rut=deadbeef">Nmap Reference Guide (Man Page)</a>
      </h2>
      <a class="result__snippet"
         href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fnmap.org">Nmap is a free and open source utility for network discovery and security auditing.</a>
    </div>
  </div>
  <div class="result results_links results_links_deep web-result">
    <div class="links_main">
      <h2 class="result__title">
        <a rel="nofollow" class="result__a"
           href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fen.wikipedia.org%2Fwiki%2FNmap&amp;rut=cafe">Nmap - Wikipedia</a>
      </h2>
      <a class="result__snippet">Nmap is a network scanner created by Gordon Lyon.</a>
    </div>
  </div>
</div>
</body></html>"##;

    #[test]
    fn parse_ddg_html_extracts_results_with_decoded_urls() {
        let results = parse_ddg_html(DDG_FIXTURE);

        assert_eq!(results.len(), 2, "fixture has exactly two results");

        assert_eq!(results[0].title, "Nmap Reference Guide (Man Page)");
        assert_eq!(results[0].url, "https://nmap.org/book/man.html");
        assert!(
            results[0].snippet.contains("network discovery"),
            "snippet text should be captured, got: {:?}",
            results[0].snippet
        );

        assert_eq!(results[1].title, "Nmap - Wikipedia");
        assert_eq!(results[1].url, "https://en.wikipedia.org/wiki/Nmap");
        assert_eq!(results[1].snippet, "Nmap is a network scanner created by Gordon Lyon.");
    }

    #[test]
    fn parse_ddg_html_empty_document_yields_no_results() {
        let results = parse_ddg_html("<html><body>no results here</body></html>");
        assert!(results.is_empty());
    }
}
