# Super Space demo run sheet (Tue 20 Oct, Raspberry Pi 5)

For the presenter only. It is not part of the demo corpus: it holds every demo
query, so it would match them on stage. `demo/install-to-home.sh ~` leaves it out,
and also `~/Videos/super-space-walkthrough.mp4`, whose slides spell out demo
queries (delete it if an older install put it there).
The queries below are the `demo` cases in `tests/golden.json`, in the same order;
change both together.

## Before going on stage

0. An hour before: reboot the Pi and leave it alone to settle (the index
   catches up, nothing else runs). A reinstall needs about 20 minutes more.
1. `superspace status`: `indexing` false, `media_queue` 0, `pending` 0 and
   `progress` null. `rates` is the last ECB business day (all of Tuesday in
   Brisbane that is Monday 19 Oct); currency answers use these cached rates
   when Wi-Fi is off.
2. Nothing else is busy: `pgrep -a llama-server` shows only ports 47801 and
   47802, `pgrep -a 'whisper-cli|ffmpeg'` shows nothing, and the first load
   average in `uptime` is under about 0.5.
3. `superspace-setup doctor`: every row ok.
4. `python3 ~/.config/omarchy/plugins/io.github.vincenth19.superspace/tests/run --demo --out /tmp/ss-bench`:
   one table, every row ok. A FAIL row says what came back instead; a FAIL on
   never-searched queries means something is eating the CPU (go back to 2).
   A WARN row is a result that only just clears its cutoff: it passes now but
   is worth a look.
5. Super+Space opens Super Space.

## On stage (about 4 minutes)

Type at normal speed. Rows keep their place while you type; results by meaning
land once you pause. Enter opens, Esc closes. Optional: turn Wi-Fi off for
steps 2-9 to prove it is offline (currency uses cached rates), and back on
before step 10, because Claude needs the internet.

| # | Type | Show | Say |
|---|---|---|---|
| 1 | Super+Space | Opens at once. | "Spotlight that understands meaning, on a Raspberry Pi, offline." |
| 2 | `apartment lease bond` | The lease PDF, page 3 highlighted. Enter opens it at page 3. | Text inside PDFs, Word, Excel, PowerPoint. |
| 3 | `where did we agree to hold the meetup` | The committee notes; the snippet shows "…hold the October meetup at The Precinct". | A question, not keywords. |
| 4 | `golden retriever` | Dog photos, and the weekend video with its dog moment (0:45). Enter plays from there. | One model reads text, photos and video. |
| 5 | `the venue costs five hundred dollars` | The 5 Oct voice memo, at 0:13. | Spoken words are searchable. |
| 6 | `songs about a breakup` | Empty Side of the Bed and Keys on the Counter. | Found by their lyrics. |
| 7 | `anjing` | Dog emoji, then the dog photos. | Indonesian for dog: any language. |
| 8 | `100k idr in usd`, `3pm sydney in jakarta`, `awkward`, `chromum` | 5.55 USD card; 11:00 AM card; 😬; Chromium despite the typo. | Quick answers, emoji, typos. |
| 9 | `byzantine empire timeline` | "Nothing on this computer matches", only the web row, last. | It never pads with near misses, and never puts the web above your files. |
| 10 | Terminal: `claude`, then "find my apartment lease and tell me the bond amount" | Claude calls Super Space (MCP) and answers $2,400, page 3 (about 20 s). | Agents use the same index instead of ls and grep. |

Close: one command to install (`omarchy plugin add https://github.com/vincenth19/omarchy-super-space --enable`), MIT, everything local.

Spares, if there is time: `pitch deck pricing` (slide 4), `quarterly sales
spreadsheet`, `wifi connect` (Omarchy's Network panel), `tasmania camping
trip`, `set up a vpn on the raspberry pi`, or `superspace search "apartment
lease bond" --json` in a terminal if Claude is slow.

## If something goes wrong

- Nothing opens: `systemctl --user status superspace` (or open it with
  `omarchy-shell shell toggle io.github.vincenth19.superspace '{}'`).
- Only names and keywords come back: the query server is down;
  `systemctl --user restart superspace-embed`.
- Claude slow or offline: show `superspace search "apartment lease bond" --json` instead.
