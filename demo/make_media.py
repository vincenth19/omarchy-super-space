# /// script
# requires-python = ">=3.10"
# dependencies = ["pillow>=10.1"]
# ///
"""Builds the demo photos, videos, voice memos and songs under demo/corpus/home.

Run on a Mac with `uv run demo/make_media.py` (needs ffmpeg and macOS `say`).
Downloads are cached in demo/corpus/.cache, so a re-run is offline. Every
downloaded file is checked for an allowed licence and listed in
demo/CREDITS.md, which this script rewrites. The timings the golden queries
use are fixed here (start times of spoken lines and video segments) and
written to demo/corpus/truth.json.
"""
import html
import json
import pathlib
import re
import shutil
import subprocess
import tempfile
import time
import urllib.parse
import urllib.request
import wave

from PIL import Image, ImageDraw, ImageFont

DEMO = pathlib.Path(__file__).resolve().parent
HOME = DEMO / "corpus" / "home"
CACHE = DEMO / "corpus" / ".cache"
UA = {"User-Agent": "superspace-demo-corpus/0.1 (https://github.com/vincenth19/omarchy-super-space)"}
ALLOWED = re.compile(r"^(CC0|Public domain|CC BY(-SA)? \d\.\d\b)", re.I)
RATE = 22050
credits = []  # (corpus path, source url, author, licence, what was changed)
SAY = "words CC0; voice: macOS System Voices (see note)"
truth = {}

# Photos: Commons title -> where it lands. Camera-style names on purpose, so
# search has to find them by what they show, not by file name.
PHOTOS = {
    "File:Golden retriever gnawing at the farmer's market.jpg": "Pictures/Camera/IMG_2041.jpg",
    "File:A Golden Retriever-9 (Barras).JPG": "Pictures/Camera/IMG_2047.jpg",
    "File:Golden Retriever with a stick (Barras).jpg": "Pictures/Camera/IMG_2052.jpg",
    "File:Callie the golden retriever puppy.jpg": "Pictures/Camera/IMG_2063.jpg",
    "File:Golden Retriever puppy (3813393).jpg": "Pictures/Camera/IMG_2078.jpg",
    "File:Dülmen, Hausdülmen, Golden Retriever -- 2022 -- 5945.jpg": "Pictures/Camera/IMG_2090.jpg",
    "File:Beagle sleeping in Hautmont (DSC07212).jpg": "Pictures/Camera/IMG_2104.jpg",
    "File:Border-Collie-tri-colour-face-1.jpg": "Pictures/Camera/IMG_2111.jpg",
    "File:Running Dachshund at the beach.jpg": "Pictures/Camera/IMG_2119.jpg",
    "File:Jack Russell Terrier Eddi running at the beach.JPG": "Pictures/Camera/IMG_2126.jpg",
    "File:Cat sleeping on a bench, Prazeres Cemetery, Lisbon, Portugal julesvernex2.jpg": "Pictures/Camera/IMG_2133.jpg",
    "File:Black-Cat-Sitia-Crete-Greece-2.jpg": "Pictures/Camera/IMG_2140.jpg",
    "File:Kali April 2016-6.jpg": "Pictures/Camera/IMG_2152.jpg",
    "File:Young tabby cat keeping watch.jpg": "Pictures/Camera/IMG_2158.jpg",
    "File:Tofta strand November 2023 07.jpg": "Pictures/Camera/IMG_2167.jpg",
    "File:Margherita pizza on plate.jpg": "Pictures/Camera/IMG_2175.jpg",
    "File:Sushi Plate in Organic Sushi Prague 4.jpg": "Pictures/Camera/IMG_2181.jpg",
    "File:Nasi Goreng Kampung.jpg": "Pictures/Camera/IMG_2186.jpg",
    "File:Flat white coffee with pretty feather pattern.jpg": "Pictures/Camera/IMG_2190.jpg",
    "File:Pavlova dessert.JPG": "Pictures/Camera/IMG_2194.jpg",
    "File:Media Viewer Launch Retrospective Whiteboard - April 11 2014 01.jpg": "Pictures/Camera/IMG_2199.jpg",
    "File:ReceiptSwiss.jpg": "Pictures/Camera/IMG_2203.jpg",
    "File:Wineglass Bay, Tasmania.jpg": "Pictures/Holidays/Tasmania/IMG_3301.jpg",
    "File:Boat shed and Cradle Mountain at Dove Lake, Tas.jpg": "Pictures/Holidays/Tasmania/IMG_3315.jpg",
    "File:Sunset, Kuta, Bali, Indonesia, 20220825 1752 0877.jpg": "Pictures/Holidays/Bali/IMG_1188.jpg",
}

