# Super Space

Spotlight for [Omarchy](https://omarchy.org) that understands what you mean,
not just what you typed. Fully local. Fast on a Raspberry Pi 5.

<!-- Demo GIF goes here. -->

Press Super+Space and type what you remember: "lease pdf", "the dog at the
beach", "where she talks about pricing", "what I copied from Chromium",
"5'11" in cm", "3pm sydney in jakarta". One search box, every kind of thing,
best match first.

## Install

```bash
omarchy plugin add https://github.com/vincenth19/omarchy-super-space --enable
```

That is the only command. When the plugin first loads, the search box opens
by itself and finishes the setup: it downloads the model, the search engine
and speech to text (about 1 GB, once) with the progress in the box, starts
the services, sets up your coding agents and asks once whether Super+Space
should open it. No terminal, no sudo, no account, no GPU.

Needs Omarchy 4 on aarch64 (a Raspberry Pi 5 is enough) or x86_64.

Prefer a terminal? `~/.config/omarchy/plugins/io.github.vincenth19.superspace/bin/superspace-setup`
does the same setup there. Run `superspace-setup` again any time to repair or
update, and `superspace-setup doctor` to check that each piece works.

## The first index

Seconds after setup finishes, everything on your computer can be found by
name and keyword. Search by meaning fills in behind it, starting with what
you use most, then the rest of your files, then photos, video and speech. The
search box shows how far it has got, and so does `superspace-setup doctor`.

How long meaning search takes to fill in, for a home like the demo's (about
2,800 apps, settings, emoji, document sections and history entries, plus 50
photos, videos and songs). These are estimates until they are measured:

| | Raspberry Pi 5 | Recent laptop |
|---|---|---|
| Names and keywords | seconds | seconds |
| Apps, settings and emoji | under a minute | seconds |
| Documents (about 400 pages and sections) | about 10 minutes | 3 to 5 minutes |
| Photos, video moments and speech | 5 to 10 minutes (about 4 s a photo) | 1 to 3 minutes (about 1 s a photo) |

After that, a file you save is searchable within seconds.

## What runs in the background

