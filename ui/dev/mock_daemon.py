#!/usr/bin/env python3
"""Mock `superspace serve` for developing the UI (Python stdlib only).

Speaks the socket protocol in DESIGN.md: newline JSON, two responses per
search (instant, then final after a delay), activate acknowledged and logged.
It never skips a stale final phase, so the UI's drop-stale-ids path is used.

Canned queries (substring match, case-insensitive):
  lease / contract / pdf      PDF hit with pages and marked snippet
  dog / golden / park         video with moments + thumbs, photo, audio
  awkward / smile / emoji     emoji rows
  cm / usd / idr / % / 2+2    instant answer (plus whatever else matches)
  qwxz / nothing              empty: true, only the web row
  slow <query>                the same, with the final phase after 1.5 s instead of 0.12 s
  anything else               apps and settings by prefix; the final phase puts a meaning hit on top

  python3 ui/dev/mock_daemon.py [--sock PATH] [--thumbs DIR] [--log FILE]

Refuses to start when something already listens on the socket.
"""

import argparse, json, os, re, signal, socket, socketserver, struct, sys, threading, time, zlib

APPS = [  # desktop id, name, generic name
    ("chromium", "Chromium", "Web Browser"),
    ("foot", "Foot", "Terminal"),
    ("org.gnome.Nautilus", "Files", "File Manager"),
    ("org.gnome.Evince", "Document Viewer", "PDF viewer"),
    ("mpv", "mpv Media Player", "Multimedia player"),
    ("btop", "btop++", "System monitor"),
    ("libreoffice-writer", "LibreOffice Writer", "Word processor"),
]
SETTINGS = [  # id, title, icon
    ("wifi", "Wi-Fi", "network-wireless"),
    ("bluetooth", "Bluetooth", "bluetooth"),
    ("theme", "Theme", "preferences-desktop-theme"),
    ("font", "Font", "preferences-desktop-font"),
]


def row(key, kind, title, subtitle, icon, action, score=0.5, why="name", snippet=None, hits=None):
    r = {"key": key, "kind": kind, "title": title, "subtitle": subtitle, "icon": icon,
         "action": action, "score": score, "why": why}
    if snippet:
        r["snippet"] = snippet
    if hits:
        r["hits"] = hits
    return r


def marked(text, words):
    """Snippet whose marks cover every occurrence of each word (character ranges)."""
    marks = []
    for w in words:
        marks += [(m.start(), m.end()) for m in re.finditer(re.escape(w), text, re.I)]
    return {"text": text, "marks": sorted(marks)}


def web(q):
    return row("web:" + q, "web", "Search the web for “%s”" % q, "Opens your browser", "web-browser",
               {"type": "url", "url": "https://duckduckgo.com/?q=" + q}, 0.0, "keyword")


def answer_for(q):
    s = q.lower()
    if re.fullmatch(r"[\d\s.+\-*/()]+", s) and re.search(r"\d\s*[-+*/]", s):
        try:
            v = eval(s, {"__builtins__": {}})  # digits and operators only, checked above
        except Exception:
            return None
        out = ("%.10g" % v)
        return {"kind": "math", "input": q.strip(), "input_label": "Expression", "output": out,
                "output_label": "Result", "copy": out, "detail": ""}
    if "cm" in s:
        return {"kind": "units", "input": "5′ 11″", "input_label": "Feet and inches", "output": "180.34 cm",
                "output_label": "Centimetres", "copy": "180.34", "detail": ""}
    if "idr" in s or "usd" in s:
        return {"kind": "currency", "input": "Rp 100,000", "input_label": "Indonesian rupiah", "output": "US$ 6.13",
                "output_label": "US dollar", "copy": "6.13", "detail": "ECB rate of 16 Oct"}
    if "%" in s:
        return {"kind": "percent", "input": "15% of 80", "input_label": "Percent", "output": "12",
                "output_label": "Result", "copy": "12", "detail": ""}
    return None


