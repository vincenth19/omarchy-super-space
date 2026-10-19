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
moment), named `Commons.Color` in the QML because Qt 6.12's QtQuick has a `Color`
of its own that hides the theme's (the box was unthemed white there), `Style.font.*` and `Style.font.menuFamily`, `Style.spacing.*` and
`Style.space()`, `Style.cornerRadius` (Hyprland's rounding), `Border.surfaceSpec`
for borders. The layer namespace is `omarchy-menu`, so a theme's rules for
menus (blur, no animation) apply. Secondary text is the text colour at the
opacities Omarchy's menu uses. Checked under Tokyo Night, Catppuccin Latte and
Basilica.

## Keys

Type to search · ↑/↓ or Ctrl+N/P move · Enter open (on an answer: copy) ·
Ctrl+Enter the alternative (show in folder, or copy) · Ctrl+1…9 open row N ·
Ctrl+C copy the answer, path or URL (copies the selected text if there is any) ·
Esc close.

The field edits like any text field: ←/→ move the cursor, Shift selects,
Ctrl+←/→ and Alt+←/→ jump words (Alt as on macOS: to the end of the next word),
Home/End and Ctrl+Home/End, Ctrl+A select all, Ctrl+Backspace and Alt+Backspace
delete the word before the cursor, Ctrl+U deletes to the start, Ctrl+X/C/V cut,
copy and paste a selection. Super (Cmd in the VM) has the macOS meaning where it
reaches the box: Super+←/→ the start or end, Super+Backspace delete to the
start, Super+A select all. Hyprland's own bindings win: on both test machines
Super+←/→ (focus), Super+Shift+←/→ and Super+Backspace never reach it, and the
VM's Omarchy turns Super+A/C/V/X into Ctrl+A/C/V/X, which do. (On the Pi, a
Super chord sent with wtype takes the keyboard from the box, so it closes.)

Tab goes into the selected row's sections: its pages, slides, lines or video
and audio sections, or the emoji of a strip. The first one in drawn order is
picked and every hit is listed with its text; ←/→ step them, Enter opens at
the picked one, Esc, Shift+Tab, typing or moving the cursor go back to the
field (`mode` in `state` is `"sections"` or `"text"`). Tab on a row without
hits does nothing. Outside the sections the picked hit is still marked: it is
where Enter opens.

