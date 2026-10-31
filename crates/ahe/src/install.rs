use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

pub mod harnesses;

pub use harnesses::{Harness, NativeStatus};

// ------------------------- semantic declarations -------------------------

/// Semantic lifecycle event a hook attaches to. Hosts write these names;
/// each dialect emitter maps them onto the agent's native event names or
/// reports the item unsupported.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HookEvent {
    SessionStart,
    SessionEnd,
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    Stop,
    SubagentStart,
    SubagentStop,
    PermissionRequest,
    Notification,
    /// After context compaction completes (Devin `PostCompaction`).
    PostCompaction,
}

/// What a hook is for. Dialects that cannot implement the requested intent
/// report `Unsupported`; generated extension observers accept only `Notify`.
/// An observation-only callback must never claim to enforce or rewrite.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HookIntent {
    /// Block or veto (e.g. deny a tool call, keep the agent working).
    #[default]
    Enforce,
    /// Feed hook output back into the agent's context.
    Inject,
    /// Rewrite what the model sees after the event.
    Rewrite,
    /// Observe only; never affects the run.
    Notify,
}

/// One lifecycle hook: a shell command the agent's hook store invokes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HookDecl {
    pub event: HookEvent,
    /// Shell command executed by the agent's hook runner.
    pub command: String,
    #[serde(default)]
    pub intent: HookIntent,
    /// Tool-name/pattern filter where the dialect supports matchers.
    #[serde(default)]
    pub matcher: Option<String>,
    /// Seconds the agent should allow the hook to run.
    #[serde(default)]
    pub timeout: Option<u64>,
}

impl HookEvent {
    /// Kebab-case name — the report item suffix and the wire form. Derived
    /// from the serde `kebab-case` rename so the two can never drift.
    pub fn kebab_name(self) -> String {
        match serde_json::to_value(self) {
            Ok(serde_json::Value::String(name)) => name,
            _ => unreachable!("unit variant serializes to its rename string"),
        }
    }
}

pub fn merge(base: &mut Vec<HookDecl>, overlay: Vec<HookDecl>) {
    for hook in overlay {
        if !base.iter().any(|h| {
            h.event == hook.event && h.command == hook.command && h.matcher == hook.matcher
        }) {
            base.push(hook);
        }
    }
}

pub fn validate(hooks: &[HookDecl]) -> Result<(), &'static str> {
    for hook in hooks {
        if hook.command.trim().is_empty() {
            return Err("hook requires a non-empty command");
        }
    }
    Ok(())
}

// ----------------------------- descriptor ---------------------------------

/// How an agent stores lifecycle hooks.
#[derive(Clone, Copy, Debug)]
pub enum HookSurface {
    /// No hook store — every hook reports unsupported.
    None,
    /// Claude dialect: a JSON document deep-merged at `at` with
    /// `{"Event":[{matcher?,hooks:[{type:command,command,timeout}]}]}`.
    /// `events` are the semantic events the store accepts (native names
    /// match 1:1). Used by Claude Code, Codex, Devin and Command Code.
    Claude {
        file: &'static str,
        at: &'static str,
        events: &'static [HookEvent],
    },
    /// Grok's variant: one whole file per hook owner in `dir`, containing
    /// the same Claude-shape fragment.
    ClaudeBundle {
        dir: &'static str,
        events: &'static [HookEvent],
    },
    /// Gemini/Antigravity named bundles merged into one document:
    /// `{"a4aa":{"Event":[{command,timeout,type}],"enabled":true}}`.
    /// `map` translates semantic events to native names.
    NamedBundle {
        file: &'static str,
        map: &'static [(HookEvent, &'static str)],
    },
    /// Cursor's flat store: `{version:1,hooks:{<event>:[{command}]}}`
    /// merged at the document root. `map` translates event names.
    Flat {
        file: &'static str,
        map: &'static [(HookEvent, &'static str)],
    },
    /// Code-extension agents: hooks are generated code units (Pi
    /// extensions, OpenCode plugins), not config.
    Extension {
        file: &'static str,
        kind: ExtensionKind,
        map: &'static [(HookEvent, &'static str)],
    },
}

/// Generated-code dialects for [`HookSurface::Extension`].
#[derive(Clone, Copy, Debug)]
pub enum ExtensionKind {
    /// `~/.pi/agent/extensions/*.ts`: `pi.on('<event>', …)`.
    Pi,
    /// `~/.config/opencode/plugins/*.js`: plugin event handlers.
    OpenCode,
}

