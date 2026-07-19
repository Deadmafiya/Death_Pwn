# TODO: Remaining Work for deathPWN

This document tracks advanced features and pipeline stages that are partially built but need further implementation to be fully operational.

## 1. 4-Stage AI Pipeline

**Completed:**
- All schema types defined in `deathpwn-core/src/schema/mod.rs` (Stage1Understanding, Stage2Knowledge, Stage3Plan, Stage4Render)

**Remaining:**
- Build stage runners for Understand, Retrieve, Plan, and Render stages
- Wire stage runners into Engine (currently uses simplified single-stage flow via `FailoverClient::complete_validated()` → `clean_command()`)

## 2. Multi-Step Goal Completion Loop

**Completed:**
- GoalVerdict and GoalContext schema types defined in `schema/mod.rs`
- FeedbackLoop with availability check, auto-install, and ai-driven argv correction fully built in `deathpwn-core/src/exec/feedback.rs`

**Remaining:**
- Implement goal loop state machine inside Engine
- Wire FeedbackLoop into Engine (currently Engine calls ShellRunner directly)
- Wire AI GoalCheck calls to determine when goal is achieved

## 3. Preferred Commands Integration

**Completed:**
- `DEATHPWN_PREFERENCE_FILE` env var + auto-discovery (`$XDG_CONFIG_HOME/deathpwn/preference.json`, `~/.config/deathpwn/preference.json`, `./preference.json`) in `config.rs`
- `preference.json` loaded into `Config::preferences` HashMap
- Preferences injected into AI system prompt in `engine.rs`

**Remaining:**
- Direct preference-to-command mapping without AI for exact matches
- Preference-aware plan caching (cache key should include preferences)

## 4. History and Cache Systems

**Completed:**
- CLI args implemented: `--cache`/`--no-cache`/`--clear-history`/`--history [on|off|clear]`
- Artifacts system writes command output to `DEATHPWN_ARTIFACTS_DIR`

**Remaining:**
- In-memory plan cache (PlanCache structure) wiring into Engine
- TUI history browsing (arrow-up/down to recall previous commands)
- `/history` inline commands in the input line
