#!/usr/bin/env python3
"""anti-rot 会话内反腐败钩子（Claude 钩子契约）。

挂载平台：Devin / Codex / Claude（三者同契约）。
stdin  ← {"hook_event_name","session_id","tool_name","tool_input",
          "tool_response","transcript_path",...}
stdout → {}                        （无事发生）
       → {"hookSpecificOutput":{"hookEventName":..., "additionalContext":...}}

三个事件：
- SessionStart：注入一段紧凑纪律提示（不重复、不占预算）。
- PostToolUse：按会话累计轻量模式计数；命中即提醒，且每规则每会话只发一次。
- SessionEnd/Stop：把该会话立即摄入 anti-rot 台账（若可定位转录文件），
  并向 hooks/journal.jsonl 追加一行遥测。

原则：快（纯 stdlib，不碰台账 sqlite）、哑（异常一律静默返回 {}）、
只写 ~/.anti-rot/hooks/ 下的小状态文件，不读不写任何会话内容原文。
"""
import json
import os
import re
import subprocess
import sys
import time
from collections import deque

HOME = os.path.expanduser("~")
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))  # anti-rot 仓库根
STATE_DIR = os.path.join(HOME, ".anti-rot", "hooks", "state")
JOURNAL = os.path.join(HOME, ".anti-rot", "hooks", "journal.jsonl")

SESSION_MSG = (
    "[anti-rot 会话纪律] 派发后等完成通知（一般≥10min/大型≥30min 观察窗口，"
    "不拆成反复轮询）；同一命令不连续重复执行（疑似轮询先想事件驱动）；"
    "同一文件不反复重读；全量回归整个任务只跑一次。"
)

REMINDERS = {
    "repeat_cmd": "同一 shell 命令已执行 {} 次——若在轮询等待，请改用完成通知或退避等待。",
    "repeat_read": "同一文件已读取 {} 次——疑似上下文失忆，请提炼一次结果再续用。",
    "wait_calls": "本会话已出现 {} 次等待/休眠类调用——等待窗口不是终止依据，优先宿主完成通知。",
    "test_runs": "本会话已跑 {} 次测试——请收敛为定向验证，完整回归整个任务只跑一次。",
    "repeat_fail": "同一命令连续失败 {} 次——先诊断根因再重试，勿原地打转。",
}

SHELL_TOOLS = {"bash", "shell", "exec", "execute_command", "run_command", "Shell", "Bash"}
READ_TOOLS = {"read", "view", "Read", "readFile", "read_file"}
WAIT_RE = re.compile(r"(wait|poll|sleep|WaitFor|TaskOutput)", re.I)
TEST_RE = re.compile(r"(cargo test|npm test|npm run test|pytest|go test|unittest|vitest|pnpm test)", re.I)
FAIL_RE = re.compile(r"(error|failed|exit code: [1-9]|command not found|No such file)", re.I)

LIMITS = {"repeat_cmd": 4, "repeat_read": 3, "wait_calls": 3, "test_runs": 6, "repeat_fail": 3}

# ---------------------------------------------------------- 条件触发外部审计
# 多指标联合 → 风险分；越线则调用独立审计执行体（默认 hermes -z），
# 把审计纠偏回注原对话。ANTIROT_AUDIT_CMD 可替换执行体（测试用 cat 打桩）。
AUDIT_DIR = os.path.join(HOME, ".anti-rot", "hooks", "audits")
AUDIT_CMD = os.environ.get(
    "ANTIROT_AUDIT_CMD",
    "hermes -z {prompt_q} -t '' --usage-file {usage_q}",
)
AUDIT_TIMEOUT_S = 45          # 子进程硬上限（挂载 timeout 留余量）
ROT_AUDIT_AT = 70             # 腐败分阈值：只审显著腐化，长而健康的不审
MIN_SAMPLE_CALLS = 25         # 采样下限：会话太新不审计
AUDIT_ROT_STEP = 12           # 上次审计以来腐败分须再涨
AUDIT_COOLDOWN_S = 900        # 两次审计最小间隔（15 分钟）
TRANSCRIPT_TAIL_B = 60_000    # 送审的轨迹尾部上限

