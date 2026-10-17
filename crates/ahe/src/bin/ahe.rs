//! ahe CLI — one invocation = one lifecycle event.
//!
//!   ahe run --program P.json --harness devin --event stop \
//!       --state-dir DIR
//!
//! stdin: the host's event payload JSON. stdout: the harness's hook contract
//! JSON. State (checkpoints/counters/once-fences) persists per session under
//! `--state-dir` keyed by `session_id`, written atomically so a crashed
//! invocation never half-writes a checkpoint.

use ahe::check::check_program;
use ahe::emit::emit;
use ahe::eval::{run_program, State};
use ahe::model::Program;
use ahe::HookEvent;
use serde_json::{Map, Value};
use std::io::Read;
use std::path::PathBuf;

fn arg(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

fn state_path(dir: &str, session: &str) -> PathBuf {
    // The session key comes from a semi-trusted payload field — whitelist
    // filename characters so `../` can't escape --state-dir.
    let safe: String = session
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe = if safe.is_empty() { "default" } else { &safe };
    let mut p = PathBuf::from(dir);
    p.push(format!("{safe}.json"));
    p
}

fn load_state(dir: &str, session: &str) -> State {
    let path = state_path(dir, session);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return State::default();
    };
    serde_json::from_str::<State>(&text).unwrap_or_else(|e| {
        eprintln!(
            "state file {} is corrupt ({e}); starting fresh",
            path.display()
        );
        State::default()
    })
}

fn save_state(dir: &str, session: &str, s: &State) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let v = serde_json::to_value(s).map_err(std::io::Error::other)?;
    // Atomic: write-tmp-then-rename (Assumption A1's durability side).
    let tmp = state_path(dir, &format!(".{session}.tmp"));
    std::fs::write(&tmp, v.to_string())?;
    std::fs::rename(tmp, state_path(dir, session))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("run") {
        eprintln!("usage: ahe run --program P --harness H --event E --state-dir D");
        std::process::exit(2);
    }
    let (Some(prog_path), Some(harness), Some(event_s), Some(dir)) = (
        arg(&args, "--program"),
        arg(&args, "--harness"),
        arg(&args, "--event"),
        arg(&args, "--state-dir"),
    ) else {
        eprintln!("missing required flags");
        std::process::exit(2);
    };
    let event: HookEvent = match serde_json::from_str(&format!("\"{event_s}\"")) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("unknown event {event_s:?}: {e}");
            std::process::exit(2);
        }
    };

    let mut stdin = String::new();
    std::io::stdin().read_to_string(&mut stdin).unwrap_or(0);
    let payload: Value = serde_json::from_str(&stdin).unwrap_or_else(|_| {
        let mut o = Map::new();
        o.insert("_unparsed".into(), stdin.into());
        Value::Object(o)
    });
    // Session key: hosts spell it differently — coalesce the baseline's
    // full key table plus its nested-object fallbacks.
    let session = [
        "session_id",
        "sessionId",
        "conversation_id",
        "conversationId",
        "thread_id",
        "threadId",
        "chat_id",
        "chatId",
        "id",
    ]
    .iter()
    .find_map(|k| {
        payload
            .get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    })
    .map(str::to_string)
    .or_else(|| {
        ["session", "conversation", "thread", "chat"]
            .iter()
            .find_map(|k| {
                payload.get(k).and_then(|sub| {
                    sub.get("id")
                        .or_else(|| sub.get("session_id"))
                        .and_then(Value::as_str)
                })
            })
            .map(str::to_string)
    })
    .unwrap_or_else(|| "default".into());

    let prog = match Program::from_json(&std::fs::read_to_string(&prog_path).unwrap_or_else(|e| {
        eprintln!("cannot read {prog_path}: {e}");
        std::process::exit(2)
    })) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("program {prog_path} does not parse: {e}");
            std::process::exit(2)
        }
    };
    for err in check_program(&prog) {
        eprintln!("check: {err}"); // ill-formed program: report, still run —
    } // hook semantics prefer degrade-over-deny at invocation time.

    let mut state = load_state(&dir, &session);
    let rsp = run_program(&prog, event, &payload, &mut state);
    // Emit first: if the host cannot express the response, nothing commits —
    // no checkpoints consumed, no journal lines, no spawns. Refusal is
    // reported on stderr and the host gets a fail-open `{}`.
    let contract = match emit(&harness, event, &rsp) {
        Ok(c) => c,
        Err(refused) => {
            for r in &refused {
                eprintln!("emit refused: {r}");
            }
            for v in &rsp.violations {
                eprintln!("violation: {v}");
            }
            println!("{{}}");
            return;
        }
    };
    if let Err(e) = save_state(&dir, &session, &state) {
        eprintln!("state save failed: {e}");
    }
    // Perform the returned intents: journal lines append, spawns detach.
    for (sink, fields) in &rsp.journal {
        let mut rec = fields.clone();
        rec.insert(
            "ts".into(),
            Value::from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            ),
        );
        if let Some(parent) = std::path::Path::new(sink).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(sink)
        {
            use std::io::Write as _;
            let _ = writeln!(f, "{}", Value::Object(rec));
        }
    }
    for argv in &rsp.spawns {
        if let Some((head, args)) = argv.split_first() {
            let _ = std::process::Command::new(head)
                .args(args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    }
    for v in &rsp.violations {
        eprintln!("violation: {v}");
    }
    println!("{contract}");
}
