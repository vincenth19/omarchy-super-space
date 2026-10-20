# Super Space UI

`SuperSpace.qml` (repo root) is an Omarchy 4 `menu` plugin, kept loaded so it
opens instantly. `ui/ResultRow.qml` draws a result, `ui/AnswerCard.qml` an
instant answer. It talks to `superspace serve` over one socket,
`$XDG_RUNTIME_DIR/superspace.sock` or, as for the daemon, `$SUPERSPACE_SOCK`
(DESIGN.md "Protocol"). Every result,
answers included, runs through the daemon (`activate`, with `"alt": true` for
Ctrl+Enter); the UI itself only runs `wl-copy` for Ctrl+C, and `mpv --start=T`
(through `uwsm-app`, as the daemon runs it) for a click on a timeline away from
its sections, a time the protocol cannot name. Like Spotlight, it
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
of its own that hides the theme's (the box was unthemed white there), `Style.font.*` sizes, `Style.spacing.*` and
`Style.space()`, `Style.cornerRadius` (Hyprland's rounding), `Border.surfaceSpec`
for borders. The layer namespace is `omarchy-menu`, so a theme's rules for
menus (blur, no animation) apply. Secondary text is the text colour at the
opacities Omarchy's menu uses. Checked under Tokyo Night, Catppuccin Latte and
Basilica.

The font is the system's UI sans, `Qt.application.font.family` (the GTK font
under Omarchy's `QT_QPA_PLATFORMTHEME=gtk3`: Adwaita Sans on the VM and the
Pi; fontconfig's sans otherwise), with tabular figures for places and times.
Left of the query, where the rows have their icons, is Omarchy's mark (glyph
U+E900 of the `omarchy` font), dim while the field is empty; without that font
nothing is drawn there.

## Keys

Type to search · ↓/↑ or Ctrl+N/P choose the highlighted row, then move · Enter open (on an answer: copy) ·
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

