//! Runtime: the dynamic semantics of `docs/dsl-design.md` §4.
//!
//! `eval` is a total function — an ill-typed operand yields `false` plus a
//! recorded violation, never a stuck evaluator. `run_program` is an ordered
//! fold of matching handlers into the [`Response`] merge monoid. `spawn` and
//! `journal` are returned as *intents* (the host shim performs the side
//! effect); `run` actually executes because its stdout must merge.

use crate::model::{Expr, Program, Step};
use crate::HookEvent;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Per-session mutable state — the `Σ` of the spec. Serializes directly;
/// adding a field keeps persistence in sync automatically.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct State {
    pub checkpoints: BTreeMap<String, Value>,
    pub counters: BTreeMap<String, i64>,
    /// Bounded session lists (recent command signatures, audit history).
    pub lists: BTreeMap<String, Vec<Value>>,
    pub fired: BTreeSet<String>,
}

/// Join order is significant: `Block` outranks `Approve`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Decision {
    #[default]
    None,
    Approve,
    Block,
}

/// The response monoid `(Rsp, ⊕, ∅)` — merge is associative, `∅` the
/// identity, and `Block` absorbing in `decision` (spec Lemma 1–2).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Response {
    pub decision: Decision,
    pub reasons: Vec<String>,
    pub contexts: Vec<String>,
    /// `rewrite` patches, shallow-merged in step order.
    pub patch: Map<String, Value>,
    pub spawns: Vec<Vec<String>>,
    pub followups: Vec<String>,
    /// (sink, record) intents — the host shim appends the JSONL lines.
    pub journal: Vec<(String, Map<String, Value>)>,
    /// Ill-typed evaluations and failed `run` children — observable, never
    /// silently swallowed. A well-typed program produces none (Theorem 2).
    pub violations: Vec<String>,
    /// Host-facing user messages (`systemMessage` family).
    pub system: Vec<String>,
    /// Raw stdout JSON of a `run` child — transient carrier for `capture`
    /// mode; not part of the monoid (merge ignores it).
    pub captured: Option<Value>,
}

impl Response {
    /// `⊕` — append in order; `decision` joins by `max` (Block > Approve).
    pub fn merge(mut self, o: Response) -> Response {
        self.decision = self.decision.max(o.decision);
        self.reasons.extend(o.reasons);
        self.contexts.extend(o.contexts);
        self.patch.extend(o.patch);
        self.spawns.extend(o.spawns);
        self.followups.extend(o.followups);
        self.journal.extend(o.journal);
        self.violations.extend(o.violations);
        self.system.extend(o.system);
        self
    }
}

/// JSON truthiness: `null` and `false` are false; everything else true.
fn truthy(v: &Value) -> bool {
    !matches!(v, Value::Null | Value::Bool(false))
}

fn snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn camel(s: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in s.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Dotted-path lookup; `camelCase`/`snake_case` name variants coalesce.
fn lookup<'a>(mut v: &'a Value, path: &str) -> Option<&'a Value> {
    for seg in path.split('.') {
        let obj = v.as_object()?;
        v = obj
            .get(seg)
            .or_else(|| obj.get(&snake(seg)))
            .or_else(|| obj.get(&camel(seg)))?;
    }
    Some(v)
}

fn ord(l: &Value, r: &Value, v: &mut Vec<String>, what: &str) -> Option<std::cmp::Ordering> {
    match (l, r) {
        (Value::Number(a), Value::Number(b)) => a
            .as_f64()
            .and_then(|x| b.as_f64().map(|y| (x, y)))
            .and_then(|(x, y)| x.partial_cmp(&y)),
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        _ => {
            v.push(format!("{what}: operands {l} and {r} are not ordered"));
            None
        }
    }
}

