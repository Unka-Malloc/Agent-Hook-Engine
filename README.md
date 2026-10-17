# Hook Engine

A Rust runtime that turns agent lifecycle hooks into a domain-specific
language. One hook definition is written once in the DSL; the engine maps it
onto each agent harness's native hook surface (Devin, Claude Code, Codex,
Cursor, Kimi, opencode, and hosts with no hook surface at all).

Hook Engine is intended to become the hook capability of
Adapter-For-All-Agents (A4AA); it is developed standalone first so the DSL and
the harness mapping can settle before being absorbed.

## Current state

Collection and migration-design phase. The Rust crate is a seed containing the
event/action taxonomy; the Python hooks it replaces are gathered under
`legacy/` as the migration baseline.

- `legacy/` — redacted reference copies of the in-production Python hooks
- `crates/hook-engine` — the runtime crate (seed)
- `docs/` — DSL design, per-harness capability matrix, migration plan

## Run and verify

```
scripts/verify.sh    # fmt + clippy + tests
```

## Documentation

Entry point: `docs/README.md`.
