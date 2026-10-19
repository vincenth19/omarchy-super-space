#!/usr/bin/env python3
"""A stand-in for `superspace` that speaks the same protocol, so tests/run can
be checked before the real daemon exists. Python 3 stdlib only.

  fake_daemon.py serve --sock PATH --home DIR   the socket server
  fake_daemon.py search "q" --json              CLI (talks to SUPERSPACE_SOCK)
  fake_daemon.py mcp                            MCP server on stdio
  fake_daemon.py ui open "q" | key NAME | state the UI's IPC target

It answers golden queries from tests/golden.json (an oracle, so a correct
bench passes) and anything else by plain keyword search over text files in
its home (so live add/delete works). SUPERSPACE_FAKE_FAULTS (comma list)
breaks it on purpose, so `tests/run --self-test` can prove each check fails
when it should: slow, no-instant, web-first, pad, wrong-locator (also the
snippet), near-miss, thin-margin, prefs-noop, parity, slow-index, ui-stuck,
one-section (only a row's best hit), no-filter (type words searched as plain
words), greedy-filter (a filter on every query).
'wait' is not a fault: finals come 300 ms late and say so in timing.wait_ms, as
the real daemon's typing pause does.
"""
import json
import os
import pathlib
import re
import socket
import socketserver
import sys
import threading
import time
import urllib.parse

HERE = pathlib.Path(__file__).resolve().parent
FAULTS = set(filter(None, os.environ.get("SUPERSPACE_FAKE_FAULTS", "").split(",")))
CASES = json.loads((HERE / "golden.json").read_text())["cases"]
TEXT = {".md", ".txt", ".html"}
KIND_BY_EXT = {".jpg": "image", ".png": "image", ".mp4": "video", ".m4a": "audio", ".mp3": "audio"}


def norm(q):
    return " ".join(q.lower().split())


def example(regex):
    """A literal string matching one of the simple title regexes in golden.json."""
    s = re.sub(r"\(\?i\)|\\b|\?", "", regex).split("|")[0]
    return s.replace("\\.", ".").replace("\\", "")


def answer_for(q, spec):
    if "num" in spec:
        out = f"{spec['num']:g}"
    elif "range" in spec:
        out = f"{sum(spec['range']) / 2:g}"
    else:
        out = next(s for s in ("11:00 am", "1,000 MB", "9:00") if re.search(spec["re"], s))
    return {"kind": spec.get("kind", "math"), "input": q, "input_label": "", "output": out,
            "output_label": "", "copy": out, "detail": "fake"}


def oracle_rows(cases):
    rows = []
    for c in cases:
        want = c["want"]
        keys = want.get("key", [])
        key = keys[0] if isinstance(keys, list) and keys else keys or f"{want['kind']}:{c['id']}"
        loc, hits = {}, []
        at = c.get("at", {})
        for k, v in at.items():
            if k == "t":
                lo, hi = v[0] if isinstance(v[0], list) else v
                loc.update(t=lo + 0.5, t_end=min(hi, lo + 4))
            else:
                loc[k] = v
        if loc:
            if "wrong-locator" in FAULTS:
                loc = {k: (v + 30 if k == "t" else v + 1 if isinstance(v, int) else v) for k, v in loc.items()}
            hits = [{"locator": loc, "score": 0.8, "text": ""}, {"locator": {}, "score": 0.6, "text": ""}]
        hits += [{"locator": {"t": lo + 0.5, "t_end": min(hi, lo + 4)}, "score": 0.7, "text": ""} for lo, hi in c.get("sections", [])]
        if "one-section" in FAULTS:
            hits = hits[:1]
        kind = want.get("kind") or (key.split(":")[0] if not key.startswith("file:") else KIND_BY_EXT.get(pathlib.Path(key).suffix, "file"))
        rows.append({"key": key, "kind": kind, "title": example(want["title"]) if "title" in want else key.rsplit("/", 1)[-1],
                     "subtitle": "", "icon": "text-x-generic", "action": {"type": "open", "path": key, "locator": {}},
                     "score": round(0.9 - 0.01 * len(rows), 3), "why": "meaning", "hits": hits,
                     "snippet": {"text": "" if "wrong-locator" in FAULTS else example(c.get("snippet", "")), "marks": []}})
    return rows