/// `⟦expr⟧_{p,Σ}` — total; ill-typed forms yield `false` + a violation.
pub fn eval(e: &Expr, p: &Value, s: &State, v: &mut Vec<String>) -> Value {
    use Expr::*;
    match e {
        Lit { v } => v.clone(),
        Payload { path } => lookup(p, path).cloned().unwrap_or(Value::Null),
        Checkpoint { key } => s.checkpoints.get(key).cloned().unwrap_or(Value::Null),
        Counter { name } => Value::from(*s.counters.get(name).unwrap_or(&0)),
        Env { name } => std::env::var(name).map(Value::from).unwrap_or(Value::Null),
        Eq { a, b } => Value::from(eval(a, p, s, v) == eval(b, p, s, v)),
        Ne { a, b } => Value::from(eval(a, p, s, v) != eval(b, p, s, v)),
        Lt { a, b } | Le { a, b } | Gt { a, b } | Ge { a, b } => {
            let (l, r) = (eval(a, p, s, v), eval(b, p, s, v));
            Value::from(match ord(&l, &r, v, "ordering comparison") {
                None => false,
                Some(o) => match e {
                    Lt { .. } => o.is_lt(),
                    Le { .. } => o.is_le(),
                    Gt { .. } => o.is_gt(),
                    _ => o.is_ge(),
                },
            })
        }
        Contains { a, b } => {
            let (h, n) = (eval(a, p, s, v), eval(b, p, s, v));
            Value::from(match (h, n) {
                (Value::String(h), Value::String(n)) => h.contains(&n),
                (Value::Array(h), n) => h.contains(&n),
                (h, n) => {
                    v.push(format!("contains: {h} has no membership over {n}"));
                    false
                }
            })
        }
        Matches { re, on } => {
            let target = eval(on, p, s, v);
            // Non-strings match against their compact JSON form — a regex over
            // a tool_response object means "does the response look like X".
            let text = match target.as_str() {
                Some(t) => t.to_string(),
                None if target.is_null() => {
                    v.push("matches: target is null".into());
                    return Value::from(false);
                }
                None => target.to_string(),
            };
            let hit = match regex::Regex::new(re) {
                Ok(r) => r.is_match(&text),
                Err(e) => {
                    v.push(format!("matches: invalid regex {re:?}: {e}"));
                    false
                }
            };
            Value::from(hit)
        }
        And { args } => Value::from(args.iter().all(|a| truthy(&eval(a, p, s, v)))),
        Or { args } => Value::from(args.iter().any(|a| truthy(&eval(a, p, s, v)))),
        Not { arg } => Value::from(!truthy(&eval(arg, p, s, v))),
        Add { a, b } | Sub { a, b } | Mul { a, b } | Min { a, b } | Max { a, b } => {
            let (x, y) = (eval(a, p, s, v), eval(b, p, s, v));
            match (x.as_f64(), y.as_f64()) {
                (Some(x), Some(y)) => Value::from(match e {
                    Add { .. } => x + y,
                    Sub { .. } => x - y,
                    Mul { .. } => x * y,
                    Min { .. } => x.min(y),
                    _ => x.max(y),
                }),
                _ => {
                    v.push(format!("arithmetic on non-numbers {x} and {y}"));
                    Value::Null
                }
            }
        }
        Now => Value::from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as f64)
                .unwrap_or(0.0),
        ),
        Len { on } => match eval(on, p, s, v) {
            Value::String(t) => Value::from(t.chars().count()),
            Value::Array(a) => Value::from(a.len()),
            Value::Object(o) => Value::from(o.len()),
            other => {
                v.push(format!("len: {other} has no length"));
                Value::Null
            }
        },
        Take { n, on } => match eval(on, p, s, v) {
            Value::String(t) => Value::from(t.chars().take(*n as usize).collect::<String>()),
            other => {
                v.push(format!("take: {other} is not a string"));
                Value::Null
            }
        },
        Replace { re, with, on } => {
            let t = eval(on, p, s, v);
            match (regex::Regex::new(re), t.as_str()) {
                (Ok(r), Some(t)) => Value::from(r.replace_all(t, with.as_str()).to_string()),
                (Err(e), _) => {
                    v.push(format!("replace: invalid regex {re:?}: {e}"));
                    Value::Null
                }
                (_, None) => {
                    v.push(format!("replace: {t} is not a string"));
                    Value::Null
                }
            }
        }
        FileTail { path, bytes } => {
            let (path, bytes) = (eval(path, p, s, v), eval(bytes, p, s, v));
            let read = || -> Option<String> {
                let mut f = std::fs::File::open(path.as_str()?).ok()?;
                let size = f.metadata().ok()?.len();
                f.seek(std::io::SeekFrom::End(-(size.min(bytes.as_u64()?) as i64)))
                    .ok()?;
                let mut buf = Vec::new();
                f.read_to_end(&mut buf).ok()?;
                Some(String::from_utf8_lossy(&buf).into_owned())
            };
            Value::from(read().unwrap_or_default())
        }
        Format { template, vars } => {
            let mut out = template.clone();
            for (k, e) in vars {
                let val = match eval(e, p, s, v) {
                    Value::String(s) => s,
                    other => other.to_string(),
                };
                out = out.replace(&format!("{{{k}}}"), &val);
            }
            Value::from(out)
        }
        Obj { fields } => Value::Object(
            fields
                .iter()
                .map(|(k, e)| (k.clone(), eval(e, p, s, v)))
                .collect(),
        ),
        Arr { args } => Value::Array(args.iter().map(|a| eval(a, p, s, v)).collect()),
        Field { on, path } => {
            let base = eval(on, p, s, v);
            lookup(&base, path).cloned().unwrap_or(Value::Null)
        }
        ListLen { key } => Value::from(s.lists.get(key).map(Vec::len).unwrap_or(0)),
        ListCount { key, eq } => {
            let needle = eval(eq, p, s, v);
            Value::from(
                s.lists
                    .get(key)
                    .map(|l| l.iter().filter(|x| **x == needle).count())
                    .unwrap_or(0),
            )
        }
        ListCountMax { key } => Value::from(
            s.lists
                .get(key)
                .map(|l| {
                    let mut seen: Vec<&Value> = Vec::new();
                    l.iter()
                        .filter(|x| {
                            let new = !seen.contains(x);
                            seen.push(x);
                            new
                        })
                        .map(|x| l.iter().filter(|y| *y == x).count())
                        .max()
                        .unwrap_or(0)
                })
                .unwrap_or(0),
        ),
        ListLast { key } => s
            .lists
            .get(key)
            .and_then(|l| l.last())
            .cloned()
            .unwrap_or(Value::Null),
        CounterSum { prefix, ge, offset } => {
            let prefix = eval(prefix, p, s, v);
            let prefix = prefix.as_str().unwrap_or_default().to_string();
            let threshold = ge
                .as_ref()
                .map(|g| eval(g, p, s, v).as_f64().unwrap_or(0.0));
            let off = offset.unwrap_or(0);
            Value::from(
                s.counters
                    .iter()
                    .filter(|(k, _)| k.starts_with(prefix.as_str()))
                    .map(|(_, n)| *n)
                    .filter(|n| threshold.is_none_or(|g| (*n as f64) >= g))
                    .map(|n| n + off)
                    .sum::<i64>(),
            )
        }
        CounterCount { prefix } => {
            let prefix = eval(prefix, p, s, v);
            let prefix = prefix.as_str().unwrap_or_default().to_string();
            Value::from(
                s.counters
                    .keys()
                    .filter(|k| k.starts_with(prefix.as_str()))
                    .count(),
            )
        }
        FiredCount => Value::from(s.fired.len()),
        ListUnique { key } => Value::from(
            s.lists
                .get(key)
                .map(|l| {
                    let mut seen: Vec<&Value> = Vec::new();
                    for x in l {
                        if !seen.contains(&x) {
                            seen.push(x);
                        }
                    }
                    seen.len()
                })
                .unwrap_or(0),
        ),
        First { args } => args
            .iter()
            .map(|a| eval(a, p, s, v))
            .find(|x| !x.is_null())
            .unwrap_or(Value::Null),
        FileSize { path } => eval(path, p, s, v)
            .as_str()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| Value::from(m.len()))
            .unwrap_or(Value::from(0)),
        JsonExtract { on } => {
            let t = eval(on, p, s, v);
            let s = t.as_str().unwrap_or_default();
            let found = regex::Regex::new(r"(?s)\{.*\}")
                .ok()
                .and_then(|r| r.find(s))
                .and_then(|m| serde_json::from_str::<Value>(m.as_str()).ok());
            match found {
                Some(j) => j,
                None => {
                    v.push("json_extract: no JSON object found".into());
                    Value::Null
                }
            }
        }
    }
}

