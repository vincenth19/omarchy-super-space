#!/usr/bin/env bash
# Copy the demo corpus into a home's Documents, Downloads, Music, Pictures and Videos.
#   demo/install-to-home.sh [TARGET_HOME]    default: ~/.cache/superspace-dev/home
# Existing files are kept; files with the same name are overwritten. The
# clipboard history fixture goes in only when TARGET_HOME is not your real home.
# Your real home also does not get the corpus's demo run sheet or the walkthrough
# video: they spell out demo queries, so they would match them on stage (the
# presenter's run sheet is demo/RUN-SHEET.md).
set -euo pipefail
src="$(cd "$(dirname "$0")" && pwd)/corpus"
dest="${1:-$HOME/.cache/superspace-dev/home}"
[[ -d $src/home ]] || { echo "no corpus at $src/home: run demo/build.sh first" >&2; exit 1; }
mkdir -p "$dest"
# Only the five folders: build tools sometimes leave hidden caches in the corpus.
folders=(Documents Downloads Music Pictures Videos)
for d in "${folders[@]}"; do [[ -d $src/home/$d ]] && cp -R "$src/home/$d" "$dest/"; done
if [[ $(cd "$dest" && pwd -P) != $(cd "$HOME" && pwd -P) ]]; then
  mkdir -p "$dest/.local/state/omarchy"
  cp "$src/clipboard-history.json" "$dest/.local/state/omarchy/clipboard-history.json"
else
  rm -f "$dest/Documents/Meetup/super-space-demo-run-sheet.md" "$dest/Videos/super-space-walkthrough.mp4"
fi
echo "installed $(cd "$src/home" && find "${folders[@]}" -type f 2>/dev/null | wc -l | tr -d ' ') files into $dest"
