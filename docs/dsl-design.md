# Hook DSL design

Goal: one hook definition, written once, mapped onto every agent harness's
native hook surface.

## Model

A hook program is a set of `on <event>` handlers. Each handler is an ordered
list of primitive steps. The engine evaluates steps against the event payload
and emits the host's native response contract.

```text
on session_start:
  - inject prompts.session_discipline
  - inject prompts.memory_router

on post_tool_use:
  - counters.observe(payload)
  - when counters.exceeded: inject reminders[key]   # once per rule per session
  - when rot >= 70 and due: spawn audit.subprocess -> maybe_block

on stop:
  - journal.append(summary)
  - when checkpoint.advances(prompt_id):
      inject contracts.distill_incremental         # block + instruction
```

## Universal primitives

Extracted from the Python baseline; each is expressible by at least one
observed harness and declared unsupported where absent.

| Primitive | Meaning | Evidence |
|---|---|---|
| `inject` | Add context to the agent | additionalContext / ephemeralMessage / additional_context / agent_message |
| `block` | Refuse an action, reason shown to agent | `decision:"block"` (Devin/Claude/Codex) |
| `rewrite` | Rewrite tool input before execution | Devin `updatedInput` |
| `spawn` | Detached subprocess side-effect | anti-rot journal/scan, audits |
| `checkpoint` | Durable per-key dedup marker | `distilled-*` state files, prompt_id checkpoints |
| `counters` | Per-session behavioral counters | anti-rot state files |
| `prompt` | Ask the running agent a follow-up | Cursor `followup_message`, injectSteps |
| `journal` | Append telemetry line | anti-rot journal.jsonl |
| `poll` | Cold-path scan for hosts with no hook surface | poll.py fallback |

## Event vocabulary

`session_start`, `session_end`, `user_prompt_submit`, `pre_tool_use`,
`post_tool_use`, `permission_request`, `stop`, `subagent_stop`,
`post_compaction`, `idle`.

Semantics note learned from production: `stop` is per-turn on every observed
harness, never once per session. Consumers needing end-of-session state must
checkpoint per turn, not per session.

## Per-harness mapping

`install.py` already encodes the four config shapes the emit side must
generate: nested-group JSON (Codex/Claude), flat-array JSON (Cursor), managed
TOML block (Kimi), plugin file (opencode), plus the Devin
`hooks.<Event>[].hooks[]` shape. The DSL compiles to these; a host with no
hook surface degrades to `poll`.

## Capability matrix (evidence so far)

| Harness | Events | inject | block | rewrite | spawn | checkpoint | prompt |
|---|---|---|---|---|---|---|---|
| Devin | full lifecycle set incl. post_compaction | yes | yes | yes | external | yes (prompt_id) | via block+reason |
| Claude Code | full set incl. subagent_stop, notification | yes | yes | ? | external | manual | via block+reason |
| Codex | stop observed; others unverified | yes | yes | ? | external | manual | via block+reason |
| Cursor | flat events incl. beforeSubmitPrompt | yes (additional_context) | ? | ? | external | manual | followup_message |
| Kimi | toml events; stop observed | ? | ? | ? | external | manual | ? |
| opencode | plugin events: session.idle | via session.prompt | ? | ? | plugin | manual | session.prompt |
| dsh / antigravity / commandcode | none observed | — | — | — | — | — | — |

`?` = unverified; the matrix is filled in as each adapter lands — gaps are
recorded as unsupported, never silently assumed.
