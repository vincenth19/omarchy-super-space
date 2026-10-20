# Super Space: design

Spotlight for Omarchy that understands meaning, fully local. Super+Space opens
one search box over apps, settings, files (with the text inside PDFs and Office
files), photos, video moments, spoken words, song lyrics, clipboard, emoji and
instant answers. One model, EmbeddingGemma 2, embeds text, images and audio
into the same 768-dim space. Demo target: Raspberry Pi 5 (8 GB, CPU only) on
Tue 20 Oct 2026; it must also run on x86_64 laptops.

## Budgets (the tests enforce these, measured on the Pi)

| What | Budget |
|---|---|
| Instant rows (names, keywords, answers) after a keystroke | under 15 ms at the socket |
| Final rows (meaning) | under 250 ms p95 |
| A saved text file becomes searchable / a deleted one disappears | under 2 s |
| Daemon memory (not counting llama-server) | under 150 MB with 50k parts |
| Daemon binary | under 15 MB |

## Rules that never bend

1. Local results always rank above the web row. The web row ("Search the web
   for …") is always last, and is the only row when nothing local is a real match.
2. When nothing local is a real match, say so: `empty: true`, and the UI shows
   "Nothing on this computer matches". Meaning search must not pad results with
   near misses; every kind has its own score cutoff, tuned by the test bench.
3. Keyword and name matches must work as well as meaning: exact file names,
   exact phrases and app names always hit.
4. Everything works from the keyboard.
5. Nothing leaves the machine except two opt-in fetches: exchange rates
   (Frankfurter/ECB, twice a day) and the web row when chosen.

## Architecture

```
SuperSpace.qml  ──one persistent Unix socket──▶  superspace serve (Rust)
 (Omarchy menu plugin)                           ├─ sources: apps, settings, emoji, clipboard(+source), history
                                                 ├─ files: crawl, exclude rules, inotify, extract text + locators
 superspace search/read --json  ─┐               ├─ media: frames, transcripts, lyrics (background queue)
 superspace mcp (stdio JSON-RPC) ├─ same socket ─├─ answers: maths, units, currency, time zones
 skills/superspace/SKILL.md     ─┘               ├─ index: sqlite (items, parts, FTS5, picks) + in-memory vectors
                                                 └─ HTTP ▶ llama-server :47801 (queries, text only, normal priority)
                                                    HTTP ▶ llama-server :47802 (documents + --mmproj, idle priority)
```

Crates (one owner each; do not edit another owner's crate):

| Crate / folder | Owns |
|---|---|
| `crates/core` (ss-core) | shared types and protocol (lib.rs, frozen; ask before changing) |
| `crates/superspace` | daemon: store, vector index, ranking, prefs, picks, watcher, queues, socket server, CLI, MCP, action runner |
| `crates/files` | roots, exclusion rules, crawl, text extraction with locators |
| `crates/media` | photos, video frames + thumbnails, speech transcripts, lyrics |
| `crates/sources` | apps, settings, emoji, clipboard + source tracking, browser history |
| `crates/answers` | instant answers |
| `ui/` + `SuperSpace.qml` | the search box |
| `tests/`, `demo/` | test bench, demo corpus, golden queries |
| `manifest.json`, `bin/`, `systemd/`, `skills/`, `.github/`, `README.md` | packaging |

## Runtime layout

| Path | What |
|---|---|
| `~/.config/omarchy/plugins/io.github.vincenth19.superspace/` | the plugin (git clone). Never written at runtime. |
| `~/.local/share/superspace/bin/superspace` | daemon binary (GitHub release asset per arch) |
| `~/.local/share/superspace/llama/` | llama.cpp b11718 prebuilt (`llama-b11718-bin-ubuntu-arm64.tar.gz` / `-x64`) |
| `~/.local/share/superspace/models/` | `embeddinggemma-2-Q8_0.gguf` (310 MB), `mmproj-embeddinggemma-2-Q8_0.gguf` (555 MB) from `ggml-org/embeddinggemma-2-GGUF` |
| `~/.local/state/superspace/index.sqlite` | the index |
| `~/.cache/superspace/` | frames, thumbnails, rate cache |
| `~/.config/superspace/preferences.toml` | preferences (human and agent editable, hot reloaded) |
| `$XDG_RUNTIME_DIR/superspace.sock` | the socket |
| `~/.config/systemd/user/superspace{,-embed,-embed-index}.service` | services |

llama-server flags: query server `-t 2 -c 512 -b 512 -ub 512 -np 1 --embeddings
--port 47801`; index server `--mmproj … -t 3 -c 4096 -b 4096 -ub 4096 -np 1
--embeddings --port 47802` with `Nice=19 CPUWeight=idle IOSchedulingClass=idle`.
Embedding requests: `POST /v1/embeddings {"input": ["text", …]}`; an image is
`{"input": [{"content": [{"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,…"}}]}]}`.
Normalize vectors. Query text goes through `ss_core::query_text`, documents through `ss_core::doc_text`.

Measured on the Pi (b11718, Q8_0): text ~100 tokens/s; a photo or frame ~3.9 s
at any size (~81 tokens); photo search ranked the right photo first for all
test queries, Indonesian included ("anjing" found the dog), but unrelated
queries still score 0.50–0.62 against photos versus 0.72–0.80 for real
matches, so photo cutoffs need the bench.

## Protocol

Newline-delimited JSON over `$XDG_RUNTIME_DIR/superspace.sock`; types in
`crates/core/src/lib.rs` (`Request`, `Response`, `Row`, `Answer`). A search
gets two responses with the same id: `phase: "instant"` (names, keywords,
answer) within milliseconds, then `phase: "final"`. Clients drop responses
whose id is not their latest. The daemon may skip the final phase of a search
superseded on the same connection. `activate` runs the action in the daemon
(one code path for UI, CLI and agents) and records the pick.

Type words (decided 19 Oct, leading plurals 20 Oct): kind words at the end of
the query, or at its start when plural ("apps firefox", "settings audio") or
before of/about/with/…, joined by and/or/,/&/only, filter by kind — video(s),
photo(s)/image(s)/picture(s), audio/song(s)/music/(voice) memo(s),
text/doc(s)/document(s)/pdf(s)/note(s), app(s), setting(s), clipboard, emoji,
history/website(s). A singular one at the start is part of a name ("text
editor", "emoji picker", "history essay"). The daemon strips them, filters, and
reports `Response.filter` for the UI's chip; a client's own `kinds` wins. A
query whose words start a name of something the filter would hide ("take a
screenshot", "Settings Manager", "movies to watch" for movies-to-watch.md) is
not filtered, and a filter that would leave nothing while an app or setting
matches the words ("spotify music") falls back to no filter.

```
→ {"op":"search","id":7,"q":"golden retriever"}
← {"id":7,"phase":"instant","rows":[…],"empty":false,"timing":{…}}
← {"id":7,"phase":"final","rows":[{"key":"file:Videos/dog-park.mp4","kind":"video","title":"dog-park.mp4",
     "subtitle":"~/Videos · 0:12, 0:47","icon":"mpv","action":{"type":"open","path":"…","locator":{}},
     "score":0.78,"why":"meaning","hits":[{"locator":{"t":12.0,"t_end":16.0},"score":0.78,"text":"","thumb":"…/12.jpg"}]}],
     "empty":false,"timing":{"instant_ms":2.1,"embed_ms":96,"total_ms":104}}
→ {"op":"activate","id":8,"key":"file:Videos/dog-park.mp4","q":"golden retriever","hit":0}
```

Actions: PDFs open at the page (`evince -p N` when evince is the default,
else `xdg-open`); audio/video at the moment (`mpv --start=T`); apps via
`uwsm-app -- gtk-launch ID`; `paste` uses
`omarchy-clipboard-paste-text`-style copy then Shift+Insert.

## Ranking

1. Candidates: fuzzy name match (nucleo-matcher) over titles and `names`;
   FTS5 BM25 over part text; vector top-k over parts (brute-force dot product,
   in memory).
2. Parts group into one row per item; the row keeps its best parts as `hits`
   (pages, slides, moments) and the best snippet with marks.
3. Score = blend of name, keyword and meaning signals (weights tuned by the
   bench), plus learned picks (frecency per item and per typed prefix, so
   "te" → the app you always pick), plus preference boosts. Pins put an item
   first for a query.
4. Per-kind meaning cutoffs decide what counts as a real match; rows below
   every cutoff and without a name/keyword match are dropped. Nothing left →
   `empty: true`.
5. Order: answer card (separate field) · top hit · then by score with gentle
   kind grouping · web row last, always. At most 3 emoji rows and 2 history
   rows unless the query asks for them; at most 4 rows of one kind come before
   the best row of each other kind that really matches. Under an answer card,
   rows that match by meaning only must clear their cutoff by 0.03. A title
   word one typo away from a query word of 5+ letters counts as a name match
   ("chromum" → Chromium). Learning uses score order, not this placement.

### Learned scorer (decided 18 Oct)

The final order is a weighted sum of ~15 features per row: name score, BM25,
cosine, cosine margin over the next row and over the item's hub score, kind,
modified-recency, frecency, picks for the typed prefix, preference boosts,
similarity to `prefer` sentences. Starting weights are fitted on the bench;
every activation then updates the user's weights online (pairwise: the picked
row beats the rows shown above it), with picks decaying over a few weeks and a
short freshness boost for new items so habits cannot bury them. Weights live
in `~/.local/state/superspace/weights.json`; `explain` shows each feature's
contribution. A cross-encoder (Qwen3-Reranker-0.6B, top 5, after typing
pauses) ships only if it beats this on the bench.

### Setup (decided 18 Oct)

`omarchy plugin add <repo> --enable` is the only command. When the plugin
loads and `~/.local/share/superspace/bin/superspace` is missing, the box opens
once and starts setup itself (`bin/superspace-setup --progress` in a transient
user unit, JSON progress lines; a slow or stopped daemon never starts it), shows
download progress inline, asks once whether Super+Space should open Super
Space, and switches to search when the daemon answers. No sudo, no terminal.

## Preferences (`~/.config/superspace/preferences.toml`)

Hot reloaded; the same edits are available as `Pref` requests, CLI
(`superspace pref pin "lease" file:Documents/lease.pdf`) and MCP tools, so
agents can change them.

```toml
prefer = ["I'm preparing an Omarchy meetup demo this month"]  # embedded; gently boosts related items
[[pin]]   q = "lease"  key = "file:Documents/lease.pdf"
[[boost]] path = "~/Documents/Meetup"  by = 0.10
[[bury]]  kind = "clip"  by = 0.10
[[exclude]] path = "~/Downloads/old"
```

`explain` returns every signal behind a row's score.

## What to index (no settings needed)

Roots: XDG Documents, Downloads, Music, Pictures, Videos, Desktop, plus
`~/Work`, `~/Projects`, `~/Notes`, recursively; `$HOME` itself top level only.
Skip: any dot-prefixed name; node_modules, bower_components, __pycache__,
venv, .venv, site-packages, .tox, target/build/dist when gitignored or with
CACHEDIR.TAG, Trash, lost+found; folders containing CACHEDIR.TAG, .nomedia or
.noindex (the user's opt-out); temp/partial files (`*~ *.swp *.tmp *.part
*.crdownload ~$*`), build output, databases, disk images, lockfiles, `*.min.js
*.map`; binaries (NUL in first 8000 bytes); minified text (average line > 110
chars). In git repos only gitignore-respecting files; source code is indexed by
name, prose by content. Files over 50 MB: name only. At most 1 MiB of text
per file. Do not follow symlinks. Rescan on inotify queue overflow.

## Extraction (no new packages)

`pdftotext -enc UTF-8` split on form feeds → one part per page. DOCX, XLSX
(sharedStrings + every sheet, sheet names), PPTX (every slide + notes), ODT,
ODS, ODP, EPUB: zip + XML. HTML: strip tags/script/style. Markdown/text: by
heading/paragraph, ~800 characters per part with a small overlap, line
locators. Images: tesseract OCR text as a part (only screenshots, Downloads,
clipboard images) plus the image itself as `Embed::Image`.

## Media

- Photos: one `Embed::Image` part (scaled to ≤448 px) + OCR part if any text.
- Video: frames at 448 px, one `Embed::Image` part per frame with `t`,
  `t_end`; 160 px thumbnails as hit thumbs. Audio track → transcript parts
  as for audio.
- Sections (decided 19 Oct): once a video or recording matches (its best
  part clears its cutoff), every part whose margin over its own cutoff is
  within 0.05 of the best part's, and at most 0.015 under the cutoff, joins
  in; neighbouring frames (or transcript windows and lyric lines) merge into
  one section, one hit each, best first. Rows carry `duration`, and the UI
  draws a timeline with the sections highlighted. Each video section is
  labelled from its best frame against ~260 plain labels in
  `data/labels.txt` ("dog swimming", "city street"), shown only when the
  cosine clears 0.70; on the demo videos every label shown was right.
  Frames: the first, one a gap after the last, and one at each scene change
  (ffmpeg scene > 0.3, at least 1 s after the last), but at most two per gap
  on average. The gap is 8 s for videos over 2 minutes on machines with 4 or
  fewer cores (the Pi 5 embeds a frame in ~3.9 s), 4 s otherwise;
  `SUPERSPACE_FRAME_GAP` sets it,
  and changing it re-indexes videos. Past 8 minutes the gap widens, so a video
  gets at most 960 / gap frames (120 at 8 s). Near-black frames (fades, black
  title cards) are skipped. ffmpeg, ffprobe, tesseract, whisper and pdftotext
  run in an idle-weight systemd scope (else nice 19) and the daemon's indexing
  threads at nice 19 / idle I/O, so indexing never slows a search.
- Speech: a local speech-to-text model with timestamps (media agent picks and
  measures: official prebuilt binaries preferred), ~15 s transcript windows as
  text parts with `t`; exact words go to FTS5 so spoken phrases hit by keyword.
- Music: lyrics from tags (USLT/SYLT) or a sibling `.lrc` first; otherwise
  transcribe. One part for the whole lyrics (theme: "songs about a breakup")
  plus timed lines.
- Songs vs recordings (decided 20 Oct): audio is a song when it has an artist
  or album tag or lyrics, or sits under ~/Music, unless its folders (or genre)
  name speech: Voice Memos, Recordings, Podcasts, Meetings, Spoken Word. A
  title tag alone does not count: phone recorders set one. Songs show their
  title tag (else the file name without extension and track number) over "Song
  · Artist · 3:42"; recordings keep their file name over "Recording · 5 Oct ·
  0:43" (the day from a leading YYYY-MM-DD in the name, else creation_time).
- Media indexing runs on a separate background queue, at idle priority, after
  all text is indexed.

## Sources

- Apps: desktop entries (user copies override system ones; NoDisplay/Hidden
  removed), names + GenericName + Keywords as `names`, Comment as text.
- Settings: `omarchy commands --json` and the Omarchy menu
  (`/usr/share/omarchy/default/omarchy/omarchy-menu.jsonc` + user extension),
  so "wifi connect" offers the Wi-Fi actions directly.
- Emoji: Unicode emoji list + CLDR annotations (English and Indonesian),
  embedded so "awkward" finds 😬; a row to open Omarchy's emoji picker.
- Clipboard: `~/.local/state/omarchy/clipboard-history.json` (list of
  `{text, type}`, newest first). Super Space records each new clip's source
  (active window class + title from `hyprctl activewindow -j`, and the page URL
  when it was a browser) and shows "from Chromium · Meetup notes". Copies
  Omarchy's capture.sh treats as sensitive (CLIPBOARD_STATE=sensitive or a
  password-manager hint) get no source record.
- Browser history: every profile of Chromium, Chrome, Brave and Edge
  (`History` `urls` + `Bookmarks`) and of Firefox and Zen (`places.sqlite`
  `moz_places`, with its `-wal`, which holds the latest visits), each copied
  to $XDG_RUNTIME_DIR and read there, never live. The newest visit to a page
  wins across browsers; rows say where they came from ("Chromium ·
  example.com · 2 h ago") with that browser's icon. Rows are keyed, indexed
  and shown without the query string and fragment (tokens live there); only
  opening the row uses the full URL. A source that fails to read keeps its
  existing rows.

## Answers

Loose phrasing must work: `5'11" in cm`, `60 mph in km/h`, `1 gb in mb`,
`1 tbsp in ml`, `15% of 80`, `half a mile in metres`, `100k idr in usd`,
`1m idr to aud`, `20 bucks in aud`, `usd to idr`, `12pm here in london`,
`3pm sydney in jakarta`, `time in berlin`. Home time zone from the system.
Drawn as a two-sided card (input → output) like Raycast.

## Agents

`superspace search "q" --json [--kind file,video] [--limit N]`,
`superspace read KEY [--part N] --json`, `superspace similar KEY`,
`superspace explain "q" KEY`, `superspace pref …`; `superspace mcp` exposes
the same as MCP tools (stdio JSON-RPC, no SDK); `skills/superspace/SKILL.md`
tells agents to use it instead of ls/find/rg for the user's files.
`superspace-setup` registers both with every installed agent (mise installs
included). Agents can search clipboard and history too; `[[exclude]] kind =
"clip"` or `"history"` turns that off, and excluded paths are never readable.

## UI

Opens instantly on Super+Space (keepLoaded menu plugin). One joined panel
(20 Oct: separate tiles felt disjointed), in the system's sans: the field,
with Omarchy's mark where the rows have icons, then the answer card, then
rows. A row's headline says why it matched: the matched passage in quotes,
from its sentence with the most matched words, those words in bold, with the
name and folder under it as the citation; otherwise the title over the
subtitle. At its right is where Enter lands (p. 3, row 4, 0:13–0:27), large
on the selected row. Pages, slides, lines and sheets are numbered chips;
video and audio a timeline of the whole length with each section on it.
Keys: type to search · the first ↓ (or ↑) picks the highlighted row, then
↑/↓ or Ctrl+N/P move · Enter open · Ctrl+Enter alternative (reveal in
folder / copy path) · ←/→ step the picked row's parts (pages, sections,
emoji), and → at the end of the text does so for the top row (20 Oct: the
arrows move the options, not the cursor; typing gives them back to the
cursor) · Tab lists every part with its text · Ctrl+1…9
open row N · Ctrl+C copy answer or path · Esc close. Otherwise the field
edits like a macOS text field: ←/→, Shift to select, Ctrl or Alt for words,
Super for line ends where Hyprland lets it through (ui/README.md). The wheel
moves the list by the system's scroll lines, a touchpad by its own pixels,
never a fling.

Calm while typing (decided 18 Oct, after watching it type): the instant
phase still updates on every keystroke, but rows keep their place and a
chosen selection stays on the same key; the daemon starts the meaning search only
after 150 ms with no newer query on that connection. Responses only add
while typing: removals (rows dropping, the answer card going, "Nothing on this
computer matches") wait until typing has paused 400 ms. A new answer takes the
selection, and the empty notice never shows next to an answer. Until a key
chooses a row, every response selects the top one (20 Oct). No
fade-out/fade-in between updates. Keys that act on a row (Enter, Ctrl+1…9,
Ctrl+C, Tab, ↑/↓, → at the end of the text) end the calm first and wait for
the query's final phase, so they never act on a row left from an earlier
query (19 Oct); with no daemon connected they act at once.

Icons: apps use their own icon; files use the icon of the app that opens
their mime type on this machine (`xdg-mime query default`); the web row uses
the default browser's icon (`xdg-settings get default-web-browser`).

Theme-agnostic: every colour, font, radius and transparency comes from
Omarchy's shell theme (the same tokens Omarchy's own launcher and menus use,
which every theme sets through `colors.toml`/`shell.toml`). No hard-coded
colours or fonts, light themes included. The UI tests screenshot it under the
default theme, a light theme and a third-party theme (basilica).

## Demo-ready means

`tests/run` passes on the Pi: budgets, golden queries (≥ 0.9 recall@5,
correct top hit for the demo script), every "no results" query empty, web row
always last, answers table, preferences, CLI/MCP parity, live add/delete,
keyboard paths through the UI, and a fresh install from the repo.
