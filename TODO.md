# TODO: Future Roadmap for deathPWN

This document tracks advanced features and pipeline stages that have been deferred to prioritize a fast, simple single-stage AI command resolver.

## 1. 4-Stage AI Pipeline
- **Stage 1 (Understand)**: Semantic intent parser converting natural language requests into structured `Stage1Understanding` JSON.
- **Stage 2 (Retrieve)**: Grounding engine performing web searches via DuckDuckGo to obtain relevant tool usage examples and BlackArch command syntaxes.
- **Stage 3 (Plan)**: Chain-of-command planner producing structured multi-command execution plans (`Stage3Plan`).
- **Stage 4 (Render)**: Post-execution visualizer formatting raw terminal command stdout/stderr outputs into rich structured TUI widgets (tables, key-values, lists).

## 2. Multi-Step Goal Completion Loop
- Autonomous agent capability that iteratively plans, executes, triages, and self-corrects commands until a goal (e.g., getting a shell or finding open web ports) is successfully achieved.
- AI-driven Goal Check capability to judge goal completion dynamically.
- Cancellation safety policies to abort the goal chain on user input (Ctrl+C / Ctrl+X).

## 3. Preferred Commands Integration
- Dedicated user override configuration (`~/.config/deathpwn/preferred_commands.json`) mapping specific tasks (e.g. "host discovery") to custom user commands (e.g., `arp-scan`).
- Semantic matching in Stage 2/3 to inject and prioritize these overrides over standard generated commands.

## 4. History and Cache Systems
- **Plan Cache (`PlanCache`)**: Exact-match normalized in-memory key lookup to bypass the AI planning step for identical requests.
- **Artifacts System (`Artifacts`)**: Disk persistence layer recording raw command execution trails to a session directory.
- **TUI & CLI Commands**: Inline commands (`/history on/off/clear`) and CLI flags (`--history`) to enable, disable, or wipe history.
