# Super Space UI

`SuperSpace.qml` (repo root) is an Omarchy 4 `menu` plugin, kept loaded so it
opens instantly. `ui/ResultRow.qml` draws a result, `ui/AnswerCard.qml` an
instant answer. It talks to `superspace serve` over one socket,
`$XDG_RUNTIME_DIR/superspace.sock` or, as for the daemon, `$SUPERSPACE_SOCK`
(DESIGN.md "Protocol"). Every result,
answers included, runs through the daemon (`activate`, with `"alt": true` for
Ctrl+Enter); the UI itself only runs `wl-copy` for Ctrl+C. Like Spotlight, it
closes on Esc, on a click outside the box, and when another window takes the
keyboard.

When the plugin loads and `~/.local/share/superspace/bin/superspace` does
not exist (setup never ran), the box opens by itself and starts setup at once,
so `omarchy plugin add … --enable` is the only command. This is the only
place setup starts: once per plugin load, never when the binary exists, and
never because the daemon is slow or down (the box then says "Waiting for Super
Space (superspace.service)…"). Setup is the plugin's
`bin/superspace-setup --progress`, run as the systemd unit `superspace-setup`
so nothing in the shell can kill it mid-download; the box follows the JSON
lines it writes to `$XDG_RUNTIME_DIR/superspace-setup.jsonl`. It shows each
step and download progress, and on `ask-bind` asks "Make Super+Space open
Super Space?" (Enter yes, which runs `superspace-setup bind`; Esc no; only
while the query is empty). Closing the box clears setup's message (an error,
or "ready") unless setup is still running.

Open it (the plugin binds no key until then):

```
omarchy-shell shell toggle io.github.vincenth19.superspace '{}'
```

## Theme

Every colour, font, size, radius and transparency comes from Omarchy's theme
singletons, the same ones Omarchy's own menu uses: `Color.menu.*` (background,
text, border, scrim, selected-*), `Color.accent` (marked words, picked page or
moment), `Style.font.*` and `Style.font.menuFamily`, `Style.spacing.*` and
`Style.space()`, `Style.cornerRadius` (Hyprland's rounding), `Border.surfaceSpec`
for borders. The layer namespace is `omarchy-menu`, so a theme's rules for
menus (blur, no animation) apply. Secondary text is the text colour at the
opacities Omarchy's menu uses. Checked under Tokyo Night, Catppuccin Latte and
Basilica.

## Keys

Type to search · ↑/↓ or Ctrl+N/P move · Enter open (on an answer: copy) ·
Ctrl+Enter the alternative (show in folder, or copy) · ←/→ step through a row's pages or
moments, or pick an emoji in a strip (with fewer than two, they move the text cursor) · Tab list a row's
hits · Ctrl+1…9 open row N · Ctrl+C copy the answer, path or URL (copies the
selected text if there is any) · Ctrl+U clear · Esc close.

## Test bench

```
omarchy-shell superspace open 'golden retriever'   # open if closed, set the query
omarchy-shell superspace key Down                  # Up Left Right Enter Escape Tab Backspace,
                                                   # ctrl+Enter ctrl+1 … ctrl+9 ctrl+n ctrl+p ctrl+c ctrl+u
omarchy-shell superspace state
```

(`omarchy-shell` is `qs ipc -n -p $OMARCHY_PATH/shell call -- superspace …`;
over SSH set `XDG_RUNTIME_DIR` and `OMARCHY_PATH`.) `state` returns:

```
{"open":true,"query":"lease","connected":true,"phase":"final",
 "selected":0,"selectedKey":"file:Documents/lease-2026.pdf","hit":0,"expanded":false,
 "empty":false,"answer":null,"rows":[{"key":"…","kind":"file","title":"…","hits":3},…],
 "activated":{"op":"activate","id":10,"key":"…","q":"lease","hit":1,"alt":false},
 "perf":[["instant",3],["final",4]],"setup":null,"askBind":false,
 "progress":{"done":1240,"total":2800,"stage":"photos"},
 "theme":{"background":"#1e1913","text":"#ebe1cc"}}
```

`phase` is `"final"` once the latest final phase is shown as it is, else
`"instant"` (see typing below); `selected` indexes `rows` (-1 on the answer
card, where `selectedKey` is `"answer"`); `hit` indexes the selected row's `hits` (null without hits);
`activated` is the last `activate` sent; `perf` is the UI's own time per
applied response (ms, model update and layout) since it opened; `setup` is
setup's last progress line; `progress` is the latest response's indexing
progress (null when it had none); `theme` the menu colours the UI draws with.

Consecutive emoji rows are drawn as one strip of up to 8 (the rest are
dropped). In `state` a strip is one row: `key` and `kind` of its first emoji,
`hits` the number of emoji; `hit` is the picked emoji and `selectedKey` its
key, which Enter, Ctrl+1…9 and Ctrl+C use. The subtitle names the picked emoji.
Clicking an emoji pastes that emoji.

While a response carries `"progress": {done, total, stage}`, a quiet line
above the key hints says "Indexing · 1,240 of 2,800 · photos" (`stage`, what
is embedded now, verbatim); it goes with the first response without it.

Hits are labelled p. N (ch. N in an EPUB, whose parts are chapters), slide N,
sheet · row N, line N, or a time as m:ss (h:mm:ss from an hour on). Photo
rows draw no strip: the photo is the icon. Every indexed string (titles,
subtitles, snippets, hit labels, sheet names, answers, emoji) is drawn as plain
text; the snippet, which marks words with markup, escapes the text first.

Calm while typing: until typing has paused for 400 ms, responses only add.
New rows go on top of the rows on screen, a new answer replaces the card, and
a response without an answer keeps it; "Nothing on this computer matches"
does not appear. Once typing pauses, the latest final phase is shown as it
is, so rows may drop and the card may go; a final phase arriving after the
pause is shown as it is too. An instant phase always only adds (its final
phase follows). The selection stays on the same key, except that a new answer
(or a new value) takes it, so Enter and Ctrl+C act on the answer. The notice
never shows with an answer card or a local row.

←/→ step through hits in the order they are drawn (page, slide, line or time),
starting from the best one, so after Right `hit` is the index of the next
hit in the document, not necessarily 1.

Changed QML only takes effect after a shell restart (`omarchy-restart-shell`):
Qt keeps compiled components by URL, and the shell's plugin reload cannot
clear that cache (`Qt.clearComponentCache` does not exist in QML on Qt 6.11).

## Mock daemon

```
python3 ui/dev/mock_daemon.py [--sock PATH] [--thumbs DIR] [--log FILE] [--indexing SECONDS]
```

Python stdlib only; canned two-phase answers for `lease`, `novel` (EPUB chapters), `golden retriever`,
`awkward` (11 emoji, the picker setting after the ninth), `5'11" in cm`, `qwxz` (empty), `slow <query>` (final after 1.5 s),
app/setting prefixes, `sheet` (markup in every indexed string, its `<img>` pointing at 127.0.0.1:47806), and
`100k idr in usd`, `3pm sydney in jakarta` and `byzantine empire timeline` with every prefix answered as the
live daemon did (answers that come and go, `empty: true` beside an answer, rows that come and go); `--indexing` adds `progress` to every response for that many seconds. Point a dev shell at it with `SUPERSPACE_SOCK=PATH`
when the real daemon has the default socket. It refuses to start when something already listens
on the socket, and removes the socket on exit only if it is still its own.
