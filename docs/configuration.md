# Configuration

All configuration via environment variables.

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
| `DEATHPWN_SHELL` | `$SHELL` → `/bin/sh` | Shell for subprocess execution |
| `DEATHPWN_MAX_GOAL_STEPS` | `12` | Safety cap on goal-completion loop iterations |
| `DEATHPWN_MAX_CORRECTIONS` | `2` | Max AI-driven argv corrections per command |
| `DEATHPWN_HTTP_TIMEOUT_SECS` | `30` | HTTP request timeout |
| `DEATHPWN_ARTIFACTS_DIR` | `$XDG_DATA_HOME/deathpwn/` | Root directory for command output artifacts |

## Example

```bash
export DEATHPWN_PROVIDER_A_URL="https://api.openai.com/v1"
export DEATHPWN_PROVIDER_A_KEY="sk-abc123"
export DEATHPWN_PROVIDER_A_MODEL="gpt-4o"
export DEATHPWN_PROVIDER_B_URL="https://api.anthropic.com/v1"
export DEATHPWN_PROVIDER_B_KEY="sk-ant-xyz789"
export DEATHPWN_PROVIDER_B_MODEL="claude-sonnet-4-20250514"
export DEATHPWN_MAX_GOAL_STEPS=20
```
