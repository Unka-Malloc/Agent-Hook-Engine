use ahe::{check, emit, eval, install, model, HookEvent};
use serde_json::json;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let program = model::Program::from_json(
        r#"{"name":"example","handlers":[{"on":"stop","steps":[{"op":"inject","text":"Fixture context."}]}]}"#,
    )?;
    let errors = check::check_program(&program);
    if !errors.is_empty() {
        return Err(format!("{errors:?}").into());
    }
    let mut state = eval::State::default();
    let response = eval::run_program(
        &program,
        HookEvent::Stop,
        &json!({"session_id":"example"}),
        &mut state,
    );
    if !response.violations.is_empty() {
        return Err(response.violations.join("; ").into());
    }
    let contract =
        emit::emit("devin", HookEvent::Stop, &response).map_err(|errors| errors.join("; "))?;
    assert_eq!(
        contract["hookSpecificOutput"]["additionalContext"],
        "Fixture context."
    );
    let plan = install::adapt(
        &install::harnesses::CODEX.surface,
        &[install::HookDecl {
            event: install::HookEvent::SessionStart,
            command: "fixture-hook".into(),
            intent: install::HookIntent::Notify,
            matcher: None,
            timeout: None,
        }],
    );
    assert_eq!(plan.artifacts.len(), 1);
    assert_eq!(plan.report[0].support, install::Support::Native);
    println!(
        "{}",
        serde_json::to_string(&json!({"contract":contract,"installation":plan}))?
    );
    Ok(())
}