AUDIT_PROMPT = """你是独立的会话审计员（anti-rot）。审查这段 AI 会话的近期轨迹尾部与运行计数，只判断流程风险，不复述内容。

计数：{counters}
轨迹尾部（可能不完整）：
{tail}

输出严格 JSON（无其它文本）：
{{"risk":0-100,"verdict":"continue|advise|correct|pause",
"findings":["≤3条,每条≤30字"],
"correction":"给该会话的纠偏指令,≤200字,直接可执行"}}

判据：轮询等待空转 / 上下文失忆反复读 / 验证失衡反复回归 / 范围漂移 /
协作冲突 / 成本失控 / 环境摩擦。
risk<40→continue；40-70→advise 或 correct；>70→pause。
correction 必须是发给该会话模型本身的、可直接照做的纠偏指令；若无则空串。只输出 JSON。"""


def _shell_q(s: str) -> str:
    return "'" + s.replace("'", "'\\''") + "'"


def _state(st: dict, key, default):
    return st.setdefault(key, default)


def _transcript_mb(payload: dict) -> float:
    tp = payload.get("transcript_path") or payload.get("transcriptPath") or ""
    try:
        return os.path.getsize(tp) / 1_000_000 if tp else 0.0
    except OSError:
        return 0.0


def _tail(path: str, n: int) -> str:
    try:
        with open(path, "rb") as f:
            f.seek(0, os.SEEK_END)
            size = f.tell()
            f.seek(max(0, size - n))
            return f.read().decode("utf-8", "replace")
    except OSError:
        return ""


def assess_capacity(st: dict, payload: dict) -> int:
    """0-100 容量分：会话规模/时长——只证明「会话不小」，不参与触发判定。"""
    mb = _transcript_mb(payload)
    calls = st.get("tool_calls", 0)
    dur_h = max(0.0, (time.time() - st.get("started", time.time())) / 3600)
    return int(round(min(
        min(mb, 2.0) / 2.0 * 50 + min(calls, 300) / 300 * 30
        + min(dur_h, 6) / 6 * 20, 100)))


def assess_rot(st: dict) -> int:
    """0-100 腐败分：只算腐化行为计数。长而干净的会话恒为 ~0。"""
    repeats = max(
        (st["recent_sigs"].count(s) for s in set(st.get("recent_sigs", []))),
        default=0,
    )
    return int(round(min(
        min(st.get("waits", 0) * 5, 20)               # 等待/轮询空转
        + min(st.get("tests", 0) * 2.5, 15)           # 验证失衡
        + min(max(repeats - 3, 0) * 4, 15)            # 命令重复（>3 才计）
        + min(sum(st.get("sig_fails", {}).values()) * 4, 20)  # 连败
        + min(len(st.get("fired", [])) * 5, 15)       # 轻提醒已触发数
        + min(sum(v - 1 for v in st.get("reads", {}).values()
                if v >= 3) * 3, 15),                  # 文件反复重读
        100)))


def _audit_due(st: dict, rot: int) -> bool:
    if rot < ROT_AUDIT_AT or st.get("tool_calls", 0) < MIN_SAMPLE_CALLS:
        return False
    audits = st.get("audits", [])
    if not audits:
        return True
    last = audits[-1]
    return (
        rot - last.get("rot", 0) >= AUDIT_ROT_STEP
        and time.time() - last.get("ts", 0) >= AUDIT_COOLDOWN_S
    )


