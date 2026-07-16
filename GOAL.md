# deathPWN — Project Goal & Vision (v2)

> The smartest terminal for penetration testers and ethical hackers.
> Type raw English. It understands the *intent* and the *goal*, runs the right
> tool(s), and renders clean, consistent, beautiful output. It is a **terminal**
> first — with an intelligent execution core underneath. No chatty persona.

> **Scope note:** deathPWN is a **personal, single-user tool** built for one
> operator (me) on **my BlackArch system**. Design choices favor power and speed
> over guardrails, because there is exactly one trusted user.

---

## 1. Vision

deathPWN is a natural-language-driven terminal for offensive security work. The
user speaks plainly ("enumerate the web server on 10.0.0.5", "do OSINT on
example.com", "find SQLi on this login form") and deathPWN:

1. Decides whether the input is **already a real command** or **natural
   language** (raw input).
2. For raw input: understands the **intent + goal**, retrieves the needed
   **theory + command** via online search.
3. Runs the **actual command(s)** — one, or a whole chain to complete a goal.
4. Renders the result as **clean, colorized, tabular, consistent** output —
   nothing more than what's needed.

### Core principles (non-negotiable)
- **Simplicity for the user.** Raw English in, real results out. No ceremony.
- **Terminal first, no persona.** deathPWN never injects conversational filler
  ("feel free to ask", "I hope this helps", "let me know if…"). It emits command
  output and structured results only.
- **Intent vs. goal sharpness.** It must reliably tell a *single command*
  ("show me open ports") from a *multi-step goal completion* ("get me a foothold
  on this box") — and act accordingly.
- **Consistency over noise.** Output is deterministic in shape. No unneeded
  information, no chatter, no rephrasing of what the tool already said.
- **Beautiful & hacker-friendly UI.** Tables, highlighted/colorized text,
  panels, formatted sections — a consistent visual language across every tool.

---

## 2. Locked decisions

| Area | Decision |
|------|----------|
| Language / stack | **Rust** |
| AI providers | **Two OpenAI-compatible custom endpoints** (primary + fallback); URLs + keys in **env** |
| Model preference | Fast models (e.g. **gpt-oss**) to mask multi-call latency (see §9 open issue) |
| Execution model | **Fully automatic** — no per-command confirmation |
| Privilege | **deathPWN runs as sudo/root**; every command inherits root |
| Target platform | **BlackArch Linux** (personal machine; most tools pre-installed) |
| Users | **Single trusted user (me only)** — no multi-user, no safety gate |
| Tool coverage (v1) | **Recon**, **Web**, **General shell** |
| Scope enforcement | **None** — operator is fully responsible |
| Knowledge source | **Live online search via DuckDuckGo** (free, no API key) — fetches theory + command. GitHub awesome-hacking cloning is **deprecated** as the core mechanism |
| Session | **Stateful** — remembers targets, prior scans, findings within a session |
| Intent/emotion layer | **Intent + goal only** — no personality, no emotional narration |
| Output | **Command output only**, formatted/colorized/tabular, zero noise |
| Config & secrets | **Environment variables**; no hardcoded paths |

---

## 3. Pipeline (architectural core)

### Step 0 — Command vs. Raw-input detector  ⭐ (critical, runs above everything)
Before any AI is touched, deathPWN determines whether the typed line is
**actually a command**.
- **This is a real terminal-level check, NOT a hardcoded wordlist.** We do not
  match on literals like `ls`/`cd`. Instead we resolve the input the way a shell
  would: parse it as a shell command and check whether the leading token
  resolves to an **executable in `$PATH`, a shell builtin, an alias, or a valid
  path/shell construct**.
- If it resolves → **direct command**: execute immediately in the shell, no AI.
- If it does not resolve → tag as **`raw input`** and hand it to the AI pipeline.

This keeps normal terminal usage instant and only spends AI on genuine natural
language.

### Stage 1 — Understanding (identification)
**Input:** the `raw input` + session state (current targets, discovered
hosts/ports/services, prior findings).
**Job:**
- Extract the **intent**, the **concrete parameters** (target, ports, URL, etc.),
  and classify the request as `single_command` | `goal_completion`.
- Carry a **goal context** object forward (see §5 on loop termination).
**Output contract (validated):** structured JSON (intent, params, mode, goal
context).

### Stage 2 — Knowledge retrieval via ONLINE SEARCH (DuckDuckGo)  ⭐ (replaces repo cloning)
**Backend:** **DuckDuckGo** — free, no API key required.
**Job:** perform a live web search to obtain two things for the intent:
- the **theory** — enough understanding of the technique/tool to act correctly;
- a **candidate command** (or commands) with correct syntax/flags.
**Why:** curated GitHub link-lists (awesome-*) give tool *names*, not runnable
syntax. Online search gives both the concept and a concrete, current command.
**Output contract (validated):** retrieved theory summary + candidate
command(s) with tool names and argv.

### Stage 3 — Execution (main model + runtime)
**Job:**
- Finalize the concrete command(s) from Stage 2 grounded in the session.
- Emit **one command** for a single-command intent, or a **chained sequence**
  for goal completion (recon → enumerate discovered services → targeted test).
- Execute automatically (as root). See §4 for the execution feedback loop.
**Output contract (validated):** ordered commands, each with tool name, full
argv, purpose tag, and (for chains) the dependency on the previous step.

### Stage 4 — Output rendering
**Job:** turn raw stdout/stderr/exit into deathPWN's consistent presentation:
tables, highlighted findings, colorized severity, sectioned panels. Emit **only**
information present in the command output.
- **v1:** LLM-assisted formatting (acceptable latency tradeoff for now).
- **Deferred upgrade:** per-tool deterministic parsers (nmap XML, nuclei/ffuf
  JSON) for zero-nondeterminism formatting — postponed because building a custom
  parser per tool is costly; revisit once the tool set stabilizes. (see §9)

---

## 4. Execution feedback loop  ⭐ (critical for goal completion)

Around every command execution:

1. **Tool-availability check.** Before running, verify the tool exists
   (e.g. `which nmap`). 
2. **Auto-install on miss.** If the check errors, invoke a small AI step that
   produces the correct install command for BlackArch (pacman / AUR / `go
   install`), run it, then **resume** the original command.
3. **Run** the command (as root).
4. **Non-zero exit handling (policy).** Feed exit code + stderr back to the
   execution AI, which **classifies** the failure rather than blindly retrying:
   - **127 / not found** → handled by the install loop above (not a failure).
   - **benign "no results"** (host down, nothing found — many pentest tools exit
     non-zero here) → treat as a valid empty result, **report, no retry**.
   - **fixable usage error** (bad flag/arg/syntax) → AI corrects and retries,
     **max 2 attempts**.
   - **transient** (network/timeout/rate-limit) → **retry once**.
   - **fatal/unrecoverable** → **report cleanly**.
   Retry budget is capped at **2 self-corrections per command**. On exhaustion,
   report the failure; in goal-completion mode the goal-check (§5) decides
   whether to try an alternate path or stop. All attempts are logged.

---

## 5. Goal completion & loop termination

- A **goal context** object is created in Stage 1 and **passed to every stage /
  agent**, so any stage can recognize when the goal is satisfied.
- After each execution round, the acting AI performs a **goal-achieved check**:
  "given what we've done and found, is the goal complete?" If yes → stop and
  render. If no → plan the next step.
- **Safety cap:** a maximum step/iteration limit prevents runaway loops even if
  the goal-check misfires.

---

## 6. Interrupts & control  ⭐

- **Ctrl+C** — abort the **currently running command**. The AI that launched it
  is **notified that the command was stopped by the user**, so it can adapt
  rather than assume completion.
- **Ctrl+X** — **stop everything** (current command + any pending chain) and
  return to a fresh prompt for new input.

---

## 7. Session, caching & state

- **Stateful engagement:** remembers current target(s), discovered
  hosts/ports/services, and prior findings; follow-ups chain naturally ("now
  scan those open ports") without repeating the target.
- **Parameter-aware cache (semantic cache).** Cache NL→plan results to skip
  redundant AI calls — **but keyed on the normalized intent AND concrete
  parameters.** Example correctness rule: `scan port on 192.168.1.1` then later
  `scan port on 192.168.1.2` are **different** (different target) → the second
  must NOT reuse the first's command. Cache hit requires matching intent *and*
  matching parameters.
- **Session artifacts (light):** save each command's raw output to a per-session
  log/artifacts directory so results can be reviewed or exported later.
  *(Reporting = saved scan outputs + findings you can revisit; kept minimal in
  v1.)*

---

## 8. Dual-provider fallback (reliability core)

Every AI call (all stages) uses one resilient wrapper:
1. Call **Provider A** (primary, OpenAI-compatible, fast model).
2. Validate the response against the stage's **strict schema**.
3. **Immediate failover to Provider B** if EITHER the API call fails
   (network/timeout/5xx/rate-limit) **OR** the output does not match the stage's
   required layout (schema/parse validation fails).
4. Provider B runs the **exact same request**, validated the same way.
5. Log every attempt (provider, latency, outcome) for audit.

Notes: schema validation (typed Rust structs, parse-or-fail) is the correctness
gate. Optional circuit breaker to avoid hammering a failing provider. No cost
caps needed (API is not paid per-use for this project).

---

## 9. Open questions / known issues
- [ ] **Latency (known, unsolved).** 3–4 sequential AI calls + fallback make each
      NL request slow for a "terminal." Interim mitigation: **fast models
      (gpt-oss)**. Real fix (parallelization / merging stages / caching) TBD.
- [x] **Non-zero exit policy.** Resolved — classify-then-act, max 2
      self-corrections per command (see §4.4).
- [x] **Online-search backend.** Resolved — **DuckDuckGo** (free). Still to tune:
      result ranking/trust and how much theory to inject into the model.
- [x] **Provider details.** Resolved — base URLs + model names + keys for
      Provider A/B all supplied via **env**.
- [ ] **Deterministic per-tool parsers.** Deferred; revisit when tool set is
      stable (big consistency + speed win later).
- [ ] **Prompt-injection from tool output.** Lower priority given single-user /
      self-target, but recon output is attacker-influenced — revisit if scope
      widens.
- [ ] **Eval harness.** Optional NL→command golden set to measure "smartness"
      later.

## 10. Non-goals (v1)
- No conversational/assistant persona or chit-chat.
- No scope/authorization enforcement, no command safety gate (single trusted
  user, personal machine).
- No exploitation/wireless toolsets yet (recon + web + general shell first).
- Not cross-platform — **BlackArch** is the target.
- No cost/token budgeting (API not paid per-use).
