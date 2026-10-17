//! Lowering `↓_a` (spec §0): translate a merged [`Response`] into a concrete
//! host contract. Fails closed — any action kind the capability matrix marks
//! `Unsupported` or `Unknown` refuses emission rather than being dropped
//! silently (Theorem 4). `Partial` kinds carry an explicit lowering rule.

use crate::caps;
use crate::eval::{Decision, Response};
use crate::{ActionKind, HookEvent, Support};
use serde_json::{Map, Value};

/// Contract families observed in the wild: `Generic` covers the
/// claude/codex/kimi/devin `decision`+`hookSpecificOutput` family; `Cursor`
/// is the flat `{"continue": ..., "additional_context": ...}` shape.
fn family(harness: &str) -> &'static str {
    match harness {
        "cursor" => "cursor",
        "antigravity" => "antigravity",
        _ => "generic",
    }
}

fn pascal(ev: HookEvent) -> &'static str {
    use HookEvent as E;
    match ev {
        E::SessionStart => "SessionStart",
        E::SessionEnd => "SessionEnd",
        E::UserPromptSubmit => "UserPromptSubmit",
        E::PreToolUse => "PreToolUse",
        E::PostToolUse => "PostToolUse",
        E::PostToolUseFailure => "PostToolUseFailure",
        E::PermissionRequest => "PermissionRequest",
        E::Stop => "Stop",
        E::StopFailure => "StopFailure",
        E::SubagentStart => "SubagentStart",
        E::SubagentStop => "SubagentStop",
        E::PreCompaction => "PreCompact",
        E::PostCompaction => "PostCompact",
        E::Notification => "Notification",
        E::Idle => "Idle",
        E::TeammateIdle => "TeammateIdle",
        E::PreShellExec => "PreShellExec",
        E::PreMcpExec => "PreMcpExec",
        E::PostAgentResponse => "PostAgentResponse",
    }
}

/// Which action kinds did this response actually exercise?
fn used_kinds(r: &Response) -> Vec<ActionKind> {
    let mut kinds = Vec::new();
    if r.decision == Decision::Block {
        kinds.push(ActionKind::Block);
    }
    if r.decision == Decision::Approve {
        kinds.push(ActionKind::Approve);
    }
    if !r.contexts.is_empty() {
        kinds.push(ActionKind::InjectContext);
    }
    if !r.patch.is_empty() {
        kinds.push(ActionKind::RewriteInput);
    }
    if !r.followups.is_empty() {
        kinds.push(ActionKind::Followup);
    }
    if !r.spawns.is_empty() {
        kinds.push(ActionKind::Spawn);
    }
    if !r.journal.is_empty() {
        kinds.push(ActionKind::Journal);
    }
    if !r.system.is_empty() {
        kinds.push(ActionKind::SystemMessage);
    }
    kinds
}

