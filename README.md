# deathPWN


# WARNING: deathPWN is in ALPHA Phase — it may not perfectly work on your system
→ wait for the stable update


Natural-language-driven offensive security terminal for BlackArch Linux.

User types raw English ("enumerate the web server on 10.0.0.5") — deathPWN understands the intent, resolves the right pentesting tool commands, and executes them live through an AI orchestration pipeline with zero conversational fluff.

## Quick Start

```bash
cargo build --release
export DEATHPWN_PROVIDER_A_URL="https://your-llm/v1"
export DEATHPWN_PROVIDER_A_KEY="sk-..."
export DEATHPWN_PROVIDER_A_MODEL="model-name"
export DEATHPWN_PROVIDER_B_URL="https://your-fallback-llm/v1"
export DEATHPWN_PROVIDER_B_KEY="sk-..."
export DEATHPWN_PROVIDER_B_MODEL="model-name"
./target/release/deathPWN
```

For persistent config, copy `.example.env` to `.env` and fill in your keys.

## CLI Arguments

```
deathPWN [OPTIONS]

Options:
  --no-cache, --disable-cache  Disable in-memory command caching
  --cache, --enable-cache      Enable in-memory command caching
  --clear-history              Delete all command output artifacts and exit
  --history on|off|clear       Enable, disable, or clear command history
  -h, --help                   Print help information
```

## What It Does

**Implemented (current alpha):**
- **NL → commands**: Type natural language, get pentesting tool invocations via AI resolution
- **Command or raw**: Step-0 detector classifies input as a direct shell command or NL — runs direct commands immediately without AI
- **Dual-provider failover**: Two AI providers (A/B) with automatic fallback if the primary fails
- **TUI**: ratatui terminal with scrollable output, telemetry pane, interactive Discovered Target Matrix, and mouse-driven clipboard copy
- **Text scraping**: IPs, ports, URLs, and filepaths auto-scraped from all output and organized per-target
- **User command preferences**: `preference.json` maps tasks to preferred tools (injected into AI prompt)

**Planned (designed, partially built):**
- **4-stage pipeline**: Full Understand → Retrieve → Plan → Render pipeline (schema types exist; current engine uses simplified single-stage flow)
- **Goal completion loop**: Multi-step autonomous goals iterating until done (cap: 12 steps)
- **Feedback loop**: Auto-install missing tools via pacman/AUR, auto-correct bad argv
- **Web search integration**: DuckDuckGo search for current tool syntax (client exists, not yet wired)
- **Plan cache**: In-memory command caching for repeated requests

## Crate Layout

| Crate | Purpose |
|-------|---------|
| `deathpwn-core` | Library — all business logic, AI pipeline, execution, search, schema types |
| `deathpwn-tui` | Binary — ratatui TUI frontend, produces `deathPWN` binary |

## Key Concepts

- **Simplified engine flow**: Input → Detector → DirectCommand (run immediately) or NL → AI resolution → Execute. Schema types for the full 4-stage pipeline are defined but stage runners are not yet wired.
- **Persistent shell session**: One shell process stays alive across commands, preserving cwd, env, and shell state
- **Discovered Target Matrix**: Right-click any IP, port, URL, or filepath to copy to clipboard (OSC 52 + xclip/xsel/wl-copy fallback)
- **Artifacts**: Every command's raw output saved to `$XDG_DATA_HOME/deathpwn/`
- **CancelToken**: Ctrl+C sends SIGTERM to process group with cooperative cancellation
- **Preference file**: `preference.json` (auto-discovered at `$XDG_CONFIG_HOME/deathpwn/preference.json`, `~/.config/deathpwn/preference.json`, or `./preference.json`) maps task descriptions to preferred command overrides

## Docs

- [`architecture.md`](./docs/architecture.md) — full architecture and data flow
- [`pipeline.md`](./docs/pipeline.md) — 4-stage AI pipeline details (design reference)
- [`configuration.md`](./docs/configuration.md) — environment variable reference
- [`tui.md`](./docs/tui.md) — TUI layout, key bindings, mouse interactions
