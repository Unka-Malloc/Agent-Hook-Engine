//! Static semantics of `docs/dsl-design.md` §3: expression typing and
//! event×action legality, checked before a program ever reaches a wire.

use crate::model::{Expr, Program, Step};
use crate::HookEvent;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Null,
    Bool,
    Num,
    Str,
    Arr,
    Obj,
    /// Dynamically typed (payload reads) — truthiness defined at runtime.
    Json,
}

fn ty_of(v: &Value) -> Ty {
    match v {
        Value::Null => Ty::Null,
        Value::Bool(_) => Ty::Bool,
        Value::Number(_) => Ty::Num,
        Value::String(_) => Ty::Str,
        Value::Array(_) => Ty::Arr,
        Value::Object(_) => Ty::Obj,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CheckError {
    /// `matcher` is only meaningful where a tool name exists to match.
    MatcherOnNonTool(HookEvent),
    /// The action kind is not legal on this lifecycle event (§6 table).
    IllegalStep {
        event: HookEvent,
        op: &'static str,
    },
    /// Operand type can't ever satisfy the operator.
    IllTyped(&'static str),
    BadRegex(String),
}

use std::fmt;
impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MatcherOnNonTool(e) => write!(f, "matcher on non-tool event {e:?}"),
            Self::IllegalStep { event, op } => write!(f, "{op} is not legal on {event:?}"),
            Self::IllTyped(m) => write!(f, "ill-typed: {m}"),
            Self::BadRegex(r) => write!(f, "invalid regex {r:?}"),
        }
    }
}
impl std::error::Error for CheckError {}

/// Events where a `matcher` may discriminate on `payload.tool_name`.
const TOOL_EVENTS: &[HookEvent] = &[
    HookEvent::PreToolUse,
    HookEvent::PostToolUse,
    HookEvent::PostToolUseFailure,
];

/// §6 legality relation `L ⊆ Event × ActionKind`, specialized to the ops
/// whose placement is constrained. Everything else is legal everywhere.
fn legal(event: HookEvent, op: &'static str) -> bool {
    use HookEvent as E;
    match op {
        "rewrite" => matches!(event, E::PreToolUse | E::PreShellExec | E::PreMcpExec),
        "approve" => event == E::PermissionRequest,
        "block" => matches!(
            event,
            E::PreToolUse
                | E::PostToolUse
                | E::PostToolUseFailure
                | E::PermissionRequest
                | E::Stop
                | E::StopFailure
                | E::PreShellExec
                | E::PreMcpExec
                | E::PostAgentResponse
        ),
        _ => true,
    }
}

/// `Γ ⊢ e : T` — returns the inferred type; records each violated premise.
fn ty(e: &Expr, errs: &mut Vec<CheckError>) -> Ty {
    use Expr::*;
    let boolable = |t: Ty| matches!(t, Ty::Bool | Ty::Json);
    match e {
        Lit { v } => ty_of(v),
        Payload { .. } | Checkpoint { .. } => Ty::Json,
        Counter { .. } => Ty::Num,
        Env { .. } => Ty::Str,
        Eq { a, b } | Ne { a, b } => {
            ty(a, errs);
            ty(b, errs);
            Ty::Bool
        }
        Lt { a, b } | Le { a, b } | Gt { a, b } | Ge { a, b } => {
            let (ta, tb) = (ty(a, errs), ty(b, errs));
            for t in [ta, tb] {
                if !matches!(t, Ty::Num | Ty::Str | Ty::Json) {
                    errs.push(CheckError::IllTyped("ordering on non-Num/Str operand"));
                }
            }
            if matches!((ta, tb), (Ty::Num, Ty::Str) | (Ty::Str, Ty::Num)) {
                errs.push(CheckError::IllTyped("ordering compares Num with Str"));
            }
            Ty::Bool
        }
        Contains { a, b } => {
            if !matches!(ty(a, errs), Ty::Str | Ty::Arr | Ty::Json) {
                errs.push(CheckError::IllTyped("contains: haystack is not Str/Arr"));
            }
            ty(b, errs);
            Ty::Bool
        }
        Matches { re, on } => {
            if regex::Regex::new(re).is_err() {
                errs.push(CheckError::BadRegex(re.clone()));
            }
            if !matches!(ty(on, errs), Ty::Str | Ty::Json) {
                errs.push(CheckError::IllTyped("matches: target is not Str"));
            }
            Ty::Bool
        }
        And { args } | Or { args } => {
            for a in args {
                if !boolable(ty(a, errs)) {
                    errs.push(CheckError::IllTyped("boolean operand is not Bool/Json"));
                }
            }
            Ty::Bool
        }
        Not { arg } => {
            if !boolable(ty(arg, errs)) {
                errs.push(CheckError::IllTyped("not: operand is not Bool/Json"));
            }
            Ty::Bool
        }
        Add { a, b } | Sub { a, b } | Mul { a, b } | Min { a, b } | Max { a, b } => {
            for t in [ty(a, errs), ty(b, errs)] {
                if !matches!(t, Ty::Num | Ty::Json) {
                    errs.push(CheckError::IllTyped("arithmetic operand is not Num/Json"));
                }
            }
            Ty::Num
        }
        Now => Ty::Num,
        Len { on } | Take { on, .. } => {
            ty(on, errs);
            if matches!(e, Len { .. }) {
                Ty::Num
            } else {
                Ty::Str
            }
        }
        Replace { re, on, .. } => {
            if regex::Regex::new(re).is_err() {
                errs.push(CheckError::BadRegex(re.clone()));
            }
            ty(on, errs);
            Ty::Str
        }
        FileTail { path, bytes } => {
            ty(path, errs);
            if !matches!(ty(bytes, errs), Ty::Num | Ty::Json) {
                errs.push(CheckError::IllTyped("file_tail bytes is not Num"));
            }
            Ty::Str
        }
        Format { vars, .. } => {
            for e in vars.values() {
                ty(e, errs);
            }
            Ty::Str
        }
        Obj { fields } => {
            for e in fields.values() {
                ty(e, errs);
            }
            Ty::Obj
        }
        Arr { args } => {
            for e in args {
                ty(e, errs);
            }
            Ty::Arr
        }
        Field { on, .. } => {
            ty(on, errs);
            Ty::Json
        }
        ListLen { .. } | ListCountMax { .. } | FiredCount => Ty::Num,
        ListCount { eq, .. } => {
            ty(eq, errs);
            Ty::Num
        }
        ListLast { .. } => Ty::Json,
        CounterSum { prefix, ge, .. } => {
            ty(prefix, errs);
            if let Some(g) = ge {
                ty(g, errs);
            }
            Ty::Num
        }
        CounterCount { prefix } => {
            ty(prefix, errs);
            Ty::Num
        }
        ListUnique { .. } => Ty::Num,
        First { args } => {
            for a in args {
                ty(a, errs);
            }
            Ty::Json
        }
        FileSize { path } => {
            ty(path, errs);
            Ty::Num
        }
        JsonExtract { on } => {
            ty(on, errs);
            Ty::Json
        }
    }
}

