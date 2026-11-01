use ahe::install::{
    adapt, harnesses, Artifact, HookDecl, HookEvent, HookIntent, HookSurface, InstallationPlan,
    Support, WriteMode,
};

fn hook(event: HookEvent, command: &str) -> HookDecl {
    HookDecl {
        event,
        command: command.to_owned(),
        intent: HookIntent::default(),
        matcher: None,
        timeout: None,
    }
}

fn support(plan: &InstallationPlan, item: &str) -> Option<Support> {
    plan.report
        .iter()
        .find(|entry| entry.item == item)
        .map(|entry| entry.support)
}

// --------------------------- declaration serde ----------------------------

#[test]
fn hook_decl_round_trips_the_installation_wire_form() {
    let json = serde_json::json!({
        "event": "session-start",
        "command": "echo hi",
        "intent": "inject",
        "matcher": "Write|Edit",
        "timeout": 10
    });
    let decl: HookDecl = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(decl.event, HookEvent::SessionStart);
    assert_eq!(decl.intent, HookIntent::Inject);
    assert_eq!(decl.matcher.as_deref(), Some("Write|Edit"));
    assert_eq!(decl.timeout, Some(10));
    assert_eq!(serde_json::to_value(&decl).unwrap(), json);
    let minimal: HookDecl =
        serde_json::from_value(serde_json::json!({"event": "stop", "command": "x.sh"})).unwrap();
    assert_eq!(minimal.intent, HookIntent::Enforce);
    assert_eq!(HookEvent::PostCompaction.kebab_name(), "post-compaction");
}

#[test]
fn merge_dedupes_hooks_by_event_command_matcher() {
    let mut base = vec![hook(HookEvent::Stop, "a")];
    ahe::install::merge(
        &mut base,
        vec![
            hook(HookEvent::Stop, "a"),
            hook(HookEvent::Stop, "b"),
            HookDecl {
                matcher: Some("Write".into()),
                ..hook(HookEvent::Stop, "a")
            },
        ],
    );
    assert_eq!(base.len(), 3);
    assert_eq!(base[1].command, "b");
    assert_eq!(base[2].matcher.as_deref(), Some("Write"));
}

#[test]
fn validate_rejects_a_blank_hook_command() {
    assert_eq!(
        ahe::install::validate(&[hook(HookEvent::Stop, "  ")]),
        Err("hook requires a non-empty command")
    );
    assert!(ahe::install::validate(&[hook(HookEvent::Stop, "x.sh")]).is_ok());
    assert!(ahe::install::validate(&[]).is_ok());
}

// ------------------------------ claude -------------------------------------

#[test]
fn claude_deep_merges_hooks_with_matcher_and_timeout() {
    let plan = adapt(
        &harnesses::CLAUDE.surface,
        &[
            HookDecl {
                matcher: Some("Write|Edit".into()),
                timeout: Some(30),
                ..hook(HookEvent::PreToolUse, "guard.sh")
            },
            hook(HookEvent::Notification, "note.sh"),
        ],
    );
    let artifact = plan
        .artifacts
        .iter()
        .find(|a| a.path == std::path::Path::new(".claude/settings.json"))
        .expect("claude settings artifact");
    assert!(
        matches!(&artifact.mode, WriteMode::JsonMerge { at } if at == "hooks"),
        "{:?}",
        artifact.mode
    );
    let fragment: serde_json::Value = serde_json::from_str(&artifact.content).unwrap();
    let entry = &fragment["PreToolUse"][0];
    assert_eq!(entry["matcher"], "Write|Edit");
    assert_eq!(entry["hooks"][0]["type"], "command");
    assert_eq!(entry["hooks"][0]["command"], "guard.sh");
    assert_eq!(entry["hooks"][0]["timeout"], 30);
    assert_eq!(
        fragment["Notification"][0]["hooks"][0]["command"],
        "note.sh"
    );
    assert_eq!(support(&plan, "hooks.pre-tool-use"), Some(Support::Native));
    assert_eq!(support(&plan, "hooks.notification"), Some(Support::Native));
}

// ------------------------------- codex -------------------------------------

