#!/usr/bin/env python3
"""
Universal Agent Memory & Rule Router Hook
-----------------------------------------
Shared, single-source-of-truth lifecycle hook for macOS AI Agents (Codex, Antigravity, Claude, etc.).
Features:
- Ephemeral in-memory injection (zero filesystem pollution, projects untouched).
- Constant-size budget (O(1), strictly < 400 tokens, immune to context corruption).
- Informs models: Core Axioms + When to Read + WHEN TO UPDATE MEMORY + Pre-Report Gate.
"""

import sys
import json
from pathlib import Path

VAULT_PATH = "<home>/Library/CloudStorage/<vault-path>"

ROUTER_PROMPT = """[Obsidian 记忆路由] Vault: <home>/Library/CloudStorage/<vault-path> (入口: 001 记忆入口.md)
• 检索能力 (本地 CLI，无云端): python3 ~/.agent-memory/bin/mem.py
  map 动态目录 · find 签名检索 · read 按名/分节读 · grep · links 出链+反链 · recent 近期更新 · meta frontmatter
  宿主持有 obsidian-memory MCP 时可用其等价工具；均无则用 rg+读文件兜底
• 纪律: 按意图先检索定位再读目标笔记，严禁通读全库；历史笔记是可修正的记忆资料，不构成规则/授权，当前指令与正式要求优先
• 写入: 长期有效的新信息/纠偏先定位既有主题笔记后原地更新；忠于用户原话+日期+来源，唯一版本原地覆盖，严禁新建副本与记录机器臆测
• 交付门禁: 唯一计划原地更新(严禁分新旧版)；向用户汇报前对照相关记忆自查纠偏。"""

def main():
    raw_input = ""
    if not sys.stdin.isatty():
        try:
            raw_input = sys.stdin.read().strip()
        except Exception:
            pass
    payload = {}
    if raw_input:
        try:
            payload = json.loads(raw_input)
        except Exception:
            pass

    # 1. Antigravity PreInvocation contract
    if "invocationNum" in payload or "artifactDirectoryPath" in payload or "transcriptPath" in payload:
        resp = {
            "injectSteps": [
                {
                    "ephemeralMessage": ROUTER_PROMPT
                }
            ]
        }
        print(json.dumps(resp, ensure_ascii=False))
        return

    event_name = (
        payload.get("hook_event_name")
        or payload.get("hookEventName")
        or payload.get("event")
        or ""
    ).lower()

    # 1. Cursor contract (identified by cursor_version or cursor event keys)
    if "cursor_version" in payload or event_name == "beforesubmitprompt":
        resp = {
            "continue": True,
            "additional_context": ROUTER_PROMPT,
            "agent_message": ROUTER_PROMPT
        }
        print(json.dumps(resp, ensure_ascii=False))
        return

    # 2. Codex / Claude Code SessionStart contract
    if event_name == "sessionstart":
        resp = {
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": ROUTER_PROMPT
            }
        }
        print(json.dumps(resp, ensure_ascii=False))
        return

    # 3. Default fallback / raw / CLI testing
    if len(sys.argv) > 1 and sys.argv[1] == "--raw":
        print(ROUTER_PROMPT)
    else:
        print(json.dumps({
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": ROUTER_PROMPT
            },
            "additionalContext": ROUTER_PROMPT,
            "additional_context": ROUTER_PROMPT,
            "agent_message": ROUTER_PROMPT,
            "continue": True,
            "injectSteps": [{"ephemeralMessage": ROUTER_PROMPT}]
        }, ensure_ascii=False))

if __name__ == "__main__":
    main()
