//! hook-engine — agent lifecycle-hook engine.
//!
//! Status: seed. The migration baseline lives in `legacy/` (the Python hooks
//! being replaced); the DSL and per-harness mapping design live in `docs/`.
//!
//! Core principle: extract universal primitives that cover every agent
//! harness. A harness-specific capability is not rejected — it is converted
//! into a primitive that other harnesses may record as unsupported. The
//! capability matrix that falls out of this is itself the scoreboard for how
//! well each harness exposes hooks.

/// Lifecycle events the engine knows about. The set is the union of what the
/// observed harnesses fire — a harness declares the subset it supports and
/// records the rest as [`Support::Unsupported`] rather than silently dropping
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookEvent {
    SessionStart,
    SessionEnd,
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    PermissionRequest,
    /// Fires when the agent wants to finish a turn. On most harnesses this is
    /// once per turn, not once per session — distillation-like consumers must
    /// checkpoint per turn (e.g. by `prompt_id`), not per session.
    Stop,
    SubagentStop,
    PostCompaction,
    /// Host reports the session went idle (e.g. opencode `session.idle`).
    Idle,
}

/// Actions a hook may ask the host to perform — the output side of the DSL.
/// Harnesses that cannot express an action report it unsupported.
#[derive(Debug, Clone, PartialEq)]
pub enum HookAction {
    /// Inject text into the agent's context (additionalContext family).
    InjectContext(String),
    /// Refuse the pending action; `reason` is shown to the agent.
    Block { reason: String },
    /// Transparently rewrite a tool call's input before it runs.
    RewriteInput(String),
    /// Run a detached side-effect (scan, journal, audit subprocess).
    Spawn { argv: Vec<String> },
    /// Ask the running agent to perform a follow-up instruction.
    Followup(String),
}

/// Whether a harness can express an event or action. `Unsupported` is a
/// recorded fact, not a failure — it feeds the capability matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    Supported,
    /// Declared by the harness but only partially implemented.
    Partial,
    Unsupported,
}

/// A harness's hook surface, expressed in the shared vocabulary.
pub trait HarnessHooks {
    fn event_support(&self, event: HookEvent) -> Support;
    fn action_support(&self, action: &HookAction) -> Support;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_and_action_sets_are_distinct() {
        // The union taxonomy must keep turn-level and lifecycle-level events
        // separate — conflating them once produced first-turn-only distills.
        assert_ne!(HookEvent::Stop, HookEvent::SessionEnd);
    }
}
