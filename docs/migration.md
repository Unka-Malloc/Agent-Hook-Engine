# Migration design — Python baseline → Rust runtime

Baseline: the redacted reference copies under `legacy/`. Each item lists what
it does today, the DSL primitives it becomes, and migration notes.

## Order

The dispatcher is the runtime's own shape — port it first so later items
compile into it.

### 1. `legacy/agent-hooks/hook-dispatch.py` → engine core

- Today: single entry per event; fans stdin payload to parallel child
  processes; merges stdout contracts (block wins, contexts concatenated,
  `updatedInput` shallow-merged, unknown keys first-wins); single-child events
  `exec` through.
- Becomes: the executor — registry of handlers per event, parallel fan-out,
  output merge. The merge contract IS the DSL's response semantics.
- Port notes: subprocess children become in-process handlers; keep the
  external-command child type for out-of-tree hooks.

### 2. `legacy/agent-hooks/universal-memory-router.py` → `inject` primitive

- Today: static prompt injected on SessionStart, with multi-contract stdout
  (Antigravity injectSteps / Cursor additional_context / Claude
  hookSpecificOutput) chosen by payload sniffing.
- Becomes: `on session_start: inject <text>`; contract selection moves to the
  emit layer — the DSL never payload-sniffs.

### 3. `legacy/agent-memory/distill-hook.py` → `checkpoint` + `block`/`prompt`

- Today: once-per-turn dedup via prompt_id checkpoint (legacy hosts:
  once-per-session); emits block+instruction so the running agent writes a
  memory-candidate file with hot context.
- Becomes: `on stop: when checkpoint.advances(prompt_id) ->
  prompt distill_contract`. The checkpoint store is a primitive — keyed,
  durable, atomic.
- Port notes: keep "new prompt only" semantics; keep transcript evidence gate
  where the harness supplies a transcript path.

### 4. `legacy/anti-rot/anti-rot-hook.py` → `counters` + `inject` + `spawn` + `journal`

- Today: per-session counters on PostToolUse (repeat commands, re-reads,
  waits, test runs, consecutive failures) with once-per-rule reminders;
  conditional external audit subprocess at high rot scores; SessionEnd journal
  + detached ledger scan.
- Becomes: the largest DSL surface — `counters.observe`, `when <cond>`,
  `inject once`, `spawn`, `journal`, and `prompt`-mediated audit. Port in
  slices: counters+reminders first, audit path last.
- Port notes: its per-Stop journal write should become SessionEnd-only in the
  new engine (today it journals every turn on hook-per-turn harnesses).

### 5. `legacy/agent-memory/install.py` → emit layer

- Today: idempotent installer that writes each host's native hook config
  (nested groups, flat array, TOML block, opencode plugin), backup-first.
- Becomes: the DSL→host-config compiler. Its host-shape table is the direct
  specification for emitters.
- Port notes: `POLL_ONLY` classification was wrong about Devin (it has a full
  hook surface); correctness now lives in the capability matrix, not a static
  list.

### 6. `legacy/agent-memory/poll.py` → `poll` primitive

- Today: incremental transcript scanner for hosts without hooks, producing
  raw evidence packages for adjudication.
- Becomes: the cold-path runtime job for `Support::Unsupported` events — the
  engine still covers a host even when no hook can be installed.

## Out of scope (memory domain, not hook domain)

`adjudicate.py` and `mem.py` consume what the distill hook produces; they are
the downstream pipeline, not hook behavior. The engine's boundary ends where a
hook's effect lands (a candidate file written, context injected, an action
blocked).
