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
    /// Cursor `postToolUseFailure`). On harnesses without the dedicated event
    /// it lowers to `on post_tool_use` guarded by the response payload.
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

/// The kind of a [`HookAction`] — used as the capability-matrix key, since the
/// action payload differs per program but support is declared per kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    InjectContext,
    Block,
    Approve,
    RewriteInput,
    Spawn,
    Run,
    Journal,
    Checkpoint,
    Counter,
    Followup,
}

/// Actions a hook may ask the host to perform — the output side of the DSL.
/// Harnesses that cannot express an action report its kind unsupported.
#[derive(Debug, Clone, PartialEq)]
pub enum HookAction {
    /// Inject text into the agent's context (additionalContext family).
    InjectContext(String),
    /// Refuse the pending action; `reason` is shown to the agent.
    Block { reason: String },
    /// Explicitly allow (PermissionRequest).
    Approve,
    /// Transparently rewrite a tool call's input before it runs.
    RewriteInput(serde_json::Value),
    /// Detached side-effect process (scan, ingest).
    Spawn { argv: Vec<String> },
    /// Bounded child hook whose stdout contract merges back into this run.
    Run {
        argv: Vec<String>,
        timeout_ms: Option<u64>,
    },
    /// Append a JSONL record to a named sink.
    Journal {
        sink: String,
        fields: serde_json::Map<String, serde_json::Value>,
    },
    /// Durable keyed marker (per-prompt dedup etc.).
    Checkpoint {
        key: String,
        value: serde_json::Value,
    },
    /// Per-session counter mutation.
    Counter { name: String, delta: i64 },
    /// Ask the running agent to perform a follow-up instruction.
    Followup(String),
}

impl HookAction {
    pub fn kind(&self) -> ActionKind {
        match self {
            Self::InjectContext(_) => ActionKind::InjectContext,
            Self::Block { .. } => ActionKind::Block,
            Self::Approve => ActionKind::Approve,
            Self::RewriteInput(_) => ActionKind::RewriteInput,
            Self::Spawn { .. } => ActionKind::Spawn,
            Self::Run { .. } => ActionKind::Run,
            Self::Journal { .. } => ActionKind::Journal,
            Self::Checkpoint { .. } => ActionKind::Checkpoint,
            Self::Counter { .. } => ActionKind::Counter,
            Self::Followup(_) => ActionKind::Followup,
        }
    }
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

/// A harness's hook surface, expressed in the shared vocabulary.
pub trait HarnessHooks {
    fn event_support(&self, event: HookEvent) -> Support;
    fn action_support(&self, kind: ActionKind) -> Support;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_level_and_lifecycle_events_stay_distinct() {
        // Conflating them once produced first-turn-only distills.
        assert_ne!(HookEvent::Stop, HookEvent::SessionEnd);
    }
}
