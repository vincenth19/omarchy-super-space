---
name: superspace
description: >
  Find things on this computer by meaning, not just by name. Use INSTEAD of
  ls, find, grep or rg whenever the user asks about their own files, notes,
  documents, PDFs, slides, spreadsheets, photos, screenshots, videos, music,
  recordings, clipboard history, apps or settings ("that PDF about the lease",
  "the photo of the dog at the beach", "where in the talk did she mention
  pricing", "what did I copy from Chromium earlier"). Also use it to read a
  found item's text, find similar items, explain a ranking, or change the
  user's search preferences (pin, boost, bury, exclude). Fully local.
---

# Super Space

`superspace` is the user's local search index: apps, Omarchy settings, files
with the text inside them (PDF pages, Office and OpenDocument files, EPUB,
Markdown), photos (by what is in them), moments in videos, spoken words,
song lyrics, clipboard history (with the app it was copied from), emoji and
browser history. It understands meaning in many languages, and exact names
and phrases still hit. Everything runs on this machine.

## When to use it

- The user describes something they have, rather than giving an exact path.
- The thing may be inside a file (a sentence in a PDF, a slide, a spoken
  phrase in a video) or is not text at all (a photo, a video moment).
- They ask about something they copied, opened or saw earlier.

Use ordinary tools (`ls`, `rg`) for source code in the current project, for
exact paths the user gave you, and for files outside the user's home.

## Commands

If the `superspace` MCP server is connected, its tools do the same things;
use whichever you have. On the command line always pass `--json`. Each
result has a `key`; use it with the other commands.

```bash
superspace search "lease agreement pdf" --json                # best matches, best first
superspace search "dog on the beach" --json --kind image,video --limit 5
superspace read file:Documents/lease.pdf --json               # the item's text, by part
superspace read file:Documents/lease.pdf --part 3 --json      # one part (page, slide, moment)
superspace similar file:Pictures/dog.jpg --json               # items like this one
superspace explain "lease" file:Documents/lease.pdf --json    # why it ranked where it did
superspace open file:Documents/lease.pdf                      # open it on the user's screen
```

`open` acts on the user's desktop (a PDF opens at the page, a video at the
moment); use it only when they ask you to open something.

Kinds: `app setting file image video audio clip emoji history`.

A result row looks like:

```json
{"key": "file:Videos/dog-park.mp4", "kind": "video", "title": "dog-park.mp4",
 "subtitle": "~/Videos · 0:12, 0:47", "score": 0.78, "why": "meaning",
 "action": {"type": "open", "path": "/home/me/Videos/dog-park.mp4", "locator": {}},
 "hits": [{"locator": {"t": 12.0, "t_end": 16.0}, "score": 0.78, "text": ""}]}
```

- `hits` are the parts that matched, best first: `page`, `slide`, `sheet` and
  `row`, `line`, or `t`/`t_end` (seconds) for audio and video. Cite them:
  "page 4 of lease.pdf", "0:12 in dog-park.mp4".
- `"empty": true` means nothing on this computer really matches. Say so; do
  not offer near misses as answers.
- File keys are `file:` plus the path relative to `$HOME`.

If `superspace` is not running, `superspace-setup doctor` shows which piece
fails; the user can run `superspace-setup` to repair it.

## Preferences

Preferences live in `~/.config/superspace/preferences.toml`; the daemon
reloads it on save. Prefer the CLI, which checks each edit:

```bash
superspace prefs --json                                  # current preferences
superspace pref pin "lease" file:Documents/lease.pdf     # this item first for "lease"
superspace pref boost ~/Documents/Meetup 0.1             # a folder, a kind or a key, by 0.1
superspace pref bury clip 0.1
superspace pref exclude ~/Downloads/old                  # never index it
superspace pref prefer "I'm preparing an Omarchy meetup demo this month"
superspace pref unpin "lease"                            # unboost, unbury, unexclude, unprefer undo the rest
```

Editing the file directly is fine too. Its shape:

```toml
prefer = ["I'm preparing an Omarchy meetup demo this month"]  # gently boosts related items
[[pin]]
q = "lease"
key = "file:Documents/lease.pdf"
[[boost]]
path = "~/Documents/Meetup"
by = 0.10
[[bury]]
kind = "clip"
by = 0.10
[[exclude]]
path = "~/Downloads/old"     # never index this folder
```

Change preferences only when the user asks, and tell them what you changed.

## Privacy

Results are the user's private files, photos and clipboard (which can hold
passwords and tokens). Super Space never sends them anywhere; do not either.
Quote only what the task needs, never paste clipboard entries or file
contents into web searches or other online services without the user's
say-so, and do not read items that have nothing to do with the request.
