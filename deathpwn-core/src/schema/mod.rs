use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage1Understanding {
    pub intent: String,
    pub params: IntentParams,
    pub mode: Mode,
    pub goal_summary: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentParams {
    pub target: Option<String>,
    pub ports: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    SingleCommand,
    GoalCompletion,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_understanding() -> Stage1Understanding {
        let mut extra = BTreeMap::new();
        extra.insert("aggressive".to_string(), "true".to_string());
        Stage1Understanding {
            intent: "port_scan".to_string(),
            params: IntentParams {
                target: Some("192.168.1.1".to_string()),
                ports: Some("1-1024".to_string()),
                url: None,
                extra,
            },
            mode: Mode::GoalCompletion,
            goal_summary: "map the host".to_string(),
        }
    }

    #[test]
    fn stage1_round_trips_through_json() {
        let original = sample_understanding();
        let json = serde_json::to_string(&original).expect("serialize");
        let back: Stage1Understanding = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, back);
    }

    #[test]
    fn mode_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&Mode::SingleCommand).unwrap(),
            "\"single_command\""
        );
        assert_eq!(
            serde_json::to_string(&Mode::GoalCompletion).unwrap(),
            "\"goal_completion\""
        );
    }

    #[test]
    fn mode_unknown_variant_is_rejected() {
        assert!(serde_json::from_str::<Mode>("\"burp\"").is_err());
    }

    #[test]
    fn stage1_missing_required_field_is_rejected() {
        // No `intent`, no `mode` — must fail to parse.
        let bad = r#"{ "params": { "extra": {} }, "goal_summary": "x" }"#;
        assert!(serde_json::from_str::<Stage1Understanding>(bad).is_err());
    }

    #[test]
    fn intent_params_omitted_options_default_to_none() {
        let json = r#"{ "extra": {} }"#;
        let p: IntentParams = serde_json::from_str(json).expect("deserialize");
        assert_eq!(p.target, None);
        assert_eq!(p.ports, None);
        assert_eq!(p.url, None);
        assert!(p.extra.is_empty());
    }
}
