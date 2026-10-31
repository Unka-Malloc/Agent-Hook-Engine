---
name: ahe-sdk
description: Embed the ahe hook engine in a Rust host — program model, static checks, evaluation, per-harness emission, capability matrix.
---

# ahe SDK mode

Embed `ahe` and run hook programs through the library pipeline.

## When to use

- The host is Rust and wants the DSL in-process: parse, check, evaluate,
  emit, inspect the capability matrix.
- The host owns event delivery and placement of installation artifacts.

## Canonical calls

```rust
let program = ahe::model::Program::from_json(&doc)?;
let errors = ahe::check::check_program(&program);
if !errors.is_empty() { return Err(format!("{errors:?}").into()); }
let response = ahe::eval::run_program(&program, event, &payload, &mut state);
if !response.violations.is_empty() { return Err(response.violations.join("; ").into()); }
let contract = ahe::emit::emit(harness, event, &response).map_err(|errors| errors.join("; "))?;
let matrix = ahe::caps::capabilities(harness);
let installation = ahe::install::adapt(&surface, &hooks);
```

The host supplies the document, event, payload and mutable State, and persists State when needed. See crates/ahe/examples/sdk.rs for a complete runnable fixture. Installation declarations and runtime DSL programs have separate wire contracts; installation support is not proof of runtime action support.

## Rules that keep the contract

- A capability a harness cannot carry reports **unsupported** through the
  matrix — never silently dropped. Consult `caps::capabilities(harness)` before
  promising a mechanism on a given harness.
- The emitter lowers semantic steps to each harness's native hook shape;
  the same program is valid input for every harness — degradation is
  declared, not improvised.
- Evaluation preserves program ordering but environment, time, file and Run expressions depend on their declared inputs. The SDK host owns State persistence and execution of returned effects.
