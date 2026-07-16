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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage2Knowledge {
    pub theory: String,
    pub candidates: Vec<CandidateCommand>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateCommand {
    pub tool: String,
    pub argv: Vec<String>,
    pub purpose: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage3Plan {
    pub commands: Vec<PlannedCommand>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedCommand {
    pub tool: String,
    pub argv: Vec<String>,
    pub purpose: String,
    pub depends_on_prev: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage4Render {
    pub sections: Vec<RenderSection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderSection {
    pub title: String,
    pub kind: SectionKind,
    pub body: RenderBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    Table,
    KeyValue,
    Text,
    Findings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderBody {
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    KeyValue(Vec<(String, String)>),
    Text(String),
    Findings(Vec<FindingItem>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FindingItem {
    pub severity: String,
    pub title: String,
    pub detail: String,
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

    fn sample_knowledge() -> Stage2Knowledge {
        Stage2Knowledge {
            theory: "nmap enumerates services".to_string(),
            candidates: vec![CandidateCommand {
                tool: "nmap".to_string(),
                argv: vec!["-sV".to_string(), "192.168.1.1".to_string()],
                purpose: "service/version scan".to_string(),
            }],
        }
    }

    fn sample_plan() -> Stage3Plan {
        Stage3Plan {
            commands: vec![
                PlannedCommand {
                    tool: "nmap".to_string(),
                    argv: vec!["-sV".to_string(), "192.168.1.1".to_string()],
                    purpose: "scan".to_string(),
                    depends_on_prev: false,
                },
                PlannedCommand {
                    tool: "nikto".to_string(),
                    argv: vec!["-h".to_string(), "192.168.1.1".to_string()],
                    purpose: "web scan".to_string(),
                    depends_on_prev: true,
                },
            ],
        }
    }

    #[test]
    fn stage2_round_trips_through_json() {
        let original = sample_knowledge();
        let json = serde_json::to_string(&original).unwrap();
        let back: Stage2Knowledge = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
    }

    #[test]
    fn stage3_round_trips_and_preserves_order() {
        let original = sample_plan();
        let json = serde_json::to_string(&original).unwrap();
        let back: Stage3Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
        assert_eq!(back.commands.len(), 2);
        assert_eq!(back.commands[0].tool, "nmap");
        assert!(!back.commands[0].depends_on_prev);
        assert!(back.commands[1].depends_on_prev);
    }

    #[test]
    fn planned_command_missing_field_is_rejected() {
        // No `depends_on_prev` — must fail to parse.
        let bad = r#"{ "tool": "nmap", "argv": [], "purpose": "x" }"#;
        assert!(serde_json::from_str::<PlannedCommand>(bad).is_err());
    }

    #[test]
    fn render_body_variants_round_trip() {
        let bodies = vec![
            RenderBody::Table {
                headers: vec!["port".to_string(), "state".to_string()],
                rows: vec![vec!["22".to_string(), "open".to_string()]],
            },
            RenderBody::KeyValue(vec![("os".to_string(), "linux".to_string())]),
            RenderBody::Text("raw output".to_string()),
            RenderBody::Findings(vec![FindingItem {
                severity: "high".to_string(),
                title: "anon ftp".to_string(),
                detail: "ftp allows anonymous login".to_string(),
            }]),
        ];
        for body in bodies {
            let json = serde_json::to_string(&body).unwrap();
            let back: RenderBody = serde_json::from_str(&json).unwrap();
            assert_eq!(body, back);
        }
    }

    #[test]
    fn render_body_table_uses_external_snake_case_tag() {
        let body = RenderBody::Table {
            headers: vec!["h".to_string()],
            rows: vec![vec!["r".to_string()]],
        };
        let json = serde_json::to_string(&body).unwrap();
        assert_eq!(json, r#"{"table":{"headers":["h"],"rows":[["r"]]}}"#);
    }

    #[test]
    fn section_kind_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&SectionKind::KeyValue).unwrap(),
            "\"key_value\""
        );
        assert_eq!(
            serde_json::to_string(&SectionKind::Findings).unwrap(),
            "\"findings\""
        );
    }

    #[test]
    fn stage4_render_round_trips() {
        let original = Stage4Render {
            sections: vec![RenderSection {
                title: "Open Ports".to_string(),
                kind: SectionKind::Table,
                body: RenderBody::Table {
                    headers: vec!["port".to_string(), "state".to_string()],
                    rows: vec![vec!["22".to_string(), "open".to_string()]],
                },
            }],
        };
        let json = serde_json::to_string(&original).unwrap();
        let back: Stage4Render = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
    }
}
