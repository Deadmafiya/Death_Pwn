### Task 2: schema/ — all stage structs

**Files:**
- Create: `deathpwn-core/src/schema/mod.rs` (core crate — all typed stage structs, serde derives, and unit tests in one module)
- Modify: `deathpwn-core/Cargo.toml` (core crate — add serde + serde_json deps)
- Modify: `deathpwn-core/src/lib.rs` (core crate — declare `pub mod schema;`)
- Test: `deathpwn-core/src/schema/mod.rs` — unit tests live in a `#[cfg(test)] mod tests` block in the same file (Rust convention; manifest lists no separate test file)

**Interfaces:**
- Consumes: nothing (foundational — Task 2 has no upstream dependency per manifest).
- Produces (all `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]`, enums `#[serde(rename_all = "snake_case")]`):
  - `struct Stage1Understanding { intent: String, params: IntentParams, mode: Mode, goal_summary: String }`
  - `struct IntentParams { target: Option<String>, ports: Option<String>, url: Option<String>, extra: BTreeMap<String, String> }`
  - `enum Mode { SingleCommand, GoalCompletion }`
  - `struct Stage2Knowledge { theory: String, candidates: Vec<CandidateCommand> }`
  - `struct CandidateCommand { tool: String, argv: Vec<String>, purpose: String }`
  - `struct Stage3Plan { commands: Vec<PlannedCommand> }`
  - `struct PlannedCommand { tool: String, argv: Vec<String>, purpose: String, depends_on_prev: bool }`
  - `struct Stage4Render { sections: Vec<RenderSection> }`
  - `struct RenderSection { title: String, kind: SectionKind, body: RenderBody }`
  - `enum SectionKind { Table, KeyValue, Text, Findings }`
  - `enum RenderBody { Table { headers: Vec<String>, rows: Vec<Vec<String>> }, KeyValue(Vec<(String, String)>), Text(String), Findings(Vec<FindingItem>) }` — externally tagged (serde default) with `#[serde(rename_all = "snake_case")]`; chosen over internal tagging because internal tags are incompatible with the newtype variants (`Text(String)`, `KeyValue(...)`, `Findings(...)`). Round-trip test locks the wire format.
  - `struct FindingItem { severity: String, title: String, detail: String }`
  - `enum FailureClass { NotFound, BenignEmpty, FixableUsage, Transient, Fatal }`
  - `struct ExecFailureVerdict { class: FailureClass, corrected_argv: Option<Vec<String>> }`
  - `struct GoalVerdict { achieved: bool, reason: String, next_step_hint: Option<String> }`

These types are the strict deserialization targets for the AI stages (spec §4): a stage is "validated" when the provider's JSON `content` parses into the matching struct via `serde_json::from_str`; any parse failure is what triggers failover in Task 4. Later tasks consume these exact names: Stage 1–4 pipeline (Tasks 11–14), FeedbackLoop (`ExecFailureVerdict`/`FailureClass`, Task 8), PlanCache (`Stage3Plan`/`IntentParams`, Task 10), goal loop (`GoalVerdict`/`Mode`, Task 15), and the TUI renderer (`Stage4Render`/`SectionKind`/`RenderBody`, Task 16).

---

- [ ] **Step 1: Add serde + serde_json to the core crate.** Edit `deathpwn-core/Cargo.toml` so the `[dependencies]` section reads exactly (Task 1 already added `thiserror`):

```toml
[dependencies]
thiserror = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

- [ ] **Step 2: Declare the schema module so it compiles empty.** Add the module declaration to `deathpwn-core/src/lib.rs` (keep the existing `#![forbid(unsafe_code)]` at the top and any existing `pub mod error;` / `pub mod config;` from Task 1):

```rust
pub mod schema;
```

  Create `deathpwn-core/src/schema/mod.rs` with only the imports and an empty test module (no types yet — this is the failing-test surface for Step 3):

```rust
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests {
    use super::*;
}
```

  Run `cargo build -p deathpwn-core`. Expected: builds clean (the `BTreeMap`/serde imports are unused for now — that is a warning, not an error).

