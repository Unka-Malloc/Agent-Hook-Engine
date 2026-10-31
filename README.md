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

AHE owns hook declarations, installation planning and hook DSL lowering.
A4AA consumes that installation contract and harness catalog while retaining
Agent session driving and callbacks. The host writes returned installation
artifacts, executes Agent sessions and applies the capabilities its harness
supports. AOR owns workflow gates, not hook adaptation. Probator and GitVault
are optional local tools, not AHE runtime or build dependencies.

Consume AHE through the Rust SDK or the per-event `ahe run` CLI; both share
the runtime pipeline. The CLI persists session state but is not a resident
NDJSON service. See [integration modes](docs/integration-modes.md) for the
installation boundary and the CLI's diagnostic and fail-open behavior.

## Crate layout

One `ahe` crate contains the shared library modules and the CLI:

- `model` — the DSL AST (JSON program → handlers → steps/conditions)
- `check` — static semantics: expression typing + event×action legality
- `eval` — dynamic semantics: total expression eval, ordered step fold, the
  `Response` merge monoid, bounded `run` children, per-session state
- `emit` — lowering `↓_a`: `Response` → a concrete host contract
  (generic `hookSpecificOutput` family, cursor's flat `continue` shape,
  antigravity's per-event contract family: PreToolUse →
  `decision`/`reason`/`overwrite`, PostToolUse → `{}` only,
  PreInvocation/PostInvocation → `injectSteps[].ephemeralMessage` +
  `terminationBehavior`, Stop → `decision`), failing closed on
  Unsupported/Unknown action kinds
- `caps` — the probed per-harness capability matrix
- `install` — hook *installation*, a contract separate from the runtime
  DSL: kebab-case `install::HookDecl`/`install::HookSurface` declarations
  lowered by `install::adapt` into user-home-relative artifacts and a
  support report; per-harness store descriptors live in
  `install::harnesses`. Installation coverage is not runtime action
  support — `caps` stays authoritative for DSL actions
- `bin/ahe` — the invocation entrypoint: stdin payload → run → contract
  JSON on stdout; state persists per session under `--state-dir`

## Install

```
cargo install ahe
```

## Usage

```
ahe run --program program.json --harness devin --event post_tool_use \
    --state-dir ~/.local/state/ahe < payload.json
```

`examples/memory-distill.json` is a complete real-world hook program
(incremental memory distillation) expressed in the DSL.

## Run and verify

```
scripts/verify.sh    # fmt + clippy + tests
```

## Documentation

Entry point: [docs/README.md](docs/README.md).
