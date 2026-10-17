//! Per-harness capability tables — the factual record of who can express
//! what. Entries marked `Supported` are either wired in a real host config or
//! documented in the host's official spec; `Unknown` means unprobed — never
//! assumed deficient.

use crate::{ActionKind, HookEvent, Support};
use Support::{Partial, Supported as S, Unknown as U, Unsupported as X};

pub struct HarnessCaps {
    /// The wire-contract family emit lowers onto: `generic`
    /// (`hookSpecificOutput`+`decision`), `cursor` (flat `continue`), or
    /// `antigravity` (`injectSteps`). The only authority for the mapping —
    /// emit reads it instead of holding a second dispatch table.
    pub family: &'static str,
    pub events: &'static [(HookEvent, Support)],
    pub actions: &'static [(ActionKind, Support)],
    /// Wire-name overrides where a host's event spelling diverges from the
    /// family's default (e.g. Devin's `PostCompaction` vs Claude's
    /// `PostCompact`).
    pub event_names: &'static [(HookEvent, &'static str)],
    /// Payload fields beyond the common envelope this harness is known to
    /// send (e.g. Devin's per-turn `prompt_id`, Claude's `transcript_path`).
    pub payload_fields: &'static [&'static str],
}

// Devin: official lifecycle docs list eight events.
const DEVIN_EVENTS: &[(HookEvent, Support)] = &[
    (HookEvent::SessionStart, S),
    (HookEvent::SessionEnd, S),
    (HookEvent::UserPromptSubmit, S),
    (HookEvent::PreToolUse, S),
    (HookEvent::PostToolUse, S),
    (HookEvent::PermissionRequest, S),
    (HookEvent::Stop, S),
    (HookEvent::PostCompaction, S),
];

const DEVIN_ACTIONS: &[(ActionKind, Support)] = &[
    (ActionKind::InjectContext, S), // additionalContext
    (ActionKind::Block, S),         // decision:block + reason
    (ActionKind::Approve, S),       // decision:approve
    (ActionKind::RewriteInput, S),  // updatedInput merge
    (ActionKind::Spawn, S),
    (ActionKind::Journal, S),
    (ActionKind::SystemMessage, S),  // systemMessage channel
    (ActionKind::Followup, Partial), // block+reason is the only channel
];

// Claude: probed — events wired in ~/.claude/settings.json; PreCompact and
// Notification are documented hook events that this config simply doesn't use.
const CLAUDE_EVENTS: &[(HookEvent, Support)] = &[
    (HookEvent::SessionStart, S),
    (HookEvent::SessionEnd, S),
    (HookEvent::UserPromptSubmit, S),
    (HookEvent::PreToolUse, S),
    (HookEvent::PostToolUse, S),
    (HookEvent::PostToolUseFailure, S), // wired
    (HookEvent::PermissionRequest, S),
    (HookEvent::Stop, S),
    (HookEvent::StopFailure, S), // wired
    (HookEvent::SubagentStart, S),
    (HookEvent::SubagentStop, S),
    (HookEvent::PreCompaction, S),  // documented (PreCompact)
    (HookEvent::PostCompaction, S), // wired (PostCompact)
    (HookEvent::Notification, S),   // documented
    (HookEvent::TeammateIdle, S),   // wired
];

const CLAUDE_ACTIONS: &[(ActionKind, Support)] = &[
    (ActionKind::InjectContext, S),
    (ActionKind::Block, S),
    (ActionKind::Approve, S),
    (ActionKind::RewriteInput, U),
    (ActionKind::Spawn, S),
    (ActionKind::Journal, S),
    (ActionKind::SystemMessage, S), // documented systemMessage field
    (ActionKind::Followup, Partial),
];

// Codex: probed — eight events wired in ~/.codex/hooks.json. SessionStart
// matchers discriminate on `source` (startup|resume|clear|compact).
const CODEX_EVENTS: &[(HookEvent, Support)] = &[
    (HookEvent::SessionStart, S),
    (HookEvent::UserPromptSubmit, S),
    (HookEvent::PreToolUse, S),
    (HookEvent::PostToolUse, S),
    (HookEvent::PermissionRequest, S),
    (HookEvent::SubagentStart, S),
    (HookEvent::SubagentStop, S),
    (HookEvent::Stop, S),
];

