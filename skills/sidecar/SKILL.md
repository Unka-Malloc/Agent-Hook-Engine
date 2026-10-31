---
name: ahe-sidecar
description: Drive the ahe hook engine per lifecycle event — program file, stdin event payload, persistent session state and stdout harness contract.
---

# ahe Per-event CLI mode

Invoke `ahe run` once per lifecycle event. The program is read from the
explicit program file; the event payload arrives on stdin and the harness
hook contract leaves on stdout. The binary uses the same checker,
evaluator and emitter as the SDK. This is a per-event CLI, not a resident
NDJSON service.

## When to use

- The host is not Rust or wants per-event execution in a separate process.
- The host can supply an explicit harness, event and session state directory.

## Invocation shape

```sh
ahe run --program program.json --harness devin --event stop \
    --state-dir state < event.json > contract.json
```

- Each invocation handles one lifecycle event, not a program-to-artifact compilation.
- Session checkpoints, counters and once-fences persist under `--state-dir`,
  keyed by the payload's `session_id`; repeated invocations are not stateless.
- Diagnostics go to stderr. Invalid invocation or missing required flags,
  an unreadable or unparseable JSON program, and an unknown event exit 2.
- Static checker diagnostics are printed but evaluation continues.
- Refused emission prints `{}` and returns successful process status
  (fail-open), without saving State or executing returned journal/spawn
  effects. Run children may already have executed during evaluation;
  refusal does not roll them back.
- Accepted emission saves State and executes returned effects. State-save
  failures and evaluation violations are diagnostics, not necessarily a
  nonzero exit. SDK hosts can explicitly reject checker and emitter errors.
- Do not add arbitrary host elapsed-time cutoffs to viable invocations.

## Rules that keep the contract

- Parse stdout as the machine-readable harness contract, not diagnostics.
- Keep the host-owned state directory stable for the intended session lifetime.
- Use `ahe::install` for hook installation plans; `ahe run` evaluates an event.