def _run_audit(st: dict, payload: dict, sid: str, rot: int, cap: int):
    """同步跑一次外部审计；超时/失败返回 None。产物落 audits/。"""
    tp = payload.get("transcript_path") or payload.get("transcriptPath") or ""
    tail = _tail(tp, TRANSCRIPT_TAIL_B) if tp else "(无法定位轨迹文件)"
    counters = json.dumps(
        {k: st.get(k) for k in ("tool_calls", "waits", "tests", "fired", "sig_fails")}
        | {"unique_sigs": len(set(st.get("recent_sigs", []))),
           "reads": len(st.get("reads", {})),
           "rot_score": rot, "capacity_score": cap},
        ensure_ascii=False,
    )
    prompt = AUDIT_PROMPT.format(counters=counters, tail=tail)
    usage = os.path.join(AUDIT_DIR, f"{sid}-{int(time.time())}.usage.json")
    cmd = AUDIT_CMD.format(prompt_q=_shell_q(prompt), usage_q=_shell_q(usage))
    try:
        os.makedirs(AUDIT_DIR, exist_ok=True)
        r = subprocess.run(
            ["bash", "-lc", cmd], capture_output=True, text=True,
            timeout=AUDIT_TIMEOUT_S,
        )
        text = (r.stdout or "").strip()
        m = re.search(r"\{.*\}", text, re.S)
        verdict = json.loads(m.group(0)) if m else None
        if not isinstance(verdict, dict) or "verdict" not in verdict:
            return None
        record = {"ts": int(time.time()), "rot": rot, "capacity": cap,
                  "verdict": verdict, "cmd": "audit", "session": sid}
        st.setdefault("audits", []).append(
            {"ts": record["ts"], "rot": rot,
             "verdict": verdict.get("verdict"), "risk": verdict.get("risk")}
        )
        with open(os.path.join(
            AUDIT_DIR, f"{sid}-{record['ts']}.json"), "w", encoding="utf-8"
        ) as f:
            json.dump(record, f, ensure_ascii=False, indent=1)
        return verdict
    except Exception:
        return None


def _audit_response(verdict: dict, rot: int) -> dict:
    """审计结论 → 钩子响应：pause/correct 级回注纠偏指令。"""
    v = (verdict or {}).get("verdict") or "advise"
    corr = ((verdict or {}).get("correction") or "").strip()
    finds = "；".join((verdict or {}).get("findings") or [])[:120]
    head = f"[anti-rot 外部审计] 腐败分 {rot}"
    if corr:
        msg = f"{head}，审计结论：{corr}（命中：{finds}）"
    else:
        msg = f"{head}：{finds or '多指标越线，请收敛当前做法'}"
    out = {"hookSpecificOutput": {
        "hookEventName": "PostToolUse", "additionalContext": msg}}
    if v == "pause":
        out["systemMessage"] = f"anti-rot 审计介入（腐败 {rot}）：已要求本会话纠偏"
        out["decision"] = "block"
        out["reason"] = msg
    return out


def _load_state(sid: str) -> dict:
    try:
        with open(os.path.join(STATE_DIR, f"{sid}.json"), encoding="utf-8") as f:
            d = json.load(f)
            d.setdefault("recent_sigs", [])
            d.setdefault("fired", [])
            return d
    except Exception:
        return {"recent_sigs": [], "fired": [], "reads": {}, "sig_fails": {},
                "waits": 0, "tests": 0, "started": time.time()}


def _save_state(sid: str, st: dict) -> None:
    try:
        os.makedirs(STATE_DIR, exist_ok=True)
        tmp = os.path.join(STATE_DIR, f".{sid}.tmp")
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(st, f)
        os.replace(tmp, os.path.join(STATE_DIR, f"{sid}.json"))
    except Exception:
        pass


def _sig(cmd: str) -> str:
    """命令签名：与台账同款的粗粒度归一。"""
    c = re.sub(r"\s+", " ", cmd.strip())[:80]
    c = re.sub(r"[\d]{4,}", "N", c)
    return c


def _reminder(key: str, n: int, st: dict, sid: str) -> dict:
    st["fired"].append(key)
    _save_state(sid, st)
    return {"hookSpecificOutput": {
        "hookEventName": "PostToolUse",
        "additionalContext": f"[anti-rot 提醒] {REMINDERS[key].format(n)}"}}


