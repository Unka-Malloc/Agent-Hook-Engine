//! The DSL program model — the typed form every hook program lowers to.
//!
//! Programs serialize as JSON: `{"name": ..., "handlers": [{"on": <event>,
//! "matcher": <tool-name regex>, "steps": [<step>...]}]}`. Steps and
//! conditions are `{"op": <name>, ...}` objects — one uniform tag style.

use crate::HookEvent;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Program {
    pub name: String,
    #[serde(default)]
    pub handlers: Vec<Handler>,
}

impl Program {
    pub fn from_json(text: &str) -> serde_json::Result<Self> {
        serde_json::from_str(text)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Handler {
    pub on: HookEvent,
    /// Tool-name regex restricting tool events; absent matches all.
    #[serde(default)]
    pub matcher: Option<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
}

/// One primitive step. Action steps map to [`crate::ActionKind`] kinds;
/// state and control steps are engine-internal primitives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Step {
    /// Add text to the agent's context.
    Inject { text: Text },
    /// Refuse the pending action; `reason` is shown to the agent.
    Block { reason: Text },
    /// Explicitly allow (permission decisions).
    Approve,
    /// Merge a patch into the tool call's input before it runs.
    Rewrite { patch: Map<String, Value> },
    /// Ask the running agent to perform a follow-up instruction.
    Followup { text: Text },
    /// Detached side-effect process (scan, ingest); never awaited.
    Spawn { argv: Vec<ArgvItem> },
    /// Bounded child process. Default: stdout parses as the hook contract and
    /// merges into this response. With `capture`, stdout parses as plain JSON
    /// stored under that checkpoint key instead (data-returning child).
    Run {
        argv: Vec<ArgvItem>,
        timeout_ms: Option<u64>,
        #[serde(default)]
        capture: Option<String>,
    },
    /// Append one JSON record to a named sink (telemetry). Both sides are
    /// expressions — sink paths and record fields are commonly computed.
    Journal { sink: Expr, fields: Expr },
    /// Write a durable checkpoint value under `key` for this session.
    CheckpointSet { key: String, value: Expr },
    /// Per-session counter mutation. `name` evaluates to the key — dynamic
    /// keys (per command-signature, per file path) are the normal case.
    CounterAdd { name: Expr, delta: Expr },
    /// Set a counter to an explicit value (e.g. reset on success).
    CounterSet { name: Expr, value: Expr },
    /// Push onto a bounded per-session list (recent-signatures deque etc.).
    ListPush {
        key: String,
        value: Expr,
        /// Keep at most this many newest items.
        max: Option<u64>,
    },
    /// Emit a host-facing `systemMessage` (user-visible, not agent context).
    SystemMessage { text: Text },
    /// Run `then` only when `cond` holds.
    When { cond: Expr, then: Vec<Step> },
    /// Run `then` at most once per `key` per session (reminders, dedup).
    Once { key: String, then: Vec<Step> },
}

/// One argv element: a literal string or an expression evaluated to a value
/// (non-strings serialize as compact JSON).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgvItem {
    Lit(String),
    Expr(Expr),
}

impl From<&str> for ArgvItem {
    fn from(s: &str) -> Self {
        Self::Lit(s.to_string())
    }
}
impl From<String> for ArgvItem {
    fn from(s: String) -> Self {
        Self::Lit(s)
    }
}

/// Human-facing text: a literal string or an evaluated expression — reminder
/// texts embed computed counts, so a literal-only field can't express them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Text {
    Lit(String),
    Expr(Expr),
}

impl From<&str> for Text {
    fn from(s: &str) -> Self {
        Self::Lit(s.to_string())
    }
}
impl From<String> for Text {
    fn from(s: String) -> Self {
        Self::Lit(s)
    }
}
impl From<Expr> for Text {
    fn from(e: Expr) -> Self {
        Self::Expr(e)
    }
}

