//! ahe — Agent Hook Engine: agent lifecycle-hook engine.
//!
//! One hook program, written once in the DSL ([`model`]), is evaluated by the
//! engine and lowered onto each agent harness's native hook surface — the
//! per-harness reality lives in [`caps`]. The Python hooks being replaced are
//! gathered under `legacy/`; the design rationale lives in `docs/`.
//!
//! Core principle: extract universal primitives that cover every agent
//! harness. A harness-specific capability is not rejected — it is converted
//! into a primitive that other harnesses may record as unsupported. The
//! capability matrix that falls out of this is itself the scoreboard for how
//! well each harness exposes hooks.

pub mod caps;
pub mod check;
pub mod emit;
pub mod eval;
pub mod model;

use serde::{Deserialize, Serialize};

/// Lifecycle events the engine knows about — the union of what observed
/// harnesses fire. A harness declares the subset it supports and records the
/// rest as [`Support::Unsupported`] or [`Support::Unknown`], never silently
/// dropping them. Harness-unique events become vocabulary too (the universal-
/// primitive rule), e.g. [`HookEvent::Idle`] from opencode's `session.idle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    SessionStart,
    SessionEnd,
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    /// Tool call finished with a failure (Kimi/Claude `PostToolUseFailure`,
    /// Cursor `postToolUseFailure`). On harnesses without the dedicated event,
    /// programs express it as `on post_tool_use` + a payload-failure guard —
    /// that is the program-level idiom, not an engine lowering.
    PostToolUseFailure,
    PermissionRequest,
    /// Fires when the agent wants to finish a turn. On every observed harness
    /// this is once per turn, not once per session — consumers needing
    /// end-of-session state must checkpoint per turn (e.g. by `prompt_id`).
    Stop,
    /// Turn ended abnormally (Kimi/Claude `StopFailure`).
    StopFailure,
    SubagentStart,
    SubagentStop,
    /// Before context compaction (Claude's `PreCompact`).
    PreCompaction,
    /// After context compaction (Devin `PostCompaction`, Claude `PostCompact`).
    PostCompaction,
    /// Host-side notification event (Claude's `Notification`).
    Notification,
    /// Host reports the session went idle (opencode `session.idle`).
    Idle,
    /// A teammate/subagent went idle (Claude `TeammateIdle`).
    TeammateIdle,
    /// Cursor's pre-gate for shell commands (`beforeShellExecution`).
    PreShellExec,
    /// Cursor's pre-gate for MCP calls (`beforeMCPExecution`).
    PreMcpExec,
    /// Cursor fires after each agent response (`afterAgentResponse`).
    PostAgentResponse,
}

/// The kind of an action step — the capability-matrix key, since the
/// action payload differs per program but support is declared per kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    InjectContext,
    Block,
    Approve,
    RewriteInput,
    Spawn,
    Journal,
    Followup,
    /// Host-facing user message (`systemMessage` family).
    SystemMessage,
    // Engine-internal kinds (run/checkpoint/counter/list/when/once) have no
    // matrix cells: they execute inside the engine or the shim identically
    // on every harness, so a per-host capability would be dead data.
}

/// Whether a harness can express an event or action. `Unsupported` and
/// `Unknown` are different recorded facts — an unprobed harness is not a
/// deficient one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    Supported,
    /// Expressible only through an indirection (e.g. a block-reason used as
    /// the instruction channel where no first-class inject exists).
    Partial,
    Unsupported,
    /// Not yet probed — never conflate with Unsupported.
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_level_and_lifecycle_events_stay_distinct() {
        // Conflating them once produced first-turn-only distills — pin the
        // wire names, not just enum identity.
        assert_eq!(
            serde_json::from_str::<HookEvent>("\"stop\"").unwrap(),
            HookEvent::Stop
        );
        assert_eq!(
            serde_json::from_str::<HookEvent>("\"session_end\"").unwrap(),
            HookEvent::SessionEnd
        );
    }
}