const CODEX_ACTIONS: &[(ActionKind, Support)] = &[
    (ActionKind::InjectContext, S), // additionalContextLimit field exists
    (ActionKind::Block, S),         // decision:block contract
    (ActionKind::Approve, U),
    (ActionKind::RewriteInput, U),
    (ActionKind::Spawn, S),
    (ActionKind::Journal, S),
    (ActionKind::SystemMessage, S),
    (ActionKind::Followup, Partial),
];

// Cursor: probed — nine events wired in ~/.cursor/hooks.json (flat array,
// camelCase names). Response contract: {"continue": true} /
// additional_context / followup_message.
const CURSOR_EVENTS: &[(HookEvent, Support)] = &[
    (HookEvent::SessionStart, S),       // sessionStart
    (HookEvent::UserPromptSubmit, S),   // beforeSubmitPrompt
    (HookEvent::PreToolUse, S),         // preToolUse
    (HookEvent::PostToolUse, S),        // postToolUse
    (HookEvent::PostToolUseFailure, S), // postToolUseFailure
    (HookEvent::PreShellExec, S),       // beforeShellExecution
    (HookEvent::PreMcpExec, S),         // beforeMCPExecution
    (HookEvent::PostAgentResponse, S),  // afterAgentResponse
    (HookEvent::Stop, S),               // stop
];

const CURSOR_ACTIONS: &[(ActionKind, Support)] = &[
    (ActionKind::InjectContext, S), // additional_context
    (ActionKind::Followup, S),      // followup_message / agent_message
    (ActionKind::Block, S),         // continue:false halts the loop
    (ActionKind::RewriteInput, U),
    (ActionKind::Spawn, S),
    (ActionKind::Journal, S),
    (ActionKind::SystemMessage, Partial), // no user channel — folds into contexts
];

// Kimi: probed — seven events wired in ~/.kimi-code/config.toml [[hooks]].
const KIMI_EVENTS: &[(HookEvent, Support)] = &[
    (HookEvent::UserPromptSubmit, S),
    (HookEvent::PreToolUse, S),
    (HookEvent::PostToolUse, S),
    (HookEvent::PostToolUseFailure, S),
    (HookEvent::PermissionRequest, S),
    (HookEvent::Stop, S),
    (HookEvent::StopFailure, S),
];

const KIMI_ACTIONS: &[(ActionKind, Support)] = &[
    (ActionKind::InjectContext, U),
    (ActionKind::Block, Partial), // same family as Codex/Claude per contract
    (ActionKind::Spawn, S),
    (ActionKind::Journal, S),
    (ActionKind::SystemMessage, S),
    (ActionKind::Followup, Partial),
];

// opencode: probed — plugin surface; session.idle wired via
// agent-memory-distill.js. The plugin event API exposes more event types
// (unprobed which).
const OPENCODE_EVENTS: &[(HookEvent, Support)] = &[(HookEvent::Idle, S)];

const OPENCODE_ACTIONS: &[(ActionKind, Support)] = &[
    (ActionKind::Followup, S), // session.prompt API → followup_message field
    (ActionKind::InjectContext, S),
    (ActionKind::Spawn, S),
];

// Antigravity: the memory router carries a real PreInvocation contract
// (injectSteps.ephemeralMessage on invocationNum/artifactDirectoryPath
// payloads) — a prompt-submission-class event with inject support.
const ANTIGRAVITY_EVENTS: &[(HookEvent, Support)] = &[(HookEvent::UserPromptSubmit, Partial)];

const ANTIGRAVITY_ACTIONS: &[(ActionKind, Support)] = &[
    (ActionKind::InjectContext, S), // injectSteps.ephemeralMessage
    (ActionKind::Followup, Partial),
];

