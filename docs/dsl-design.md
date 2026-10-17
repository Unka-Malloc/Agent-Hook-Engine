# AHE — Agent Hook Engine DSL: formal specification

The language has two orthogonal vocabularies inside one grammar: the
**lifecycle** axis (`on`) selects when a handler fires; the **mechanism** axis
(`steps`) selects what it does. Both are one language — one parser, one
evaluation pipeline, one merge contract. The axes interact at exactly two
points: the fired event determines the payload's schema, and a legality
relation constrains which actions are meaningful on which events.

## 0. Language–host relation (axiom)

Let `P` be the primitive set — events ∪ step kinds ∪ expression forms — and
`M_a` the native hook mechanism set of harness `a`. The language is the
**superset**:

```
Axiom (coverage).  ∀a, ∀m ∈ M_a. ∃p ∈ P : ↓_a(p) = m
  Every native mechanism is representable. The primitive set is designed
  top-down — the union of all observed mechanisms plus needed
  generalizations — never cut to the intersection or to one host's limits.

↓_a : P ⇀ M_a     (lowering, partial)
  ↓_a(p) undefined ⇔ the capability matrix marks p Unsupported for a.
  Unknown forbids any lowering claim until probed — it is neither
  supported nor unsupported.

coverage_a = |dom(↓_a)| / |P|
  The capability matrix is this metric materialized per (event, action)
  cell — the scoreboard of which harnesses expose hooks well.
```

Per-harness adaptation therefore selects a **subset** of `P` — what `a` can
carry — and a primitive a host lacks stays in the language, marked
Unsupported for that host, not removed.

## 1. Concrete syntax (EBNF)

Programs are JSON values. Steps and expressions are tagged objects with a
common `op` key; `text` and argv positions accept either a literal string or
an expression (`Text`, `ArgvItem` — untagged unions).

