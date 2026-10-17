#!/usr/bin/env python3
"""
agent-memory poll — incremental fallback scanner for sources not covered by
distill hooks (dsh, missed sessions, flagged sessions).

Produces raw evidence packages (raw/<source>-<date>.jsonl) containing only
user-authored messages since the last watermark. These are *evidence*, not
candidates — an adjudicator agent distills them through the same gates before
anything reaches the inbox.

Usage:
  poll.py                 # incremental scan, all sources
  poll.py --since DATE    # override watermark (YYYY-MM-DD)
  poll.py --source codex  # one source only
  poll.py --status        # show watermarks
"""

import argparse
import datetime as dt
import json
import os
import re
import sqlite3
import subprocess
import sys
import zlib
from pathlib import Path

HOME = Path(os.path.expanduser("~"))
MEM = HOME / ".agent-memory"
RAW = MEM / "raw"
STATE = MEM / "state"
FLAGGED = MEM / "flagged"
WATERMARKS = STATE / "watermarks.json"

# ---------------------------------------------------------------- sources

def src_codex(since: dt.datetime):
    """Codex JSONL rollouts → user messages."""
    root = HOME / ".codex" / "sessions"
    if not root.is_dir():
        return
    for p in sorted(root.rglob("*.jsonl")):
        try:
            if dt.datetime.fromtimestamp(p.stat().st_mtime) < since:
                continue
        except Exception:
            continue
        with p.open("r", errors="replace") as fh:
            for i, line in enumerate(fh):
                try:
                    o = json.loads(line)
                except Exception:
                    continue
                r = o.get("record") or o
                if (r.get("type") == "response_item" and
                        isinstance(r.get("payload"), dict) and
                        r["payload"].get("type") == "message" and
                        r["payload"].get("role") == "user"):
                    txt = "\n".join(c.get("text", "")
                                    for c in r["payload"].get("content", [])
                                    if isinstance(c, dict))
                    if txt.strip():
                        yield {"file": p.name, "seq": i,
                               "ts": o.get("timestamp", ""), "text": txt}


def src_kimi(since: dt.datetime):
    """Kimi wire.jsonl + history per workspace → user turns."""
    root = HOME / ".kimi-code"
    for wf in (root / "workspaces.json",):
        pass  # workspaces enumerated implicitly via dirs
    for d in sorted(root.glob("*/sessions/*/")):
        wire = d / "wire.jsonl"
        if not wire.is_file():
            continue
        try:
            if dt.datetime.fromtimestamp(wire.stat().st_mtime) < since:
                continue
        except Exception:
            continue
        with wire.open("r", errors="replace") as fh:
            for i, line in enumerate(fh):
                try:
                    o = json.loads(line)
                except Exception:
                    continue
                pl = o.get("payload") or {}
                if (o.get("message_type") == "TurnBegin" or
                        pl.get("role") == "user"):
                    txt = pl.get("content") or ""
                    if isinstance(txt, list):
                        txt = " ".join(str(c.get("text", ""))
                                       for c in txt if isinstance(c, dict))
                    if str(txt).strip():
                        yield {"file": str(wire.relative_to(root)), "seq": i,
                               "ts": o.get("timestamp", ""),
                               "text": str(txt)}


def src_dsh(since: dt.datetime):
    """dsh compressed sessions → user-role items only."""
    root = HOME / ".dsh" / "sessions"
    if not root.is_dir():
        return
    for p in sorted(root.rglob("*.z")):
        try:
            if dt.datetime.fromtimestamp(p.stat().st_mtime) < since:
                continue
        except Exception:
            continue
        try:
            data = zlib.decompress(p.read_bytes())
        except Exception:
            continue
        for i, line in enumerate(data.splitlines()):
            try:
                o = json.loads(line)
            except Exception:
                continue
            # dsh uses the same response_item/message shape family
            if o.get("role") == "user" or \
                    (isinstance(o.get("item"), dict) and
                     o["item"].get("role") == "user"):
                txt = o.get("text") or json.dumps(o.get("item", {}),
                                                 ensure_ascii=False)
                yield {"file": p.name, "seq": i,
                       "ts": o.get("timestamp", ""), "text": str(txt)[:8000]}


def src_opencode(since: dt.datetime):
    """opencode.db messages → user role."""
    db = HOME / ".local" / "share" / "opencode" / "opencode.db"
    if not db.is_file():
        return
    try:
        con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
        since_ms = int(since.timestamp() * 1000)
        cur = con.execute(
            "SELECT json_extract(data,'$.time.created') ts, "
            " json_extract(data,'$.id') id "
            "FROM message WHERE json_extract(data,'$.role')='user' "
            "AND json_extract(data,'$.time.created')>=? "
            "ORDER BY ts", (since_ms,))
        rows = cur.fetchall()
        mids = [mid for _, mid in rows]
        texts: dict = {}
        if mids:
            # one bounded scan of part instead of one scan per message
            ph = ",".join("?" * len(mids))
            for m, t in con.execute(
                    "SELECT json_extract(data,'$.messageID'), "
                    " json_extract(data,'$.text') FROM part "
                    f"WHERE json_extract(data,'$.messageID') IN ({ph})",
                    mids):
                texts.setdefault(m, []).append(str(t or ""))
        for ts, mid in rows:
            t = dt.datetime.fromtimestamp((ts or 0) / 1000)
            yield {"file": "opencode.db", "seq": mid,
                   "ts": t.isoformat(), "text": " ".join(texts.get(mid, []))}
        con.close()
    except Exception:
        pass


