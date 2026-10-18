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
   once the cable is out.
2. Nothing else is busy: `pgrep -a llama-server` shows only ports 47801 and
   47802, `pgrep -a 'whisper-cli|ffmpeg'` shows nothing, and the first load
   average in `uptime` is under about 0.5.
3. `superspace-setup doctor`: every row ok.
4. `python3 ~/.config/omarchy/plugins/io.github.vincenth19.superspace/tests/run --demo --out /tmp/ss-bench`:
   one table, every row ok. A FAIL row says what came back instead; a FAIL on
   never-searched queries means something is eating the CPU (go back to 2).
   A WARN row is a result that only just clears its cutoff: it passes now but
   is worth a look.
5. Super+Space opens Super Space. The network cable is in.

## On stage

Type at normal speed. Rows keep their place while you type; results by meaning
land once you pause. Enter opens, Esc closes.

| # | Type | Show |
|---|---|---|
| 1 | Super+Space | It opens at once. Everything below runs on the Pi's CPU, nothing leaves it. |
| 2 | `apartment lease bond` | The lease PDF, best hit page 3. Enter opens it at page 3. |
| 3 | `where did we agree to hold the meetup` | The committee notes (.docx); its snippet names The Precinct. |
| 4 | `pitch deck pricing` | The Kopi Club deck, slide 4. |
| 5 | `quarterly sales spreadsheet` | The Bean There Q3 sales workbook. |
| 6 | `golden retriever` | The photos, and the weekend video with its moments (0:10-0:30, 0:45-0:59). ←/→ steps through the moments, Tab shows them all, Enter plays from the moment. |
| 7 | `anjing` | Indonesian for dog: the dog emoji first (that is their exact name), then the dog photos and the video. |
| 8 | `the venue costs five hundred dollars` | The voice memo of 5 Oct, at 0:13. Enter plays from there. Spoken words are searchable. |
| 9 | `songs about a breakup` | Empty Side of the Bed and Keys on the Counter, found by their lyrics. |
| 10 | `100k idr in usd`, then `3pm sydney in jakarta` | Answer cards (about 6 USD; 11 am). Ctrl+C copies the answer. |
| 11 | `wifi connect` | Omarchy's Network panel (Wi-Fi), a setting. |
| 12 | `awkward` | 😬. Enter pastes it into the window you came from. |
| 13 | `chromum` | Chromium, despite the typo. |
| 14 | `byzantine empire timeline` | "Nothing on this computer matches", and only the web row. It does not pad with near misses. |
| 15 | Terminal: `superspace search "apartment lease bond" --json` | The same rows for coding agents (also `superspace mcp`). |
| 16 | Pull the network cable, repeat 2 and 6 | It all still works offline. |

Spares, if there is time: `tasmania camping trip` (the itinerary PDF), `set up
a vpn on the raspberry pi` (the WireGuard notes).

## If something goes wrong

- Nothing opens: `systemctl --user status superspace` (or open it with
  `omarchy-shell shell toggle io.github.vincenth19.superspace '{}'`).
- Only names and keywords come back: the query server is down;
  `systemctl --user restart superspace-embed`.
- The projector fails: the 27 inch monitor on the side table.