# Videos stitched from Commons clips: (title, start, length). Segment times in
# the result are the ground truth for "is the dog on screen".
VIDEOS = {
    "Videos/weekend-clips-oct-2026.mp4": [
        ("File:Strong waves from Hurricane Erin lead to beaches in Wellfleet, MA closing on Aug. 22, 2025.webm", 0, 10, "waves breaking on a beach, no dog"),
        ("File:Golden retriever swimming the doggy paddle.webm", 0, 20, "golden retriever swimming"),
        ("File:Traffic on Slovenska street 2.webm", 0, 15, "city street traffic, no dog"),
        ("File:Golden retriever swimming the doggy paddle.webm", 20, 14, "golden retriever swimming"),
    ],
    "Videos/home-video-sept-2026.mp4": [
        ("File:Cat playing with a laser pointer.webm", 0, 14, "cat chasing a laser pointer"),
        ("File:Milchkochen.webm", 0, 10.9, "milk boiling in a pot on a stove, no cat"),
        ("File:Cat Play 03.webm", 0, 8, "cat playing"),
    ],
}

# Public-domain recordings (pre-1925, US). No lyrics file: their titles and
# tags are all there is, which the bench uses as a control.
SONGS_PD = {
    "File:After You've Gone (Harris 1918 recording).wav": ("Music/Marion Harris - After You've Gone (1918).mp3", "After You've Gone", "Marion Harris", "1918"),
    "File:MeekerBallGame.ogg": ("Music/Edward Meeker - Take Me Out to the Ball Game (1908).mp3", "Take Me Out to the Ball Game", "Edward Meeker", "1908"),
}

# Spoken lines: (voice, start second, text). Start times are fixed so the
# ground truth does not depend on the speed of the voice; the build fails if
# a line runs into the next one.
MEMOS = {
    "Music/Voice Memos/2026-10-05 meetup budget.m4a": [
        ("Karen", 0, "Okay, quick voice memo about the meetup budget."),
        ("Daniel", 4, "Sure. What have we got so far?"),
        ("Karen", 8, "The sponsor is giving us six hundred dollars."),
        ("Daniel", 13, "And the venue?"),
        ("Karen", 16, "The venue costs five hundred dollars, and that includes the projector."),
        ("Daniel", 22, "Then the pizza is about two hundred and eighty."),
        ("Karen", 27, "So we are over by two hundred and seventy five. Let's find another sponsor for the stickers."),
        ("Daniel", 35, "I will email the library about a free room as a backup."),
    ],
    "Music/Voice Memos/2026-10-08 groceries.m4a": [
        ("Samantha", 0, "Note to self for the shops after work."),
        ("Samantha", 4, "Oat milk, sourdough bread, three avocados and a bag of coffee beans."),
        ("Samantha", 11, "Also pick up the dry cleaning before six."),
    ],
    "Music/Voice Memos/2026-10-11 app idea.m4a": [
        ("Daniel", 0, "App idea."),
        ("Daniel", 2.5, "A little sensor in each plant pot that sends a message when the soil is dry."),
        ("Daniel", 9, "It could learn how often each plant needs water, and remind me on my phone."),
        ("Daniel", 17, "Call it Thirsty Fern."),
    ],
    "Music/Voice Memos/2026-10-12 pengingat.m4a": [
        ("Damayanti", 0, "Pengingat untuk hari Jumat."),
        ("Damayanti", 4, "Bayar sewa apartemen, enam ratus dua puluh dolar per minggu."),
        ("Damayanti", 11, "Dan telepon Nenek di Bali hari Minggu."),
    ],
}

# Original songs written for this corpus (CC0), sung by macOS singing voices,
# with a timed .lrc next to each.
SONGS = {
    "Music/Kitchen Radio - Empty Side of the Bed.mp3": ("Empty Side of the Bed", "Bad News", 9, [
        "You packed your boxes on a Sunday afternoon",
        "Now the empty side of the bed is cold",
        "I still make two cups of coffee every morning",
        "We said goodbye at the station in the rain",
        "I deleted all your photos then I found them again",
        "Maybe one day I will stop missing you",
    ]),
    "Music/Kitchen Radio - Keys on the Counter.mp3": ("Keys on the Counter", "Cellos", 8, [
        "You took the spare key and left the light on",
        "Your keys are on the counter by the door",
        "We broke up over dinner at the old cafe",
        "I kept your jumper but I gave back the ring",
        "Our song comes on the radio and I turn it off",
        "Goodbye to the summer we spent together",
    ]),
    "Music/Kitchen Radio - Saturday Market Morning.mp3": ("Saturday Market Morning", "Good News", 11, [
        "Saturday morning at the farmers market",
        "Fresh bread and mangoes and a coffee in my hand",
        "The kids are dancing and the band is playing",
        "Sunshine on the river, what a lovely day",
    ]),
    "Music/Kitchen Radio - Ferry to the City.mp3": ("Ferry to the City", "Organ", 8, [
        "Riding the ferry to the city every morning",
        "Pelicans are floating by the bridge",
        "The river is shining and the boats are going",
        "Home again by six for dinner with my friends",
    ]),
}