/// Parse a child's stdout against the shared hook contract into a `Response`.
/// Covers the claude/codex/kimi/devin family plus cursor's flat contract.
fn from_contract(v: &Value) -> Response {
    let mut r = Response::default();
    let dec = v.get("decision").and_then(Value::as_str).unwrap_or("");
    if dec == "block" {
        r.decision = Decision::Block;
    } else if dec == "approve" || v.get("continue") == Some(&Value::Bool(true)) {
        r.decision = Decision::Approve;
    }
    for k in ["reason", "blockReason"] {
        if let Some(t) = v.get(k).and_then(Value::as_str) {
            r.reasons.push(t.into());
        }
    }
    for k in ["additionalContext", "additional_context", "systemMessage"] {
        if let Some(t) = v.get(k).and_then(Value::as_str) {
            if k == "systemMessage" {
                r.system.push(t.into());
            } else {
                r.contexts.push(t.into());
            }
        }
    }
    for k in ["followup_message", "agent_message"] {
        if let Some(t) = v.get(k).and_then(Value::as_str) {
            r.followups.push(t.into());
        }
    }
    if let Some(h) = v.get("hookSpecificOutput") {
        if let Some(t) = h.get("additionalContext").and_then(Value::as_str) {
            r.contexts.push(t.into());
        }
        if let Some(Value::Object(p)) = h.get("updatedInput") {
            r.patch.extend(p.clone());
        }
    }
    r
}

