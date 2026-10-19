# Migration design — prior hook pipeline → Rust runtime

Baseline: a private hook pipeline ported to this engine. Each item lists what
it did, the DSL primitives it became, and migration notes. The reference
scripts were used during the port and are retained privately; this document
records the port contract, not the private system.

## Order

The dispatcher is the runtime's own shape — port it first so later items
compile into it.

### 1. Dispatcher fan-out → engine core — **landed**

- Landed as `eval::run_program` + `Response::merge`: ordered handler fold,
  the monoid merge (block wins, contexts concatenated, patch shallow-merge),
  child-contract parsing in `from_contract`, and `run`'s bounded subprocess
  whose stdout merges back — exactly the dispatcher's fan-out, in-process.
- `from_contract` covers every contract family the engine emits (generic,
  cursor flat, antigravity per-event), and unmapped known values surface on
  the violations channel — parity with the dispatcher's first-seen-wins
  forward semantics.
- The `ahe` binary's stdin→contract-stdout loop is the deployment
  shape a dispatch entrypoint compiled to.
- Remaining gap vs the prior dispatcher: `run` children execute serially
  inside the fold, not in parallel — parallelism is an optimization layer
  (the merge order is fixed by program order, so results are identical).

### 2. Prompt-injection router → `inject` primitive

- Did: a static prompt injected on SessionStart, with multi-contract stdout
  (injectSteps / additional_context / hookSpecificOutput) chosen by payload
  sniffing.
- Becomes: `on session_start: inject <text>`; contract selection moves to the
  emit layer — the DSL never payload-sniffs.
- Status: **landed** — prompt texts live in per-host program files; emit
  covers every shape the router emitted.

### 3. Session-memory distill hook → `checkpoint` + `block`

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
- Status: **landed** — wired for devin/claude/codex/cursor/kimi
  (`~/.config/<host>` → `ahe run --program <hooks-dir>/programs/<host>.json`).

### 4. Session-discipline guard hook → `counters` + `inject` + `spawn` + `journal`

- Landed: `counter_add`/`counter_set`/`counter_sum`/`counter_count`,
  `list_push`/`list_*`, `when`/`once`, `inject`, `spawn`, `journal`,
  `run capture` — the rule table is encoded in per-host program files
  (`<hooks-dir>/programs/*.json`), verified against the baseline's
  thresholds (repeat_cmd 4, repeat_read/wait/fail 3, test_runs 6).
- Port notes: its per-Stop journal write is SessionEnd-only in the new
  engine (baseline journaled every turn on hook-per-turn harnesses);
  conditional external audit is preserved via `run` with `capture`.

### 5. Config installer → emit layer

- Becomes: the DSL→host-config compiler. `emit()` covers the response half
  (generic + cursor + antigravity contracts); config-file generation
  (nested groups, flat array, TOML block, plugin manifests) is pending.
- Port notes: the baseline's event-surface table was wrong about one host;
  correctness lives in `caps`.

### 6. Incremental fallback poller → `poll` primitive

- Cold-path runtime job for `Support::Unsupported` events; not yet ported.