The first ↓ or ↑ (or Ctrl+N/P) from the field stays on the highlighted row
and gives it ←/→ (the next ones move between rows): ←/→ then step that row's
parts instead of the cursor: its pages, slides, lines, sheets, video
and audio sections or the emoji of a strip, in drawn order, and Enter opens at
the picked one (on a row without parts ←/→ do nothing). → where the cursor
cannot go further (at the end of the text, nothing selected) does the same for
the selected row when it has parts, so typing then → steps the top row's pages
with no ↑/↓ first; elsewhere ←/→ move the cursor. Typing, Backspace,
Delete, paste, Home, End, Ctrl+A (Super+A) or moving the cursor any other way
(a click, Shift+←/→) give ←/→ back to the cursor; Esc still closes. Tab goes into the selected row's
sections: the picked part stays picked and every hit is listed with
its text; ←/→ step them, Enter opens at the picked one, Esc, Shift+Tab, typing
or moving the cursor go back to the field. `mode` in `state` is `"text"`
(←/→ move the cursor), `"rows"` (they step the chosen row's parts) or
`"sections"`. Tab on a row without hits does nothing. The picked hit is
always marked: it is where Enter opens. The key hints under the list follow
the mode ("↵ open    → pages …" while the selected row has parts, "← → pages
↵ open there …" once they step them).

The wheel moves the list by the system's scroll lines per notch (three lines
of 24 px at the theme's spacing scale: 78 px on both test machines), eased over
140 ms; a high-resolution wheel (the VM turns the Mac's trackpad into one) and a
touchpad move it by exactly their own amount. Nothing flings or overshoots.

## Test bench

```
omarchy-shell superspace open 'golden retriever'   # open if closed, set the query
omarchy-shell superspace key Down                  # Up Left Right Home End Enter Escape Tab shift+Tab Backspace,
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
keys do nothing over IPC: Left and Right move no cursor, though they step a
chosen row's parts, and Right the selected row's parts while the cursor is at
the end, as it is after `open`; Home and End only give ←/→ back to the cursor;
Backspace deletes the last character). `mode` is `"rows"` after ↑/↓, `"sections"`
after Tab (then `expanded` is true too), else `"text"`. `phase` is `"final"` once the latest final phase is shown as it is, else
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

A row's headline says why it matched. When a file, photo, video or recording
matched by what is in it (`why` is not `"name"`) and has text for it, the
headline is that passage in quotes: the snippet for the best hit, the picked
hit's own text once ←/→ pick another, from its sentence with the most of the
words the daemon marked in the snippet (after "…" when that is not the first),
each such word in bold to its end wherever it starts a word (weight, never
colour alone): "…Decision: **we agreed** to **hold** the October **meetup** at
The Precinct". A video
section's label ("dog swimming") is not quoted. The title and subtitle then
make the citation line under it ("lease-2026.pdf · ~/Documents"). Other rows
keep the title over the subtitle. A song matched by its lyrics shows them as
the quote; titles and subtitles are drawn as the daemon sends them. When ←/→
can bring a passage that needs two lines, the headline keeps two lines, so
stepping never moves the rows below.

At the right of each row is where Enter lands: the picked hit's place, in a
large tabular figure on the selected row and small on the others (the column
is as wide as the widest of the row's places drawn large, and as tall, on
every row, so neither selecting nor stepping reflows it), or the kind ("PDF",
"App") when there is no place. A sheet's place is drawn as "row N"; its chip
names the sheet, so sheet rows have chips even with one hit. Under it,
when the picked hit has a thumbnail (a video section, a copied image), its
frame. Hits are labelled p. N (ch. N in an EPUB, whose parts are chapters),
slide N, sheet · row N, line N, or a time range as m:ss–m:ss (h:mm:ss from an
hour on). With more than one (sheets always), pages, slides, lines and sheets are also numbered
chips (at least 28 × 24 px; the number, or the sheet and row) under the
citation, the picked one outlined in the accent and bold; clicking a chip opens
there. Photo rows have no frame: the photo is the icon. Video rows wear mpv's
icon, as audio rows do, not the first frame the daemon sends, which is
unreadable at icon size. Every indexed string
(titles, subtitles, snippets, hit labels, sheet names, answers, emoji) is drawn
as plain text; the quote escapes the text before it adds its bold.

Video and audio rows with a `duration` (seconds) draw it as a bar: each hit
is a segment from `t` to `t_end`, the picked one raised. Under the bar each
section is labelled with its time range at its start and the whole length at
the end, the picked one first and bold, then the others in drawn order, each
only where it overlaps no label already placed. The pointer over the bar draws
a line there, reads its time in the place column and shows the nearest
section's frame; a click within 4 px of a section plays it from its start (as
Enter), anywhere else plays from that time (`mpv --start`, above). ↓ then ←/→
(or Tab, then ←/→) step sections, Enter plays from the picked one. Without a
`duration`, sections are time chips.

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
phase follows). Until ↑/↓, → or Tab has chosen a row, every response puts the
selection on the top (the answer card, else the first row), so Enter and the
number of ↓ to a row do not depend on how fast responses came; a chosen row
keeps the selection by its key, except that a new answer (or a new value)
takes it, so Enter and Ctrl+C act on the answer. The notice
never shows with an answer card or a local row. Enter, Ctrl+Enter, Ctrl+1…9,
Ctrl+C, Tab, ↑/↓ and → at the end never act on a row kept from an earlier query: they end
the calm at once, so the latest final phase is shown as it is first, and until
it has arrived they wait for it (typing again, or losing or regaining the
daemon, drops them; with no daemon they act at once, on what is shown).
Plain ←/→ wait too behind a waiting ↑/↓, Ctrl+N/P, → or Tab, and Shift+Tab
and Esc behind a waiting Tab (those keys change what they mean), so they act on
the same rows; otherwise ←/→ go to the field at once and Esc closes at once,
dropping the waiting keys (Esc cancels a waiting Enter, Ctrl+1…9 or Ctrl+C).
With no daemon nothing waits, so Esc always closes.

←/→ step through hits in the order they are drawn (page, slide, line or
time), from the picked one, so `hit` is an index into `hits` (the best hit is
0), not a position: on a video whose best section is in the middle, → picks
the section after it in time, whatever its index.

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
