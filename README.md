# deathPWN


# WARNING: deathPWN is in ALPHA Phase it may not perfecly work on your system
-> wait for the stable update


Natural-language-driven offensive security terminal for BlackArch Linux.

User types raw English ("enumerate the web server on 10.0.0.5") — deathPWN understands the intent, searches the web for relevant tool commands, executes the right pentesting tools, and renders structured output — all through an AI orchestration pipeline with zero conversational fluff.

## Quick Start

```bash
cargo build --release
export DEATHPWN_PROVIDER_A_URL="https://your-llm/v1"
export DEATHPWN_PROVIDER_A_KEY="sk-..."
export DEATHPWN_PROVIDER_A_MODEL="model-name"
./target/release/deathPWN
```

## What It Does

- **NL → commands**: Type natural language, get pentesting tool invocations
- **Web search integration**: DuckDuckGo search fetches current tool syntax/techniques
- **Command or raw**: Step-0 detector classifies typed input as a shell command or NL
- **Goal completion loop**: Multi-step goals iterate until done (cap: 12 steps)
- **Feedback loop**: Missing tools get installed via AI-resolved pacman/AUR commands, bad argv gets auto-corrected
- **Dual-provider failover**: Two AI providers (A/B) with schema-validated fallback
- **TUI**: ratatui-based terminal with scrollable output, status bar, structured analysis pane

## Crate Layout

| Crate | Purpose |
|-------|---------|
| `deathpwn-core` | Library — all business logic, AI pipeline, execution, search, caching |
| `deathpwn-tui` | Binary — ratatui TUI frontend, produces `deathPWN` binary |

## Key Concepts

- **4-stage pipeline**: Understand → Retrieve → Plan → Render
- **Goal loop**: For GoalCompletion mode, iterates Plan → Execute → GoalCheck
- **Session state**: Accumulates known targets, hosts, ports, services, findings across a session
- **Artifacts**: Every command's raw output saved to `$XDG_DATA_HOME/deathpwn/`
- **CancelToken**: Ctrl+C sends SIGTERM to process group with cooperative cancellation

## Docs

- [`architecture.md`](./architecture.md) — full architecture and data flow
- [`pipeline.md`](./pipeline.md) — 4-stage AI pipeline details
- [`configuration.md`](./configuration.md) — environment variable reference
- [`tui.md`](./tui.md) — TUI layout and key bindings
