#!/usr/bin/env python3
"""
agent-memory installer — wires the distill hook into each agent host's
lifecycle config. Idempotent, backup-first, preserves all unrelated entries.

  install.py             # install everywhere detected
  install.py --check     # status only, change nothing
  install.py --host codex --host kimi
  install.py --uninstall [--host X]

Hosts:
  codex     ~/.codex/hooks.json           hooks.Stop[]           (nested groups)
  claude    ~/.claude/settings.json       hooks.Stop[]           (nested groups)
  kimi      ~/.kimi-code/config.toml      [[hooks]] event="Stop" (managed block)
  cursor    ~/.cursor/hooks.json          hooks.stop[]           (flat array)
  opencode  ~/.config/opencode/plugins/   agent-memory-distill.js
  dsh/devin no hook surface → poll-only, recorded in registry
"""

import argparse
import json
import os
import shutil
import sys
from datetime import datetime
from pathlib import Path

HOME = Path(os.path.expanduser("~"))
MEM = HOME / ".agent-memory"
BIN = MEM / "bin"
HOOK = BIN / "distill-hook.py"
SKILL_DIR = HOME / ".agents" / "skills" / "memory-distill"

PY = shutil.which("python3") or "python3"


def backup(p: Path) -> Path:
    ts = datetime.now().strftime("%Y%m%d-%H%M%S")
    b = p.with_suffix(p.suffix + f".bak-agent-memory-{ts}")
    shutil.copy2(p, b)
    return b


def hook_cmd(host: str) -> str:
    return f"{PY} {HOOK} {host}"


def already(command: str) -> bool:
    return "distill-hook.py" in (command or "")


def group_has(group: dict) -> bool:
    return any(already(h.get("command", "")) for h in group.get("hooks", []))


def load_json(p: Path) -> dict:
    return json.loads(p.read_text(encoding="utf-8"))


def save_json(p: Path, d: dict, do_backup: bool = True) -> None:
    if do_backup and p.exists():
        backup(p)
    p.write_text(json.dumps(d, indent=2, ensure_ascii=False) + "\n",
                 encoding="utf-8")


# ---------------------------------------------------------------- adapters

def install_nested(path: Path, host: str, check: bool, uninstall: bool):
    """Codex/Claude settings.json nested-group shape."""
    name = host
    if not path.exists():
        return f"{name}: no config at {path} — skipped"
    d = load_json(path)
    hooks = d.setdefault("hooks", {})
    events = hooks.setdefault("Stop", [])
    found = any(group_has(g) for g in events if isinstance(g, dict))

    if uninstall:
        if not found:
            return f"{name}: nothing installed"
        backup(path)
        for g in events:
            if isinstance(g, dict):
                g["hooks"] = [h for h in g.get("hooks", [])
                              if not already(h.get("command", ""))]
        hooks["Stop"] = [g for g in events
                         if not isinstance(g, dict) or g.get("hooks")]
        save_json(path, d, do_backup=False)
        return f"{name}: uninstalled"

    if check or found:
        return f"{name}: {'installed' if found else 'NOT installed'}"
    backup(path)
    events.append({"hooks": [{
        "type": "command",
        "command": hook_cmd(host),
        "timeout": 15,
        "statusMessage": "agent-memory distill",
    }]})
    save_json(path, d, do_backup=False)
    return f"{name}: installed (Stop → {HOOK.name} {host})"


def install_cursor(path: Path, check: bool, uninstall: bool):
    """Cursor hooks.json flat-array shape."""
    if not path.exists():
        return f"cursor: no config at {path} — skipped"
    d = load_json(path)
    events = d.setdefault("hooks", {}).setdefault("stop", [])
    found = any(already(e.get("command", "")) for e in events)
    if uninstall:
        if not found:
            return "cursor: nothing installed"
        backup(path)
        d["hooks"]["stop"] = [e for e in events
                              if not already(e.get("command", ""))]
        save_json(path, d, do_backup=False)
        return "cursor: uninstalled"
    if check or found:
        return f"cursor: {'installed' if found else 'NOT installed'}"
    backup(path)
    events.append({"command": hook_cmd("cursor"), "timeout": 15})
    save_json(path, d, do_backup=False)
    return "cursor: installed (stop hook)"


KIMI_MARK_BEGIN = "# >>> agent-memory-hook >>>"
KIMI_MARK_END = "# <<< agent-memory-hook <<<"
KIMI_BLOCK = f"""{KIMI_MARK_BEGIN}
[[hooks]]
event = "Stop"
command = "{PY} {HOOK} kimi"
timeout = 15
{KIMI_MARK_END}
"""


def install_kimi(path: Path, check: bool, uninstall: bool):
    if not path.exists():
        return f"kimi: no config at {path} — skipped"
    txt = path.read_text(encoding="utf-8")
    found = KIMI_MARK_BEGIN in txt or "distill-hook.py" in txt
    if uninstall:
        if not found:
            return "kimi: nothing installed"
        backup(path)
        if KIMI_MARK_BEGIN in txt:
            i, j = txt.index(KIMI_MARK_BEGIN), txt.index(KIMI_MARK_END)
            txt = txt[:i] + txt[j + len(KIMI_MARK_END):].lstrip("\n")
        else:
            txt = "\n".join(l for l in txt.splitlines()
                            if "distill-hook.py" not in l)
        path.write_text(txt, encoding="utf-8")
        return "kimi: uninstalled"
    if check or found:
        return f"kimi: {'installed' if found else 'NOT installed'}"
    backup(path)
    if not txt.endswith("\n"):
        txt += "\n"
    path.write_text(txt + "\n" + KIMI_BLOCK, encoding="utf-8")
    return "kimi: installed (Stop hook)"