/// `↓_a(rsp)` — Err on unsupported *or* unknown kinds (Theorem 4: nothing
/// reaches the wire that the matrix can't stand behind). `Partial` kinds get
/// their recorded indirection applied.
pub fn emit(harness: &str, ev: HookEvent, rsp: &Response) -> Result<Value, Vec<String>> {
    let caps =
        caps::capabilities(harness).ok_or_else(|| vec![format!("unknown harness {harness}")])?;
    let mut refused = Vec::new();
    for k in used_kinds(rsp) {
        match caps
            .actions
            .iter()
            .find(|(kind, _)| kind == &k)
            .map(|(_, s)| *s)
        {
            Some(Support::Supported) | Some(Support::Partial) => {}
            Some(Support::Unsupported) => refused.push(format!("{k:?} unsupported on {harness}")),
            Some(Support::Unknown) | None => {
                refused.push(format!("{k:?} unprobed on {harness} — refusing to emit"))
            }
        }
    }
    if !refused.is_empty() {
        return Err(refused);
    }

    // Partial lowering: a host without a first-class channel carries the
    // kind on its context channel (recorded per harness in the matrix).
    let partial = |kind: ActionKind| {
        caps.actions
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, s)| *s == Support::Partial)
            .unwrap_or(false)
    };
    let followup_as_context = partial(ActionKind::Followup);
    let system_as_context = partial(ActionKind::SystemMessage);
    let mut contexts = rsp.contexts.clone();
    if followup_as_context {
        contexts.extend(rsp.followups.iter().map(|f| format!("[followup] {f}")));
    }
    if system_as_context {
        contexts.extend(rsp.system.iter().map(|s| format!("[system] {s}")));
    }

    Ok(match family(harness) {
        "cursor" => {
            let mut o = Map::new();
            o.insert(
                "continue".into(),
                Value::from(rsp.decision != Decision::Block),
            );
            if !contexts.is_empty() {
                o.insert("additional_context".into(), contexts.join("\n\n").into());
            }
            if !rsp.followups.is_empty() {
                o.insert("followup_message".into(), rsp.followups.join("\n").into());
            }
            if !rsp.reasons.is_empty() {
                o.insert("reason".into(), rsp.reasons.join("\n").into());
            }
            Value::Object(o)
        }
        "antigravity" => {
            // PreInvocation contract: injectSteps.ephemeralMessage + continue.
            let mut o = Map::new();
            o.insert(
                "continue".into(),
                Value::from(rsp.decision != Decision::Block),
            );
            let mut steps = Vec::new();
            for t in contexts.iter().chain(rsp.reasons.iter()) {
                steps.push(Value::Object(Map::from_iter([(
                    "ephemeralMessage".into(),
                    Value::from(t.clone()),
                )])));
            }
            if !steps.is_empty() {
                o.insert("injectSteps".into(), Value::Array(steps));
            }
            Value::Object(o)
        }
        _ => {
            let mut o = Map::new();
            match rsp.decision {
                Decision::Block => {
                    o.insert("decision".into(), "block".into());
                    if !rsp.reasons.is_empty() {
                        o.insert("reason".into(), rsp.reasons.join("\n").into());
                    }
                }
                Decision::Approve => {
                    o.insert("decision".into(), "approve".into());
                }
                Decision::None => {}
            }
            if !rsp.system.is_empty() && !system_as_context {
                o.insert("systemMessage".into(), rsp.system.join("\n").into());
            }
            if !rsp.followups.is_empty() && !followup_as_context {
                o.insert("followup_message".into(), rsp.followups.join("\n").into());
            }
            // hookSpecificOutput always carries the event name, even on a
            // pass-through — the baseline did the same.
            let mut hso = Map::new();
            hso.insert("hookEventName".into(), pascal(ev).into());
            if !contexts.is_empty() {
                hso.insert("additionalContext".into(), contexts.join("\n\n").into());
            }
            if !rsp.patch.is_empty() {
                hso.insert("updatedInput".into(), Value::Object(rsp.patch.clone()));
            }
            o.insert("hookSpecificOutput".into(), Value::Object(hso));
            Value::Object(o)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    // `json` is test-only.

    fn rsp(f: impl FnOnce(&mut Response)) -> Response {
        let mut r = Response::default();
        f(&mut r);
        r
    }

    #[test]
    fn devin_block_plus_context_emits_the_real_contract() {
        let r = rsp(|r| {
            r.decision = Decision::Block;
            r.reasons.push("no".into());
            r.contexts.push("ctx".into());
        });
        let out = emit("devin", HookEvent::Stop, &r).unwrap();
        assert_eq!(out["decision"], "block");
        assert_eq!(out["reason"], "no");
        assert_eq!(out["hookSpecificOutput"]["additionalContext"], "ctx");
    }

    #[test]
    fn unsupported_and_unknown_kinds_refuse_the_wire() {
        // Cursor's RewriteInput is Unknown — emission must refuse, not drop.
        let r = rsp(|r| {
            r.patch.insert("k".into(), json!(1));
        });
        let err = emit("cursor", HookEvent::PreToolUse, &r).unwrap_err();
        assert!(err[0].contains("unprobed")); // Theorem 4
    }

    #[test]
    fn partial_followup_lowers_onto_the_context_channel() {
        let r = rsp(|r| r.followups.push("fix the lint".into()));
        let out = emit("devin", HookEvent::Stop, &r).unwrap();
        assert_eq!(
            out["hookSpecificOutput"]["additionalContext"],
            "[followup] fix the lint"
        );
    }

    #[test]
    fn cursor_uses_the_flat_continue_contract() {
        let r = rsp(|r| {
            r.followups.push("keep going".into());
        });
        let out = emit("cursor", HookEvent::Stop, &r).unwrap();
        assert_eq!(out["continue"], true);
        assert_eq!(out["followup_message"], "keep going");
    }

    #[test]
    fn cursor_system_message_folds_into_contexts() {
        // SystemMessage is Partial on cursor — carried on the context
        // channel instead of being dropped (Theorem 4's recorded lowering).
        let r = rsp(|r| r.system.push("heads up".into()));
        let out = emit("cursor", HookEvent::Stop, &r).unwrap();
        assert_eq!(out["additional_context"], "[system] heads up");
    }

    #[test]
    fn opencode_followup_reaches_the_wire() {
        // Followup=S on opencode — the generic arm must emit it, not drop.
        let r = rsp(|r| r.followups.push("continue please".into()));
        let out = emit("opencode", HookEvent::Idle, &r).unwrap();
        assert_eq!(out["followup_message"], "continue please");
    }

    #[test]
    fn antigravity_uses_the_injectsteps_contract() {
        // The harness's own shape — contexts become ephemeralMessage steps.
        let r = rsp(|r| r.contexts.push("remember this".into()));
        let out = emit("antigravity", HookEvent::UserPromptSubmit, &r).unwrap();
        assert_eq!(out["continue"], true);
        assert_eq!(out["injectSteps"][0]["ephemeralMessage"], "remember this");
    }

    #[test]
    fn system_message_is_gated_and_emitted() {
        let r = rsp(|r| r.system.push("note".into()));
        let out = emit("devin", HookEvent::Stop, &r).unwrap();
        assert_eq!(out["systemMessage"], "note");
    }
}
