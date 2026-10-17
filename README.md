# AHE — Agent Hook Engine

A Rust runtime that turns agent lifecycle hooks into a domain-specific
language. One hook definition is written once in the DSL; the engine maps it
onto each agent harness's native hook surface (Devin, Claude Code, Codex,
Cursor, Kimi, opencode, and hosts with no hook surface at all).

The primitive set is a **superset** of every harness's native mechanism —
adaptation selects the subset each host can carry, and the capability matrix
that falls out of that is the scoreboard for how well each harness exposes
hooks. The design axioms and the complete formal specification (EBNF grammar,
typing rules, big-step semantics, and proofs) live in
[`docs/dsl-design.md`](docs/dsl-design.md).

AHE is developed standalone so the DSL and the harness mapping can settle;
it is designed to be embedded as the hook capability of a larger agent
adapter.

## Crate layout

- `model` — the DSL AST (JSON program → handlers → steps/conditions)
- `check` — static semantics: expression typing + event×action legality
- `eval` — dynamic semantics: total expression eval, ordered step fold, the
  `Response` merge monoid, bounded `run` children, per-session state
- `emit` — lowering `↓_a`: `Response` → a concrete host contract
  (generic claude/codex/kimi/devin family + cursor's flat shape), failing
  closed on Unsupported/Unknown action kinds
- `caps` — the probed per-harness capability matrix
- `bin/ahe` — the invocation entrypoint: stdin payload → run → contract
  JSON on stdout; state persists per session under `--state-dir`

## Install

```
cargo install ahe
```

## Usage

```
ahe run --program program.json --harness devin --event post_tool_use \
    --state-dir ~/.agent-hooks/state < payload.json
```

`examples/memory-distill.json` is a production hook (incremental memory
distillation) already expressed in the DSL.

## Run and verify

```
scripts/verify.sh    # fmt + clippy + tests
```

## Documentation

Entry point: `docs/README.md`.