WALKTHROUGH = ("Videos/super-space-walkthrough.mp4", "Karen", 50, [
    (0, "Super Space", "Welcome to this short walkthrough of the new search on my Raspberry Pi."),
    (8, "Super + Space", "Press super and space together to open the search box."),
    (16, "lease bond -> page 3", "Type a few words, like lease bond, and the right page of the PDF comes up first."),
    (26, "Moments in videos", "It can even jump to the moment something appears inside a video."),
    (35, "Any theme", "It follows whatever Omarchy theme you use, light or dark."),
    (43, "Thanks!", "Thanks for watching, see you at the meetup in Fortitude Valley."),
])


def dest(rel):
    p = HOME / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    return p


def fetch(url, path):
    if path.exists():
        return path
    path.parent.mkdir(parents=True, exist_ok=True)
    for attempt in range(6):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=120) as r:
                tmp = path.with_suffix(path.suffix + ".part")
                tmp.write_bytes(r.read())
                tmp.rename(path)
                return path
        except urllib.error.HTTPError as e:
            if e.code not in (429, 503):
                raise
            time.sleep(10 * (attempt + 1))
    raise SystemExit(f"could not download {url}")


def commons(title, width=None):
    """Download a Commons file (a scaled JPEG when width is given) after checking its licence."""
    meta = CACHE / "meta" / (re.sub(r"[^\w.-]", "_", title) + f".{width}.json")
    if not meta.exists():
        q = {"action": "query", "format": "json", "titles": title, "prop": "imageinfo",
             "iiprop": "url|extmetadata", "iiextmetadatafilter": "LicenseShortName|Artist|LicenseUrl"}
        if width:
            q["iiurlwidth"] = width
        fetch("https://commons.wikimedia.org/w/api.php?" + urllib.parse.urlencode(q), meta)
        time.sleep(1)
    page = next(iter(json.loads(meta.read_text())["query"]["pages"].values()))
    info = page["imageinfo"][0]
    m = info["extmetadata"]
    lic = m["LicenseShortName"]["value"]
    if not ALLOWED.match(lic):
        raise SystemExit(f"{title}: licence {lic!r} is not allowed")
    author = html.unescape(re.sub(r"<[^>]+>", "", m.get("Artist", {}).get("value", "unknown"))).strip()
    url = info["thumburl"] if width else info["url"]
    path = fetch(url, CACHE / "files" / (re.sub(r"[^\w.-]", "_", title) + (f".{width}.jpg" if width else "")))
    return path, info["descriptionurl"], author, lic


def ff(*args):
    subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", *map(str, args)], check=True)


def speech(voice, text):
    """16-bit mono PCM at RATE of one spoken line."""
    with tempfile.TemporaryDirectory() as d:
        f = pathlib.Path(d) / "s.wav"
        subprocess.run(["say", "-v", voice, "--file-format=WAVE", f"--data-format=LEI16@{RATE}", "-o", f, text], check=True)
        with wave.open(str(f)) as w:
            return w.readframes(w.getnframes())