/// Bounded `run` child: stdin gets the event payload, stdout parses through
/// `from_contract` and merges. Timeout/crash contributes ∅ + a violation.
fn run_child(
    argv: &[String],
    timeout_ms: u64,
    payload: &Value,
    capture: bool,
    v: &mut Vec<String>,
) -> Response {
    let Some((head, args)) = argv.split_first() else {
        v.push("run: empty argv".into());
        return Response::default();
    };
    let deadline = Duration::from_millis(timeout_ms);
    let mut child = match Command::new(head)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            v.push(format!("run {head}: spawn failed: {e}"));
            return Response::default();
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.to_string().as_bytes());
    }
    let mut out = child.stdout.take().expect("piped stdout");
    let reader = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = out.read_to_end(&mut b);
        b
    });
    let t0 = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if t0.elapsed() < deadline => std::thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
                v.push(format!("run {head}: timed out after {timeout_ms}ms"));
                return Response::default();
            }
            Err(e) => {
                v.push(format!("run {head}: wait failed: {e}"));
                return Response::default();
            }
        }
    }
    let bytes = reader.join().unwrap_or_default();
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(j) => {
            let mut r = from_contract(&j);
            r.captured = Some(j);
            r
        }
        Err(_) if capture => {
            // Data-returning child: prose output is still the datum — the
            // program extracts with `json_extract` et al.
            Response {
                captured: Some(Value::from(String::from_utf8_lossy(&bytes).into_owned())),
                ..Default::default()
            }
        }
        Err(e) => {
            v.push(format!("run {head}: stdout is not a response: {e}"));
            Response::default()
        }
    }
}

/// Evaluate an argv element; non-strings serialize compact (JSON).
fn argv_of(item: &crate::model::ArgvItem, p: &Value, s: &State, v: &mut Vec<String>) -> String {
    match item {
        crate::model::ArgvItem::Lit(t) => t.clone(),
        crate::model::ArgvItem::Expr(e) => match eval(e, p, s, v) {
            Value::String(t) => t,
            other => other.to_string(),
        },
    }
}

fn eval_text(t: &crate::model::Text, p: &Value, s: &State, v: &mut Vec<String>) -> String {
    match t {
        crate::model::Text::Lit(t) => t.clone(),
        crate::model::Text::Expr(e) => match eval(e, p, s, v) {
            Value::String(s) => s,
            other => other.to_string(),
        },
    }
}

