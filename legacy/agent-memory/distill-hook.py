#!/usr/bin/env python3
"""
agent-memory distill hook — Stop/SessionEnd lifecycle hook for all agents.

Incremental on hosts that pass a per-turn prompt_id (Devin): distills once
per user prompt — a distilled prompt is never redone, and later turns still
get their own pass, so nothing said after the first turn is lost. On hosts
without prompt_id it falls back to once per session: if the session had real
user interaction and was not distilled yet, asks the agent (still holding its
hot context) to write a JSONL candidate file to ~/.agent-memory/inbox/ before
it stops. Writes to the vault are never performed here — canonical notes are
only produced by the separate adjudication flow.

Contracts handled (multi-field defensive output, like universal-memory-router):
- Codex   ~/.codex/hooks.json        Stop  -> block + reason
- Claude  ~/.claude/settings.json    Stop  -> decision:block + reason
- Kimi    ~/.kimi-code/config.toml   Stop  -> same family
- Cursor  ~/.cursor/hooks.json       stop  -> followup_message best-effort

Usage: distill-hook.py <host>   (host = codex|claude|kimi|cursor|opencode|generic)
"""

import json
import os
import sys
import time
from pathlib import Path

HOME = Path(os.path.expanduser("~"))
MEM = HOME / ".agent-memory"
STATE = MEM / "state"
INBOX = MEM / "inbox"
SKILL = HOME / ".agents" / "skills" / "memory-distill" / "SKILL.md"

MIN_USER_MESSAGES = 1  # transcript must contain at least this many user turns


def read_payload() -> dict:
    raw = ""
    if not sys.stdin.isatty():
        try:
            raw = sys.stdin.read().strip()
        except Exception:
            pass
    if not raw:
        return {}
    try:
        return json.loads(raw)
    except Exception:
        return {}


def session_id_of(payload: dict, host: str) -> str:
    for key in ("session_id", "sessionId", "conversation_id", "conversationId",
                "thread_id", "threadId", "chat_id", "chatId", "id"):
        v = payload.get(key)
        if isinstance(v, str) and v:
            return v
    # nested fallbacks seen across hosts
    for key in ("session", "conversation", "thread", "chat"):
        sub = payload.get(key)
        if isinstance(sub, dict):
            v = sub.get("id") or sub.get("session_id")
            if isinstance(v, str) and v:
                return v
    return "unknown"


def transcript_path(payload: dict) -> str | None:
    for key in ("transcript_path", "transcriptPath", "transcript"):
        v = payload.get(key)
        if isinstance(v, str) and v:
            return v
    return None


def count_user_messages(path: str | None) -> int:
    """Cheap evidence check: does the transcript contain real user turns?"""
    if not path:
        return MIN_USER_MESSAGES  # unknown: allow, agent will NOOP anyway
    p = Path(path)
    if not p.is_file():
        return MIN_USER_MESSAGES
    n = 0
    try:
        with p.open("r", encoding="utf-8", errors="replace") as fh:
            for line in fh:
                if '"user"' in line or 'role":"user' in line or "user/message" in line:
                    n += 1
                    if n >= MIN_USER_MESSAGES:
                        return n
    except Exception:
        return MIN_USER_MESSAGES
    return n


def already_distilled(host: str, sid: str) -> bool:
    flag = STATE / f"distilled-{host}-{sid}"
    if flag.exists():
        return True
    # Also treat an existing inbox file as proof (e.g. agent wrote it directly).
    try:
        for f in INBOX.glob(f"{host}-{sid}*.jsonl"):
            return True
    except Exception:
        pass
    return False


def mark_distilled(host: str, sid: str, pid: str = "") -> None:
    try:
        (STATE / f"distilled-{host}-{sid}").write_text(
            json.dumps({"pid": pid, "ts": time.strftime("%Y-%m-%dT%H:%M:%S")},
                       ensure_ascii=False), encoding="utf-8")
    except Exception:
        pass


def checkpoint_pid(host: str, sid: str) -> str | None:
    """Incremental checkpoint: prompt_id of the last distilled turn.

    None = never distilled under the incremental scheme (incl. legacy
    timestamp-only flags and unparsable files — those sessions still have an
    undistilled tail, so the next turn is allowed to run)."""
    try:
        d = json.loads((STATE / f"distilled-{host}-{sid}").read_text(
            encoding="utf-8"))
        if isinstance(d, dict):
            v = d.get("pid")
            return v if isinstance(v, str) else None
    except Exception:
        pass
    return None


def instruction(host: str, sid: str, incremental: bool = False) -> str:
    text = (
        "[记忆蒸馏·会话结束前最后一步] 本会话即将结束。请按 "
        f"{SKILL} 的 Distill 契约，把本会话中用户本人表达的、长期有效、"
        "可跨会话使用的规则/决策/设计蒸馏为候选条目："
        f"写入 {INBOX}/{host}-{sid}.jsonl"
    )
    if incremental:
        text += (
            "。增量模式：若该文件已存在，先读它去重，只追加自上次蒸馏以来"
            "新出现的候选；没有新增内容就不写文件直接结束"
        )
    else:
        text += (
            "（每行一个 JSON 对象，字段见技能；没有值得沉淀的内容就写单行 "
            '{"noop":true}）'
        )
    text += (
        "。只写 inbox，严禁直接改 canonical 笔记，禁止把 agent 推断"
        "当作用户观点。写完后正常结束。"
    )
    return text


def emit(payload_out: dict) -> None:
    print(json.dumps(payload_out, ensure_ascii=False))


def main() -> None:
    host = sys.argv[1] if len(sys.argv) > 1 else "generic"
    payload = read_payload()
    sid = session_id_of(payload, host)

    # Hosts that tell us the hook already fired once: let it stop for real.
    if payload.get("stop_hook_active") or payload.get("stopHookActive"):
        emit({})
        return

    STATE.mkdir(parents=True, exist_ok=True)
    INBOX.mkdir(parents=True, exist_ok=True)

    if count_user_messages(transcript_path(payload)) < MIN_USER_MESSAGES:
        emit({})
        return

    # Incremental mode (hosts passing per-turn prompt_id, e.g. Devin):
    # distill once per user prompt — a prompt that already triggered a
    # distill is never redone, later turns still get their own pass.
    pid = payload.get("prompt_id") or payload.get("promptId") or ""
    if pid:
        if checkpoint_pid(host, sid) == pid:
            emit({})
            return
        mark_distilled(host, sid, pid)
        text = instruction(host, sid, incremental=True)
    else:
        # Legacy hosts without prompt_id: once per session.
        if already_distilled(host, sid):
            emit({})
            return
        mark_distilled(host, sid)
        text = instruction(host, sid)

    # Multi-contract defensive response: each host picks the field it knows.
    emit({
        # Claude Code / Codex Stop contract
        "decision": "block",
        "reason": text,
        "hookSpecificOutput": {
            "hookEventName": "Stop",
            "additionalContext": text,
        },
        # Cursor-style continuation hint
        "followup_message": text,
        "additional_context": text,
        "agent_message": text,
        # Antigravity-style injection
        "injectSteps": [{"ephemeralMessage": text}],
        "continue": True,
    })


if __name__ == "__main__":
    main()