// dsh / commandcode: no hook configuration surface found on this machine —
// every event degrades to `poll`.
const NO_SURFACE_EVENTS: &[(HookEvent, Support)] = &[
    (HookEvent::SessionStart, X),
    (HookEvent::SessionEnd, X),
    (HookEvent::UserPromptSubmit, X),
    (HookEvent::PreToolUse, X),
    (HookEvent::PostToolUse, X),
    (HookEvent::PostToolUseFailure, X),
    (HookEvent::PermissionRequest, X),
    (HookEvent::Stop, X),
    (HookEvent::StopFailure, X),
    (HookEvent::SubagentStart, X),
    (HookEvent::SubagentStop, X),
    (HookEvent::PreCompaction, X),
    (HookEvent::PostCompaction, X),
    (HookEvent::Notification, X),
    (HookEvent::Idle, X),
    (HookEvent::TeammateIdle, X),
    (HookEvent::PreShellExec, X),
    (HookEvent::PreMcpExec, X),
    (HookEvent::PostAgentResponse, X),
];

pub fn capabilities(harness: &str) -> Option<HarnessCaps> {
    Some(match harness {
        "devin" => HarnessCaps {
            family: "generic",
            events: DEVIN_EVENTS,
            actions: DEVIN_ACTIONS,
            // Devin's wire name for post_compaction is PostCompaction, not
            // the family default PostCompact.
            event_names: &[(HookEvent::PostCompaction, "PostCompaction")],
            payload_fields: &["prompt_id"],
        },
        "claude" => HarnessCaps {
            family: "generic",
            events: CLAUDE_EVENTS,
            actions: CLAUDE_ACTIONS,
            event_names: &[],
            payload_fields: &["transcript_path", "stop_hook_active"],
        },
        "codex" => HarnessCaps {
            family: "generic",
            events: CODEX_EVENTS,
            actions: CODEX_ACTIONS,
            event_names: &[],
            payload_fields: &["source"],
        },
        "cursor" => HarnessCaps {
            family: "cursor",
            events: CURSOR_EVENTS,
            actions: CURSOR_ACTIONS,
            event_names: &[],
            payload_fields: &[],
        },
        "kimi" => HarnessCaps {
            family: "generic",
            events: KIMI_EVENTS,
            actions: KIMI_ACTIONS,
            event_names: &[],
            payload_fields: &[],
        },
        "opencode" => HarnessCaps {
            family: "generic",
            events: OPENCODE_EVENTS,
            actions: OPENCODE_ACTIONS,
            event_names: &[],
            payload_fields: &[],
        },
        "antigravity" => HarnessCaps {
            family: "antigravity",
            events: ANTIGRAVITY_EVENTS,
            actions: ANTIGRAVITY_ACTIONS,
            event_names: &[],
            payload_fields: &["invocationNum", "artifactDirectoryPath"],
        },
        "dsh" | "commandcode" => HarnessCaps {
            family: "generic",
            events: NO_SURFACE_EVENTS,
            actions: &[],
            event_names: &[],
            payload_fields: &[],
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(h: &str, e: HookEvent) -> Support {
        capabilities(h)
            .and_then(|c| c.events.iter().find(|(ev, _)| *ev == e).map(|(_, s)| *s))
            .unwrap_or(U)
    }

    #[test]
    fn devin_carries_the_per_turn_prompt_id() {
        assert!(capabilities("devin")
            .expect("devin caps")
            .payload_fields
            .contains(&"prompt_id"));
        assert_eq!(event("devin", HookEvent::PostCompaction), S);
    }

    #[test]
    fn probed_surfaces_are_richer_than_the_old_guess() {
        // Probing replaced the "codex/kimi wire Stop only" guess: both turn
        // out to have real tool-event surfaces.
        for h in ["codex", "kimi"] {
            assert_eq!(event(h, HookEvent::PreToolUse), S, "{h} pre_tool_use");
            assert_eq!(event(h, HookEvent::PostToolUse), S, "{h} post_tool_use");
        }
        assert_eq!(event("cursor", HookEvent::PreShellExec), S);
        assert_eq!(event("claude", HookEvent::TeammateIdle), S);
    }

    #[test]
    fn unprobed_is_not_unsupported() {
        assert_eq!(event("cursor", HookEvent::SubagentStop), U);
        assert_eq!(event("dsh", HookEvent::Stop), X);
    }
}