def respond(q, thumbs):
    """(instant rows, final rows, answer, empty, final delay in seconds)."""
    s = q.lower().strip()
    home = os.path.expanduser("~")
    delay = 0.12
    if s.startswith("slow"):
        delay, s = 1.5, s[4:].strip()
    words = s.split()
    answer = answer_for(q)
    if any(w in s for w in ("qwxz", "nothing")):
        return [web(q)], [web(q)], None, True, delay
    if any(w in s for w in ("lease", "contract", "pdf")):
        pdf = row("file:Documents/lease-2026.pdf", "file", "lease-2026.pdf", "~/Documents · p. 4, 9, 12",
                  "application-pdf", {"type": "open", "path": home + "/Documents/lease-2026.pdf", "locator": {"page": 4}},
                  0.9, "name")
        final = dict(pdf, why="meaning", snippet=marked(
            "The tenant may end this lease with two months' notice; the deposit is returned within 14 days.", words),
            hits=[{"locator": {"page": 4}, "score": 0.81, "text": "The tenant may end this lease with two months' notice."},
                  {"locator": {"page": 9}, "score": 0.74, "text": "Deposit: returned within 14 days of the end of the lease."},
                  {"locator": {"page": 12}, "score": 0.66, "text": "Signatures of landlord and tenant."}])
        docx = row("file:Documents/rental-notes.docx", "file", "rental-notes.docx", "~/Documents", "x-office-document",
                   {"type": "open", "path": home + "/Documents/rental-notes.docx", "locator": {}}, 0.6, "meaning",
                   marked("Questions for the landlord before signing the lease", words))
        return [pdf, web(q)], [final, docx, web(q)], answer, False, delay
    if any(w in s for w in ("dog", "golden", "park", "retriever")):
        t = thumbs or [""]
        moments = [{"locator": {"t": tt, "t_end": tt + 4.0}, "score": sc, "text": "", "thumb": t[i % len(t)]}
                   for i, (tt, sc) in enumerate([(47.0, 0.78), (12.0, 0.75), (95.0, 0.71), (131.0, 0.69)])]
        video = row("file:Videos/dog-park.mp4", "video", "dog-park.mp4", "~/Videos · 0:12, 0:47, 1:35, 2:11", "mpv",
                    {"type": "open", "path": home + "/Videos/dog-park.mp4", "locator": {"t": 47.0}}, 0.78, "meaning",
                    hits=moments)
        photo = row("file:Pictures/golden1.jpg", "image", "golden1.jpg", "~/Pictures · 12 Oct", t[0] or "image-x-generic",
                    {"type": "open", "path": home + "/Pictures/golden1.jpg", "locator": {}}, 0.74, "meaning")
        audio = row("file:Music/park-walk.mp3", "audio", "park-walk.mp3", "~/Music · spoken at 1:05, 3:20", "audio-x-generic",
                    {"type": "open", "path": home + "/Music/park-walk.mp3", "locator": {"t": 65.0}}, 0.6, "meaning",
                    marked("…and then the golden retriever ran across the whole park…", words),
                    [{"locator": {"t": 65.0, "t_end": 80.0}, "score": 0.6, "text": "the golden retriever ran across the whole park"},
                     {"locator": {"t": 200.0, "t_end": 215.0}, "score": 0.55, "text": "we took the dog home after the park"}])
        return [web(q)], [video, photo, audio, web(q)], answer, False, delay
    if any(w in s for w in ("awkward", "smile", "emoji")):
        emoji = [("😬", "grimacing face", "awkward, nervous"), ("😅", "grinning face with sweat", "phew, relief"),
                 ("🙃", "upside-down face", "silly, sarcasm")]
        rows = [row("emoji:" + e, "emoji", name, kw, "", {"type": "paste", "text": e}, 0.7, "meaning") for e, name, kw in emoji]
        rows.append(row("setting:emoji-picker", "setting", "Open emoji picker", "Omarchy", "face-smile",
                        {"type": "run", "argv": ["omarchy-menu", "emoji"]}, 0.3, "keyword"))
        return rows[:1] + [web(q)], rows + [web(q)], answer, False, delay
    names = [row("app:" + i, "app", n, g, i, {"type": "launch", "desktop_id": i}, 0.8)
             for i, n, g in APPS if s and (n.lower().startswith(s) or g.lower().startswith(s))]
    names += [row("setting:" + i, "setting", t, "Omarchy setting", ic, {"type": "run", "argv": ["omarchy-menu", i]}, 0.7)
              for i, t, ic in SETTINGS if s and t.lower().startswith(s)]
    if answer and not names:
        return [web(q)], [web(q)], answer, False, delay
    meaning = row("file:Notes/meetup.md", "file", "meetup.md", "~/Notes · line 12", "text-markdown",
                  {"type": "open", "path": home + "/Notes/meetup.md", "locator": {"line": 12}}, 0.55, "meaning",
                  marked("Demo on the Pi: open Super Space, type " + q + ", show the video moment.", words),
                  [{"locator": {"line": 12}, "score": 0.55, "text": "Demo on the Pi: open Super Space."}])
    return names + [web(q)], [meaning] + names + [web(q)], answer, False, delay


