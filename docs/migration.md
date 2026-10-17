# Migration design — Python baseline → Rust runtime

Baseline: the redacted reference copies under `legacy/`. Each item lists what
it does today, the DSL primitives it becomes, and migration notes. Status is
marked per item.

## Order

The dispatcher is the runtime's own shape — port it first so later items
compile into it.

### 1. `legacy/agent-hooks/hook-dispatch.py` → engine core — **landed**

- Landed as `eval::run_program` + `Response::merge`: ordered handler fold,
  the monoid merge (block wins, contexts concatenated, patch shallow-merge),
  child-contract parsing in `from_contract`, and `run`'s bounded subprocess
  whose stdout merges back — exactly the dispatcher's fan-out, in-process.
- The `ahe` binary's stdin→contract-stdout loop is the deployment
  shape a `hook-dispatch` entrypoint compiled to.
- Remaining gap vs the Python dispatcher: `run` children execute serially
  inside the fold, not in parallel — parallelism is an optimization layer
  (the merge order is fixed by program order, so results are identical).

### 2. `legacy/agent-hooks/universal-memory-router.py` → `inject` primitive

- Today: static prompt injected on SessionStart, with multi-contract stdout
  (Antigravity injectSteps / Cursor additional_context / Claude
  hookSpecificOutput) chosen by payload sniffing.
- Becomes: `on session_start: inject <text>`; contract selection moves to the
  emit layer — the DSL never payload-sniffs.
- Status: **landed** — prompt texts live in the per-host program files;
  emit covers every shape the router emitted.

### 3. `legacy/agent-memory/distill-hook.py` → `checkpoint` + `block`

- Becomes: `on stop: when checkpoint.advances(prompt_id) -> block
  distill_contract` — fully expressed in `examples/memory-distill.json`,
  executable end-to-end via `ahe run` (verified: new prompt_id
  blocks once with the distill instruction, repeat prompt_id passes).
- Transcript gate (parity): the baseline skips sessions whose transcript
  exists but contains no user-role line; the program reproduces it as
  `or[ file_size(transcript)<1, contains(file_tail(transcript,2MB),'"user"') ]`.
  The tail window bounds the read; transcripts larger than 2 MB with the
  last user line beyond it are the only divergence (baseline read the
  whole file — unbounded by design).
- Status: **landed** — wired in production for devin/claude/codex/cursor/
  kimi (`~/.config/<host>` → `ahe run --program ~/.agent-hooks/programs/<host>.json`);
  legacy scripts deleted after zero-config-reference check.

### 4. `legacy/anti-rot/anti-rot-hook.py` → `counters` + `inject` + `spawn` + `journal`

- Landed: `counter_add`/`counter_set`/`counter_sum`/`counter_count`,
  `list_push`/`list_*`, `when`/`once`, `inject`, `spawn`, `journal`,
  `run capture` — the rule table is encoded in the per-host program files
  (`~/.agent-hooks/programs/*.json`), verified against the baseline's
  thresholds (repeat_cmd 4, repeat_read/wait/fail 3, test_runs 6).
- Port notes: its per-Stop journal write is SessionEnd-only in the new
  engine (baseline journaled every turn on hook-per-turn harnesses);
  conditional external audit is preserved via `run` with `capture`.

### 5. `legacy/agent-memory/install.py` → emit layer

- Becomes: the DSL→host-config compiler. `emit()` covers the response half
  (generic + cursor contracts); config-file generation (nested groups, flat
  array, TOML block, opencode plugin) is pending.
- Port notes: `POLL_ONLY` was wrong about Devin; correctness lives in `caps`.

### 6. `legacy/agent-memory/poll.py` → `poll` primitive

- Cold-path runtime job for `Support::Unsupported` events; not yet ported.

## Out of scope (memory domain, not hook domain)

`adjudicate.py` and `mem.py` consume what the distill hook produces; they are
the downstream pipeline, not hook behavior. The engine's boundary ends where a
hook's effect lands (a candidate file written, context injected, an action
blocked).