#[test]
fn codex_merges_hooks_at_the_document_root() {
    let plan = adapt(
        &harnesses::CODEX.surface,
        &[
            hook(HookEvent::SessionStart, "session-start.sh"),
            hook(HookEvent::SessionEnd, "unsupported.sh"),
        ],
    );
    let artifact = plan
        .artifacts
        .iter()
        .find(|a| a.path == std::path::Path::new(".codex/hooks.json"))
        .expect("codex hooks artifact");
    assert!(
        matches!(&artifact.mode, WriteMode::JsonMerge { at } if at.is_empty()),
        "{:?}",
        artifact.mode
    );
    let fragment: serde_json::Value = serde_json::from_str(&artifact.content).unwrap();
    assert_eq!(
        fragment["SessionStart"][0]["hooks"][0]["command"],
        "session-start.sh"
    );
    assert!(fragment.get("SessionEnd").is_none());
    assert_eq!(support(&plan, "hooks.session-start"), Some(Support::Native));
    assert_eq!(
        support(&plan, "hooks.session-end"),
        Some(Support::Unsupported)
    );
}

// ------------------------------- devin -------------------------------------

#[test]
fn devin_carries_post_compaction_in_the_claude_store() {
    let plan = adapt(
        &harnesses::DEVIN.surface,
        &[
            hook(HookEvent::PostCompaction, "compact.sh"),
            hook(HookEvent::Notification, "notify.sh"),
        ],
    );
    let artifact = plan
        .artifacts
        .iter()
        .find(|a| a.path == std::path::Path::new(".config/devin/config.json"))
        .expect("devin config artifact");
    let fragment: serde_json::Value = serde_json::from_str(&artifact.content).unwrap();
    assert_eq!(
        fragment["PostCompaction"][0]["hooks"][0]["command"],
        "compact.sh"
    );
    assert_eq!(
        support(&plan, "hooks.post-compaction"),
        Some(Support::Native)
    );
    assert_eq!(
        support(&plan, "hooks.notification"),
        Some(Support::Unsupported)
    );
}

// ------------------------------- cursor ------------------------------------

#[test]
fn cursor_flat_store_maps_the_prompt_event() {
    let plan = adapt(
        &harnesses::CURSOR.surface,
        &[
            hook(HookEvent::UserPromptSubmit, "route.sh"),
            hook(HookEvent::PreToolUse, "guard.sh"),
        ],
    );
    let artifact = plan
        .artifacts
        .iter()
        .find(|a| a.path == std::path::Path::new(".cursor/hooks.json"))
        .expect("cursor hooks artifact");
    let doc: serde_json::Value = serde_json::from_str(&artifact.content).unwrap();
    assert_eq!(doc["version"], 1);
    assert_eq!(doc["hooks"]["beforeSubmitPrompt"][0]["command"], "route.sh");
    assert_eq!(
        support(&plan, "hooks.user-prompt-submit"),
        Some(Support::Adapted)
    );
    assert_eq!(
        support(&plan, "hooks.pre-tool-use"),
        Some(Support::Unsupported)
    );
}

// ---------------------------- antigravity ----------------------------------

#[test]
fn antigravity_named_bundle_reports_adapted_and_unsupported() {
    let plan = adapt(
        &harnesses::ANTIGRAVITY.surface,
        &[
            hook(HookEvent::SessionStart, "start.sh"),
            hook(HookEvent::PreToolUse, "guard.sh"),
            hook(HookEvent::PostToolUse, "after.sh"),
            hook(HookEvent::Stop, "receipt.sh"),
            hook(HookEvent::Notification, "note.sh"),
        ],
    );
    let artifact = plan
        .artifacts
        .iter()
        .find(|a| a.path == std::path::Path::new(".gemini/config/hooks.json"))
        .expect("antigravity bundle artifact");
    let doc: serde_json::Value = serde_json::from_str(&artifact.content).unwrap();
    assert_eq!(doc["a4aa"]["SessionStart"][0]["command"], "start.sh");
    assert_eq!(doc["a4aa"]["PreToolUse"][0]["command"], "guard.sh");
    assert_eq!(doc["a4aa"]["PostToolUse"][0]["command"], "after.sh");
    assert_eq!(doc["a4aa"]["Stop"][0]["command"], "receipt.sh");
    assert_eq!(doc["a4aa"]["enabled"], true);
    for item in [
        "hooks.session-start",
        "hooks.pre-tool-use",
        "hooks.post-tool-use",
        "hooks.stop",
    ] {
        assert_eq!(support(&plan, item), Some(Support::Adapted), "{item}");
    }
    assert_eq!(
        support(&plan, "hooks.notification"),
        Some(Support::Unsupported)
    );
}

