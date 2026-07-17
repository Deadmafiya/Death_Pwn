# Canonical Test-Support Fake API Contract (v1)

Authoritative. Every task section MUST define/use exactly these fake APIs so
every `#[cfg(test)]` module compiles. Definitions live in Task 3 (`FakeAiProvider`,
`FakeClock`) and Task 7 (`FakeCommandRunner`). All fakes are gated
`#[cfg(any(test, feature = "test-support"))]` and re-exported from `lib.rs`.

Two genuine collisions were resolved (recorded here so nobody re-introduces them):
- `FakeAiProvider::scripted(...)` takes `Vec<Result<String, ProviderError>>` (NOT
  `Vec<String>`). The string-vec variant is named `scripted_ok(Vec<String>)`.
- `FakeClock::new(...)` takes `Vec<u64>` (NOT a scalar and NOT zero-arg). Consumers
  that want a constant clock call `FakeClock::fixed(t)`.

---

## `FakeAiProvider` (Task 3, `providers/ai.rs`)

State: `label: String`, a FIFO `responses: Mutex<VecDeque<Result<String, ProviderError>>>`,
an optional `constant: Option<Result<String, ProviderError>>` (for infinite modes),
and `calls: AtomicUsize` (incremented on every `complete()`).

Behavior of `complete()`:
- if `constant` is set → return a clone of it every call (infinite).
- else pop the front of `responses`; if empty → **panic** `"FakeAiProvider exhausted"`
  (tests script exactly the expected number of calls).

Constructors / observers (all must exist):
| Signature | Meaning |
|-----------|---------|
| `new(label: impl Into<String>, responses: Vec<Result<String, ProviderError>>) -> Self` | full control (canonical) |
| `scripted(responses: Vec<Result<String, ProviderError>>) -> Self` | label = `"fake"`, FIFO of Results |
| `with_script(label: impl Into<String>, responses: Vec<Result<String, ProviderError>>) -> Self` | labeled FIFO of Results |
| `with_responses(responses: Vec<Result<String, ProviderError>>) -> Self` | label = `"fake"`, FIFO of Results |
| `scripted_ok(bodies: Vec<String>) -> Self` | label = `"fake"`, FIFO of `Ok(body)` |
| `ok(body: impl Into<String>) -> Self` | infinite `Ok(body)` (sets `constant`) |
| `always(body: impl Into<String>) -> Self` | infinite `Ok(body)` (alias of `ok`) |
| `calls(&self) -> usize` | number of `complete()` calls |
| `call_count(&self) -> usize` | alias of `calls()` |

`scripted`, `with_responses` are behaviorally identical (two names, same FIFO-of-Results
constructor); both exist because different tasks call different names.

## `FakeClock` (Task 3, `clock.rs`)

Unchanged from Task 3: `new(times: Vec<u64>) -> Self` (canonical), `fixed(t: u64) -> Self`
(infinite constant `t`). **No scalar `new`, no zero-arg `new`.** Consumers wanting a
constant clock use `fixed(t)`.

## `FakeCommandRunner` (Task 7, `exec/mod.rs`)

A comprehensive double serving the detector (availability probes), the feedback loop
(availability + separate run/shell queues), and the engine (constant outcome).

State:
- `run_outcomes: Mutex<VecDeque<RunOutcome>>` — FIFO for `run()`.
- `shell_outcomes: Mutex<VecDeque<RunOutcome>>` — FIFO for non-probe `run_shell()`.
- `available: Mutex<HashSet<String>>` — tools for which a `command -v` probe succeeds.
- `constant: Mutex<Option<RunOutcome>>` — if set, every `run()`/`run_shell()` returns it.
- `run_calls: Mutex<Vec<CommandSpec>>`, `shell_calls: Mutex<Vec<String>>` — recorded inputs.

`default_ok()` helper = `RunOutcome { exit: Some(0), stdout: "", stderr: "", cancelled: false }`.