```ebnf
program    = "{" '"name"' ":" string [ "," '"handlers"' ":" handlers ] "}" ;
handlers   = "[" [ handler { "," handler } ] "]" ;
handler    = "{" '"on"' ":" event [ "," '"matcher"' ":" string ]
               "," '"steps"' ":" steps "}" ;
steps      = "[" [ step { "," step } ] "]" ;
matcher    = string ;                        (* regex; tool events only *)

step       = "{" '"op"' ":" step-tag "," step-body "}" ;
step-tag   = '"inject"' | '"system_message"' | '"block"' | '"approve"'
           | '"rewrite"' | '"followup"' | '"spawn"' | '"run"'
           | '"journal"' | '"checkpoint_set"'
           | '"counter_add"' | '"counter_set"' | '"list_push"'
           | '"when"' | '"once"' ;
step-body  = '"text"'   ":" text                             (* inject, system_message, followup *)
           | '"reason"' ":" text                             (* block   *)
           |                                                   (* approve *)
           | '"patch"'  ":" object                           (* rewrite *)
           | '"argv"'   ":" argv-list                        (* spawn   *)
           | '"argv"'   ":" argv-list [ "," '"timeout_ms"' ":" number ]
               [ "," '"capture"' ":" string ]                (* run     *)
           | '"sink"'   ":" expr "," '"fields"' ":" expr     (* journal *)
           | '"key"'    ":" string "," '"value"' ":" expr    (* ckpt_set*)
           | '"name"'   ":" expr   "," '"delta"' ":" expr    (* ctr_add *)
           | '"name"'   ":" expr   "," '"value"' ":" expr    (* ctr_set *)
           | '"key"'    ":" string "," '"value"' ":" expr
               [ "," '"max"' ":" number ]                    (* list_push *)
           | '"cond"'   ":" expr   "," '"then"'   ":" steps  (* when    *)
           | '"key"'    ":" string "," '"then"'   ":" steps  (* once    *) ;

text       = string | expr ;                 (* human-facing text *)
argv-list  = "[" [ argv-item { "," argv-item } ] "]" ;
argv-item  = string | expr ;                 (* evaluated → compact JSON *)

expr       = "{" '"op"' ":" expr-tag "," expr-body "}" ;
expr-tag   = '"lit"' | '"payload"' | '"checkpoint"' | '"counter"' | '"env"'
           | '"eq"' | '"ne"' | '"lt"' | '"le"' | '"gt"' | '"ge"'
           | '"contains"' | '"matches"' | '"and"' | '"or"' | '"not"'
           | '"add"' | '"sub"' | '"mul"' | '"min"' | '"max"' | '"now"'
           | '"len"' | '"take"' | '"replace"' | '"file_tail"'
           | '"format"' | '"obj"' | '"arr"' | '"field"'
           | '"list_len"' | '"list_count"' | '"list_count_max"'
           | '"list_last"' | '"list_unique"'
           | '"first"' | '"file_size"' | '"json_extract"'
           | '"counter_sum"' | '"counter_count"' | '"fired_count"' ;
expr-body  = '"v"'    ":" value                              (* lit       *)
           | '"path"' ":" string                             (* payload, file_* paths are exprs *)
           | '"key"'  ":" string                             (* ckpt/counter/list_* *)
           | '"name"' ":" string                             (* env       *)
           | '"a"' ":" expr "," '"b"' ":" expr               (* binary    *)
           | '"re"' ":" string [ "," '"with"' ":" string ] "," '"on"' ":" expr
                                                             (* matches / replace *)
           | '"args"' ":" "[" [ expr {"," expr} ] "]"        (* and/or/arr/first *)
           | '"arg"'  ":" expr                               (* not       *)
           | '"n"'    ":" number "," '"on"' ":" expr         (* take      *)
           | '"path"' ":" expr "," '"bytes"' ":" expr        (* file_tail *)
           | '"template"' ":" string "," '"vars"' ":" object-of-expr  (* format *)
           | '"fields"' ":" object-of-expr                   (* obj       *)
           | '"on"'    ":" expr "," '"path"' ":" string      (* field     *)
           | '"eq"'    ":" expr                              (* list_count needle *)
           | '"prefix"' ":" expr [ "," '"ge"' ":" expr ]
               [ "," '"offset"' ":" int ]                    (* counter_sum *)
           | '"prefix"' ":" expr ;                           (* counter_count *)

event      = '"session_start"' | '"session_end"' | '"user_prompt_submit"'
           | '"pre_tool_use"' | '"post_tool_use"' | '"post_tool_use_failure"'
           | '"permission_request"' | '"stop"' | '"stop_failure"'
           | '"subagent_start"' | '"subagent_stop"'
           | '"pre_compaction"' | '"post_compaction"'
           | '"notification"' | '"idle"' | '"teammate_idle"'
           | '"pre_shell_exec"' | '"pre_mcp_exec"' | '"post_agent_response"' ;
```

## 2. Values, types, state

```text
Val  ::= null | bool | num | str | arr | obj
Ty   ::= Null | Bool | Num | Str | Arr | Obj | Json        (Json ⊇ all; the
         dynamically-typed position of payload/checkpoint/field/first/

         list_last/json_extract reads)

Ω    ::= observable inputs: payload p, environment E, filesystem F,
         clock t, run-child outputs C
Σ    ::= (Ckpt : Key → Val, Ctr : Key → Int, List : Key → Val*, Fired : Set<Key>)
Rsp  ::= host response record — the merge monoid carrier:
         decision ∈ {∅, approve, block} (join = max, block top)
         reasons, contexts, followups, system : Str*   (concat)
         patch : Obj                                    (shallow merge)
         spawns : (Str*)* , journal : (Str × Obj)*       (intents)
         violations : Str*                               (observable ⊥)
```

`Expr` evaluation is *observational*: `now`, `env`, `file_tail`, `file_size`
read Ω; `run` executes a child of Ω. `⟦expr⟧` is pure in `(Ω, Σ)`; all
mutations happen only at step granularity on `Σ`.

## 3. Static semantics (typing / well-formedness)

Expression typing `Γ ⊢ e : T` (the `check::ty` judge; `Json` positions are
dynamic):

