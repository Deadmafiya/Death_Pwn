### Task 5: search — SearchProvider trait + DuckDuckGo HTML scrape

**Files:**
- Create: `deathpwn-core/src/search/mod.rs` (core crate — `SearchResult`, `SearchProvider` trait, `FakeSearchProvider` test-support)
- Create: `deathpwn-core/src/search/ddg.rs` (core crate — `DuckDuckGoSearch` impl + pure `parse_ddg_html`)
- Modify: `deathpwn-core/src/lib.rs` (add `pub mod search;` + re-exports)
- Modify: `deathpwn-core/Cargo.toml` (add `scraper` dependency)
- Test: unit tests live in a `#[cfg(test)] mod tests` inside `mod.rs` (fake/trait behavior) and inside `ddg.rs` (pure parser + `#[ignore]` live network), per Rust convention.

**Interfaces:**
- Consumes (from Task 1):
  - `enum DeathpwnError { Config(String), Provider(String), Search(String), Exec(String), Schema(String), Cache(String), Io(#[from] std::io::Error), Cancelled }`
  - `type Result<T> = std::result::Result<T, DeathpwnError>;`
- Consumes (already in tree from Task 3): the `async-trait`, `reqwest` (json), `tokio` dependencies and the `test-support` feature.
- Produces (later tasks — Stage 2 `Retrieve` in Task 12 — rely on these EXACT signatures):
  - `struct SearchResult { title: String, url: String, snippet: String }`
  - `#[async_trait] trait SearchProvider: Send + Sync { async fn search(&self, query: &str) -> Result<Vec<SearchResult>>; }`
  - `struct DuckDuckGoSearch` with `fn new(client: reqwest::Client) -> Self`, `impl SearchProvider`
  - `fn parse_ddg_html(html: &str) -> Vec<SearchResult>` (pure)
  - `struct FakeSearchProvider` with `fn new(results: Vec<SearchResult>) -> Self` and `fn empty() -> Self`, gated `#[cfg(any(test, feature = "test-support"))]`, re-exported for other tasks.

---

- [ ] **Step 1: Add the `scraper` dependency and declare the module.**
  Add to `deathpwn-core/Cargo.toml` under `[dependencies]` (`reqwest`, `async-trait`, `serde_json` and the `tokio` dev-dep / `test-support` feature already exist from Task 3 — this task introduces only `scraper`):

  ```toml
  # deathpwn-core/Cargo.toml — [dependencies]
  scraper = "0.20"
  ```

  Add to `deathpwn-core/src/lib.rs` (alongside the modules declared by earlier tasks):

  ```rust
  pub mod search;

  pub use search::{SearchProvider, SearchResult};
  ```

