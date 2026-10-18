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
so `omarchy plugin add … --enable` is the only command; this happens once per
plugin load, and never when the binary exists. Otherwise, with no daemon five
seconds after opening, and superspace.service not active
(a daemon that is merely slow is left alone), it starts the plugin's
`bin/superspace-setup --progress` as the systemd unit `superspace-setup`, so
nothing in the shell can kill it mid-download, and follows the JSON lines it
writes to `$XDG_RUNTIME_DIR/superspace-setup.jsonl` (a box opened again picks
a running setup back up). It shows each step and download progress, and on
`ask-bind` asks "Make Super+Space open Super Space?" (Enter yes, which runs
`superspace-setup bind`; Esc no; only while the query is empty), then setup's
"ready" line until the box closes.

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
moments (with fewer than two, they move the text cursor) · Tab list a row's
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
 "theme":{"background":"#1e1913","text":"#ebe1cc"}}
```

`selected` indexes `rows` (-1 on the answer card, where `selectedKey` is
`"answer"`); `hit` indexes the selected row's `hits` (null without hits);
`activated` is the last `activate` sent; `perf` is the UI's own time per
applied response (ms, model update and layout) since it opened; `setup` is
setup's last progress line; `theme` the menu colours the UI draws with.

Hits are labelled p. N (ch. N in an EPUB, whose parts are chapters), slide N,
sheet · row N, line N, or a time as m:ss (h:mm:ss from an hour on). While
typing, the list never shrinks: an instant phase's rows go on top of the last
final phase's other rows (and its answer stays when the instant phase has
none) until the next final phase replaces the whole list. The selection stays
on the same key, and "Nothing on this computer matches" shows only after a
final phase, until a row arrives.

←/→ step through hits in the order they are drawn (page, slide, line or time),
starting from the best one, so after Right `hit` is the index of the next
hit in the document, not necessarily 1.

Changed QML only takes effect after a shell restart (`omarchy-restart-shell`):
Qt keeps compiled components by URL, and the shell's plugin reload cannot
clear that cache (`Qt.clearComponentCache` does not exist in QML on Qt 6.11).

## Mock daemon

```
python3 ui/dev/mock_daemon.py [--sock PATH] [--thumbs DIR] [--log FILE]
```

Python stdlib only; canned two-phase answers for `lease`, `novel` (EPUB chapters), `golden retriever`,
`awkward`, `5'11" in cm`, `qwxz` (empty), `slow <query>` (final after 1.5 s)
and app/setting prefixes. Point a dev shell at it with `SUPERSPACE_SOCK=PATH`
when the real daemon has the default socket. It refuses to start when something already listens
on the socket, and removes the socket on exit only if it is still its own.