fn exec_steps(steps: &[Step], p: &Value, s: &mut State, rsp: &mut Response) {
    for step in steps {
        match step {
            Step::Inject { text } => {
                let t = eval_text(text, p, s, &mut rsp.violations);
                rsp.contexts.push(t);
            }
            Step::SystemMessage { text } => {
                let t = eval_text(text, p, s, &mut rsp.violations);
                rsp.system.push(t);
            }
            Step::Block { reason } => {
                rsp.decision = Decision::Block;
                rsp.reasons
                    .push(eval_text(reason, p, s, &mut rsp.violations));
            }
            Step::Approve => rsp.decision = rsp.decision.max(Decision::Approve),
            Step::Rewrite { patch } => rsp.patch.extend(patch.clone()),
            Step::Followup { text } => {
                let t = eval_text(text, p, s, &mut rsp.violations);
                rsp.followups.push(t);
            }
            Step::Spawn { argv } => rsp.spawns.push(
                argv.iter()
                    .map(|a| argv_of(a, p, s, &mut rsp.violations))
                    .collect(),
            ),
            Step::Run {
                argv,
                timeout_ms,
                capture,
            } => {
                let argv: Vec<String> = argv
                    .iter()
                    .map(|a| argv_of(a, p, s, &mut rsp.violations))
                    .collect();
                let child = run_child(
                    &argv,
                    timeout_ms.unwrap_or(15_000),
                    p,
                    capture.is_some(),
                    &mut rsp.violations,
                );
                match capture {
                    // Data-returning child: raw stdout JSON into state.
                    Some(key) => {
                        s.checkpoints
                            .insert(key.clone(), child.captured.unwrap_or(Value::Null));
                    }
                    // Contract-returning child: merge its response.
                    None => *rsp = std::mem::take(rsp).merge(child),
                }
            }
            Step::Journal { sink, fields } => {
                let path = eval(sink, p, s, &mut rsp.violations);
                let rec = eval(fields, p, s, &mut rsp.violations);
                let rec = match rec {
                    Value::Object(o) => o,
                    other => {
                        let mut o = Map::new();
                        o.insert("value".into(), other);
                        o
                    }
                };
                match path.as_str() {
                    Some(path) => rsp.journal.push((path.to_string(), rec)),
                    None => rsp.violations.push("journal: sink is not a string".into()),
                }
            }
            Step::CheckpointSet { key, value } => {
                let val = eval(value, p, s, &mut rsp.violations);
                s.checkpoints.insert(key.clone(), val);
            }
            Step::CounterAdd { name, delta } => {
                let key = match eval(name, p, s, &mut rsp.violations) {
                    Value::String(s) => s,
                    other => other.to_string(),
                };
                let d = eval(delta, p, s, &mut rsp.violations).as_i64().unwrap_or(0);
                *s.counters.entry(key).or_insert(0) += d;
            }
            Step::CounterSet { name, value } => {
                let key = match eval(name, p, s, &mut rsp.violations) {
                    Value::String(s) => s,
                    other => other.to_string(),
                };
                let val = eval(value, p, s, &mut rsp.violations).as_i64().unwrap_or(0);
                s.counters.insert(key, val);
            }
            Step::ListPush { key, value, max } => {
                let val = eval(value, p, s, &mut rsp.violations);
                let l = s.lists.entry(key.clone()).or_default();
                l.push(val);
                if let Some(max) = max {
                    let excess = l.len().saturating_sub(*max as usize);
                    l.drain(..excess);
                }
            }
            Step::When { cond, then } => {
                if truthy(&eval(cond, p, s, &mut rsp.violations)) {
                    exec_steps(then, p, s, rsp);
                }
            }
            Step::Once { key, then } => {
                if s.fired.insert(key.clone()) {
                    exec_steps(then, p, s, rsp);
                }
            }
        }
    }
}

fn matcher_matches(m: &str, p: &Value, v: &mut Vec<String>) -> bool {
    let Some(tool) = lookup(p, "tool_name").and_then(Value::as_str) else {
        return false;
    };
    match regex::Regex::new(m) {
        Ok(re) => re.is_match(tool),
        Err(e) => {
            v.push(format!("matcher {m:?}: invalid regex: {e}"));
            false
        }
    }
}

