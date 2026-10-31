# Integration modes

The Rust SDK and per-event `ahe run` CLI share the model, checker,
evaluator and emitter pipeline. A Rust host embeds the crate; other hosts
can invoke the CLI once per lifecycle event with persistent session state.
Installation planning is a separate contract, not runtime evaluation.

The crate carries two distinct contracts — do not conflate them:

- **Runtime DSL** — a hook *program* written in the DSL
  ([`dsl-design.md`](dsl-design.md)) is checked, evaluated per event and
  lowered onto a harness's hook-output contract. Events are snake_case
  (`ahe::HookEvent`); per-harness DSL support lives in `caps`.
- **Installation** — a hook *command* is declared once
  (`ahe::install::HookDecl`, kebab-case `install::HookEvent`) and
  `install::adapt` lowers it onto a harness's hook *store*
  (`install::HookSurface`, catalogued in `install::harnesses`) as
  user-home-relative config/extension artifacts plus a support report.

Installing a command is coverage of a store, not a claim that the harness
enforces DSL actions — `caps` stays the only authority for runtime action
support.

## SDK — embed the crate

```toml
[dependencies]
ahe = "0.1"
```

The [SDK example](../crates/ahe/examples/sdk.rs) is a complete runnable
fixture covering both contracts. It explicitly rejects checker errors
before evaluation, checks response violations and handles emitter errors.
It runs only a deterministic in-process fixture, never the emitted command:

```sh
cargo run -p ahe --example sdk
```

Installation planning returns data without writing a harness configuration.
AHE owns the declarations and installation catalog consumed by A4AA; A4AA
retains session driving and callbacks. The host writes the returned artifacts
and applies the harness's capabilities.

Evaluation preserves program ordering, but environment, time, file and Run
expressions depend on their declared inputs; it is not a pure function of
the event. Run expressions may execute child processes during evaluation.
The SDK host owns event delivery, durable `State` storage and execution of
returned effects. AHE does not provide an Agent-session driver or credential
custody; the host must authorize the program and its declared commands.

## Per-event CLI — ahe run

```sh
ahe run --program program.json --harness devin --event stop \
    --state-dir state < event.json
```

One process handles one lifecycle event: the program JSON on disk, the
event payload on stdin, the harness's hook contract on stdout. It is not a
resident NDJSON service or a stateless filter. Session
state (checkpoints, counters, once-fences) persists under `--state-dir`
keyed by `session_id`. Diagnostics go to stderr; the document on stdout is
the contract.

The CLI's diagnostic policy differs from an SDK host that rejects errors:

- Invalid invocation or missing required flags, an unreadable or unparseable
  JSON program, and an unknown event exit **2**.
- Static checker diagnostics are printed, but evaluation continues.
- Refused emission prints diagnostics and **`{}`** with successful process
  status (fail-open). That invocation does not save `State` or execute
  returned journal/spawn effects. Run children may already have executed
  during evaluation; refusal does not roll them back.
- On accepted emission, the CLI saves session state and executes returned
  effects. State-save failures and evaluation violations are diagnostics,
  not a guarantee of a nonzero exit.

Do not infer semantic acceptance from process success alone. A host needing
explicit checker/emitter rejection can enforce that policy through the SDK.

## Skills

- [SDK skill](../skills/sdk/SKILL.md) — embedding guidance (program model, checks, evaluation,
  emission, capability matrix).
- [Per-event CLI skill](../skills/sidecar/SKILL.md) — stdin/stdout discipline,
  session persistence and error handling.