/// A condition or value expression evaluated against the event context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Expr {
    /// Constant JSON value.
    Lit {
        v: Value,
    },
    /// Field on the event payload; name variants (`camelCase`/`snake_case`)
    /// coalesce at read time. Absent → null.
    Payload {
        path: String,
    },
    /// Durable checkpoint value for this session; null when unset.
    Checkpoint {
        key: String,
    },
    /// Per-session counter value; 0 when unset.
    Counter {
        name: String,
    },
    Env {
        name: String,
    },
    Eq {
        a: Box<Expr>,
        b: Box<Expr>,
    },
    Ne {
        a: Box<Expr>,
        b: Box<Expr>,
    },
    Lt {
        a: Box<Expr>,
        b: Box<Expr>,
    },
    Le {
        a: Box<Expr>,
        b: Box<Expr>,
    },
    Gt {
        a: Box<Expr>,
        b: Box<Expr>,
    },
    Ge {
        a: Box<Expr>,
        b: Box<Expr>,
    },
    Contains {
        a: Box<Expr>,
        b: Box<Expr>,
    },
    Matches {
        re: String,
        on: Box<Expr>,
    },
    And {
        args: Vec<Expr>,
    },
    Or {
        args: Vec<Expr>,
    },
    Not {
        arg: Box<Expr>,
    },
    /// Numeric arithmetic on two operands.
    Add {
        a: Box<Expr>,
        b: Box<Expr>,
    },
    Sub {
        a: Box<Expr>,
        b: Box<Expr>,
    },
    Mul {
        a: Box<Expr>,
        b: Box<Expr>,
    },
    Min {
        a: Box<Expr>,
        b: Box<Expr>,
    },
    Max {
        a: Box<Expr>,
        b: Box<Expr>,
    },
    /// Unix epoch seconds (f64).
    Now,
    /// Length of a string (chars), array, or object.
    Len {
        on: Box<Expr>,
    },
    /// First `n` chars of a string.
    Take {
        n: u64,
        on: Box<Expr>,
    },
    /// `re.replace_all(on, with)` — `$1` captures supported.
    Replace {
        re: String,
        with: String,
        on: Box<Expr>,
    },
    /// Last `bytes` of a file, lossy-decoded; "" if unreadable.
    FileTail {
        path: Box<Expr>,
        bytes: Box<Expr>,
    },
    /// String interpolation: `{name}` placeholders filled from `vars`.
    Format {
        template: String,
        vars: std::collections::BTreeMap<String, Expr>,
    },
    /// Build an object from evaluated fields.
    Obj {
        fields: std::collections::BTreeMap<String, Expr>,
    },
    /// Build an array from evaluated items.
    Arr {
        args: Vec<Expr>,
    },
    /// Dotted-path field access on an evaluated value.
    Field {
        on: Box<Expr>,
        path: String,
    },
    /// Number of items in a session list.
    ListLen {
        key: String,
    },
    /// Occurrences of `eq` in a session list.
    ListCount {
        key: String,
        eq: Box<Expr>,
    },
    /// Maximum multiplicity in a session list (repeat-degree).
    ListCountMax {
        key: String,
    },
    /// Last item of a session list, or null.
    ListLast {
        key: String,
    },
    /// Distinct-item count of a session list.
    ListUnique {
        key: String,
    },
    /// First non-null of the args (coalesce across payload spellings).
    First {
        args: Vec<Expr>,
    },
    /// File size in bytes; 0 if unreadable.
    FileSize {
        path: Box<Expr>,
    },
    /// Extract the outermost `{…}` span from a string and parse it
    /// (greedy — intended for a single JSON object embedded in prose).
    JsonExtract {
        on: Box<Expr>,
    },
    /// Sum of counters whose names start with `prefix` (an evaluated
    /// string); when `ge` is given only counters ≥ it contribute, each as
    /// (value + `offset`).
    CounterSum {
        prefix: Box<Expr>,
        ge: Option<Box<Expr>>,
        offset: Option<i64>,
    },
    /// Count of counters whose names start with `prefix`.
    CounterCount {
        prefix: Box<Expr>,
    },
    /// Number of `once` keys fired this session.
    FiredCount,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_production_distill_program() {
        // The in-production incremental distillation, expressed in the DSL —
        // proves the model carries a real hook, not a toy.
        let text = include_str!("../../../examples/memory-distill.json");
        let p = Program::from_json(text).expect("example program parses");
        assert_eq!(p.name, "memory-distill");
        assert_eq!(p.handlers.len(), 1);
        assert_eq!(p.handlers[0].on, HookEvent::Stop);
        assert!(matches!(p.handlers[0].steps[0], Step::When { .. }));
    }
}