class Daemon:
    def __init__(self, home):
        self.home = pathlib.Path(home)
        self.prefs = {"prefer": [], "pin": [], "boost": [], "bury": [], "exclude": []}
        self.lock = threading.Lock()

    def files(self):
        now = time.time()
        for p in self.home.rglob("*"):
            rel = p.relative_to(self.home)
            if any(part.startswith(".") for part in rel.parts) or not p.is_file():
                continue
            if "slow-index" in FAULTS and now - p.stat().st_mtime < 4:
                continue
            yield rel, p

    def keyword(self, q):
        words = [w for w in norm(q).split() if len(w) > 2]
        rows = []
        for rel, p in self.files():
            text = rel.as_posix().lower()
            if p.suffix in TEXT:
                text += " " + p.read_text(errors="ignore").lower()
            if words and all(w in text for w in words):
                rows.append({"key": f"file:{rel.as_posix()}", "kind": KIND_BY_EXT.get(p.suffix, "file"), "title": p.name,
                             "subtitle": f"~/{rel.parent.as_posix()}", "icon": "text-x-generic",
                             "action": {"type": "open", "path": str(p), "locator": {}}, "score": 0.5, "why": "keyword"})
        return rows

    def results(self, q):
        cases = [c for c in CASES if norm(c["q"]) == norm(q)]
        if "no-filter" in FAULTS:
            cases = [c for c in CASES if norm(q).startswith(norm(c["q"]) + " ")] or cases
        filt = "fake" if "greedy-filter" in FAULTS or any(c.get("filter") for c in cases) else None
        answer = next((answer_for(q, c["answer"]) for c in cases if "answer" in c), None)
        rows = oracle_rows([c for c in cases if "want" in c]) if cases else self.keyword(q)
        if "pad" in FAULTS and any(c.get("empty") for c in cases):
            rows = [dict(r, key=f"file:Documents/near-miss-{i}.md") for i, r in enumerate(oracle_rows(CASES[:3]))]
        if "near-miss" in FAULTS and cases:
            rows = rows + [dict(oracle_rows(CASES[:1])[0], key="history:https://x.com/near-miss", kind="history")]
        with self.lock:
            prefs = json.loads(json.dumps(self.prefs))
        for b in prefs["bury"]:
            rows = [dict(r, score=r["score"] - b["by"]) if r["kind"] == b["kind"] else r for r in rows]
        for b in prefs["boost"]:
            prefix = "file:" + b["path"].removeprefix("~/")
            rows = [dict(r, score=r["score"] + b["by"]) if r["key"].startswith(prefix) else r for r in rows]
        if prefs["bury"] or prefs["boost"]:
            rows.sort(key=lambda r: -r["score"])
        for p in prefs["pin"]:
            if norm(p["q"]) == norm(q):
                pinned = next((r for r in rows if r["key"] == p["key"]), None) or oracle_rows([{"id": "pin", "want": {"key": p["key"]}}])[0]
                rows = [dict(pinned, why="pinned")] + [r for r in rows if r["key"] != p["key"]]
        web = {"key": "web:" + q, "kind": "web", "title": f"Search the web for “{q}”", "subtitle": "",
               "icon": "web-browser", "action": {"type": "url", "url": "https://duckduckgo.com/?q=" + urllib.parse.quote(q)},
               "score": 0.0, "why": "keyword"}
        empty = not rows
        rows = [web] + rows if "web-first" in FAULTS else rows + [web]
        return rows, answer, empty, filt

    def handle(self, req, send):
        op, rid = req.get("op"), req.get("id", 0)
        if op == "search":
            t0 = time.perf_counter()
            rows, answer, empty, filt = self.results(req["q"])
            rows = rows[: req.get("limit", 30)]
            base = {"id": rid, "rows": rows, "empty": empty, **({"filter": filt} if filt else {})}
            if answer:
                base["answer"] = answer
            slow = "slow" in FAULTS
            if "no-instant" not in FAULTS:
                time.sleep(0.04 if slow else 0.001)
                send(dict(base, phase="instant", timing={"instant_ms": (time.perf_counter() - t0) * 1000, "embed_ms": 0, "total_ms": 0}))
            wait = 0.3 if "wait" in FAULTS else 0  # the daemon's typing pause, reported as wait_ms
            time.sleep((0.4 if slow else 0.02) + wait)
            ms = (time.perf_counter() - t0) * 1000
            send(dict(base, phase="final", timing={"instant_ms": 1.0, "embed_ms": ms - 1, "total_ms": ms, **({"wait_ms": wait * 1000} if wait else {})}))
        elif op == "status":
            send({"id": rid, "value": {"home": str(self.home), "version": "fake", "items": sum(1 for _ in self.files())}})
        elif op == "prefs":
            with self.lock:
                send({"id": rid, "value": json.loads(json.dumps(self.prefs))})
        elif op == "pref":
            (name, arg), = req["change"].items()
            with self.lock:
                if "prefs-noop" in FAULTS:
                    pass
                elif name in self.prefs:
                    self.prefs[name].append(arg)
                elif name[2:] in self.prefs and name.startswith("un"):
                    self.prefs[name[2:]] = [x for x in self.prefs[name[2:]] if any(x.get(k) != v for k, v in arg.items())]
                else:
                    return send({"id": rid, "error": f"unknown preference {name}"})
            send({"id": rid, "value": {"ok": True}})
        elif op == "explain":
            row = next((r for r in self.results(req["q"])[0] if r["key"] == req["key"]), None)
            if not row:
                return send({"id": rid, "error": "not a result for that query"})
            m = 0.03 if "thin-margin" in FAULTS else 0.5  # x 0.15: how far the row's cosine clears its cutoff
            send({"id": rid, "value": {"score": row["score"], "features": {"meaning": {"value": m, "contribution": m * 0.9}}}})
        elif op == "activate":
            send({"id": rid, "value": {"ok": True}})
        else:
            send({"id": rid, "error": f"{op} is not supported by the fake daemon"})