/// `⟦program⟧_e` — ordered fold of matching handlers; Theorem 3
/// (determinism) holds for fixed payload and child outputs.
pub fn run_program(prog: &Program, ev: HookEvent, p: &Value, s: &mut State) -> Response {
    let mut rsp = Response::default();
    for h in prog.handlers.iter().filter(|h| h.on == ev) {
        if let Some(m) = &h.matcher {
            if !matcher_matches(m, p, &mut rsp.violations) {
                continue;
            }
        }
        exec_steps(&h.steps, p, s, &mut rsp);
    }
    rsp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Handler;
    use serde_json::json;

    fn prog(steps: Vec<Step>) -> Program {
        Program {
            name: "t".into(),
            handlers: vec![Handler {
                on: HookEvent::Stop,
                matcher: None,
                steps,
            }],
        }
    }

    #[test]
    fn merge_is_a_monoid() {
        let a = || Response {
            decision: Decision::Approve,
            reasons: vec!["r".into()],
            ..Default::default()
        };
        let b = || Response {
            contexts: vec!["c".into()],
            ..Default::default()
        };
        let c = || Response {
            decision: Decision::Block,
            ..Default::default()
        };
        // Lemma 1: associativity and ∅ identity.
        assert_eq!(a().merge(b()).merge(c()), a().merge(b().merge(c())));
        assert_eq!(a().merge(Response::default()), a());
        // Lemma 2: block absorbing.
        assert_eq!(a().merge(c()).decision, Decision::Block);
    }

    #[test]
    fn determinism_fixed_inputs_fixed_response() {
        let p = prog(vec![
            Step::CounterAdd {
                name: Expr::Lit { v: json!("n") },
                delta: Expr::Lit { v: json!(1) },
            },
            Step::When {
                cond: Expr::Ge {
                    a: Expr::Counter { name: "n".into() }.into(),
                    b: Expr::Lit { v: json!(1) }.into(),
                },
                then: vec![Step::Inject { text: "x".into() }],
            },
        ]);
        let run = || {
            let mut s = State::default();
            run_program(&p, HookEvent::Stop, &json!({}), &mut s)
        };
        assert_eq!(run(), run()); // Theorem 3
    }

    #[test]
    fn once_fires_at_most_once() {
        let p = prog(vec![Step::Once {
            key: "k".into(),
            then: vec![Step::Inject { text: "hi".into() }],
        }]);
        let mut s = State::default();
        let r1 = run_program(&p, HookEvent::Stop, &json!({}), &mut s);
        let r2 = run_program(&p, HookEvent::Stop, &json!({}), &mut s);
        assert_eq!(r1.contexts, ["hi"]);
        assert!(r2.contexts.is_empty()); // Theorem 5
    }

    #[test]
    fn ill_typed_operands_are_false_plus_violation() {
        let mut v = Vec::new();
        let e = Expr::Lt {
            a: Expr::Lit { v: json!("x") }.into(),
            b: Expr::Lit { v: json!(3) }.into(),
        };
        assert_eq!(
            eval(&e, &json!({}), &State::default(), &mut v),
            json!(false)
        );
        assert_eq!(v.len(), 1); // observable, not stuck
    }

    #[test]
    fn run_child_merges_contract() {
        let p = prog(vec![Step::Run {
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                "printf '{\"decision\":\"block\",\"reason\":\"no\"}'".into(),
            ],
            timeout_ms: Some(5_000),
            capture: None,
        }]);
        let mut s = State::default();
        let r = run_program(&p, HookEvent::Stop, &json!({}), &mut s);
        assert_eq!(r.decision, Decision::Block);
        assert_eq!(r.reasons, ["no"]);
    }

    #[test]
    fn run_child_failure_is_isolated() {
        let p = prog(vec![Step::Run {
            argv: vec!["/bin/sh".into(), "-c".into(), "exit 3".into()],
            timeout_ms: Some(5_000),
            capture: None,
        }]);
        let mut s = State::default();
        let r = run_program(&p, HookEvent::Stop, &json!({}), &mut s);
        assert_eq!(r.decision, Decision::None); // dead child → ∅
        assert!(!r.violations.is_empty());
    }
}