// -------------------------------- grok --------------------------------------

#[test]
fn grok_bundle_writes_one_owner_file() {
    let plan = adapt(
        &harnesses::GROK.surface,
        &[
            hook(HookEvent::SessionStart, "start.sh"),
            hook(HookEvent::Notification, "note.sh"),
        ],
    );
    let artifact = plan
        .artifacts
        .iter()
        .find(|a| a.path == std::path::Path::new(".grok/hooks/a4aa.json"))
        .expect("grok bundle artifact");
    assert!(matches!(artifact.mode, WriteMode::Create));
    let doc: serde_json::Value = serde_json::from_str(&artifact.content).unwrap();
    assert_eq!(
        doc["hooks"]["SessionStart"][0]["hooks"][0]["command"],
        "start.sh"
    );
    assert_eq!(
        doc["hooks"]["Notification"][0]["hooks"][0]["command"],
        "note.sh"
    );
    assert_eq!(support(&plan, "hooks.session-start"), Some(Support::Native));
}

// ------------------------------ extensions ---------------------------------

#[test]
fn pi_extension_stub_admits_notify_hooks_only() {
    let plan = adapt(
        &harnesses::PI.surface,
        &[
            HookDecl {
                intent: HookIntent::Notify,
                ..hook(HookEvent::SessionStart, "start.sh")
            },
            hook(HookEvent::PreToolUse, "guard.sh"),
        ],
    );
    let ext = plan
        .artifacts
        .iter()
        .find(|a| a.path == std::path::Path::new(".pi/agent/extensions/a4aa.ts"))
        .expect("pi extension stub");
    assert!(matches!(ext.mode, WriteMode::Create));
    assert!(ext.content.contains("// @a4aa-managed"));
    assert!(ext.content.contains("pi.on('session_start'"));
    assert!(ext.content.contains("start.sh"));
    assert!(!ext.content.contains("guard.sh"));
    assert_eq!(
        support(&plan, "hooks.session-start"),
        Some(Support::Adapted)
    );
    assert_eq!(
        support(&plan, "hooks.pre-tool-use"),
        Some(Support::Unsupported)
    );
}

#[test]
fn opencode_and_kilo_plugins_render_the_opencode_shape() {
    for (harness, file) in [
        (&harnesses::OPENCODE, ".config/opencode/plugins/a4aa.js"),
        (&harnesses::KILO_CODE, ".config/kilo/plugin/a4aa.js"),
    ] {
        let plan = adapt(
            &harness.surface,
            &[HookDecl {
                intent: HookIntent::Notify,
                ..hook(HookEvent::SessionStart, "start.sh")
            }],
        );
        let ext = plan
            .artifacts
            .iter()
            .find(|a| a.path == std::path::Path::new(file))
            .unwrap_or_else(|| panic!("{file} artifact"));
        assert!(ext.content.contains("// @a4aa-managed"));
        assert!(
            ext.content.contains("event.type === 'session.created'"),
            "{file}"
        );
        assert!(ext.content.contains("start.sh"));
        assert_eq!(
            support(&plan, "hooks.session-start"),
            Some(Support::Adapted)
        );
    }
}

#[test]
fn generated_observers_never_claim_enforcement_or_injection() {
    for harness in [&harnesses::KILO_CODE, &harnesses::OPENCODE, &harnesses::PI] {
        for intent in [HookIntent::Enforce, HookIntent::Inject, HookIntent::Rewrite] {
            let plan = adapt(
                &harness.surface,
                &[
                    HookDecl {
                        intent,
                        ..hook(HookEvent::SessionStart, "guard.sh")
                    },
                    HookDecl {
                        intent,
                        ..hook(HookEvent::PreToolUse, "guard.sh")
                    },
                ],
            );
            assert!(plan.artifacts.is_empty());
            assert!(plan
                .report
                .iter()
                .all(|entry| entry.support == Support::Unsupported));
        }
    }
}

