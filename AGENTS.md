# Agent rules for Hook Engine

## Core principle (applies to this and every adapter project)

Prioritize extracting universal primitives that can cover all agents.
Universal primitives do not mean rejecting a harness's special capabilities —
a special capability is converted into a primitive that other harnesses may
record as "unsupported". Unsupported does not mean the harness cannot benefit
from the primitive; the capability matrix that results is the scoreboard that
shows which harnesses expose hooks well and which do not.

**Superset relation.** The primitive set `P` is a superset of every harness's
native hook mechanism `M_a`: every native mechanism `m ∈ M_a` must be
expressible as some `p ∈ P` (`↓_a(p) = m`), never the reverse. The language is
designed top-down as the union of all observed mechanisms plus needed
generalizations; per-harness adaptation then selects the subset the host can
actually carry. Design docs record these axioms verbatim so the contract does
not drift between deliveries.

## Verify

`scripts/verify.sh` — fmt + clippy + tests across the workspace.

## Signature ordering (hard rule)

Finish the work first. A signature, unlock or blessing is requested only
after the implementation is complete AND `scripts/verify.sh` AND the four
publication self-checks pass — i.e. at the push/PR boundary, never earlier.

Two consequences agents keep getting wrong:

- A signing gate printing a stale-config demand on a local `git commit`
  is an environment note, not a user task. Keep working; leave changes
  uncommitted, and do not surface a signature ask.
- If a mid-work signature request appears at all, it is a sequencing bug:
  audits have not run, so there is no evidence to bless. Report the state,
  continue the work.