Three user services, nothing system-wide (plus a watcher for coding agents
you install later, see [For AI agents](#for-ai-agents)):

| Service | What it does | Priority |
|---|---|---|
| `superspace` | the index and the search; watches your folders for changes | normal for searching; its indexing work runs at idle priority |
| `superspace-embed` | the model for what you type, on 2 cores | normal, so a search never waits |
| `superspace-embed-index` | the model for documents, photos and video frames | idle: only processor and disk time nothing else wants |

`systemctl --user status 'superspace*'` shows the services and
`journalctl --user -u superspace` their log.

## What it searches

- **Apps and settings**: every app, and Omarchy's menu ("wifi connect" offers the Wi-Fi actions).
- **Files and the text inside them**: PDF pages, Word, Excel, PowerPoint, OpenDocument, EPUB, Markdown, HTML, plain text.
- **Photos** by what is in them, in any language ("anjing" finds the dog), plus text in screenshots.
- **Videos** down to the moment, shown as thumbnails on a timeline.
- **Spoken words and song lyrics** in recordings, videos and music.
- **Clipboard history**, with the app it was copied from.
- **Emoji** by meaning ("awkward" finds 😬), and browser history.
- **Instant answers**: maths, units, currency and time zones, loosely phrased.

Exact names and phrases always hit. When nothing on your computer really
matches, it says so instead of guessing.

## Keys

| Key | Does |
|---|---|
| type | search |
| ↑ ↓ or Ctrl+N Ctrl+P | move |
| Enter | open (PDFs at the page, videos at the moment) |
| Ctrl+Enter | reveal in folder or copy path |
| ← → | move the cursor (Shift selects, Ctrl or Alt by words) |
| Tab, then ← → | step through a row's pages or moments; Esc goes back |
| Ctrl+1 … Ctrl+9 | open row N |
| Ctrl+C | copy the answer or path |
| Esc | close |

## Looks like your theme

Every colour, font and corner comes from your Omarchy theme, the same way
Omarchy's own launcher and menus get theirs. Default, light or third-party,
Super Space matches it.

## Privacy

Everything runs on your computer: the index, the model and the search. The
model servers listen on 127.0.0.1 only. Nothing leaves the machine except two
opt-in fetches: exchange rates for currency answers (Frankfurter/ECB, twice a
day) and the "Search the web" row when you choose it.

Setup downloads from GitHub and Hugging Face. llama.cpp, whisper.cpp and the
models are checked against checksums pinned in the plugin. The daemon is
checked against the checksum in its GitHub release (also in the release
notes): that catches a broken download, not a release someone has replaced.

Your coding agents can search everything the box can, clipboard and browser
history included. To keep either out of the index, for you and your agents
alike, add `[[exclude]]` with `kind = "clip"` or `kind = "history"` to your
preferences, or run `superspace pref exclude clip`.

Hidden files and folders are never indexed, nor is any folder containing a
`.noindex`, `.nomedia` or `CACHEDIR.TAG` file.

## For AI agents

Setup gives your coding agents a skill and an MCP server, so they search your
files by meaning instead of with `ls`, `find` and `grep`:

- **MCP server** (`superspace mcp`): Claude Code, Codex, Gemini CLI, GitHub
  Copilot CLI and OpenCode, whichever you have installed.
- **Skill** (`skills/superspace`): every agent that reads `~/.agents/skills`,
  `~/.claude/skills`, `~/.codex/skills`, `~/.pi/agent/skills` or
  `~/.hermes/skills`, Crush, Pi and Hermes included.

An agent you install after setup gets both by itself: a watcher
(`superspace-agents.path`) notices a new program in `~/.local/bin` or in
mise's shims (Omarchy installs agents with mise) and runs
`superspace-setup agents`, which links the skill and registers the MCP server
with every agent that is really installed, not Omarchy's install-on-first-run
placeholders. Run that by hand any time; `superspace-setup doctor` shows each
agent it registered. A session already open sees the MCP server once
restarted.

If you are an agent: read [skills/superspace/SKILL.md](skills/superspace/SKILL.md).
Use the commands below with `--json` for the user's own files; `"empty": true`
means nothing really matches. To install, run the `omarchy plugin add` line
above; `superspace-setup doctor` exits 0 once every piece works and names the
one that does not. Ask before passing `--yes`, which rebinds Super+Space.

```bash
superspace search "lease agreement" --json
superspace read file:Documents/lease.pdf --part 3 --json
superspace similar file:Pictures/dog.jpg --json
superspace explain "lease" file:Documents/lease.pdf --json
```

## Preferences

`~/.config/superspace/preferences.toml` is yours (and your agents') to edit;
changes apply on save.

```toml
prefer = ["I'm preparing an Omarchy meetup demo this month"]  # gently boosts related things
[[pin]]
q = "lease"
key = "file:Documents/lease.pdf"   # always first for "lease"
[[boost]]
path = "~/Documents/Meetup"
by = 0.10
[[bury]]
kind = "clip"
by = 0.10
[[exclude]]
path = "~/Downloads/old"           # never indexed
```

Or from the command line: `superspace pref pin "lease" file:Documents/lease.pdf`.
Super Space also learns from what you open, so the app you always pick for
"te" comes first.

## Uninstall

```bash
superspace-setup uninstall
omarchy plugin remove io.github.vincenth19.superspace
```

This stops and removes the services, the agent watcher, the downloads, the
index, the agent setup and the Super+Space binding (your previous one comes
back). Run both: a plugin left in place sets itself up again when it next
loads. Your preferences stay in `~/.config/superspace`; delete that folder too
to leave nothing behind.

## Licence

MIT. See [LICENSE](LICENSE).

## Credits

- [EmbeddingGemma 2](https://huggingface.co/ggml-org/embeddinggemma-2-GGUF)
  by Google: one model for text, images and audio.
- [llama.cpp](https://github.com/ggml-org/llama.cpp) by ggml-org, which runs it.
- [whisper.cpp](https://github.com/ggml-org/whisper.cpp) by ggml-org, with
  OpenAI's Whisper base model and the Silero voice detector, for spoken words.