```
Γ ⊢ lit v : ty(v)              Γ ⊢ payload p : Json      Γ ⊢ ckpt k : Json
Γ ⊢ counter k : Num            Γ ⊢ env n : Str           Γ ⊢ now : Num
Γ ⊢ field e p : Json           Γ ⊢ first a* : Json       Γ ⊢ list_last k : Json
Γ ⊢ json_extract e : Json      Γ ⊢ fired_count : Num

Γ ⊢ aᵢ : Bool | Json (∀i)      Γ ⊢ a : Bool | Json
---------------- T-And/Or      ----------------- T-Not
Γ ⊢ and/or a* : Bool           Γ ⊢ not a : Bool

Γ ⊢ a : Num|Str|Json  Γ ⊢ b : Num|Str|Json   ¬(lit-Num vs lit-Str)
------------------------------------------------- T-Ord
Γ ⊢ lt|le|gt|ge a b : Bool
Γ ⊢ eq|ne a b : Bool                            (any operands — total)

Γ ⊢ a : Str|Arr|Json           Γ ⊢ a : Str|Json          Γ ⊢ re valid regex
----------- T-Contains         ------------ T-Matches    (checked statically)
Γ ⊢ contains a b : Bool        Γ ⊢ matches re a : Bool

Γ ⊢ a : Num|Json  Γ ⊢ b : Num|Json        Γ ⊢ on : _
------------------- T-Arith               -------- T-Len (Num),
Γ ⊢ add|sub|mul|min|max a b : Num          take→Str, replace→Str (re valid)

Γ ⊢ path : _   Γ ⊢ bytes : Num|Json       Γ ⊢ varsᵢ : _ (∀i)
----------------- T-FileTail              ---------------- T-Format
Γ ⊢ file_tail p b : Str                    Γ ⊢ format t vars : Str

Γ ⊢ fieldsᵢ : _ (∀i)    Γ ⊢ argsᵢ : _ (∀i)
---------------- T-Obj  ------------------ T-Arr
Γ ⊢ obj f : Obj         Γ ⊢ arr a* : Arr

Γ ⊢ list_len k : Num    Γ ⊢ list_count k e : Num   Γ ⊢ list_count_max k : Num
Γ ⊢ list_unique k : Num  Γ ⊢ counter_sum p .. : Num  Γ ⊢ counter_count p : Num
Γ ⊢ file_size p : Num    Γ ⊢ env n : Str
```

Step well-formedness `e; Γ ⊢ s` under fired event `e` — legality relation
`L ⊆ Event × StepKind` (§6):

```
e ∈ {pre_tool_use, pre_shell_exec, pre_mcp_exec}     e = permission_request
------------------------- S-Rewrite                ------------- S-Approve
e;Γ ⊢ rewrite o                                      e;Γ ⊢ approve

e ∈ block-legal events (§6)
--------------- S-Block
e;Γ ⊢ block r

Γ ⊢ c : Bool|Json   e;Γ ⊢ sᵢ (∀i)        e;Γ ⊢ sᵢ (∀i)
----------------------------- S-When     --------------- S-Once
e;Γ ⊢ when c s*                          e;Γ ⊢ once k s*

Γ ⊢ n : _   Γ ⊢ d : Num|Json             Γ ⊢ sink : _   Γ ⊢ fields : _
---------------------------- S-Counter   ---------------------------- S-Journal
e;Γ ⊢ counter_add n d                    e;Γ ⊢ journal sink fields

handler.matcher set ⇒ e ∈ {pre_tool_use, post_tool_use,
                           post_tool_use_failure}        S-Matcher

All remaining step kinds are legal on every event; every embedded Text/
ArgvItem/Expr position is typed by the same judge.
```

`check_program` collects every violated premise; an empty vector means the
program is well-formed (the premise of §5 Theorem 2). At invocation time the
CLI *reports* errors and still runs — degrade-over-deny is the operational
contract; the static judge's soundness role is rejecting programs that are
*provably* broken before they are shipped.

## 4. Dynamic semantics (big-step)

