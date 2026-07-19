# Memory

## Project Overview
See @README.md for project overview. This is a Rust workspace with two crates: `deathpwn-core` (library) and `deathpwn-tui` (binary → `deathPWN`). The project is a natural-language-driven offensive security terminal for BlackArch Linux.

## Code Style Guidelines
- Use descriptive variable names
- Follow existing patterns in the codebase
- Extract complex conditions into meaningful boolean variables
- All I/O boundaries are trait-abstracted (CommandRunner, AiProvider, SearchProvider, Clock)
- `#![forbid(unsafe_code)]` applies to `deathpwn-core`
- Test-support doubles are gated behind `#[cfg(any(test, feature = "test-support"))]`

## Architecture Notes

**Workspace structure:**
- `deathpwn-core` — All business logic. Modules: `cancel`, `clock`, `config`, `detector`, `engine`, `error`, `exec` (runner, feedback, installer), `providers` (ai, openai, failover), `schema` (all types in mod.rs), `search` (ddg, mod)
- `deathpwn-tui` — ratatui TUI frontend. Files: `main.rs` (event loop, reload, crossterm thread), `app.rs` (App state, key bindings, event handling, text scraping, mouse handling, clipboard), `ui/mod.rs` (layout orchestration + Stage4Render→Lines), `ui/panes.rs` (4 widget renderers), `ui/theme.rs` (6-color palette)

**Engine flow (simplified — current implementation):**
```
Input → Detector::classify()
  ├─ DirectCommand → ShellRunner::run_streaming() (no AI)
  └─ RawInput (NL) → FailoverClient::complete_validated() → clean_command()
                       → ShellRunner::run_streaming()
```

**Key design decisions:**
- Dual-provider failover (A then B) for all AI calls — no circuit breaker in v1
- Persistent ShellRunner session with sentinel-delimited output (`==DEATHPWN_STDOUT_DONE==`)
- Crossterm event pump on dedicated OS thread, tokio runtime for the event loop
- mpsc channels: job_tx/rx (UI→engine), event_tx/rx (engine→UI)
- CancelToken: cooperative async cancellation via atomic flag + notify, process-group-aware SIGTERM → 300ms → SIGKILL escalation
- Plan cache, FeedbackLoop, DuckDuckGo search, and GoalCompletion loop exist in codebase but are NOT wired into Engine yet
- The full 4-stage pipeline has schema types defined in `schema/mod.rs` but stage runners are not built; Engine uses a simplified single-stage flow

## Common Workflows

```bash
# Build
cargo build --release

# Run
./target/release/deathPWN

# Run (dev)
cargo run -p deathpwn-tui

# Test all
cargo test

# Test single crate
cargo test -p deathpwn-core
cargo test -p deathpwn-tui

# Type-check
cargo check

# Lint
cargo clippy -- -D warnings

# Format-check
cargo fmt -- --check

# Format-apply
cargo fmt
```