def timeline(lines, total, wav):
    """Place (voice, start, text) lines at their start seconds; returns [(start, end, text)]."""
    buf = bytearray(int(total * RATE) * 2)
    spans = []
    for i, (voice, start, text) in enumerate(lines):
        pcm = speech(voice, text)
        end = start + len(pcm) / 2 / RATE
        limit = lines[i + 1][1] if i + 1 < len(lines) else total
        if end > limit:
            raise SystemExit(f"{text!r} ends at {end:.2f}s, after the next line at {limit}s")
        o = int(start * RATE) * 2
        buf[o:o + len(pcm)] = pcm
        spans.append((start, round(end, 2), text))
    with wave.open(str(wav), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(bytes(buf))
    return spans


def font(size):
    return ImageFont.load_default(size=size)


def paper(rel, lines, size, bg, fg, step, fsize, angle=0, quality=88):
    img = Image.new("RGB", size, bg)
    d = ImageDraw.Draw(img)
    for i, line in enumerate(lines):
        d.text((40, 40 + i * step), line, fill=fg, font=font(fsize))
    if angle:
        img = img.rotate(angle, expand=True, fillcolor=(70, 60, 50))
    img.save(dest(rel), quality=quality)
    credits.append((rel, "generated by demo/make_media.py", "Super Space contributors", "CC0", "synthetic image with known text"))


def photos():
    for title, rel in PHOTOS.items():
        path, src, author, lic = commons(title, width=1280)
        shutil.copyfile(path, dest(rel))
        credits.append((rel, src, author, lic, "scaled to 1280 px wide"))
    paper("Downloads/scan0003.jpg", [
        "BEAN THERE COFFEE CO", "South Bank - Little Stanley St", "ABN 12 345 678 901", "",
        "03/10/2026 08:42      Table 7", "", "2 x Flat white (oat)    $11.40", "1 x Banana bread         $6.50",
        "1 x Long black           $4.80", "", "SUBTOTAL                $22.70", "TOTAL incl GST          $22.70",
        "EFTPOS                  $22.70", "", "Thank you! See you tomorrow",
    ], (620, 760), (246, 244, 236), (30, 30, 30), 46, 26, angle=3)
    paper("Pictures/Screenshots/screenshot-2026-10-12_09-14-33.png", [
        "sam@pi5 ~ $ superspace serve",
        "error: failed to bind 127.0.0.1:47801: Address already in use (os error 98)",
        "hint: another llama-server is already running on this port",
        "sam@pi5 ~ $ ss -ltnp | grep 47801",
        "LISTEN 0 512 127.0.0.1:47801 users:((\"llama-server\",pid=2231))",
    ], (1280, 400), (24, 26, 30), (220, 220, 210), 52, 24)


def videos():
    for rel, segs in VIDEOS.items():
        args, chains, t, spans = [], [], 0.0, []
        for i, (title, ss, length, what) in enumerate(segs):
            path, src, author, lic = commons(title)
            args += ["-ss", ss, "-t", length, "-i", path]
            chains.append(f"[{i}:v]scale=1280:720:force_original_aspect_ratio=decrease,pad=1280:720:(ow-iw)/2:(oh-ih)/2,fps=30,setsar=1,format=yuv420p[v{i}]")
            spans.append({"t": round(t, 2), "t_end": round(t + length, 2), "what": what})
            credits.append((rel, src, author, lic, f"{ss}-{ss + length}s used at {t:g}-{t + length:g}s, scaled to 720p, sound removed"))
            t += length
        graph = ";".join(chains) + ";" + "".join(f"[v{i}]" for i in range(len(segs))) + f"concat=n={len(segs)}:v=1:a=0[v]"
        ff(*args, "-filter_complex", graph, "-map", "[v]", "-c:v", "libx264", "-crf", 27, "-preset", "medium", "-movflags", "+faststart", dest(rel))
        truth[rel] = spans


def walkthrough():
    rel, voice, total, slides = WALKTHROUGH
    with tempfile.TemporaryDirectory() as d:
        d = pathlib.Path(d)
        spans = timeline([(voice, s, said) for s, _, said in slides], total, d / "a.wav")
        listing = ""
        for i, (start, shown, _) in enumerate(slides):
            end = slides[i + 1][0] if i + 1 < len(slides) else total
            img = Image.new("RGB", (1280, 720), (20 + 30 * i, 40, 70))
            ImageDraw.Draw(img).text((640, 360), shown, fill=(240, 240, 240), font=font(84), anchor="mm")
            img.save(d / f"{i}.png")
            listing += f"file '{d / f'{i}.png'}'\nduration {end - start}\n"
        (d / "list.txt").write_text(listing + f"file '{d / f'{len(slides) - 1}.png'}'\n")
        ff("-f", "concat", "-safe", 0, "-i", d / "list.txt", "-i", d / "a.wav", "-r", 30, "-pix_fmt", "yuv420p",
           "-c:v", "libx264", "-crf", 28, "-c:a", "aac", "-b:a", "64k", "-shortest", "-movflags", "+faststart", dest(rel))
    truth[rel] = [{"t": s, "t_end": e, "said": x, "shown": slides[i][1]} for i, (s, e, x) in enumerate(spans)]
    credits.append((rel, "generated by demo/make_media.py (macOS say, voice Karen)", "Super Space contributors", SAY, "synthetic narration over title cards"))


def memos():
    for rel, lines in MEMOS.items():
        with tempfile.TemporaryDirectory() as d:
            wav = pathlib.Path(d) / "a.wav"
            spans = timeline(lines, lines[-1][1] + 8, wav)
            ff("-i", wav, "-c:a", "aac", "-b:a", "64k", "-metadata", "title=" + pathlib.Path(rel).stem, dest(rel))
        truth[rel] = [{"t": s, "t_end": e, "said": x} for s, e, x in spans]
        voices = sorted({v for v, _, _ in lines})
        credits.append((rel, f"generated by demo/make_media.py (macOS say, voices {', '.join(voices)})", "Super Space contributors", SAY, "synthetic voice memo"))


def songs():
    for rel, (title, voice, step, lyrics) in SONGS.items():
        lines = [(voice, 2 + i * step, line) for i, line in enumerate(lyrics)]
        with tempfile.TemporaryDirectory() as d:
            wav = pathlib.Path(d) / "a.wav"
            spans = timeline(lines, lines[-1][1] + step, wav)
            ff("-i", wav, "-c:a", "libmp3lame", "-q:a", 5, "-metadata", f"title={title}", "-metadata", "artist=Kitchen Radio",
               "-metadata", "album=Demo Tapes", "-metadata", "date=2026", dest(rel))
        lrc = [f"[ti:{title}]", "[ar:Kitchen Radio]", "[al:Demo Tapes]"]
        lrc += [f"[{int(s // 60):02d}:{s % 60:05.2f}]{x}" for s, _, x in spans]
        dest(rel).with_suffix(".lrc").write_text("\n".join(lrc) + "\n")
        truth[rel] = [{"t": s, "t_end": e, "said": x} for s, e, x in spans]
        credits.append((rel, f"written for this corpus, sung by macOS say voice {voice}", "Super Space contributors", SAY, "original song and lyrics, timed .lrc beside it"))
    for title, (rel, name, artist, year) in SONGS_PD.items():
        path, src, author, lic = commons(title)
        ff("-i", path, "-c:a", "libmp3lame", "-q:a", 4, "-metadata", f"title={name}", "-metadata", f"artist={artist}",
           "-metadata", f"date={year}", dest(rel))
        credits.append((rel, src, author, lic, "converted to MP3, tags added"))


def books():
    rel = "Documents/Books/alice-in-wonderland.epub"
    shutil.copyfile(fetch("https://www.gutenberg.org/ebooks/11.epub.noimages", CACHE / "files" / "pg11.epub"), dest(rel))
    credits.append((rel, "https://www.gutenberg.org/ebooks/11", "Lewis Carroll (1865), Project Gutenberg eBook #11", "Public domain", "none"))


def write_credits():
    rows = ["| File in the demo home | Source | Author | Licence | Changes |", "|---|---|---|---|---|"]
    rows += [f"| `{f}` | {s} | {a.replace('|', '/')} | {lic} | {c} |" for f, s, a, lic, c in credits]
    (DEMO / "CREDITS.md").write_text(
        "# Demo corpus credits\n\n"
        "Every file in the demo corpus that was not written for it, and how it was changed.\n"
        "Generated by `demo/make_media.py`; the documents from `demo/make_docs.py` and the\n"
        "clipboard fixture are original and CC0. People and companies in them are fictional.\n\n"
        "Spoken and sung audio is made with macOS `say`. Apple's macOS licence allows System Voices\n"
        "output for personal, non-commercial use only, so those files are built on the machine that\n"
        "uses them and are never committed or redistributed (demo/corpus/ is gitignored).\n\n"
        + "\n".join(rows) + "\n")


def clipboard():
    """Omarchy clipboard history fixture (newest first). install-to-home.sh only copies it into test homes."""
    clips = [
        "Tracking number: TT4829103477AU (Australia Post, parcel from Kopi Club)",
        "ssh pi@10.0.0.42 -p 2222",
        "The Precinct, Level 2, 315 Brunswick Street, Fortitude Valley QLD 4006",
        "Flat white with oat milk, extra hot, for Priya",
        "https://github.com/vincenth19/omarchy-super-space",
    ]
    p = DEMO / "corpus" / "clipboard-history.json"
    p.write_text(json.dumps([{"text": c, "type": "text"} for c in clips], indent=1, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    for tool in ("ffmpeg", "say"):
        if not shutil.which(tool):
            raise SystemExit(f"{tool} not found: run this on a Mac with ffmpeg installed")
    photos()
    books()
    videos()
    walkthrough()
    memos()
    songs()
    clipboard()
    (DEMO / "corpus" / "truth.json").write_text(json.dumps(truth, indent=1) + "\n")
    write_credits()
    print(f"media written under {HOME}; {len(credits)} credit lines")