// ----------------------------- plan types ---------------------------------

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WriteMode {
    Create,
    JsonMerge { at: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Artifact {
    pub path: PathBuf,
    pub content: String,
    pub mode: WriteMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Support {
    Native,
    Adapted,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupportEntry {
    pub item: String,
    pub support: Support,
    #[serde(default)]
    pub note: Option<String>,
}

fn scope_relative(path: &str) -> bool {
    if path.is_empty() || path.contains('\\') {
        return false;
    }
    let parsed = Path::new(path);
    !parsed.is_absolute()
        && parsed
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallationPlan {
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    #[serde(default)]
    pub report: Vec<SupportEntry>,
}

impl InstallationPlan {
    pub(crate) fn push(&mut self, path: &str, content: String, mode: WriteMode) {
        if !scope_relative(path) {
            self.report(
                path.to_owned(),
                Support::Unsupported,
                Some("artifact path escapes its declared scope"),
            );
            return;
        }
        self.artifacts.push(Artifact {
            path: PathBuf::from(path),
            content,
            mode,
        });
    }

    pub(crate) fn report(&mut self, item: impl Into<String>, support: Support, note: Option<&str>) {
        self.report.push(SupportEntry {
            item: item.into(),
            support,
            note: note.map(str::to_owned),
        });
    }
}

// ------------------------------- hooks ------------------------------------

pub fn adapt(surface: &HookSurface, hooks: &[HookDecl]) -> InstallationPlan {
    let mut plan = InstallationPlan::default();
    let item = |hook: &HookDecl| format!("hooks.{}", hook.event.kebab_name());
    match surface {
        HookSurface::None => {
            for hook in hooks {
                plan.report(
                    item(hook),
                    Support::Unsupported,
                    Some("no hook surface for this dialect"),
                );
            }
        }
        HookSurface::Claude { file, at, events } => {
            let (emit, missing): (Vec<_>, Vec<_>) =
                hooks.iter().partition(|h| events.contains(&h.event));
            for hook in missing {
                plan.report(
                    item(hook),
                    Support::Unsupported,
                    Some("event not exposed by this agent's hook store"),
                );
            }
            for hook in &emit {
                plan.report(item(hook), Support::Native, None);
            }
            if !emit.is_empty() {
                plan.push(
                    file,
                    serde_json::to_string_pretty(&claude_hook_fragment(&emit)["hooks"])
                        .unwrap_or_default(),
                    WriteMode::JsonMerge { at: at.to_string() },
                );
            }
        }
        HookSurface::ClaudeBundle { dir, events } => {
            let (emit, missing): (Vec<_>, Vec<_>) =
                hooks.iter().partition(|h| events.contains(&h.event));
            for hook in missing {
                plan.report(
                    item(hook),
                    Support::Unsupported,
                    Some("event not exposed by this agent's hook store"),
                );
            }
            for hook in &emit {
                plan.report(item(hook), Support::Native, None);
            }
            if !emit.is_empty() {
                plan.push(
                    &format!("{dir}/a4aa.json"),
                    serde_json::to_string_pretty(&claude_hook_fragment(&emit)).unwrap_or_default(),
                    WriteMode::Create,
                );
            }
        }
        HookSurface::NamedBundle { file, map } => {
            let mut bundle = serde_json::Map::new();
            let mut any = false;
            for hook in hooks {
                match map.iter().find(|(event, _)| *event == hook.event) {
                    Some((_, name)) => {
                        any = true;
                        let mut entry = serde_json::json!({
                            "command": hook.command,
                            "type": "command",
                        });
                        if let Some(timeout) = hook.timeout {
                            entry["timeout"] = serde_json::json!(timeout);
                        }
                        bundle
                            .entry(name.to_string())
                            .or_insert_with(|| serde_json::json!([]))
                            .as_array_mut()
                            .expect("bundle list")
                            .push(entry);
                        plan.report(item(hook), Support::Adapted, None);
                    }
                    None => plan.report(
                        item(hook),
                        Support::Unsupported,
                        Some("no bundle event equivalent"),
                    ),
                }
            }
            if any {
                bundle.insert("enabled".to_owned(), serde_json::json!(true));
                plan.push(
                    file,
                    serde_json::to_string_pretty(&serde_json::json!({"a4aa": bundle}))
                        .unwrap_or_default(),
                    WriteMode::JsonMerge { at: String::new() },
                );
            }
        }
        HookSurface::Flat { file, map } => {
            let mut hooks_map = serde_json::Map::new();
            let mut any = false;
            for hook in hooks {
                match map.iter().find(|(event, _)| *event == hook.event) {
                    Some((_, name)) => {
                        any = true;
                        hooks_map
                            .entry(name.to_string())
                            .or_insert_with(|| serde_json::json!([]))
                            .as_array_mut()
                            .expect("event list")
                            .push(serde_json::json!({ "command": hook.command }));
                        plan.report(item(hook), Support::Adapted, None);
                    }
                    None => plan.report(
                        item(hook),
                        Support::Unsupported,
                        Some("no flat-store event equivalent"),
                    ),
                }
            }
            if any {
                plan.push(
                    file,
                    serde_json::to_string_pretty(
                        &serde_json::json!({ "version": 1, "hooks": hooks_map }),
                    )
                    .unwrap_or_default(),
                    WriteMode::JsonMerge { at: String::new() },
                );
            }
        }
        HookSurface::Extension { file, kind, map } => {
            // One admission pass for generated-code observers: intent
            // support and event mapping are dialect-neutral policy with a
            // single authoritative implementation; only the rendering
            // format branches per extension kind.
            let (surface, adapted_note) = match kind {
                ExtensionKind::Pi => ("pi extension", "emitted as a generated pi extension"),
                ExtensionKind::OpenCode => (
                    "opencode plugin",
                    "emitted inside a generated opencode plugin",
                ),
            };
            let mut emitted = Vec::new();
            for hook in hooks {
                if hook.intent != HookIntent::Notify {
                    plan.report(item(hook), Support::Unsupported,
                        Some("generated event observers support Notify only; no veto or output injection"));
                    continue;
                }
                match map.iter().find(|(event, _)| *event == hook.event) {
                    Some((_, event)) => {
                        emitted.push((*event, hook));
                        plan.report(item(hook), Support::Adapted, Some(adapted_note));
                    }
                    None => plan.report(
                        item(hook),
                        Support::Unsupported,
                        Some(&format!("no {surface} event equivalent")),
                    ),
                }
            }
            if !emitted.is_empty() {
                let body = match kind {
                    ExtensionKind::Pi => pi_extension_body(&emitted),
                    ExtensionKind::OpenCode => opencode_plugin_body(&emitted),
                };
                plan.push(file, body, WriteMode::Create);
            }
        }
    }
    plan
}

/// Pi extension body: one `pi.on` observer per admitted hook. Observers
/// never veto — a failed command reports the event, command and reason to
/// the host log (`console.error`, the extension host's diagnostic channel)
/// and is never rethrown.
fn pi_extension_body(emitted: &[(&str, &HookDecl)]) -> String {
    let mut body = String::from("// @a4aa-managed\nexport default function (pi) {\n");
    for (event, hook) in emitted {
        let command = serde_json::to_string(&hook.command).unwrap_or_default();
        body.push_str(&format!(
            "  pi.on('{event}', async () => {{ const {{ execSync }} = await import('node:child_process'); try {{ execSync({command}, {{ stdio: 'pipe' }}) }} catch (error) {{ console.error('[a4aa] observer hook failed', {{ event: '{event}', command: {command}, reason: String(error) }}) }} }})\n"
        ));
    }
    body.push_str("}\n");
    body
}

/// OpenCode plugin body: one event dispatcher per admitted hook, same
/// fail-open diagnostic contract as [`pi_extension_body`].
fn opencode_plugin_body(emitted: &[(&str, &HookDecl)]) -> String {
    let mut body = String::from(
        "// @a4aa-managed\nexport const a4aaHooks = async () => ({\n  event: async ({ event }) => {\n",
    );
    for (event, hook) in emitted {
        let command = serde_json::to_string(&hook.command).unwrap_or_default();
        body.push_str(&format!(
            "    if (event.type === '{event}') {{ const {{ execSync }} = await import('node:child_process'); try {{ execSync({command}) }} catch (error) {{ console.error('[a4aa] observer hook failed', {{ event: '{event}', command: {command}, reason: String(error) }}) }} }}\n"
        ));
    }
    body.push_str("  },\n})\n");
    body
}

/// Claude-dialect hook fragment: `{"hooks":{"Event":[{matcher?,hooks:
/// [{type:command,command,timeout}]}]}}` grouped per event.
fn claude_hook_fragment(hooks: &[&HookDecl]) -> serde_json::Value {
    let mut events = serde_json::Map::new();
    for hook in hooks {
        let mut command = serde_json::json!({
            "type": "command",
            "command": hook.command,
        });
        if let Some(timeout) = hook.timeout {
            command["timeout"] = serde_json::json!(timeout);
        }
        let mut entry = serde_json::json!({ "hooks": [command] });
        if let Some(matcher) = &hook.matcher {
            entry["matcher"] = serde_json::json!(matcher);
        }
        events
            .entry(claude_event_name(hook.event))
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .expect("event list")
            .push(entry);
    }
    serde_json::json!({ "hooks": events })
}

fn claude_event_name(event: HookEvent) -> String {
    match event {
        HookEvent::SessionStart => "SessionStart",
        HookEvent::SessionEnd => "SessionEnd",
        HookEvent::UserPromptSubmit => "UserPromptSubmit",
        HookEvent::PreToolUse => "PreToolUse",
        HookEvent::PostToolUse => "PostToolUse",
        HookEvent::Stop => "Stop",
        HookEvent::SubagentStart => "SubagentStart",
        HookEvent::SubagentStop => "SubagentStop",
        HookEvent::PermissionRequest => "PermissionRequest",
        HookEvent::Notification => "Notification",
        HookEvent::PostCompaction => "PostCompaction",
    }
    .to_owned()
}
