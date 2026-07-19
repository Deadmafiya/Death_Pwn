# Configuration

All configuration via environment variables. Use `.env` for persistent config (copy from `.example.env`).

## Required

| Variable | Purpose |
|----------|---------|
| `DEATHPWN_PROVIDER_A_URL` | Primary AI provider endpoint (OpenAI-compatible) |
| `DEATHPWN_PROVIDER_A_KEY` | API key for provider A |
| `DEATHPWN_PROVIDER_A_MODEL` | Model name for provider A |
| `DEATHPWN_PROVIDER_B_URL` | Fallback AI provider endpoint |
| `DEATHPWN_PROVIDER_B_KEY` | API key for provider B |
| `DEATHPWN_PROVIDER_B_MODEL` | Model name for provider B |

## Optional

| Variable | Default | Purpose |
|----------|---------|---------|
| `DEATHPWN_SHELL` | `$SHELL` → `/bin/sh` | Shell for subprocess execution. Supports persistent sessions — cwd, env, and shell state carry across commands. |
| `DEATHPWN_MAX_GOAL_STEPS` | `12` | Safety cap on goal-completion loop iterations (future feature) |
| `DEATHPWN_MAX_CORRECTIONS` | `2` | Max AI-driven argv corrections per command (when FeedbackLoop is wired) |
| `DEATHPWN_HTTP_TIMEOUT_SECS` | `30` | HTTP request timeout for AI provider calls |
| `DEATHPWN_ARTIFACTS_DIR` | `$XDG_DATA_HOME/deathpwn/` | Root directory for command output artifacts |
| `DEATHPWN_PREFERENCE_FILE` | auto-discovered (see below) | Path to `preference.json` for user command overrides |
| `DEATHPWN_DISABLE_CACHE` | unset (false) | Set to `true` to disable in-memory command caching. Also settable via `--no-cache` CLI flag. |
| `DEATHPWN_DISABLE_HISTORY` | unset (false) | Set to `true` to disable command history. Also settable via `--history off` CLI flag. |

## Preference File

`preference.json` maps task descriptions to preferred command overrides. The AI system prompt injects these preferences so your custom tool choices always take priority.

**Auto-discovery order** (first found wins):
1. `$XDG_CONFIG_HOME/deathpwn/preference.json`
2. `~/.config/deathpwn/preference.json`
3. `./preference.json` (CWD)

Set `DEATHPWN_PREFERENCE_FILE` to override with an explicit path.

**Format:**
```json
{
  "host discovery": "sudo arp-scan --local",
  "port scanning": "rustscan -a __TARGET__"
}
```

## Example

```bash
export DEATHPWN_PROVIDER_A_URL="https://api.openai.com/v1"
export DEATHPWN_PROVIDER_A_KEY="sk-abc123"
export DEATHPWN_PROVIDER_A_MODEL="gpt-4o"
export DEATHPWN_PROVIDER_B_URL="https://api.anthropic.com/v1"
export DEATHPWN_PROVIDER_B_KEY="sk-ant-xyz789"
export DEATHPWN_PROVIDER_B_MODEL="claude-sonnet-4-20250514"
# Optional overrides:
export DEATHPWN_SHELL="/usr/bin/zsh"
export DEATHPWN_MAX_GOAL_STEPS=20
export DEATHPWN_HTTP_TIMEOUT_SECS=60
export DEATHPWN_DISABLE_CACHE=true
```