OPENCODE_PLUGIN = r"""// agent-memory distill — on session.idle, ask the agent (hot context) to
// write memory candidates to ~/.agent-memory/inbox/, once per session.
// Falls back to flagging the session for poll.py on any failure.

const fs = require("fs");
const path = require("path");
const os = require("os");

const MEM = path.join(os.homedir(), ".agent-memory");
const STATE = path.join(MEM, "state");
const FLAGGED = path.join(MEM, "flagged");
const SKILL = path.join(os.homedir(),
                        ".agents/skills/memory-distill/SKILL.md");

function ensureDir(p) {
  try { fs.mkdirSync(p, { recursive: true }); } catch {}
}

function distilledFile(sessionID) {
  return path.join(STATE, `distilled-opencode-${sessionID}`);
}

const INSTRUCTION =
  "[记忆蒸馏·会话结束前最后一步] 本会话即将空闲/结束。请按 " + SKILL +
  " 的 Distill 契约，把本会话中用户本人表达的、长期有效的规则/决策/设计" +
  "蒸馏为候选条目：写入 " + path.join(MEM, "inbox") +
  "/opencode-<SESSION>.jsonl（每行一个 JSON 对象；没有值得沉淀的内容就写" +
  "单行 {\"noop\":true}）。只写 inbox，严禁直接改 canonical 笔记。";

async function onIdle(client, sessionID) {
  if (!sessionID) return;
  const flag = distilledFile(sessionID);
  try { if (fs.existsSync(flag)) return; } catch {}
  ensureDir(STATE);
  try { fs.writeFileSync(flag, new Date().toISOString()); } catch {}
  try {
    if (client && client.session && client.session.prompt) {
      await client.session.prompt({
        path: { id: sessionID },
        body: {
          parts: [{
            type: "text",
            text: INSTRUCTION.replace("<SESSION>", String(sessionID)),
          }],
        },
      });
      return;
    }
  } catch {}
  // fail-open: flag for poll instead
  try {
    ensureDir(FLAGGED);
    fs.writeFileSync(path.join(FLAGGED, `opencode-${sessionID}`),
                     new Date().toISOString());
  } catch {}
}

async function setup() {
  return {
    event: async ({ event }) => {
      try {
        if (event && event.type === "session.idle") {
          const p = event.properties || {};
          await onIdle(globalThis.__agentMemoryClient,
                       p.sessionID || p.sessionId || p.id);
        }
      } catch {}
    },
  };
}

async function AgentMemoryDistill({ client } = {}) {
  globalThis.__agentMemoryClient = client;
  return {
    event: async ({ event }) => {
      try {
        if (event && event.type === "session.idle") {
          const p = event.properties || {};
          await onIdle(client, p.sessionID || p.sessionId || p.id);
        }
      } catch {}
    },
  };
}

export default {
  id: "agent-memory-distill",
  server: AgentMemoryDistill,
  setup,
};
export const agentMemoryDistill = AgentMemoryDistill;
"""


def install_opencode(plugins_dir: Path, check: bool, uninstall: bool):
    target = plugins_dir / "agent-memory-distill.js"
    if not plugins_dir.is_dir():
        return f"opencode: no plugins dir at {plugins_dir} — skipped"
    if uninstall:
        if target.exists():
            target.unlink()
            return "opencode: uninstalled"
        return "opencode: nothing installed"
    if target.exists():
        return "opencode: installed" if check else "opencode: already installed"
    if check:
        return "opencode: NOT installed"
    target.write_text(OPENCODE_PLUGIN, encoding="utf-8")
    return "opencode: installed (session.idle plugin)"


# ---------------------------------------------------------------- registry

HOSTS = {
    "codex": lambda c, u: install_nested(HOME / ".codex" / "hooks.json",
                                         "codex", c, u),
    "claude": lambda c, u: install_nested(HOME / ".claude" / "settings.json",
                                          "claude", c, u),
    "kimi": lambda c, u: install_kimi(HOME / ".kimi-code" / "config.toml",
                                      c, u),
    "cursor": lambda c, u: install_cursor(HOME / ".cursor" / "hooks.json",
                                          c, u),
    "opencode": lambda c, u: install_opencode(
        HOME / ".config" / "opencode" / "plugins", c, u),
}

POLL_ONLY = ["dsh", "devin", "antigravity", "commandcode"]


def main() -> int:
    ap = argparse.ArgumentParser(description="agent-memory installer")
    ap.add_argument("--host", action="append", choices=sorted(HOSTS),
                    help="limit to specific hosts")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--uninstall", action="store_true")
    args = ap.parse_args()

    # sanity: pipeline assets exist
    missing = [p for p in (HOOK, BIN / "adjudicate.py", BIN / "poll.py",
                           SKILL_DIR / "SKILL.md") if not p.exists()]
    if missing:
        print("pipeline files missing:", file=sys.stderr)
        for m in missing:
            print(f"  {m}", file=sys.stderr)
        return 2

    for d in (MEM / "inbox" / "done", MEM / "raw", MEM / "state",
              MEM / "flagged", MEM / "plans"):
        d.mkdir(parents=True, exist_ok=True)

    wanted = args.host or list(HOSTS)
    for name in wanted:
        try:
            print(HOSTS[name](args.check, args.uninstall))
        except Exception as ex:
            print(f"{name}: ERROR {ex}")

    if not args.check and not args.uninstall:
        if POLL_ONLY:
            print(f"\npoll-only sources (no hook surface): "
                  f"{', '.join(POLL_ONLY)}")
            print("  → covered by: python3 ~/.agent-memory/bin/poll.py")
    return 0


if __name__ == "__main__":
    sys.exit(main())
