#!/usr/bin/env python3
"""Mock `superspace serve` for developing the UI (Python stdlib only).

Speaks the socket protocol in DESIGN.md: newline JSON, two responses per
search (instant, then final after a delay), activate acknowledged and logged.
It never skips a stale final phase, so the UI's drop-stale-ids path is used.

Canned queries (substring match, case-insensitive):
  lease / contract / pdf      PDF hit with pages and marked snippet
  novel / epub                EPUB hit with chapters
  dog / golden / park         videos with 6, 2 and 1 sections (thumbs, some with text), a photo, audio with
                              3 sections (one past an hour), each with its duration
  awkward / smile / emoji     11 emoji rows, the emoji-picker setting after the ninth
  cm / usd / idr / % / 2+2    instant answer (plus whatever else matches)
  100k idr in usd, 3pm sydney in jakarta, byzantine empire timeline
                              per prefix as the live daemon answers them typed key by key: answers
                              that come and go, empty: true beside an answer, rows that come and go
  sheet                       markup in every indexed string (an <img> fetching 127.0.0.1:47806)
  qwxz / nothing              empty: true, only the web row
  many                        30 rows of every shape above (videos with thumbnails, photos, audio, PDF), for scrolling
  slow <query>                the same, with the final phase after 1.5 s instead of 0.12 s
  anything else               apps and settings by prefix; the final phase puts a meaning hit on top
  … text / photos / video / audio, "golden retriever audio and text"
                              the query without those words, only rows of those kinds (and the web row),
                              and "filter": [kinds] on both phases

  python3 ui/dev/mock_daemon.py [--sock PATH] [--thumbs DIR] [--log FILE] [--indexing SECONDS]

With --indexing, every search response carries "progress" {done, total, stage}
for that many seconds after start, as a first index would; then none.

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


def card(i, il, o, ol, copy, detail=""):
    return {"kind": "units", "input": i, "input_label": il, "output": o, "output_label": ol, "copy": copy, "detail": detail}


IMG = '<img src="http://127.0.0.1:47806/ss-mock.png">'
TYPED = {  # query: (answer, local rows), as the live daemon gave them on 19 Oct; no rows is empty: true
    "100k idr": (card("Rp 100,000", "Indonesian rupiah", "A$ 7.97", "Australian dollar", "7.97"), []),
    "100k idr in usd": (card("Rp 100,000", "Indonesian rupiah", "US$ 5.55", "US dollar", "5.55"), []),
    "3pm sydney": (card("3:00 PM", "Sydney", "2:00 PM", "Brisbane", "2:00 PM"), []),
    "3pm sydney in ja": (None, ["flight-booking-bne-hba.txt", "2026-10-12 pengingat.m4a"]),
    "3pm sydney in jakarta": (card("3:00 PM", "Sydney", "11:00 AM", "Jakarta", "11:00 AM"),
                              ["flight-booking-bne-hba.txt", "2026-10-12 pengingat.m4a"]),
    "byzantine empire timeline": (None, ["byzantium-notes.md"]),
}


def typed(s):
    """A TYPED query or a longer prefix of one, else None. Prefixes without an entry have no
    answer; for byzantine… one unrelated row on every third letter, else no rows."""
    if s in TYPED:
        return TYPED[s]
    if s.startswith("3pm sydney in ja"):
        return TYPED["3pm sydney in ja"]
    if len(s) > 2 and any(k.startswith(s) for k in TYPED):
        return None, ["unrelated page %d.html" % len(s)] if s.startswith("byz") and len(s) % 3 == 0 else []
    return None


def answer_for(q):
    s = q.lower()
    if typed(s.strip()):
        return typed(s.strip())[0]
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
    if typed(s):
        rows = [row("file:Documents/" + t, "file", t, "~/Documents", "text-x-generic",
                    {"type": "open", "path": home + "/Documents/" + t, "locator": {}}, 0.6, "meaning") for t in typed(s)[1]]
        return [web(q)], rows + [web(q)], answer, not rows, delay
    if "sheet" in s:
        xl = row("file:Downloads/Q3.xlsx", "file", "Q3 <b>budget</b>.<u>xlsx</u>", "~/Downloads · " + IMG, "x-office-spreadsheet",
                 {"type": "open", "path": home + "/Downloads/Q3.xlsx", "locator": {}}, 0.7, "meaning",
                 marked("Totals " + IMG + " & <i>sheet</i> notes", words),
                 [{"locator": {"sheet": IMG, "row": 3}, "score": 0.7, "text": "row " + IMG},
                  {"locator": {"sheet": "<b>Sheet2</b>", "row": 9}, "score": 0.6, "text": "<b>Sheet2</b> row"}])
        charts = row("file:Downloads/Q3-charts.ods", "file", "Q3-charts.ods", "~/Downloads", "x-office-spreadsheet",
                     {"type": "open", "path": home + "/Downloads/Q3-charts.ods", "locator": {}}, 0.6, "meaning",
                     hits=[{"locator": {"sheet": IMG}, "score": 0.6, "text": "", "thumb": (thumbs or [""])[0]},
                           {"locator": {"sheet": "<i>B</i>"}, "score": 0.5, "text": "", "thumb": (thumbs or [""])[0]}])
        glyph = row("emoji:<b>😬</b>", "emoji", "<i>Grimacing</i> face", IMG, "", {"type": "paste", "text": "😬"}, 0.5, "meaning")
        return [web(q)], [xl, charts, glyph, web(q)], card(IMG, "<b>in</b>", "<i>out</i>", IMG, "x", IMG), False, delay
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
    if any(w in s for w in ("novel", "epub")):
        epub = row("file:Documents/harbour-novel.epub", "file", "harbour-novel.epub", "~/Documents", "x-office-document",
                   {"type": "open", "path": home + "/Documents/harbour-novel.epub", "locator": {"page": 3}}, 0.7, "meaning",
                   marked("The lighthouse keeper rowed out to the harbour at dawn.", words),
                   [{"locator": {"page": 3}, "score": 0.7, "text": "The lighthouse keeper rowed out to the harbour at dawn."},
                    {"locator": {"page": 11}, "score": 0.66, "text": "Back at the harbour, the storm had passed."}])
        return [epub, web(q)], [epub, web(q)], answer, False, delay
    if "many" in s:
        shapes = [r for r in respond("golden", thumbs)[1] + respond("lease", thumbs)[1] if r["kind"] != "web"]
        rows = [dict(r, key="%s#%d" % (r["key"], i), title="%d · %s" % (i + 1, r["title"]),
                     icon=(thumbs or [r["icon"]])[i % len(thumbs or [0])] if r["kind"] == "image" else r["icon"])
                for i, r in enumerate(shapes[i % len(shapes)] for i in range(30))]
        return [web(q)], rows + [web(q)], answer, False, delay
    if any(w in s for w in ("dog", "golden", "park", "retriever")):
        t = thumbs or [""]

        def media(name, kind, folder, icon, spans, duration=None, snippet=None):
            """A video or audio row; spans are its sections (t, t_end, score, text), best first."""
            hits = [{"locator": {"t": a, "t_end": b}, "score": sc, "text": said, **({"thumb": t[i % len(t)]} if kind == "video" else {})}
                    for i, (a, b, sc, said) in enumerate(spans)]
            r = row("file:%s/%s" % (folder, name), kind, name, "~/" + folder, icon,
                    {"type": "open", "path": "%s/%s/%s" % (home, folder, name), "locator": hits[0]["locator"]}, spans[0][2], "meaning", snippet, hits)
            return dict(r, duration=duration) if duration else r

        six = media("dog-park.mp4", "video", "Videos", "mpv", [
            (47.0, 51.0, 0.78, "golden retriever"), (12.0, 16.0, 0.75, ""), (95.0, 99.0, 0.71, "dog fetching a ball"),
            (131.0, 139.0, 0.69, ""), (3.0, 5.0, 0.67, "a golden retriever on the grass by the lake at the park"), (150.0, 160.0, 0.66, "")], 165.0)
        two = media("weekend-clips-oct-2026.mp4", "video", "Videos", "mpv", [(45.07, 53.07, 0.69, ""), (10.0, 30.0, 0.65, "")], 59.0)
        one = media("beach-day.mp4", "video", "Videos", "mpv", [(12.0, 20.0, 0.67, "")], 95.0)
        audio = media("park-walk.mp3", "audio", "Music", "audio-x-generic", [
            (65.0, 80.0, 0.6, "the golden retriever ran across the whole park"), (1210.0, 1225.0, 0.57, "the dog needs a bath after this"),
            (3725.0, 3740.0, 0.55, "we took the dog home after the park")], 3900.0,
            marked("…and then the golden retriever ran across the whole park…", words))
        photo = row("file:Pictures/golden1.jpg", "image", "golden1.jpg", "~/Pictures · 12 Oct", t[0] or "image-x-generic",
                    {"type": "open", "path": home + "/Pictures/golden1.jpg", "locator": {}}, 0.74, "meaning",
                    hits=[{"locator": {}, "score": 0.74, "text": "", "thumb": t[0]}])  # a photo's one hit is itself
        return [web(q)], [six, photo, two, audio, one, web(q)], answer, False, delay
    if any(w in s for w in ("awkward", "smile", "emoji")):
        emoji = [("😬", "Grimacing face", "awkward · nervous"), ("😅", "Grinning face with sweat", "phew · relief"),
                 ("🙃", "Upside-down face", "silly · sarcasm"), ("😳", "Flushed face", "awkward · embarrassed"),
                 ("😶", "Face without mouth", "awkward · blank"), ("😑", "Expressionless face", "awkward · meh"),
                 ("😐", "Neutral face", "awkward · deadpan"), ("🫠", "Melting face", "embarrassed · melt"),
                 ("😕", "Confused face", "confused · dunno"), ("🤷", "Person shrugging", "dunno · idk"),
                 ("🙈", "See-no-evil monkey", "embarrassed · oops")]
        rows = [row("emoji:" + e, "emoji", name, kw, e, {"type": "paste", "text": e}, 0.7, "meaning") for e, name, kw in emoji]
        rows.insert(9, row("setting:emoji-picker", "setting", "Open emoji picker", "Omarchy", "face-smile",
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


KINDS = {"text": "file", "photo": "image", "photos": "image", "video": "video", "videos": "video", "audio": "audio"}


def kinds(q):
    """(the query without its kind words, the kinds they name), as the daemon reads a typed filter; else (q, None)."""
    words = q.split()
    ks = list(dict.fromkeys(KINDS[w.lower()] for w in words if w.lower() in KINDS))
    rest = [w for w in words if w.lower() not in KINDS and w.lower() != "and"]
    return (" ".join(rest), ks) if ks and rest else (q, None)


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


def progress(server):
    """While --indexing runs, how far a pretend first index has got."""
    f = (time.time() - server.started) / server.indexing if server.indexing else 1
    if f >= 1:
        return None
    return {"done": int(f * 2800), "total": 2800, "stage": ["apps and settings", "documents", "emoji", "photos"][int(f * 4)]}


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
                words, ks = kinds(q)
                instant, final, answer, empty, delay = respond(words, self.server.thumbs)
                if ks:
                    instant, final = ([r for r in rows if r["kind"] in ks + ["web"]] for rows in (instant, final))
                    empty = all(r["kind"] == "web" for r in final)
                ms = (time.perf_counter() - t0) * 1000
                p = progress(self.server)
                base = {"id": rid, "empty": empty, **({"answer": answer} if answer else {}), **({"progress": p} if p else {}),
                        **({"filter": ks} if ks else {})}
                send({**base, "empty": empty or all(r["kind"] == "web" for r in instant), "phase": "instant", "rows": instant, "timing": {"instant_ms": ms, "embed_ms": 0, "total_ms": ms}})
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
    ap.add_argument("--indexing", type=float, default=0, help="send progress for this many seconds after start")
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
    server.thumbs, server.log, server.indexing, server.started = make_thumbs(a.thumbs), a.log, a.indexing, time.time()
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