def png(path, w, h, rgb):
    """A small gradient PNG, so the UI has real thumbnails without any packages."""
    raw = b"".join(b"\x00" + bytes(int(c * (0.55 + 0.45 * y / h)) for c in rgb) * w for y in range(h))
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
                + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


def make_thumbs(arg):
    if arg:
        return sorted(os.path.join(arg, f) for f in os.listdir(arg) if f.endswith(("-224.jpg", ".png")))
    d = os.path.join(os.environ.get("XDG_CACHE_HOME", os.path.expanduser("~/.cache")), "superspace-mock")
    os.makedirs(d, exist_ok=True)
    out = []
    for i, rgb in enumerate([(200, 150, 80), (90, 140, 200), (120, 170, 90), (190, 90, 110)]):
        out.append(os.path.join(d, "thumb%d.png" % i))
        png(out[-1], 160, 90, rgb)
    return out


class Handler(socketserver.StreamRequestHandler):
    def handle(self):
        lock = threading.Lock()

        def send(obj):
            with lock:
                try:
                    self.wfile.write((json.dumps(obj, ensure_ascii=False) + "\n").encode())
                    self.wfile.flush()
                except OSError:
                    pass

        for line in self.rfile:
            try:
                req = json.loads(line)
            except ValueError:
                continue
            op, rid = req.get("op"), req.get("id", 0)
            if op == "search":
                q = req.get("q", "")
                t0 = time.perf_counter()
                instant, final, answer, empty, delay = respond(q, self.server.thumbs)
                ms = (time.perf_counter() - t0) * 1000
                base = {"id": rid, "empty": empty, **({"answer": answer} if answer else {})}
                send({**base, "phase": "instant", "rows": instant, "timing": {"instant_ms": ms, "embed_ms": 0, "total_ms": ms}})
                threading.Timer(delay, send, [{**base, "phase": "final", "rows": final,
                                               "timing": {"instant_ms": ms, "embed_ms": delay * 1000, "total_ms": delay * 1000}}]).start()
                log(self.server, {"search": q, "id": rid})
            elif op == "activate":
                log(self.server, {"activate": req})
                send({"id": rid, "value": "ok"})
            else:
                send({"id": rid, "value": {"mock": True}})


def log(server, obj):
    line = json.dumps({"t": round(time.time(), 3), **obj}, ensure_ascii=False)
    print(line, flush=True)
    if server.log:
        with open(server.log, "a") as f:
            f.write(line + "\n")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--sock", default=os.path.join(os.environ.get("XDG_RUNTIME_DIR", "/tmp"), "superspace.sock"))
    ap.add_argument("--thumbs", help="folder of *-224.jpg or *.png thumbnails (default: generated PNGs)")
    ap.add_argument("--log", help="also append every request to this file")
    a = ap.parse_args()
    probe = socket.socket(socket.AF_UNIX)
    try:
        probe.connect(a.sock)
        sys.exit("mock_daemon: something already listens on %s; not starting" % a.sock)
    except OSError:
        pass
    finally:
        probe.close()
    if os.path.exists(a.sock):
        os.unlink(a.sock)
    server = socketserver.ThreadingUnixStreamServer(a.sock, Handler)
    server.daemon_threads = True
    server.thumbs, server.log = make_thumbs(a.thumbs), a.log
    ino = os.stat(a.sock).st_ino
    signal.signal(signal.SIGTERM, signal.default_int_handler)  # kill PID still cleans up
    print("mock_daemon: listening on", a.sock, flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        # Remove the socket only if it is still ours, never a real daemon's.
        if os.path.exists(a.sock) and os.stat(a.sock).st_ino == ino:
            os.unlink(a.sock)


if __name__ == "__main__":
    main()