def src_kilo(since: dt.datetime):
    """kilo.db ui_messages → human-sounding messages (skip dispatch/synthetic)."""
    db = HOME / "Library" / "Application Support" / "Code" / "User" / \
        "globalStorage" / "kilocode.kilo-code" / "settings" / "kilo.db"
    if not db.is_file():
        return
    try:
        con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
        for (ts, said, txt) in con.execute(
                "SELECT ts, say, text FROM ui_messages WHERE say IN "
                "('text','user_feedback') ORDER BY ts"):
            t = dt.datetime.fromtimestamp((ts or 0) / 1000)
            if t < since:
                continue
            txt = txt or ""
            # cheap dispatch-prompt heuristic
            if len(txt) > 6000 or txt.lstrip().startswith("<"):
                continue
            yield {"file": "kilo.db", "seq": mid,
                   "ts": t.isoformat(), "text": txt}
        con.close()
    except Exception:
        pass


SOURCES = {"codex": src_codex, "kimi": src_kimi, "dsh": src_dsh,
           "opencode": src_opencode, "kilo": src_kilo}


# ---------------------------------------------------------------- watermarks

def load_marks() -> dict:
    try:
        return json.loads(WATERMARKS.read_text())
    except Exception:
        return {}


def save_marks(m: dict) -> None:
    STATE.mkdir(parents=True, exist_ok=True)
    WATERMARKS.write_text(json.dumps(m, indent=2))


# ---------------------------------------------------------------- main

def is_probably_dispatch(text: str) -> bool:
    """Cheap pre-filter: kill obvious non-user artifacts."""
    t = text.strip()
    if not t or len(t) < 8:
        return True
    pats = [
        r"^<system-reminder", r"^<command-message", r"^goal[_ ]round",
        r"^<local-command", r"^\s*\{\"", r"^Caveat:",
        r"^<external_", r"^<environment_context", r"^<user_instructions",
        r"^<permissions", r"^<agents?\b", r"^<skill\b",
        r"dispatch(?:ed|ing)?\s+subagent", r"^你是.{0,12}(子|分)智能体",
        r"^task[_ ]id\s*[:=]", r"<\s*instructions\s*>",
    ]
    return any(re.search(p, t, re.I | re.M) for p in pats)


def cmd_status(_a):
    m = load_marks()
    print(json.dumps(m, indent=2) if m else "no watermarks yet")
    flags = [p.name for p in FLAGGED.glob("*")] if FLAGGED.is_dir() else []
    if flags:
        print(f"\nflagged sessions: {len(flags)}")
        for f in flags[:10]:
            print(f"  {f}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--since", help="YYYY-MM-DD, overrides watermarks")
    ap.add_argument("--source", choices=sorted(SOURCES), action="append")
    ap.add_argument("--status", action="store_true")
    args = ap.parse_args()

    if args.status:
        return cmd_status(args)

    marks = load_marks()
    today = dt.datetime.now()
    default_since = today - dt.timedelta(days=7)

    RAW.mkdir(parents=True, exist_ok=True)
    FLAGGED.mkdir(parents=True, exist_ok=True)

    wanted = args.source or list(SOURCES)
    total = 0
    for name in wanted:
        fn = SOURCES[name]
        since = (dt.datetime.fromisoformat(args.since)
                 if args.since else
                 dt.datetime.fromisoformat(marks.get(name, "")
                                         or default_since.isoformat()))
        batch = []
        for ev in fn(since):
            if is_probably_dispatch(ev["text"]):
                continue
            batch.append(ev)
            if len(batch) >= 500:
                break  # bound per source per run
        if batch:
            out = RAW / f"{name}-{today:%Y%m%d-%H%M%S}.jsonl"
            with out.open("w", encoding="utf-8") as fh:
                for ev in batch:
                    fh.write(json.dumps({"source": name, **ev},
                                        ensure_ascii=False) + "\n")
            print(f"{name}: {len(batch)} user messages → {out.name}")
            total += len(batch)
        else:
            print(f"{name}: 0 new")
        marks[name] = today.isoformat()

    save_marks(marks)
    # consume flags
    for f in FLAGGED.glob("*"):
        try:
            f.unlink()
        except Exception:
            pass
    print(f"\ntotal {total} evidence messages; watermarks updated")
    return 0


if __name__ == "__main__":
    sys.exit(main())
