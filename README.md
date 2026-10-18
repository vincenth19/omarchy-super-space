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

The first time you open Super Space it finishes setting itself up: it
downloads the model, the search engine and speech to text (about 1 GB, once)
with the progress in the box, starts the services, sets up your coding agents
and asks whether Super+Space should open it. Until then, open it with
`omarchy-shell shell toggle io.github.vincenth19.superspace '{}'`.

Prefer a terminal? `~/.config/omarchy/plugins/io.github.vincenth19.superspace/bin/superspace-setup`
does the same setup there. Run `superspace-setup` again any time to repair or
update, and `superspace-setup doctor` to check that each piece works.

Needs Omarchy 4 on aarch64 or x86_64. No GPU, no sudo, no account.

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
| ← → | step through a row's pages or moments |
| Tab | expand a row's pages or moments |
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
day) and the "Search the web" row when you choose it. Setup downloads its
files from GitHub and Hugging Face, checked against pinned checksums.

Hidden files and folders are never indexed, nor is any folder containing a
`.noindex`, `.nomedia` or `CACHEDIR.TAG` file.

## Agents

Setup gives your coding agents a skill and an MCP server, so they search your
files by meaning instead of with `ls`, `find` and `grep`:

- **MCP server** (`superspace mcp`): Claude Code, Codex, Gemini CLI, GitHub
  Copilot CLI and OpenCode, whichever you have installed.
- **Skill** (`skills/superspace`): every agent that reads `~/.agents/skills`,
  `~/.claude/skills`, `~/.codex/skills`, `~/.pi/agent/skills` or
  `~/.hermes/skills`, Crush, Pi and Hermes included.

The same commands work in a terminal:

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

This stops the services and removes the downloads, the index, the agent setup
and the Super+Space binding (your previous one comes back). Your preferences
stay.

## Licence

MIT. See [LICENSE](LICENSE).

## Credits

- [EmbeddingGemma 2](https://huggingface.co/ggml-org/embeddinggemma-2-GGUF)
  by Google: one model for text, images and audio.
- [llama.cpp](https://github.com/ggml-org/llama.cpp) by ggml-org, which runs it.
- [whisper.cpp](https://github.com/ggml-org/whisper.cpp) by ggml-org, with
  OpenAI's Whisper base model and the Silero voice detector, for spoken words.