`run(spec)`:
1. record `spec` into `run_calls`.
2. if `constant` set → return clone.
3. else pop `run_outcomes`; empty → `default_ok()`.

`run_shell(script)`:
1. record `script` into `shell_calls`.
2. if `constant` set → return clone.
3. if `script` contains `"command -v"` → it is an availability probe: take the LAST
   shell-word of `script` as the tool (strip a leading `--`); return `exit: Some(0)` if
   the tool ∈ `available`, else `exit: Some(127)` (both with empty stdout/stderr).
4. else pop `shell_outcomes`; empty → `default_ok()`.

Constructors / builders / observers (all must exist):
| Signature | Meaning |
|-----------|---------|
| `new() -> Self` | empty; all queues empty, no constant |
| `with_outcomes(outcomes: Vec<RunOutcome>) -> Self` | seed `run_outcomes` (canonical Task 7) |
| `available(self, tool: impl Into<String>) -> Self` | builder: add tool to `available` set |
| `always(outcome: RunOutcome) -> Self` | set `constant` |
| `push(&self, outcome: RunOutcome)` | push `run_outcomes` (alias of `push_run`) |
| `push_run(&self, outcome: RunOutcome)` | push `run_outcomes` |
| `push_shell(&self, outcome: RunOutcome)` | push `shell_outcomes` |
| `calls(&self) -> Vec<String>` | all calls in order: run calls as `"tool arg1 arg2"`, shell calls as the script string |
| `run_calls(&self) -> Vec<CommandSpec>` | recorded `run()` specs |
| `shell_calls(&self) -> Vec<String>` | recorded `run_shell()` scripts |

**`on_shell` is removed.** The detector (Task 6) uses `available(tool)` for known tokens;
unknown tokens fall through the probe branch to exit 127 → `RawInput`, which is exactly
the intended semantics.

---

## Per-file consumer fixes (from self-review)

- **Task 4**: `FakeClock::new(0)` → `FakeClock::fixed(0)` (all 4 sites); fix the consume-note
  that says `new(start_ms: u64)` to describe `new(Vec<u64>)`/`fixed(u64)`.
- **Task 5**: `FakeAiProvider::ok(&str)` — now exists (infinite Ok). No change needed beyond
  confirming it compiles against the contract.
- **Task 6**: replace `FakeCommandRunner::new().on_shell(script, outcome)` usage with
  `FakeCommandRunner::new().available("<known-token>")…`; known tokens → `available`,
  unknown tokens are simply not added (probe returns 127 → `RawInput`).
- **Task 8**: `FakeAiProvider::scripted(responses)` (Results) — exists. `.available(tool)`,
  `.push_run`, `.push_shell`, `.run_calls() -> Vec<CommandSpec>`, `.shell_calls()` — all exist.
- **Task 9**: `FakeClock::new(1_700_000_000_000)` and other scalars → `FakeClock::fixed(...)`.
- **Task 11**: `FakeClock::new(0)` → `FakeClock::fixed(0)`.
- **Task 12**: `FakeAiProvider::with_script("A", vec![...])` — exists.
- **Task 13**: `FakeAiProvider::with_responses(...)` and `call_count()` — exist;
  `FakeClock::new(0)` → `FakeClock::fixed(0)`.
- **Task 15**: `FakeAiProvider::always(&str)` — exists; `FakeAiProvider::scripted(Vec<String>)`
  → `FakeAiProvider::scripted_ok(vec![...])`; `FakeClock::new()` → `FakeClock::fixed(0)`;
  `FakeCommandRunner::always(RunOutcome)` — exists.

## Ordering fix (Class D)

Task 6 (detector) depends on Task 7 (exec) types. **Execution order is 1–5, then 7, then 6,
then 8–16.** Add a one-line note under the Task 6 heading: "Implement AFTER Task 7 — consumes
`CommandRunner`/`RunOutcome`/`CancelToken`/`FakeCommandRunner` defined there." Task numbers
stay as labels; only dispatch order changes.