```text
⟦expr⟧_{Ω,Σ}  → Val × violations*       (total; ill-typed operands yield a
                                         defined fallback + a recorded
                                         violation, never a stuck evaluator)
⟦step*⟧_{e,Ω} : Σ → Σ' × Rsp            (ordered fold; state steps mutate Σ,
                                         action steps append to Rsp,
                                         when/once gate sub-folds)
⟦program⟧_e   = ⟦steps⟧ of every handler with on = e whose matcher holds,
                ⊕-merged in program order

truthy(v): false ⇔ v ∈ {null, false}; every other value true.
lookup(path): dotted access; camelCase/snake_case segment names coalesce;
              absent → null.

run argv timeout_ms capture:
  child stdin := event payload; wall-clock bound := timeout_ms (default 15s)
  capture unset : stdout parses through the shared contract → ⊕ into Rsp;
                  timeout/crash/non-contract output contributes ∅ + violation
  capture k     : stdout parses as plain JSON → Ckpt[k]; prose output stores
                  the raw string under k (extraction is the program's job)

Decision join: ∅ < approve < block — block is absorbing.
Merge ⊕: fieldwise — decision by max, all list fields concatenated in order,
patch shallow object merge; `captured` is transient plumbing, not in Rsp.

Intents: spawns, journal lines, and state persistence are *returned* to the
host shim and performed only after `emit` accepts the response — a refused
emission commits nothing (no checkpoint, no journal, no spawn).
```

## 5. Metatheory

**Assumption A1 (serialization).** The harness serializes hook invocations per
session — two hook calls never overlap state writes. Holds on every observed
harness; required only by `once`. State files are written write-tmp-rename,
so a crashed invocation leaves Σ atomic.

**Lemma 1 (merge monoid).** `(Rsp, ⊕, ∅)` is a monoid. *Proof.* Fieldwise:
`decision` joins by `max` on the chain ∅ < approve < block — associative,
identity ∅; `reasons/contexts/followups/system/spawns/journal/violations`
concatenate — associative, identity `[]`; `patch` shallow-merges —
associative, identity `{}`. `captured` is excluded by definition. ∎

**Lemma 2 (block absorbing).** `decision(r₁ ⊕ r₂) = block ⇔ decision(rᵢ) =
block for some i`. *Proof.* Immediate from `max` on the chain. ∎

**Theorem 1 (termination).** For every well-formed program `P`, event `e`,
and observable input Ω: `⟦P⟧_e` terminates. *Proof.* Structural induction on
the step tree. The grammar has no recursion or loop form; `when`/`once`
strictly decrease the tree. Each step is constant-work except: `run`, bounded
by `timeout_ms` with a kill on expiry; `counter_sum`/`counter_count`, which
iterate `Ctr` — finite per Σ; `list_*`, which iterate finite lists; `format`/
`obj`/`arr`, bounded by their static arity; `file_tail`, bounded by `bytes`;
`file_size`, one `stat`; regexes — the engine's regex dialect is the
linear-time Rust `regex` crate (no backreferences/backtracking), so `matches`/
`replace`/`matcher` are linear in the input. `⟦expr⟧` is structurally
recursive, hence total. ∎

**Theorem 2 (progress and well-formedness of output).** If `check_program P
= []`, event `e` fires with payload `p : P_e`, then `⟦P⟧_e` evaluates to a
well-formed `Rsp` — every field carries values of its declared shape — and
evaluation never gets stuck: every residual type failure surfaces in
`violations`, which is part of the observable output, not a crash path.
*Proof.* `⟦expr⟧` is total by definition (each ill-typed operand form has a
defined fallback: ordering/containment mismatches yield `false`, arithmetic
mismatches yield `null`, etc., each pushing a violation). Steps fold
totally; actions append only values of their field's type (`inject`/`followup`
text is coerced to `Str`, `counter` deltas coerced to `Int`, `journal` fields
coerced to `Obj`). `run` failure modes return `∅` + violation. ∎

