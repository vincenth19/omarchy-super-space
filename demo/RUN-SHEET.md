# Super Space run sheet (Tue 20 Oct, live from the Try Omarchy VM)

For the presenter only: it spells out every query, so keep it out of the
folders Super Space indexes. The demo queries are the first seven `demo`
cases in `tests/golden.json`, in this order; change both together. About 5
minutes: speak at about 140 words a minute, pause 2-3 s after every reveal,
one big visual per beat.

Earlier today: put the final build on the VM (and the Pi, the fallback), and
record the Claude step on the VM (Cmd+Shift+5): that recording is the backup.

## Pre-stage (30 min before, Wi-Fi on, in the VM)

1. Restart the VM, so its clock and exchange rates are current (`date`
   matches the Mac). On the Mac, `caffeinate -d`.
2. Clear the clipboard history of test copies: `omarchy-menu-clipboard`,
   Shift+Delete, confirm. Copies like "lease" or "100k idr in usd" would rank
   above the real results.
3. `superspace-setup`: registers Claude (MCP server and skill) and the agent
   watcher.
4. `claude`: log in, so it starts signed in.
5. `superspace-setup doctor`: every row ok.
6. `python3 ~/.config/omarchy/plugins/io.github.vincenth19.superspace/tests/run --demo --out /tmp/ss-bench`:
   the first seven rows (tonight, in order) ok; the rest are spares. It also
   warms every search. A FAIL row says what came back instead.
7. Do not disturb: Focus on the Mac, Super+Ctrl+, in the VM. Close every other
   window on both.
8. The backup recording is open in QuickTime on its first frame.

## On stage

Type at normal speed; results by meaning land once you pause. Ctrl+A then
typing replaces the query. ↓ picks the top row, then ←/→ step its sections or
pages; Enter opens at the picked one; Esc closes. (On v0.1.1: Tab, then ←/→.)

| Beat | Do, and the one visual | Say |
|---|---|---|
| **Open** 20 s | Super+Shift+F, `~/Pictures/Camera`: IMG_2041.jpg, IMG_2047.jpg... | "You remember what is in a file, never what you named it. The lease with the bond. The video where the dog swims. The memo where someone said what the venue costs. You can't find any of them." |
| **The turn** 30 s | Same window. Super+W when done. | "Files evolved. They used to be text. Now they are PDFs, photos, videos, voice memos, songs. Search did not: it still matches the letters of a file name, and nobody names a photo. IMG_2041 could be anything." |
| **Headline** 15 s | Super+Space: the empty box. Say it, pause 3 s. Then Wi-Fi off in the Mac menu bar. | "Super Space understands what you mean — on your computer, offline." ... "No internet from here on." |
| **Demo** 2.5 min | `golden retriever video`: the weekend video is the top row, both dog sections marked. ↓, ← to 0:10–0:30, → back to 0:45–0:59, Enter plays it there (q closes it). | "No file name says dog. It watched the video, and knows which parts have the dog." |
| | Super+Space, Ctrl+A, `apartment lease bond`: the lease, p. 3, "rental bond of $2,400". ↓, → to p. 4, ← back to p. 3, Enter opens it at p. 3 (Super+W closes it). | "Inside PDFs, Word, Excel and slides, and it opens at the page." |
| | Super+Space, Ctrl+A, `the venue costs five hundred dollars`: the 5 Oct voice memo, at 0:13. | "Spoken words are searchable." |
| | Ctrl+A, `songs about a breakup`: Keys on the Counter, Empty Side of the Bed. | "Found by their lyrics." |
| | Ctrl+A, `anjing`: the dog emoji, then the dog photos. | "Indonesian for dog. Any language." |
| | Ctrl+A, `100k idr in usd`: about 5.5 USD. Esc. | "And quick answers." |
| | The desktop. Pause 3 s. | "No file names. No folders. No internet. No cloud." |
| **One more thing** 60 s | Pause. Wi-Fi back on (your phone's hotspot beats venue Wi-Fi). Super+Enter, `claude`, ask "what is the bond on my apartment lease": it calls Super Space and answers $2,400, page 3 (about 20 s; talk over it). | "One more thing. Your AI agents get the same index: Claude doesn't grep through my folders, it asks Super Space, like you just did." |
| | Hold up the Raspberry Pi. Pause 3 s. | "Everything you just saw also runs on this — an $80 computer." |
| **Close** 30 s | The install command, typed large in the terminal, not run. | "You remember what is in a file, never what you named it. Now that is all you need to remember. One command: `omarchy plugin add https://github.com/vincenth19/omarchy-super-space --enable`. MIT licensed, and everything stays on your computer. Thank you." |

Spares (all in the `--demo` table): `where did we agree to hold the meetup`,
`pitch deck pricing` (slide 4), `golden retriever` (photos and the video),
`3pm sydney in jakarta`, `awkward`, `chromum`, `byzantine empire timeline`
(nothing on this computer, only the web row).

## If something goes wrong

- The box does not open: `omarchy-shell shell toggle io.github.vincenth19.superspace '{}'`.
- Only names and keywords come back: `systemctl --user restart superspace-embed`.
- Claude slow, offline or signed out: play the recording.
- The VM hangs: carry on from the same beat on the Pi.
