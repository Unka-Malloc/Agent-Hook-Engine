//! CLI-facing behavior: a host invokes `ahe run` once per lifecycle event —
//! program JSON on disk, event payload on stdin, harness contract JSON on
//! stdout, session state durable under --state-dir.

use std::io::Write;
use std::process::{Command, Stdio};

fn ahe(args: &[&str], stdin_payload: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ahe"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ahe");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin_payload.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn missing_invocation_reports_usage_and_exits_two() {
    // given no `run` subcommand, the CLI fails fast with its contract
    let (code, _stdout, stderr) = ahe(&[], "");
    assert_eq!(code, 2);
    assert!(stderr.contains("usage: ahe run"), "stderr: {stderr}");
}

#[test]
fn run_lowers_a_stop_hook_onto_the_devin_surface() {
    // given a program that injects reminder text on stop, when the host
    // forwards a stop event, then stdout carries the harness contract and
    // session state lands under --state-dir
    let dir = std::env::temp_dir().join(format!("ahe-it-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let program = dir.join("program.json");
    std::fs::write(
        &program,
        r#"{"name":"t","handlers":[{"on":"stop","steps":[{"op":"inject","text":"remember this"}]}]}"#,
    )
    .unwrap();
    let state_dir = dir.join("state");
    let (code, stdout, stderr) = ahe(
        &[
            "run",
            "--program",
            program.to_str().unwrap(),
            "--harness",
            "devin",
            "--event",
            "stop",
            "--state-dir",
            state_dir.to_str().unwrap(),
        ],
        r#"{"session_id":"s1"}"#,
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    let contract: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("stdout is JSON contract");
    assert!(contract.is_object());
    assert!(
        state_dir.join("s1.json").exists(),
        "session state not persisted"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn once_fence_fires_only_on_the_first_event_of_a_session() {
    // given a program that injects a reminder at most once per session,
    // the first stop event carries the text into the contract and the
    // second does not — the persisted state machine in action
    let dir = std::env::temp_dir().join(format!("ahe-it-once-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let program = dir.join("program.json");
    std::fs::write(
        &program,
        r#"{"name":"t","handlers":[{"on":"stop","steps":[{"op":"once","key":"remind","then":[{"op":"inject","text":"remember this"}]}]}]}"#,
    )
    .unwrap();
    let state_dir = dir.join("state");
    let run = |session: &str| {
        ahe(
            &[
                "run",
                "--program",
                program.to_str().unwrap(),
                "--harness",
                "devin",
                "--event",
                "stop",
                "--state-dir",
                state_dir.to_str().unwrap(),
            ],
            &format!(r#"{{"session_id":"{session}"}}"#),
        )
    };
    let (code1, first, stderr1) = run("s2");
    assert_eq!(code1, 0, "stderr: {stderr1}");
    assert!(first.contains("remember this"), "first run: {first}");
    let (code2, second, stderr2) = run("s2");
    assert_eq!(code2, 0, "stderr: {stderr2}");
    assert!(
        !second.contains("remember this"),
        "once step re-fired: {second}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
