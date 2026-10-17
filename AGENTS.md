# Agent rules for Hook Engine

## Core principle (applies to this and every adapter project)

Prioritize extracting universal primitives that can cover all agents.
Universal primitives do not mean rejecting a harness's special capabilities —
a special capability is converted into a primitive that other harnesses may
record as "unsupported". Unsupported does not mean the harness cannot benefit
from the primitive; the capability matrix that results is the scoreboard that
shows which harnesses expose hooks well and which do not.

## Verify

`scripts/verify.sh` — fmt + clippy + tests across the workspace.