---

- [ ] **Step 3: Write the failing test — Stage 1 group (understanding, params, mode).** Replace the `tests` module in `deathpwn-core/src/schema/mod.rs` with:

```rust
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
```

- [ ] **Step 4: Run test to verify it fails.** `cargo test -p deathpwn-core schema::`. Expected: fails to compile — `error[E0422]/E0412/E0433: cannot find type/value 'Stage1Understanding'`, `'IntentParams'`, `'Mode'` in this scope (the referenced types do not exist yet).

- [ ] **Step 5: Implement the Stage 1 types.** Insert these definitions into `deathpwn-core/src/schema/mod.rs` above the `#[cfg(test)] mod tests` block (below the imports):

```rust
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
```

- [ ] **Step 6: Run test to verify it passes.** `cargo test -p deathpwn-core schema::`. Expected: PASS — `test result: ok. 5 passed` (`stage1_round_trips_through_json`, `mode_serializes_snake_case`, `mode_unknown_variant_is_rejected`, `stage1_missing_required_field_is_rejected`, `intent_params_omitted_options_default_to_none`).

- [ ] **Step 7: Commit.** `git add deathpwn-core/Cargo.toml deathpwn-core/src/lib.rs deathpwn-core/src/schema/mod.rs` && `git commit -m "feat(deathpwn): add Stage1 understanding schema (intent/params/mode)"`

---

- [ ] **Step 8: Write the failing test — Stage 2 & Stage 3 groups (knowledge, plan).** Append these helpers and tests inside the `mod tests` block in `deathpwn-core/src/schema/mod.rs`:

```rust
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
```

- [ ] **Step 9: Run test to verify it fails.** `cargo test -p deathpwn-core schema::`. Expected: fails to compile — `error[E0422]/E0412: cannot find type 'Stage2Knowledge'`, `'CandidateCommand'`, `'Stage3Plan'`, `'PlannedCommand'` in this scope.

- [ ] **Step 10: Implement the Stage 2 & Stage 3 types.** Insert these definitions above the `#[cfg(test)] mod tests` block in `deathpwn-core/src/schema/mod.rs`:

```rust
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
```

- [ ] **Step 11: Run test to verify it passes.** `cargo test -p deathpwn-core schema::`. Expected: PASS — `test result: ok. 8 passed` (the 5 from Step 6 plus `stage2_round_trips_through_json`, `stage3_round_trips_and_preserves_order`, `planned_command_missing_field_is_rejected`).

- [ ] **Step 12: Commit.** `git add deathpwn-core/src/schema/mod.rs` && `git commit -m "feat(deathpwn): add Stage2 knowledge and Stage3 plan schemas"`

---

- [ ] **Step 13: Write the failing test — Stage 4 render group (sections, kind, body, findings).** Append these tests inside the `mod tests` block in `deathpwn-core/src/schema/mod.rs`:

```rust
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
```

- [ ] **Step 14: Run test to verify it fails.** `cargo test -p deathpwn-core schema::`. Expected: fails to compile — `error[E0422]/E0412/E0433: cannot find 'Stage4Render'`, `'RenderSection'`, `'SectionKind'`, `'RenderBody'`, `'FindingItem'` in this scope.

- [ ] **Step 15: Implement the Stage 4 render types.** Insert these definitions above the `#[cfg(test)] mod tests` block in `deathpwn-core/src/schema/mod.rs`:

```rust
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
```

- [ ] **Step 16: Run test to verify it passes.** `cargo test -p deathpwn-core schema::`. Expected: PASS — `test result: ok. 12 passed` (the 8 prior plus `render_body_variants_round_trip`, `render_body_table_uses_external_snake_case_tag`, `section_kind_serializes_snake_case`, `stage4_render_round_trips`).

- [ ] **Step 17: Commit.** `git add deathpwn-core/src/schema/mod.rs` && `git commit -m "feat(deathpwn): add Stage4 render schema (sections/kind/body/findings)"`

