#!/usr/bin/env python3
"""hook-dispatch — 单一生命周期钩子入口，并行扇出到子钩子。

用法: hook-dispatch.py <EventName>
  stdin  = 宿主钩子 payload，原样转发给每个子进程
  stdout = 归并后的单个 JSON 契约

归并规则（多子进程时）:
- decision: 任一 "block" 则 block；"approve" 只在无人 block 时透传
- 字符串字段 (reason/systemMessage/followup_message/additional_context/
  agent_message): 非空拼接
- hookSpecificOutput: additionalContext 拼接、updatedInput 浅合并、
  hookEventName 统一为当前事件
- injectSteps: 拼接；continue: 或；未知键: 先见先得
单子进程事件直接 exec 进子进程（stdin 原样透传，零额外进程）。
子进程超时/异常各自隔离：死掉的子进程贡献为空，不影响其它输出。
事件未注册或 argv 与 payload 均无事件名 → 输出 {}。
"""
import json
import os
import shlex
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

# 事件 -> [(命令, 子进程超时秒)]。命令按 argv 解析，不走 shell。
# 路径为脱敏占位：<hooks-bin>/<agent-hooks>/<mem-bin> 是各脚本在本机的安装目录。
REGISTRY = {
    "SessionStart": [
        ("python3 <hooks-bin>/anti-rot-hook.py", 80),
        ("python3 <agent-hooks>/universal-memory-router.py", 10),
    ],
    "PostToolUse": [
        ("python3 <hooks-bin>/anti-rot-hook.py", 80),
    ],
    "Stop": [
        ("python3 <hooks-bin>/anti-rot-hook.py", 80),
        ("python3 <mem-bin>/distill-hook.py devin", 80),
    ],
    "SessionEnd": [
        ("python3 <hooks-bin>/anti-rot-hook.py", 80),
    ],
}

STR_FIELDS = ("reason", "systemMessage", "followup_message",
              "additional_context", "agent_message")
LIST_FIELDS = ("injectSteps",)
KNOWN = set(STR_FIELDS) | set(LIST_FIELDS) | {
    "decision", "hookSpecificOutput", "continue"}


def read_stdin() -> bytes:
    if sys.stdin.isatty():
        return b""
    try:
        return sys.stdin.buffer.read()
    except Exception:
        return b""


def event_of(raw: bytes, argv_event: str) -> str:
    if argv_event:
        return argv_event
    try:
        payload = json.loads(raw.decode("utf-8", "replace") or "{}")
    except Exception:
        return ""
    return (payload.get("hook_event_name") or payload.get("hookEventName")
            or payload.get("event") or "")


def run_child(spec: tuple, raw: bytes) -> dict:
    cmd, timeout = spec
    try:
        r = subprocess.run(shlex.split(cmd), input=raw,
                           capture_output=True, timeout=timeout)
        out = (r.stdout or b"").decode("utf-8", "replace")
    except Exception:
        return {}
    for line in reversed([ln for ln in out.splitlines() if ln.strip()]):
        try:
            d = json.loads(line)
            if isinstance(d, dict):
                return d
        except Exception:
            continue
    return {}


def merge(event: str, outs: list) -> dict:
    merged: dict = {}
    block = False
    for out in outs:
        if not isinstance(out, dict):
            continue
        if out.get("decision") == "block":
            block = True
        elif out.get("decision") == "approve" and "decision" not in merged:
            merged["decision"] = "approve"
        for k in STR_FIELDS:
            v = out.get(k)
            if isinstance(v, str) and v.strip():
                merged[k] = f"{merged[k]}\n\n{v}" if merged.get(k) else v
        for k in LIST_FIELDS:
            v = out.get(k)
            if isinstance(v, list):
                merged.setdefault(k, []).extend(v)
        hso = out.get("hookSpecificOutput")
        if isinstance(hso, dict):
            tgt = merged.setdefault("hookSpecificOutput",
                                    {"hookEventName": event})
            for k, v in hso.items():
                if k == "hookEventName":
                    continue
                if k == "additionalContext" and isinstance(v, str) and v.strip():
                    tgt["additionalContext"] = (
                        f"{tgt['additionalContext']}\n\n{v}"
                        if tgt.get("additionalContext") else v)
                elif k == "updatedInput" and isinstance(v, dict):
                    tgt.setdefault("updatedInput", {}).update(v)
                else:
                    tgt[k] = v
        if out.get("continue"):
            merged["continue"] = True
        for k, v in out.items():  # 未知键先见先得
            if k not in KNOWN and k not in merged:
                merged[k] = v
    if block:
        merged["decision"] = "block"
    return merged


def main() -> None:
    argv_event = sys.argv[1] if len(sys.argv) > 1 else ""
    children = REGISTRY.get(argv_event)
    if children is None and argv_event:
        children = []
    raw = b""
    if children is None:  # argv 无事件名，从 payload 里认
        raw = read_stdin()
        children = REGISTRY.get(event_of(raw, ""), [])
    if not children:
        print("{}")
        return
    if len(children) == 1 and argv_event:
        argv = shlex.split(children[0][0])  # exec 直通：stdin 未被消费
        try:
            os.execvp(argv[0], argv)
        except Exception:
            print("{}")
            return
    if not raw:
        raw = read_stdin()
    with ThreadPoolExecutor(max_workers=len(children)) as pool:
        outs = list(pool.map(lambda c: run_child(c, raw), children))
    print(json.dumps(merge(argv_event or event_of(raw, ""), outs),
                     ensure_ascii=False))


if __name__ == "__main__":
    main()