def serve(sock, home):
    daemon = Daemon(home)

    class Handler(socketserver.StreamRequestHandler):
        def handle(self):
            lock = threading.Lock()

            def send(obj):
                with lock:
                    try:
                        self.wfile.write((json.dumps(obj, ensure_ascii=False) + "\n").encode())
                        self.wfile.flush()
                    except (OSError, ValueError):
                        pass  # the client went away
            for line in self.rfile:
                if line.strip():
                    threading.Thread(target=daemon.handle, args=(json.loads(line), send), daemon=True).start()

    if os.path.exists(sock):
        os.unlink(sock)
    with socketserver.ThreadingUnixStreamServer(sock, Handler) as srv:
        srv.daemon_threads = True
        srv.serve_forever()


def ask(req):
    """One request to the socket; returns the final response (or the value/error)."""
    with socket.socket(socket.AF_UNIX) as s:
        s.connect(os.environ.get("SUPERSPACE_SOCK") or os.path.join(os.environ.get("XDG_RUNTIME_DIR", "/tmp"), "superspace.sock"))
        s.sendall((json.dumps(req) + "\n").encode())
        for line in s.makefile():
            r = json.loads(line)
            if r.get("id") == req["id"] and r.get("phase") != "instant":
                return r


def search(q, limit=30):
    r = ask({"op": "search", "id": 1, "q": q, "limit": limit})
    if "parity" in FAULTS and len(r["rows"]) > 2:
        r["rows"][0], r["rows"][1] = r["rows"][1], r["rows"][0]
    return r


def mcp():
    tool = {"name": "search", "description": "Search the user's computer", "inputSchema": {
        "type": "object", "properties": {"q": {"type": "string"}, "limit": {"type": "integer"}}, "required": ["q"]}}
    for line in sys.stdin:
        msg = json.loads(line)
        if "id" not in msg:
            continue
        m = msg.get("method")
        if m == "initialize":
            res = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}}, "serverInfo": {"name": "superspace-fake", "version": "0"}}
        elif m == "tools/list":
            res = {"tools": [tool]}
        elif m == "tools/call":
            a = msg["params"]["arguments"]
            res = {"content": [{"type": "text", "text": json.dumps(search(a["q"], a.get("limit", 30)))}]}
        else:
            print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "error": {"code": -32601, "message": m}}), flush=True)
            continue
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": res}), flush=True)


def ui(fn, arg):
    path = pathlib.Path((os.environ.get("SUPERSPACE_SOCK") or "/tmp/superspace.sock") + ".ui.json")
    st = json.loads(path.read_text()) if path.exists() else {"open": False, "query": "", "rows": [], "selected": 0, "hit": 0, "expanded": False}
    if fn == "open":
        r = search(arg)
        theme = pathlib.Path(os.environ.get("SUPERSPACE_UI_HOME", os.environ["HOME"])) / ".local/state/omarchy/current/theme/colors.toml"
        colors = dict(re.findall(r'^(\w+)\s*=\s*"(#[0-9a-fA-F]{6})"', theme.read_text(), re.M)) if theme.exists() else {}
        st = {"open": True, "query": arg, "rows": [{k: x[k] for k in ("key", "kind", "title")} | {"hits": len(x.get("hits", []))} for x in r["rows"]],
              "selected": 0, "hit": 0, "expanded": False, "empty": r["empty"], "answer": r.get("answer"),
              "theme": {"background": colors.get("background"), "text": colors.get("foreground")}}
    elif fn == "key" and "ui-stuck" not in FAULTS:
        n, name = len(st["rows"]), arg.lower()
        if name in ("down", "ctrl+n"):
            st["selected"] = min(st["selected"] + 1, n - 1)
        elif name in ("up", "ctrl+p"):
            st["selected"] = max(st["selected"] - 1, 0)
        elif name == "tab":
            st["expanded"], st["mode"] = True, "sections"
        elif name in ("right", "left"):
            hits = st["rows"][st["selected"]]["hits"] if n else 0
            st["hit"] = max(0, min(st["hit"] + (1 if name == "right" else -1), hits - 1))
        elif name == "escape" and st.get("mode") == "sections":
            st["expanded"], st["mode"] = False, "text"
        elif name in ("escape", "return"):
            st["open"] = False
    path.write_text(json.dumps(st))
    if fn == "state":
        print(json.dumps(st))


if __name__ == "__main__":
    a = sys.argv[1:]
    if a[:1] == ["serve"]:
        serve(a[a.index("--sock") + 1], a[a.index("--home") + 1])
    elif a[:1] == ["search"]:
        print(json.dumps(search(a[1], int(a[a.index("--limit") + 1]) if "--limit" in a else 30)))
    elif a[:1] == ["mcp"]:
        mcp()
    elif a[:1] == ["ui"]:
        ui(a[1], a[2] if len(a) > 2 else "")
    else:
        sys.exit(__doc__)