fn ty_text(t: &crate::model::Text, errs: &mut Vec<CheckError>) {
    if let crate::model::Text::Expr(e) = t {
        ty(e, errs);
    }
}

fn check_steps(steps: &[Step], event: HookEvent, errs: &mut Vec<CheckError>) {
    for step in steps {
        match step {
            Step::Rewrite { .. } if !legal(event, "rewrite") => {
                errs.push(CheckError::IllegalStep {
                    event,
                    op: "rewrite",
                })
            }
            Step::Approve if !legal(event, "approve") => errs.push(CheckError::IllegalStep {
                event,
                op: "approve",
            }),
            Step::Block { .. } if !legal(event, "block") => {
                errs.push(CheckError::IllegalStep { event, op: "block" })
            }
            Step::When { cond, then } => {
                if !matches!(ty(cond, errs), Ty::Bool | Ty::Json) {
                    errs.push(CheckError::IllTyped("when: cond is not Bool/Json"));
                }
                check_steps(then, event, errs);
            }
            Step::Once { then, .. } => check_steps(then, event, errs),
            // Steps carrying embedded expressions get them typed.
            Step::CheckpointSet { value, .. } | Step::ListPush { value, .. } => {
                ty(value, errs);
            }
            Step::SystemMessage { text } | Step::Inject { text } | Step::Followup { text } => {
                ty_text(text, errs);
            }
            Step::Block { reason } => ty_text(reason, errs),
            Step::Journal { sink, fields } => {
                ty(sink, errs);
                ty(fields, errs);
            }
            Step::CounterAdd { name, delta } => {
                ty(name, errs);
                if !matches!(ty(delta, errs), Ty::Num | Ty::Json) {
                    errs.push(CheckError::IllTyped("counter_add delta is not Num"));
                }
            }
            Step::CounterSet { name, value } => {
                ty(name, errs);
                ty(value, errs);
            }
            Step::Run { argv, .. } | Step::Spawn { argv } => {
                for item in argv {
                    if let crate::model::ArgvItem::Expr(e) = item {
                        ty(e, errs);
                    }
                }
            }
            _ => {}
        }
    }
}

/// `e;Γ ⊢ P` for every handler — the program-level well-formedness judge.
/// An empty result means the program is well-typed (premise of Theorem 2).
pub fn check_program(p: &Program) -> Vec<CheckError> {
    let mut errs = Vec::new();
    for h in &p.handlers {
        if h.matcher.is_some() && !TOOL_EVENTS.contains(&h.on) {
            errs.push(CheckError::MatcherOnNonTool(h.on));
        }
        check_steps(&h.steps, h.on, &mut errs);
    }
    errs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Handler, Step};
    use serde_json::json;

    fn program(on: HookEvent, steps: Vec<Step>, matcher: Option<&str>) -> Program {
        Program {
            name: "t".into(),
            handlers: vec![Handler {
                on,
                matcher: matcher.map(str::to_string),
                steps,
            }],
        }
    }

    #[test]
    fn the_production_program_is_well_typed() {
        let p = Program::from_json(include_str!("../../../examples/memory-distill.json"))
            .expect("example parses");
        assert_eq!(check_program(&p), Vec::new()); // premise of Theorem 2
    }

    #[test]
    fn illegal_placements_are_rejected() {
        let errs = check_program(&program(
            HookEvent::Stop,
            vec![Step::Rewrite {
                patch: serde_json::Map::new(),
            }],
            None,
        ));
        assert!(matches!(errs[0], CheckError::IllegalStep { .. }));
        let errs = check_program(&program(HookEvent::SessionStart, vec![Step::Approve], None));
        assert!(matches!(errs[0], CheckError::IllegalStep { .. }));
        let errs = check_program(&program(HookEvent::Stop, vec![], Some(".*")));
        assert!(matches!(errs[0], CheckError::MatcherOnNonTool(_)));
    }

    #[test]
    fn ill_typed_exprs_are_rejected_statically() {
        let p = program(
            HookEvent::Stop,
            vec![Step::When {
                cond: Expr::Lt {
                    a: Expr::Lit { v: json!("s") }.into(),
                    b: Expr::Lit { v: json!(9) }.into(),
                },
                then: vec![],
            }],
            None,
        );
        assert!(check_program(&p)
            .iter()
            .any(|e| matches!(e, CheckError::IllTyped(_))));
    }
}
