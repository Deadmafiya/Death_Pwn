# deathPWN v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build deathPWN v1 — a natural-language-driven, full-screen terminal for offensive security that turns raw English into real, executed commands with clean colorized output.

**Architecture:** A Cargo workspace with `deathpwn-core` (a library holding all logic behind traits — detector, 4-stage AI pipeline, dual-provider failover, exec feedback loop, goal loop, session/cache) and `deathpwn-tui` (a `ratatui` + `tokio` binary owning the terminal, keys, and rendering). Every non-deterministic boundary (AI provider, web search, command execution, clock) is a trait with a fake, so the whole core is deterministically unit-tested via TDD.

**Tech Stack:** Rust (edition 2021), tokio, ratatui + crossterm, reqwest, scraper, serde/serde_json, async-trait, thiserror, nix.

**Source spec:** `docs/superpowers/specs/2026-07-16-deathpwn-v1-design.md` (authoritative — where this plan and the spec disagree, the spec governs).

## Global Constraints

- **Language/edition:** Rust, edition 2021. `deathpwn-core` is `#![forbid(unsafe_code)]`.
- **Layout:** Cargo workspace, `resolver = "2"`, members `deathpwn-core` (lib) + `deathpwn-tui` (bin). All logic + traits live in core; no ratatui/terminal/async-main deps in core.
- **Providers:** two OpenAI-compatible endpoints (A primary, B fallback). URLs, keys, model names come **only from env**. No hardcoded paths or secrets.
- **Env vars (exact names):** `DEATHPWN_PROVIDER_A_URL`, `DEATHPWN_PROVIDER_A_KEY`, `DEATHPWN_PROVIDER_A_MODEL`, `DEATHPWN_PROVIDER_B_URL`, `DEATHPWN_PROVIDER_B_KEY`, `DEATHPWN_PROVIDER_B_MODEL` (all required); `DEATHPWN_SHELL` (default `$SHELL` else `/bin/sh`), `DEATHPWN_MAX_GOAL_STEPS` (default `12`), `DEATHPWN_MAX_CORRECTIONS` (default `2`), `DEATHPWN_ARTIFACTS_DIR` (default `${XDG_DATA_HOME:-~/.local/share}/deathpwn`), `DEATHPWN_HTTP_TIMEOUT_SECS` (default `30`).
- **Failover rule:** every AI call goes through `FailoverClient`. Failover to B triggers on EITHER an A transport error OR an A schema-validation failure. Both fail → aggregated `DeathpwnError::Provider`. Every attempt logged (label, latency via injected `Clock`, outcome). Circuit breaker is OFF in v1.
- **Schema-or-fail:** each AI stage has a strict typed struct; "valid" = the model's `content` parses into it via `serde_json`. Parse failure = validation failure (drives failover).
- **Cache correctness:** the plan cache key is the **exact normalized intent + normalized concrete params**. `scan port on 192.168.1.1` and `...1.2` are DIFFERENT keys (different target) and must NOT share a cached plan. No embeddings, no fuzzy match.
- **Step 0 + exec:** command-vs-raw resolution AND direct execution both go through the user's `$SHELL -c` (so aliases/builtins/functions count as real commands). Children spawn in their **own process group**; Ctrl+C cancels the running command (group SIGTERM→SIGKILL) and notifies the AI; Ctrl+X cancels everything and drains the chain.
- **Feedback loop:** availability check → auto-install (BlackArch: pacman/AUR/`go install`) → run → non-zero classification (`NotFound`/`BenignEmpty`/`FixableUsage`/`Transient`/`Fatal`) with at most `DEATHPWN_MAX_CORRECTIONS` self-corrections (`NotFound`→install loop is not counted).
- **Goal loop:** `GoalContext` created in Stage 1, passed to every stage; after each round an AI goal-check returns `GoalVerdict`; a hard cap of `DEATHPWN_MAX_GOAL_STEPS` halts runaway loops.
- **No persona:** output is command results + structured render sections only. No conversational filler.
- **Testing:** deterministic unit tests using fakes (`FakeAiProvider`, `FakeSearchProvider`, `FakeCommandRunner`, `FakeClock`) are the bulk of the suite and the TDD driver. Real network/subprocess tests are `#[ignore]`. Every task is TDD: failing test → implement → green.
- **Commits:** frequent, one per completed TDD cycle; conventional-commit messages (`feat(deathpwn): …`).
- **Non-goals (v1):** no scope/authorization gate, no exploitation/wireless tools (recon + web + general shell only), BlackArch-only, no cost budgeting, no deterministic per-tool parsers, no embeddings, circuit breaker off.

---
