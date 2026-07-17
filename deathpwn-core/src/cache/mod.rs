//! Plan cache: exact normalized-key lookup for Stage 3 plans.
//!
//! Key = `normalize_intent(intent) + "|" + normalize_params(params)`.
//! Equivalent phrasings collide; different parameters never do.

use std::collections::{BTreeMap, HashMap};

use crate::schema::{IntentParams, Stage3Plan};

/// Lowercase, trim, and collapse internal whitespace runs to single spaces.
pub fn normalize_intent(intent: &str) -> String {
    intent
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Deterministic, sorted `key=value` encoding of the intent parameters.
///
/// `target`/`ports`/`url` are emitted as fixed keys when present; every entry
/// in `extra` is included. Keys are sorted (via `BTreeMap`) so equivalent
/// parameter sets encode identically, and values are normalized like the
/// intent so casing/whitespace differences collide.
pub fn normalize_params(p: &IntentParams) -> String {
    let mut parts: BTreeMap<String, String> = BTreeMap::new();
    if let Some(target) = &p.target {
        parts.insert("target".to_string(), normalize_intent(target));
    }
    if let Some(ports) = &p.ports {
        parts.insert("ports".to_string(), normalize_intent(ports));
    }
    if let Some(url) = &p.url {
        parts.insert("url".to_string(), normalize_intent(url));
    }
    for (k, v) in &p.extra {
        parts.insert(normalize_intent(k), normalize_intent(v));
    }
    parts
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// In-memory exact-match cache of Stage 3 plans, keyed by normalized
/// intent + params. No embeddings; a hit requires intent AND params to match.
#[derive(Debug, Default)]
pub struct PlanCache {
    map: HashMap<String, Stage3Plan>,
}

impl PlanCache {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Compose the cache key: `normalize_intent + "|" + normalize_params`.
    pub fn key(intent: &str, params: &IntentParams) -> String {
        format!("{}|{}", normalize_intent(intent), normalize_params(params))
    }

    pub fn get(&self, intent: &str, params: &IntentParams) -> Option<&Stage3Plan> {
        self.map.get(&Self::key(intent, params))
    }

    pub fn put(&mut self, intent: &str, params: &IntentParams, plan: Stage3Plan) {
        self.map.insert(Self::key(intent, params), plan);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{PlannedCommand, Stage3Plan};
    use std::collections::BTreeMap;

    fn params(
        target: Option<&str>,
        ports: Option<&str>,
        url: Option<&str>,
        extra: &[(&str, &str)],
    ) -> IntentParams {
        let mut map = BTreeMap::new();
        for (k, v) in extra {
            map.insert(k.to_string(), v.to_string());
        }
        IntentParams {
            target: target.map(|s| s.to_string()),
            ports: ports.map(|s| s.to_string()),
            url: url.map(|s| s.to_string()),
            extra: map,
        }
    }

    fn sample_plan(tool: &str) -> Stage3Plan {
        Stage3Plan {
            commands: vec![PlannedCommand {
                tool: tool.to_string(),
                argv: vec!["-p".to_string(), "80".to_string()],
                purpose: "scan".to_string(),
                depends_on_prev: false,
            }],
        }
    }

    #[test]
    fn normalize_intent_lowercases_trims_and_collapses_whitespace() {
        assert_eq!(normalize_intent("  Scan   PORT  "), "scan port");
        assert_eq!(normalize_intent("SCAN PORT"), "scan port");
        assert_eq!(normalize_intent("scan\tport\non\thost"), "scan port on host");
    }

    #[test]
    fn normalize_params_sorts_key_value_pairs_and_normalizes_values() {
        let p = params(
            Some("192.168.1.1"),
            Some("22,80"),
            None,
            &[("scheme", "TCP"), ("aggr", "T4")],
        );
        // Keys sorted alphabetically: aggr, ports, scheme, target. Values lowercased.
        assert_eq!(
            normalize_params(&p),
            "aggr=t4&ports=22,80&scheme=tcp&target=192.168.1.1"
        );
    }

    #[test]
    fn normalize_params_ignores_none_fields() {
        let p = params(Some("host"), None, None, &[]);
        assert_eq!(normalize_params(&p), "target=host");
    }

    #[test]
    fn key_is_normalized_intent_pipe_normalized_params() {
        let p = params(Some("192.168.1.1"), Some("80"), None, &[]);
        assert_eq!(
            PlanCache::key("  Scan PORT ", &p),
            "scan port|ports=80&target=192.168.1.1"
        );
    }

    #[test]
    fn same_intent_and_params_hit_even_with_different_phrasing() {
        let mut cache = PlanCache::new();
        let p = params(Some("192.168.1.1"), Some("80"), None, &[]);
        cache.put("scan port", &p, sample_plan("nmap"));

        // Case + whitespace differences must still resolve to the same entry.
        let got = cache.get("  SCAN   port ", &p);
        assert!(got.is_some());
        assert_eq!(got.unwrap().commands[0].tool, "nmap");
    }

    #[test]
    fn different_target_must_miss() {
        let mut cache = PlanCache::new();
        let p1 = params(Some("192.168.1.1"), None, None, &[]);
        let p2 = params(Some("192.168.1.2"), None, None, &[]);
        cache.put("scan port on 192.168.1.1", &p1, sample_plan("nmap"));

        // Required rule (GOAL.md §7 / manifest): .1.1 vs .1.2 must NOT collide.
        assert!(cache.get("scan port on 192.168.1.2", &p2).is_none());
    }
}
