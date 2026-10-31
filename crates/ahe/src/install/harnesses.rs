use super::{ExtensionKind, HookEvent, HookSurface};

#[derive(Clone, Copy, Debug)]
pub struct Harness {
    pub surface: HookSurface,
    pub config_path: &'static str,
    pub native_status: NativeStatus,
    pub native_note: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeStatus {
    Native,
    NotOffered,
    Unadapted,
    NonNative,
}

pub const ANTIGRAVITY: Harness = Harness {
    surface: HookSurface::NamedBundle {
        file: ".gemini/config/hooks.json",
        map: &[
            (HookEvent::SessionStart, "SessionStart"),
            (HookEvent::PreToolUse, "PreToolUse"),
            (HookEvent::PostToolUse, "PostToolUse"),
            (HookEvent::Stop, "Stop"),
        ],
    },
    config_path: "{home}/.gemini/config/hooks.json",
    native_status: NativeStatus::Native,
    native_note: "named bundle SessionStart/PreToolUse/…",
};

pub const CLAUDE: Harness = Harness {
    surface: HookSurface::Claude {
        file: ".claude/settings.json",
        at: "hooks",
        events: &[
            HookEvent::SessionStart,
            HookEvent::SessionEnd,
            HookEvent::UserPromptSubmit,
            HookEvent::PreToolUse,
            HookEvent::PostToolUse,
            HookEvent::Stop,
            HookEvent::SubagentStart,
            HookEvent::SubagentStop,
            HookEvent::PermissionRequest,
            HookEvent::Notification,
        ],
    },
    config_path: "{home}/.claude/settings.json hooks",
    native_status: NativeStatus::Native,
    native_note: "Claude settings.json hooks",
};

pub const CODEX: Harness = Harness {
    surface: HookSurface::Claude {
        file: ".codex/hooks.json",
        at: "",
        events: &[
            HookEvent::SessionStart,
            HookEvent::UserPromptSubmit,
            HookEvent::PreToolUse,
            HookEvent::PermissionRequest,
            HookEvent::PostToolUse,
            HookEvent::Stop,
            HookEvent::SubagentStart,
            HookEvent::SubagentStop,
        ],
    },
    config_path: "{home}/.codex/hooks.json",
    native_status: NativeStatus::Native,
    native_note: "Claude-shape hooks.json",
};

pub const COMMAND_CODE: Harness = Harness {
    surface: HookSurface::Claude {
        file: ".commandcode/settings.json",
        at: "hooks",
        events: &[
            HookEvent::SessionStart,
            HookEvent::PreToolUse,
            HookEvent::PostToolUse,
            HookEvent::Stop,
        ],
    },
    config_path: "{home}/.commandcode/settings.json hooks",
    native_status: NativeStatus::Native,
    native_note: "Claude-shape settings.json hooks",
};

pub const COPILOT: Harness = Harness {
    surface: HookSurface::None,
    config_path:
        "{home}/.copilot/hooks/*.json — {version:1,hooks:{Event:[{type,bash,timeoutSec}]}}, unadapted",
    native_status: NativeStatus::Unadapted,
    native_note:
        "~/.copilot/hooks/*.json {version,hooks:{Event:[{type:command,bash,timeoutSec}]}} — bash-field shape not carded",
};

pub const CURSOR: Harness = Harness {
    surface: HookSurface::Flat {
        file: ".cursor/hooks.json",
        map: &[(HookEvent::UserPromptSubmit, "beforeSubmitPrompt")],
    },
    config_path: "{home}/.cursor/hooks.json",
    native_status: NativeStatus::Native,
    native_note: "flat hooks.json beforeSubmitPrompt",
};

// `~/.config/devin/config.json` `hooks` — the Claude-dialect store.
// Devin fires eight events; `SubagentStart`, `SubagentStop` and
// `Notification` have no native counterpart and report unsupported.
pub const DEVIN: Harness = Harness {
    surface: HookSurface::Claude {
        file: ".config/devin/config.json",
        at: "hooks",
        events: &[
            HookEvent::SessionStart,
            HookEvent::SessionEnd,
            HookEvent::UserPromptSubmit,
            HookEvent::PreToolUse,
            HookEvent::PostToolUse,
            HookEvent::Stop,
            HookEvent::PermissionRequest,
            HookEvent::PostCompaction,
        ],
    },
    config_path: "{home}/.config/devin/config.json hooks",
    native_status: NativeStatus::Native,
    native_note: "Claude-shape config.json hooks",
};

pub const DSH: Harness = Harness {
    surface: HookSurface::None,
    config_path: "",
    native_status: NativeStatus::NotOffered,
    native_note: "HookSurface::None",
};

pub const FACTORY: Harness = Harness {
    surface: HookSurface::None,
    config_path: "",
    native_status: NativeStatus::NotOffered,
    native_note: "HookSurface::None",
};

pub const GROK: Harness = Harness {
    surface: HookSurface::ClaudeBundle {
        dir: ".grok/hooks",
        events: &[
            HookEvent::SessionStart,
            HookEvent::SessionEnd,
            HookEvent::UserPromptSubmit,
            HookEvent::PreToolUse,
            HookEvent::PostToolUse,
            HookEvent::Stop,
            HookEvent::SubagentStart,
            HookEvent::SubagentStop,
            HookEvent::PermissionRequest,
            HookEvent::Notification,
        ],
    },
    config_path: "{home}/.grok/hooks",
    native_status: NativeStatus::Native,
    native_note: "Claude-bundle per-file hooks",
};

pub const HERMES: Harness = Harness {
    surface: HookSurface::None,
    config_path: "",
    native_status: NativeStatus::NotOffered,
    native_note: "event-bus hooks unmapped",
};

pub const KILO_CODE: Harness = Harness {
    surface: HookSurface::Extension {
        file: ".config/kilo/plugin/a4aa.js",
        kind: ExtensionKind::OpenCode,
        map: &[
            (HookEvent::SessionStart, "session.created"),
            (HookEvent::PreToolUse, "tool.execute.before"),
            (HookEvent::PostToolUse, "tool.execute.after"),
        ],
    },
    config_path: "{home}/.config/kilo/plugin/a4aa.js (native plugin dir)",
    native_status: NativeStatus::Native,
    native_note: "OpenCode plugin events in .config/kilo/plugin",
};

pub const KIMI_CODE: Harness = Harness {
    surface: HookSurface::None,
    config_path: "",
    native_status: NativeStatus::NotOffered,
    native_note: "HookSurface::None",
};

pub const KIRO: Harness = Harness {
    surface: HookSurface::None,
    config_path: "IDE trigger hooks unmapped",
    native_status: NativeStatus::NotOffered,
    native_note: "IDE trigger hooks unmapped",
};

pub const OPEN_CLAW: Harness = Harness {
    surface: HookSurface::None,
    config_path: "",
    native_status: NativeStatus::NotOffered,
    native_note: "HookSurface::None",
};

pub const OPENCODE: Harness = Harness {
    surface: HookSurface::Extension {
        file: ".config/opencode/plugins/a4aa.js",
        kind: ExtensionKind::OpenCode,
        map: &[
            (HookEvent::SessionStart, "session.created"),
            (HookEvent::PreToolUse, "tool.execute.before"),
            (HookEvent::PostToolUse, "tool.execute.after"),
        ],
    },
    config_path: "{home}/.config/opencode/plugins/a4aa.js",
    native_status: NativeStatus::Native,
    native_note: "OpenCode JS plugin events",
};

pub const PI: Harness = Harness {
    surface: HookSurface::Extension {
        file: ".pi/agent/extensions/a4aa.ts",
        kind: ExtensionKind::Pi,
        map: &[(HookEvent::SessionStart, "session_start")],
    },
    config_path: "{home}/.pi/agent/extensions/*.ts",
    native_status: NativeStatus::Native,
    native_note: "Pi TS extensions, session_start",
};

pub const GENERIC: Harness = Harness {
    surface: HookSurface::None,
    config_path: "",
    native_status: NativeStatus::NotOffered,
    native_note: "generic baseline declares no hook store",
};
