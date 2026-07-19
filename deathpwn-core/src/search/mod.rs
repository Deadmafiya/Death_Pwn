use async_trait::async_trait;

use crate::error::Result;

pub mod ddg;

pub use ddg::{parse_ddg_html, DuckDuckGoSearch};

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
        Self {
            results: Vec::new(),
        }
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