def post_tool_use(payload: dict) -> dict:
    sid = payload.get("session_id") or "unknown"
    tool = payload.get("tool_name") or ""
    inp = payload.get("tool_input") or {}
    if not isinstance(inp, dict):
        inp = {}
    resp = payload.get("tool_response")
    resp_text = json.dumps(resp, ensure_ascii=False)[:4000] if resp is not None else ""

    st = _load_state(sid)
    st["tool_calls"] = st.get("tool_calls", 0) + 1
    fired = set(st["fired"])

    if tool in SHELL_TOOLS:
        cmd = str(inp.get("command") or inp.get("cmd") or "")
        sig = _sig(cmd)
        st["recent_sigs"] = (st["recent_sigs"] + [sig])[-30:]
        if FAIL_RE.search(resp_text):
            st["sig_fails"][sig] = st["sig_fails"].get(sig, 0) + 1
        else:
            st["sig_fails"].pop(sig, None)
        if TEST_RE.search(cmd):
            st["tests"] += 1
        _save_state(sid, st)
        if "repeat_fail" not in fired and st["sig_fails"].get(sig, 0) >= LIMITS["repeat_fail"]:
            return _reminder("repeat_fail", st["sig_fails"][sig], st, sid)
        if "repeat_cmd" not in fired and st["recent_sigs"].count(sig) >= LIMITS["repeat_cmd"]:
            return _reminder("repeat_cmd", st["recent_sigs"].count(sig), st, sid)
        if "test_runs" not in fired and st["tests"] >= LIMITS["test_runs"]:
            return _reminder("test_runs", st["tests"], st, sid)
    elif tool in READ_TOOLS:
        path = str(inp.get("filePath") or inp.get("path") or inp.get("file_path") or "")
        if path:
            st["reads"][path] = st["reads"].get(path, 0) + 1
            _save_state(sid, st)
            if "repeat_read" not in fired and st["reads"][path] >= LIMITS["repeat_read"]:
                return _reminder("repeat_read", st["reads"][path], st, sid)
    elif WAIT_RE.search(tool):
        st["waits"] += 1
        _save_state(sid, st)
        if "wait_calls" not in fired and st["waits"] >= LIMITS["wait_calls"]:
            return _reminder("wait_calls", st["waits"], st, sid)
    else:
        _save_state(sid, st)
    # ---- 条件触发的外部专项审计（重路径，最后评估）----
    rot = assess_rot(st)
    if _audit_due(st, rot):
        verdict = _run_audit(st, payload, sid, rot,
                             assess_capacity(st, payload))
        _save_state(sid, st)
        return _audit_response(verdict, rot)
    return {}


def session_end(payload: dict) -> dict:
    sid = payload.get("session_id") or "unknown"
    st = _load_state(sid)
    summary = {
        "ts": int(time.time()), "session": sid, "host": "claude-contract",
        "shell_cmds": len(st.get("recent_sigs", [])),
        "unique_sigs": len(set(st.get("recent_sigs", []))),
        "tests": st.get("tests", 0), "waits": st.get("waits", 0),
        "reads_tracked": len(st.get("reads", {})),
        "reminders_fired": st.get("fired", []),
        "audits": st.get("audits", []),
        "rot_score": assess_rot(st),
        "capacity_score": assess_capacity(st, payload),
        "duration_s": int(time.time() - st.get("started", time.time())),
    }
    try:
        os.makedirs(os.path.dirname(JOURNAL), exist_ok=True)
        with open(JOURNAL, "a", encoding="utf-8") as f:
            f.write(json.dumps(summary, ensure_ascii=False) + "\n")
    except Exception:
        pass

    # 立即摄入刚结束的会话（一个文件的增量扫描，毫秒~秒级）
    tp = payload.get("transcript_path") or payload.get("transcriptPath") or ""
    if tp and os.path.isfile(tp):
        try:
            subprocess.Popen(
                [sys.executable, os.path.join(ROOT, "anti_rot.py"),
                 "scan", "--file", tp, "--include-active"],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                start_new_session=True,
            )
        except Exception:
            pass
    return {}


def main() -> None:
    try:
        payload = json.load(sys.stdin)
        if not isinstance(payload, dict):
            payload = {}
    except Exception:
        payload = {}
    ev = payload.get("hook_event_name") or payload.get("hookEventName") or ""
    try:
        if ev == "SessionStart":
            out = {"hookSpecificOutput": {
                "hookEventName": "SessionStart", "additionalContext": SESSION_MSG}}
        elif ev == "PostToolUse":
            out = post_tool_use(payload)
        elif ev in ("SessionEnd", "Stop", "SubagentStop"):
            out = session_end(payload)
        else:
            out = {}
    except Exception:
        out = {}
    sys.stdout.write(json.dumps(out, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