#[test]
fn generated_observers_report_failures_to_the_host_log_without_vetoing() {
    for (harness, native_event) in [
        (&harnesses::PI, "session_start"),
        (&harnesses::OPENCODE, "session.created"),
        (&harnesses::KILO_CODE, "session.created"),
    ] {
        let plan = adapt(
            &harness.surface,
            &[HookDecl {
                intent: HookIntent::Notify,
                ..hook(HookEvent::SessionStart, "exit-7.sh")
            }],
        );
        // Observer admission stays non-veto: the hook is emitted, not refused.
        assert_eq!(
            support(&plan, "hooks.session-start"),
            Some(Support::Adapted)
        );
        let artifact = plan
            .artifacts
            .iter()
            .find(|a| a.content.contains("exit-7.sh"))
            .expect("observer artifact");
        let line = artifact
            .content
            .lines()
            .find(|line| line.contains("exit-7.sh"))
            .unwrap();
        // A failing hook command is reported with the event, the command
        // and the failure reason on the host's diagnostic channel — the
        // observer never rethrows, never impersonates Enforce.
        assert!(line.contains("catch (error)"), "{line}");
        assert!(
            line.contains("console.error('[a4aa] observer hook failed'"),
            "{line}"
        );
        assert!(line.contains(&format!("event: '{native_event}'")), "{line}");
        assert!(line.contains(r#"command: "exit-7.sh""#), "{line}");
        assert!(line.contains("reason: String(error)"), "{line}");
        assert!(!artifact.content.contains("catch (_) {}"));
    }
}

#[test]
fn generated_observers_carry_the_declared_timeout_into_exec() {
    for harness in [&harnesses::PI, &harnesses::OPENCODE, &harnesses::KILO_CODE] {
        let plan = adapt(
            &harness.surface,
            &[
                HookDecl {
                    intent: HookIntent::Notify,
                    timeout: Some(30),
                    ..hook(HookEvent::SessionStart, "bounded.sh")
                },
                HookDecl {
                    intent: HookIntent::Notify,
                    ..hook(HookEvent::SessionStart, "unbounded.sh")
                },
            ],
        );
        let artifact = plan
            .artifacts
            .iter()
            .find(|a| a.content.contains("bounded.sh"))
            .expect("observer artifact");
        // The declared bound reaches the spawned process in milliseconds —
        // a hung observer is killed and reported instead of stalling the
        // host's event loop.
        assert!(
            artifact
                .content
                .contains(r#"execSync("bounded.sh", { stdio: 'pipe', timeout: 30000 })"#),
            "{}",
            artifact.content
        );
        // No declared bound: no timeout key — the declaration carries no
        // limit, so the generated code imposes none (Node's unbounded
        // default).
        assert!(
            artifact
                .content
                .contains(r#"execSync("unbounded.sh", { stdio: 'pipe' })"#),
            "{}",
            artifact.content
        );
    }
}

#[test]
fn extension_admission_reports_identical_outcomes_across_rendering_paths() {
    // Mixed intents and events: admission outcomes and their order are one
    // dialect-neutral policy; only the rendering format branches.
    let hooks = vec![
        HookDecl {
            intent: HookIntent::Notify,
            ..hook(HookEvent::SessionStart, "start.sh")
        },
        HookDecl {
            intent: HookIntent::Enforce,
            ..hook(HookEvent::SessionEnd, "guard.sh")
        },
        HookDecl {
            intent: HookIntent::Notify,
            ..hook(HookEvent::SubagentStart, "sub.sh")
        },
    ];
    let pi = adapt(&harnesses::PI.surface, &hooks);
    let opencode = adapt(&harnesses::OPENCODE.surface, &hooks);
    let outcomes = |plan: &InstallationPlan| {
        plan.report
            .iter()
            .map(|entry| (entry.item.clone(), entry.support))
            .collect::<Vec<_>>()
    };
    // Same admission verdicts in declaration order on both renderers.
    assert_eq!(outcomes(&pi), outcomes(&opencode));
    assert_eq!(
        outcomes(&pi),
        vec![
            ("hooks.session-start".to_owned(), Support::Adapted),
            ("hooks.session-end".to_owned(), Support::Unsupported),
            ("hooks.subagent-start".to_owned(), Support::Unsupported),
        ]
    );
    // Reporting detail stays per-dialect, byte-identical to before the
    // unification.
    assert_eq!(
        pi.report[0].note.as_deref(),
        Some("emitted as a generated pi extension")
    );
    assert_eq!(
        opencode.report[0].note.as_deref(),
        Some("emitted inside a generated opencode plugin")
    );
    assert_eq!(
        pi.report[2].note.as_deref(),
        Some("no pi extension event equivalent")
    );
    assert_eq!(
        opencode.report[2].note.as_deref(),
        Some("no opencode plugin event equivalent")
    );
    // The intent refusal is the shared policy message on both paths.
    for plan in [&pi, &opencode] {
        assert_eq!(
            plan.report[1].note.as_deref(),
            Some("generated event observers support Notify only; no veto or output injection")
        );
    }
}

// ------------------------------ boundaries ---------------------------------

#[test]
fn a_missing_surface_reports_every_hook_unsupported() {
    for harness in [&harnesses::DSH, &harnesses::GENERIC] {
        let plan = adapt(&harness.surface, &[hook(HookEvent::Stop, "x.sh")]);
        assert!(plan.artifacts.is_empty());
        assert_eq!(support(&plan, "hooks.stop"), Some(Support::Unsupported));
    }
}

#[test]
fn a_traversing_descriptor_path_is_refused_not_written() {
    let surface = HookSurface::Claude {
        file: "../hooks.json",
        at: "",
        events: &[HookEvent::Stop],
    };
    let plan = adapt(&surface, &[hook(HookEvent::Stop, "x.sh")]);
    assert!(plan.artifacts.is_empty());
    let entry = plan
        .report
        .iter()
        .find(|e| e.item == "../hooks.json")
        .expect("escape report");
    assert_eq!(entry.support, Support::Unsupported);
    assert_eq!(
        entry.note.as_deref(),
        Some("artifact path escapes its declared scope")
    );
    for file in ["/abs/hooks.json", ".x\\hooks.json"] {
        let plan = adapt(
            &HookSurface::Claude {
                file,
                at: "",
                events: &[HookEvent::Stop],
            },
            &[hook(HookEvent::Stop, "x.sh")],
        );
        assert!(plan.artifacts.is_empty(), "{file}");
    }
}

#[test]
fn a_deserialized_artifact_rejects_paths_escaping_the_scope() {
    // A host loading a plan from an untrusted file or IPC gets the same
    // line `push` holds at build time: `..`, absolute, backslash and empty
    // paths fail deserialization instead of being written outside the
    // home-relative scope.
    for path in [
        "../outside.json",
        "nested/../../outside.json",
        "/abs/hooks.json",
        ".x\\hooks.json",
        "",
    ] {
        let json = serde_json::json!({
            "path": path,
            "content": "x",
            "mode": {"kind": "create"},
        });
        assert!(serde_json::from_value::<Artifact>(json).is_err(), "{path}");
    }
    // Nested paths inside the scope still load.
    let artifact: Artifact = serde_json::from_value(serde_json::json!({
        "path": ".claude/nested/settings.json",
        "content": "x",
        "mode": {"kind": "create"},
    }))
    .unwrap();
    assert_eq!(
        artifact.path,
        std::path::Path::new(".claude/nested/settings.json")
    );
    // And a plan carrying an escaping artifact is refused as a whole.
    let plan = serde_json::json!({
        "artifacts": [{"path": "../evil.json", "content": "x", "mode": {"kind": "create"}}],
        "report": [],
    });
    assert!(serde_json::from_value::<InstallationPlan>(plan).is_err());
}

#[test]
fn installation_plan_serializes_for_foreign_hosts() {
    let plan = adapt(
        &harnesses::CODEX.surface,
        &[hook(HookEvent::SessionStart, "x.sh")],
    );
    let value = serde_json::to_value(&plan).unwrap();
    assert!(value["artifacts"].is_array());
    assert_eq!(value["report"][0]["support"], "native");
    assert_eq!(value["report"][0]["item"], "hooks.session-start");
    let round: InstallationPlan = serde_json::from_value(value).unwrap();
    assert_eq!(round.artifacts.len(), plan.artifacts.len());
    assert_eq!(
        round.artifacts[0],
        Artifact {
            path: std::path::PathBuf::from(".codex/hooks.json"),
            content: plan.artifacts[0].content.clone(),
            mode: WriteMode::JsonMerge { at: String::new() },
        }
    );
}
