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

## Environment fidelity (hard rule)

Agent infrastructure never launches an agent with an empty or minimal
"no variables" environment — the user's machine is the source of
truth. Presets, cards and specs declare how every needed value is
*resolved* from the real environment: host environment variables
(PATH and friends), values inside configuration files (TOML, YAML or
JSON — read by key lookup, never copied by hand), and every model or
provider the agent can use. The custody layer resolves those
declarations into concrete values before spawn; the child receives
the user's real environment plus explicit overrides.

An agent started "clean" loses the user's configuration, credentials,
providers and tools — that is a defect, not hardening. Security lives
in *where* values resolve (locally, at the custody boundary) and
*what* may cross a remote boundary (literal values, not references) —
never in running with no environment at all.

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