**Lemma 3 (zero violations under instantiation).** If additionally every
`Json`-typed position's runtime value conforms to what its use-site expects
(e.g. an `add` operand that is `payload x` actually reads a number), then
`violations = []`. *Proof.* The static judge rejects every *provably* ill-
typed program (mismatched literal comparisons, `Bool` operands that can never
be boolean, non-Num arithmetic on statically-typed positions, invalid
regexes, illegal event×action pairs). The only violations reachable past the
judge are those requiring dynamic `Json` positions to disagree with their
use — excluded by the premise. ∎

**Theorem 3 (determinism).** Fixed Ω — payload, environment, filesystem,
clock readings, and `run`-child outputs — and fixed Σ imply a unique `Rsp`.
*Proof.* `⟦expr⟧` is a pure function of `(Ω, Σ)`; steps fold in list order;
`⊕` is a function. ∎

**Theorem 4 (no silent drops).** An action whose kind the harness marks
`Unsupported` or `Unknown` never reaches the wire — `emit_a` returns `Err`
and the invocation commits nothing. *Proof.* `emit_a` is total over `Rsp`
only after the check `∀a ∈ used_kinds(rsp). support_a(a) ∈ {Supported,
Partial}`; on failure it returns the refusal list and the shim performs no
side effect (intents are executed after `emit` only). ∎

**Theorem 5 (once-fence).** `once k [s]` executes `s` at most once per
(session, `k`). *Proof.* The `once` guard reads and inserts `k` into `Fired`
atomically inside one invocation (A1); `Fired` persists for the session. ∎

**Corollary (lowered soundness).** For any harness `a`, event `e`, well-
formed `P`, and fixed Ω: the wire value `emit_a(⟦P⟧_e)` is either a
well-formed contract of `a`'s family carrying exactly the supported content
of the merged response, or an explicit refusal — never a partial, dropped,
or silently-degraded contract. *Proof.* Composition of Theorems 2–4 and the
coverage axiom: every action emitted is `Supported` or `Partial`, and
`Partial` kinds apply their recorded indirection (e.g. `followup` carried on
the context channel). ∎

## 6. Legality relation

| Step kind | Legal on events |
|---|---|
| `rewrite` | pre_tool_use, pre_shell_exec, pre_mcp_exec |
| `approve` | permission_request |
| `block` | pre/post_tool_use(+failure), permission_request, stop(+failure), pre_shell_exec, pre_mcp_exec, post_agent_response |
| `inject`, `system_message`, `followup`, `journal`, `spawn`, `run`, `checkpoint_set`, `counter_add`, `counter_set`, `list_push`, `when`, `once` | all |

`matcher` may appear only on tool events (`pre_tool_use`, `post_tool_use`,
`post_tool_use_failure`).

## 7. Capability matrix (probed, not guessed)

`caps::capabilities` is the data authority. Events wired in real host
configs are `supported`; documented-but-unwired are `supported`; `unknown`
is unprobed — never conflated with `unsupported`.

| Harness | Probed events | Distinctives |
|---|---|---|
| devin | 8 (docs): session_start/end, user_prompt_submit, pre/post_tool_use, permission_request, stop, post_compaction | `prompt_id` per turn; rewrite |
| claude | 13 wired + documented pre_compact/notification | `transcript_path`; stop_failure; teammate_idle; post_tool_use_failure |
| codex | 8 wired: session_start(source-matched), user_prompt_submit, pre/post_tool_use, permission_request, subagent_start/stop, stop | `source` field on session_start |
| cursor | 9 wired: session_start, user_prompt_submit, pre/post_tool_use(+failure), pre_shell_exec, pre_mcp_exec, post_agent_response, stop | flat `{"continue":true}` contract; followup_message |
| kimi | 7 wired: user_prompt_submit, pre/post_tool_use(+failure), permission_request, stop(+failure) | toml `[[hooks]]` shape |
| opencode | idle (plugin) | plugin API: session.prompt |
| antigravity | user_prompt_submit (partial — PreInvocation contract) | injectSteps.ephemeralMessage |
| dsh, commandcode | none observed | poll fallback only |
