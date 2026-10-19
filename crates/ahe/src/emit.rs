//! Lowering `↓_a` (spec §0): translate a merged [`Response`] into a concrete
//! host contract. Fails closed — any action kind the capability matrix marks
//! `Unsupported` or `Unknown` refuses emission rather than being dropped
//! silently (Theorem 4). `Partial` kinds carry an explicit lowering rule.

use crate::caps;
use crate::eval::{Decision, Response};
use crate::{ActionKind, HookEvent, Support};
use serde_json::{Map, Value};

/// The wire name for `ev` on this harness — `caps.event_names` overrides
/// win over the family's default Pascal spelling.
fn wire_name(caps: &caps::HarnessCaps, ev: HookEvent) -> &'static str {
    if let Some((_, name)) = caps.event_names.iter().find(|(e, _)| *e == ev) {
        return name;
    }
    pascal(ev)
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

    Ok(match caps.family {
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
        "antigravity" => match ev {
            // Per-event wire contracts from the host's hook spec.
            HookEvent::PreToolUse => {
                let mut o = Map::new();
                // Only real decisions reach the wire: None abstains (no key)
                // — emitting "allow" for a no-opinion program would
                // auto-approve what the host would have asked the user.
                match rsp.decision {
                    Decision::Block => {
                        o.insert("decision".into(), "deny".into());
                    }
                    Decision::Approve => {
                        o.insert("decision".into(), "allow".into());
                    }
                    Decision::None => {}
                }
                // Followups ride `contexts` (Partial → folded above); system
                // is unprobed and refused upstream. Reasons + folded intents.
                let mut reason = rsp.reasons.clone();
                reason.extend(contexts.iter().cloned());
                if !reason.is_empty() {
                    o.insert("reason".into(), reason.join("\n").into());
                }
                if !rsp.patch.is_empty() {
                    o.insert("overwrite".into(), Value::Object(rsp.patch.clone()));
                }
                Value::Object(o)
            }
            HookEvent::PostToolUse => {
                // The spec accepts only `{}` here — expressive intents have
                // no channel; refuse rather than silently drop them. Every
                // un-carriable field must refuse, or a Block decision would
                // collapse to {} while emit still reports Ok.
                if rsp.decision != Decision::None
                    || !rsp.reasons.is_empty()
                    || !rsp.patch.is_empty()
                    || !contexts.is_empty()
                    || !rsp.followups.is_empty()
                    || !rsp.system.is_empty()
                {
                    return Err(vec![
                        "antigravity PostToolUse carries no context channel".into()
                    ]);
                }
                Value::Object(Map::new())
            }
            HookEvent::Stop => {
                // "stop" carries nothing but the decision itself — a context
                // or overwrite intent has no channel and must refuse, not
                // collapse into a clean-looking contract.
                if !rsp.patch.is_empty() {
                    return Err(vec!["antigravity Stop carries no overwrite channel".into()]);
                }
                let mut o = Map::new();
                match rsp.decision {
                    Decision::Block => {
                        o.insert("decision".into(), "continue".into());
                        let mut reason = rsp.reasons.clone();
                        reason.extend(contexts.iter().cloned());
                        if !reason.is_empty() {
                            o.insert("reason".into(), reason.join("\n").into());
                        }
                    }
                    _ => {
                        if !contexts.is_empty() || !rsp.reasons.is_empty() {
                            return Err(vec![
                                "antigravity Stop carries no context channel without a block decision".into()
                            ]);
                        }
                        o.insert("decision".into(), "stop".into());
                    }
                }
                Value::Object(o)
            }
            // PreInvocation and PostInvocation share the injectSteps shape —
            // the only context channel on those events. Decisions and
            // overwrite have no channel; unwired events refuse outright.
            HookEvent::UserPromptSubmit | HookEvent::PostAgentResponse => {
                if rsp.decision == Decision::Approve
                    || !rsp.patch.is_empty()
                    || (ev == HookEvent::UserPromptSubmit && rsp.decision == Decision::Block)
                {
                    return Err(vec![format!(
                        "antigravity {ev:?} carries no channel for that intent"
                    )]);
                }
                let mut o = Map::new();
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
                if ev == HookEvent::PostAgentResponse && rsp.decision == Decision::Block {
                    o.insert("terminationBehavior".into(), "force_continue".into());
                }
                Value::Object(o)
            }
            _ => {
                return Err(vec![format!("antigravity does not wire {ev:?}")]);
            }
        },
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
            hso.insert("hookEventName".into(), wire_name(&caps, ev).into());
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
        assert_eq!(out["injectSteps"][0]["ephemeralMessage"], "remember this");
    }

    #[test]
    fn antigravity_pretool_uses_the_decision_contract() {
        let r = rsp(|r| {
            r.decision = Decision::Block;
            r.reasons.push("no".into());
            r.contexts.push("ctx".into());
            r.patch.insert("CommandLine".into(), json!("ls"));
        });
        let out = emit("antigravity", HookEvent::PreToolUse, &r).unwrap();
        assert_eq!(out["decision"], "deny");
        assert!(out["reason"].as_str().unwrap().contains("ctx"));
        assert_eq!(out["overwrite"]["CommandLine"], "ls");
        // Contexts carry on reason — never silently dropped.
        let r = rsp(|r| r.decision = Decision::Approve);
        let out = emit("antigravity", HookEvent::PreToolUse, &r).unwrap();
        assert_eq!(out["decision"], "allow");
        // None abstains — never an affirmative allow the program didn't state.
        let abstain = emit("antigravity", HookEvent::PreToolUse, &Response::default()).unwrap();
        assert!(abstain.get("decision").is_none());
    }

    #[test]
    fn antigravity_pretool_reason_lists_each_intent_once() {
        // Followup is Partial → folded into contexts upstream; the wire arm
        // must not extend reason with raw followups a second time.
        let r = rsp(|r| {
            r.decision = Decision::Block;
            r.followups.push("try again".into());
        });
        let out = emit("antigravity", HookEvent::PreToolUse, &r).unwrap();
        let reason = out["reason"].as_str().unwrap();
        assert_eq!(reason.matches("[followup] try again").count(), 1);
    }

    #[test]
    fn antigravity_posttool_refuses_expressive_intents() {
        // Spec accepts only `{}` — a context request must not reach the wire.
        let r = rsp(|r| r.contexts.push("note".into()));
        assert!(emit("antigravity", HookEvent::PostToolUse, &r).is_err());
        // Every un-carriable field refuses — a Block decision must not
        // silently collapse to {} while intents commit.
        for r in [
            rsp(|r| r.decision = Decision::Block),
            rsp(|r| r.decision = Decision::Approve),
            rsp(|r| r.reasons.push("why".into())),
            rsp(|r| {
                r.patch.insert("CommandLine".into(), json!("ls"));
            }),
            rsp(|r| r.followups.push("again".into())),
            rsp(|r| r.system.push("sys".into())),
        ] {
            assert!(emit("antigravity", HookEvent::PostToolUse, &r).is_err());
        }
        assert_eq!(
            emit("antigravity", HookEvent::PostToolUse, &Response::default()).unwrap(),
            json!({})
        );
    }

    #[test]
    fn antigravity_stop_continue_maps_block() {
        let r = rsp(|r| {
            r.decision = Decision::Block;
            r.reasons.push("tests running".into());
        });
        let out = emit("antigravity", HookEvent::Stop, &r).unwrap();
        assert_eq!(out["decision"], "continue");
        assert_eq!(out["reason"], "tests running");
        assert_eq!(
            emit("antigravity", HookEvent::Stop, &Response::default()).unwrap()["decision"],
            "stop"
        );
    }

    #[test]
    fn antigravity_stop_refuses_un_carriable_intents() {
        // "stop" carries the decision alone — an inject or overwrite without
        // a block decision has no channel and must not collapse to {}.
        for r in [
            rsp(|r| r.contexts.push("note".into())),
            rsp(|r| r.reasons.push("why".into())),
            rsp(|r| {
                r.patch.insert("CommandLine".into(), json!("ls"));
            }),
        ] {
            assert!(emit("antigravity", HookEvent::Stop, &r).is_err());
        }
    }

    #[test]
    fn antigravity_invocation_refuses_un_carriable_intents() {
        // injectSteps is the only channel: decisions (except PostInvocation's
        // terminationBehavior) and overwrite refuse; unwired events refuse.
        for (ev, r) in [
            (
                HookEvent::UserPromptSubmit,
                rsp(|r| r.decision = Decision::Block),
            ),
            (
                HookEvent::UserPromptSubmit,
                rsp(|r| r.decision = Decision::Approve),
            ),
            (
                HookEvent::PostAgentResponse,
                rsp(|r| r.decision = Decision::Approve),
            ),
            (
                HookEvent::UserPromptSubmit,
                rsp(|r| {
                    r.patch.insert("CommandLine".into(), json!("ls"));
                }),
            ),
            (HookEvent::SessionStart, Response::default()),
        ] {
            assert!(emit("antigravity", ev, &r).is_err());
        }
    }

    #[test]
    fn antigravity_postinvocation_carries_termination() {
        let r = rsp(|r| r.decision = Decision::Block);
        let out = emit("antigravity", HookEvent::PostAgentResponse, &r).unwrap();
        assert_eq!(out["terminationBehavior"], "force_continue");
    }

    #[test]
    fn system_message_is_gated_and_emitted() {
        let r = rsp(|r| r.system.push("note".into()));
        let out = emit("devin", HookEvent::Stop, &r).unwrap();
        assert_eq!(out["systemMessage"], "note");
    }
}