- [ ] **Step 2: Write the failing test — `FakeSearchProvider` honors canned results.**
  Create `deathpwn-core/src/search/mod.rs` containing ONLY the test module for now (the types it references are implemented in Step 4):

  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[tokio::test]
      async fn fake_search_provider_returns_canned_results() {
          let canned = vec![
              SearchResult {
                  title: "Nmap Reference Guide".to_string(),
                  url: "https://nmap.org/book/man.html".to_string(),
                  snippet: "network discovery and security auditing".to_string(),
              },
              SearchResult {
                  title: "Nmap - Wikipedia".to_string(),
                  url: "https://en.wikipedia.org/wiki/Nmap".to_string(),
                  snippet: "network scanner".to_string(),
              },
          ];

          let fake = FakeSearchProvider::new(canned.clone());
          let got = fake.search("nmap").await.unwrap();
          assert_eq!(got, canned);
      }

      #[tokio::test]
      async fn fake_search_provider_empty_yields_no_results() {
          let fake = FakeSearchProvider::empty();
          let got = fake.search("anything").await.unwrap();
          assert!(got.is_empty(), "empty() must return zero results");
      }
  }
  ```

- [ ] **Step 3: Run the test to verify it fails.**
  Command: `cargo test -p deathpwn-core fake_search_provider`
  Expected: fails to compile — `cannot find type SearchResult in this scope`, `cannot find type FakeSearchProvider in this scope`, `SearchProvider` trait / `search` method not found.

- [ ] **Step 4: Implement `SearchResult`, the `SearchProvider` trait, and `FakeSearchProvider`.**
  Replace the contents of `deathpwn-core/src/search/mod.rs` with the full file (test module preserved at the bottom):

  ```rust
  use async_trait::async_trait;

  use crate::error::Result;

  /// A single web search result. Consumed by Stage 2 (`Retrieve`) to build the
  /// knowledge-retrieval prompt.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct SearchResult {
      pub title: String,
      pub url: String,
      pub snippet: String,
  }

  /// The single search boundary. Real impl scrapes DuckDuckGo; tests use
  /// `FakeSearchProvider`.
  #[async_trait]
  pub trait SearchProvider: Send + Sync {
      async fn search(&self, query: &str) -> Result<Vec<SearchResult>>;
  }

  /// Test-support fake: returns a fixed set of results regardless of the query.
  /// Gated so downstream tasks can enable it via the `test-support` feature.
  #[cfg(any(test, feature = "test-support"))]
  pub struct FakeSearchProvider {
      results: Vec<SearchResult>,
  }

  #[cfg(any(test, feature = "test-support"))]
  impl FakeSearchProvider {
      pub fn new(results: Vec<SearchResult>) -> Self {
          Self { results }
      }

      pub fn empty() -> Self {
          Self { results: Vec::new() }
      }
  }

  #[cfg(any(test, feature = "test-support"))]
  #[async_trait]
  impl SearchProvider for FakeSearchProvider {
      async fn search(&self, _query: &str) -> Result<Vec<SearchResult>> {
          Ok(self.results.clone())
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[tokio::test]
      async fn fake_search_provider_returns_canned_results() {
          let canned = vec![
              SearchResult {
                  title: "Nmap Reference Guide".to_string(),
                  url: "https://nmap.org/book/man.html".to_string(),
                  snippet: "network discovery and security auditing".to_string(),
              },
              SearchResult {
                  title: "Nmap - Wikipedia".to_string(),
                  url: "https://en.wikipedia.org/wiki/Nmap".to_string(),
                  snippet: "network scanner".to_string(),
              },
          ];

          let fake = FakeSearchProvider::new(canned.clone());
          let got = fake.search("nmap").await.unwrap();
          assert_eq!(got, canned);
      }

      #[tokio::test]
      async fn fake_search_provider_empty_yields_no_results() {
          let fake = FakeSearchProvider::empty();
          let got = fake.search("anything").await.unwrap();
          assert!(got.is_empty(), "empty() must return zero results");
      }
  }
  ```

- [ ] **Step 5: Run the test to verify it passes.**
  Command: `cargo test -p deathpwn-core fake_search_provider`
  Expected: PASS — `test result: ok. 2 passed; 0 failed`.

- [ ] **Step 6: Commit.**
  `git add deathpwn-core/Cargo.toml deathpwn-core/src/lib.rs deathpwn-core/src/search/mod.rs`
  `git commit -m "feat(deathpwn): add SearchProvider trait, SearchResult, and FakeSearchProvider"`

- [ ] **Step 7: Write the failing test — `parse_ddg_html` extracts title, decoded URL, and snippet from a fixture.**
  Create `deathpwn-core/src/search/ddg.rs` containing ONLY the test module for now. The fixture mirrors the real `html.duckduckgo.com/html/` markup: results wrapped in `div.result`, title in `a.result__a` whose `href` is a `//duckduckgo.com/l/?uddg=<percent-encoded-real-url>` redirect, snippet in `.result__snippet`.

  ```rust
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
  ```

- [ ] **Step 8: Run the test to verify it fails.**
  Command: `cargo test -p deathpwn-core parse_ddg_html`
  Expected: fails to compile — `cannot find function parse_ddg_html in this scope` (the `ddg` module is not yet wired into `mod.rs` and the function does not exist).

- [ ] **Step 9: Implement `ddg.rs` (parser + scraper impl) and wire it into `mod.rs`.**
  Write the full `deathpwn-core/src/search/ddg.rs` (test module from Step 7 preserved at the bottom):

  ```rust
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
  ```

  Then add these two lines near the top of `deathpwn-core/src/search/mod.rs` (below the existing `use` lines, above the `SearchResult` definition) to wire the submodule and re-export its public items:

  ```rust
  pub mod ddg;

  pub use ddg::{parse_ddg_html, DuckDuckGoSearch};
  ```

- [ ] **Step 10: Run the tests to verify they pass.**
  Command: `cargo test -p deathpwn-core parse_ddg_html`
  Expected: PASS — `test result: ok. 2 passed; 0 failed`. Also run `cargo test -p deathpwn-core search` (or the whole crate) to confirm Step 4's fake tests still pass alongside.

- [ ] **Step 11: Commit.**
  `git add deathpwn-core/src/search/ddg.rs deathpwn-core/src/search/mod.rs`
  `git commit -m "feat(deathpwn): add DuckDuckGo HTML scrape search with pure parse_ddg_html"`

- [ ] **Step 12: Add the `#[ignore]` live-network integration test.**
  Append this test to the `#[cfg(test)] mod tests` block in `deathpwn-core/src/search/ddg.rs` (it hits the real DuckDuckGo endpoint, so it must never run in the default suite):

  ```rust
      #[tokio::test]
      #[ignore = "hits the live DuckDuckGo endpoint; run manually with --ignored"]
      async fn ddg_live_search_returns_nonempty_results() {
          let client = reqwest::Client::builder()
              .user_agent("Mozilla/5.0 (X11; Linux x86_64) deathpwn/0.1")
              .build()
              .expect("build reqwest client");

          let ddg = DuckDuckGoSearch::new(client);
          let results = ddg.search("nmap port scan").await.unwrap();

          assert!(!results.is_empty(), "live DDG should return at least one result");
          assert!(
              results.iter().all(|r| !r.url.is_empty()),
              "every live result should carry a decoded url"
          );
      }
  ```

- [ ] **Step 13: Verify the ignored test compiles and is skipped by default.**
  Command: `cargo test -p deathpwn-core ddg_live_search`
  Expected: compiles cleanly; output shows `test result: ok. 0 passed; 0 failed; 1 ignored` (the test is not executed). Manual/CI-optional run: `cargo test -p deathpwn-core ddg_live_search -- --ignored`.

- [ ] **Step 14: Final commit.**
  `git add deathpwn-core/src/search/ddg.rs`
  `git commit -m "test(deathpwn): add ignored live DuckDuckGo integration test"`