---

- [ ] **Step 18: Write the failing test — verdict group (failure class, exec verdict, goal verdict).** Append these tests inside the `mod tests` block in `deathpwn-core/src/schema/mod.rs`:

```rust
    #[test]
    fn failure_class_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&FailureClass::NotFound).unwrap(),
            "\"not_found\""
        );
        assert_eq!(
            serde_json::to_string(&FailureClass::BenignEmpty).unwrap(),
            "\"benign_empty\""
        );
        assert_eq!(
            serde_json::to_string(&FailureClass::FixableUsage).unwrap(),
            "\"fixable_usage\""
        );
        assert_eq!(
            serde_json::to_string(&FailureClass::Transient).unwrap(),
            "\"transient\""
        );
        assert_eq!(
            serde_json::to_string(&FailureClass::Fatal).unwrap(),
            "\"fatal\""
        );
    }

    #[test]
    fn exec_failure_verdict_round_trips_with_and_without_argv() {
        let with = ExecFailureVerdict {
            class: FailureClass::FixableUsage,
            corrected_argv: Some(vec!["nmap".to_string(), "-sS".to_string()]),
        };
        let without = ExecFailureVerdict {
            class: FailureClass::Fatal,
            corrected_argv: None,
        };
        for v in [with, without] {
            let json = serde_json::to_string(&v).unwrap();
            let back: ExecFailureVerdict = serde_json::from_str(&json).unwrap();
            assert_eq!(v, back);
        }
    }

    #[test]
    fn exec_failure_verdict_rejects_unknown_class() {
        let bad = r#"{"class":"exploded","corrected_argv":null}"#;
        assert!(serde_json::from_str::<ExecFailureVerdict>(bad).is_err());
    }

    #[test]
    fn goal_verdict_parses_from_model_json() {
        let json = r#"{"achieved":false,"reason":"ports still unknown","next_step_hint":"run nmap -sV"}"#;
        let v: GoalVerdict = serde_json::from_str(json).unwrap();
        assert!(!v.achieved);
        assert_eq!(v.reason, "ports still unknown");
        assert_eq!(v.next_step_hint.as_deref(), Some("run nmap -sV"));
    }

    #[test]
    fn goal_verdict_round_trips_without_hint() {
        let v = GoalVerdict {
            achieved: true,
            reason: "target fully enumerated".to_string(),
            next_step_hint: None,
        };
        let json = serde_json::to_string(&v).unwrap();
        let back: GoalVerdict = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }
```

- [ ] **Step 19: Run test to verify it fails.** `cargo test -p deathpwn-core schema::`. Expected: fails to compile — `error[E0422]/E0412/E0433: cannot find 'FailureClass'`, `'ExecFailureVerdict'`, `'GoalVerdict'` in this scope.

- [ ] **Step 20: Implement the verdict types.** Insert these definitions above the `#[cfg(test)] mod tests` block in `deathpwn-core/src/schema/mod.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    NotFound,
    BenignEmpty,
    FixableUsage,
    Transient,
    Fatal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecFailureVerdict {
    pub class: FailureClass,
    pub corrected_argv: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalVerdict {
    pub achieved: bool,
    pub reason: String,
    pub next_step_hint: Option<String>,
}
```

- [ ] **Step 21: Run test to verify it passes.** `cargo test -p deathpwn-core schema::`. Expected: PASS — `test result: ok. 17 passed` (the 12 prior plus `failure_class_serializes_snake_case`, `exec_failure_verdict_round_trips_with_and_without_argv`, `exec_failure_verdict_rejects_unknown_class`, `goal_verdict_parses_from_model_json`, `goal_verdict_round_trips_without_hint`). Also run `cargo build -p deathpwn-core` to confirm the `BTreeMap`/serde imports are now all used (no unused-import warnings remain).

- [ ] **Step 22: Commit (final).** `git add deathpwn-core/src/schema/mod.rs` && `git commit -m "feat(deathpwn): add exec-failure and goal verdict schemas; complete schema module"`