The wheel moves the list by the system's scroll lines per notch (three lines
of 24 px at the theme's spacing scale: 78 px on both test machines), eased over
140 ms; a high-resolution wheel (the VM turns the Mac's trackpad into one) and a
touchpad move it by exactly their own amount. Nothing flings or overshoots.

## Test bench

```
omarchy-shell superspace open 'golden retriever'   # open if closed, set the query
omarchy-shell superspace key Down                  # Up Left Right Enter Escape Tab shift+Tab Backspace,
                                                   # ctrl+Enter ctrl+1 … ctrl+9 ctrl+n ctrl+p ctrl+c ctrl+u
                                                   # alt+Backspace super+Backspace super+a (modifiers: ctrl shift alt super)
omarchy-shell superspace state
```

(`omarchy-shell` is `qs ipc -n -p $OMARCHY_PATH/shell call -- superspace …`;
over SSH set `XDG_RUNTIME_DIR` and `OMARCHY_PATH`.) `state` returns:

```
{"open":true,"query":"lease","connected":true,"phase":"final",
 "selected":0,"selectedKey":"file:Documents/lease-2026.pdf","hit":0,"mode":"text","expanded":false,
 "empty":false,"answer":null,"filter":null,"rows":[{"key":"…","kind":"file","title":"…","hits":3},…],
 "activated":{"op":"activate","id":10,"key":"…","q":"lease","hit":1,"alt":false},
 "perf":[["instant",3],["final",4]],"setup":null,"askBind":false,
 "progress":{"done":1240,"total":2800,"stage":"photos"},
 "theme":{"background":"#1e1913","text":"#ebe1cc"}}
```

`key` sends only the keys the box handles itself (the field's own editing
keys, such as Left or Home, do nothing over IPC; Backspace deletes the last
character). `mode` is `"sections"` after Tab (then `expanded` is true too),
else `"text"`. `phase` is `"final"` once the latest final phase is shown as it is, else
`"instant"` (see typing below); `selected` indexes `rows` (-1 on the answer
card, where `selectedKey` is `"answer"`); `hit` indexes the selected row's `hits` (null without hits);
`activated` is the last `activate` sent; `perf` is the UI's own time per
applied response (ms, model update and layout) since it opened; `setup` is
setup's last progress line; `progress` is the latest response's indexing
progress (null when it had none); `filter` is the shown response's `filter`
(below); `theme` the menu colours the UI draws with.

Consecutive emoji rows are drawn as one strip of up to 8 (the rest are
dropped). In `state` a strip is one row: `key` and `kind` of its first emoji,
`hits` the number of emoji; `hit` is the picked emoji and `selectedKey` its
key, which Enter, Ctrl+1…9 and Ctrl+C use. The subtitle names the picked emoji.
Clicking an emoji pastes that emoji.

While a response carries `"progress": {done, total, stage}`, a quiet line
above the key hints says "Indexing · 1,240 of 2,800 · photos" (`stage`, what
is embedded now, verbatim); it goes with the first response without it.

Hits are labelled p. N (ch. N in an EPUB, whose parts are chapters), slide N,
sheet · row N, line N, or a time range as m:ss–m:ss (h:mm:ss from an hour on). Photo
rows draw no strip: the photo is the icon. Every indexed string (titles,
subtitles, snippets, hit labels, sheet names, answers, emoji) is drawn as plain
text; the snippet, which marks words with markup, escapes the text first.

Video and audio rows with a `duration` (seconds) draw it as a bar: each hit
is a segment from `t` to `t_end`, the picked one raised, and "N sections" at
the end when there is more than one. Under the bar, the picked section's text
(audio after its time range), then for video every section's thumbnail with its
time range and, when the hit has text (a label or what is said), that text on
one line. Tab, then ←/→ step sections, Enter plays from the picked one. Without a `duration`, video keeps the thumbnails and audio has time chips.

When a response has `"filter": [kinds]` (the kinds the query's own words
limit it to, e.g. "golden retriever photos and text"), a chip at the right of
the box names them ("Photos + Text"); editing the words changes it. Like the
answer card, while typing it can appear but not go; it goes once typing pauses.

Calm while typing: until typing has paused for 400 ms, responses only add.
New rows go on top of the rows on screen, a new answer replaces the card, and
a response without an answer keeps it; "Nothing on this computer matches"
does not appear. Once typing pauses, the latest final phase is shown as it
is, so rows may drop and the card may go; a final phase arriving after the
pause is shown as it is too. An instant phase always only adds (its final
phase follows). The selection stays on the same key, except that a new answer
(or a new value) takes it, so Enter and Ctrl+C act on the answer. The notice
never shows with an answer card or a local row.

In the sections ←/→ step through hits in the order they are drawn (page,
slide, line or time), from the first one drawn, so `hit` is an index into
`hits` (the best hit is 0), not a position: after Tab on a video whose best
section is in the middle, `hit` is that of its earliest section.

Changed QML only takes effect after a shell restart (`omarchy-restart-shell`):
Qt keeps compiled components by URL, and the shell's plugin reload cannot
clear that cache (`Qt.clearComponentCache` does not exist in QML on Qt 6.11).

## Mock daemon

```
python3 ui/dev/mock_daemon.py [--sock PATH] [--thumbs DIR] [--log FILE] [--indexing SECONDS]
```

Python stdlib only; canned two-phase answers for `lease`, `novel` (EPUB chapters), `golden retriever` (videos
with 6, 2 and 1 sections, audio with 3, each with a duration), any query plus `text`, `photos`, `video` or `audio`
(only those kinds, with `filter`), `awkward` (11 emoji, the picker setting after the ninth), `5'11" in cm`, `qwxz` (empty), `slow <query>` (final after 1.5 s),
app/setting prefixes, `sheet` (markup in every indexed string, its `<img>` pointing at 127.0.0.1:47806), and
`100k idr in usd`, `3pm sydney in jakarta` and `byzantine empire timeline` with every prefix answered as the
live daemon did (answers that come and go, `empty: true` beside an answer, rows that come and go); `--indexing` adds `progress` to every response for that many seconds. Point a dev shell at it with `SUPERSPACE_SOCK=PATH`
when the real daemon has the default socket. It refuses to start when something already listens
on the socket, and removes the socket on exit only if it is still its own.
