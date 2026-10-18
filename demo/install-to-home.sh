#!/usr/bin/env bash
# Copy the demo corpus into a home's Documents, Downloads, Music, Pictures and Videos.
#   demo/install-to-home.sh [TARGET_HOME]    default: ~/.cache/superspace-dev/home
# Existing files are kept; files with the same name are overwritten. The
# clipboard history fixture goes in only when TARGET_HOME is not your real home.
# Your real home also does not get the corpus's demo run sheet: it holds every
# demo query, so it would match them on stage (the presenter's is demo/RUN-SHEET.md).
set -euo pipefail
src="$(cd "$(dirname "$0")" && pwd)/corpus"
dest="${1:-$HOME/.cache/superspace-dev/home}"
[[ -d $src/home ]] || { echo "no corpus at $src/home: run demo/build.sh first" >&2; exit 1; }
mkdir -p "$dest"
cp -R "$src/home/." "$dest/"
if [[ $(cd "$dest" && pwd -P) != $(cd "$HOME" && pwd -P) ]]; then
  mkdir -p "$dest/.local/state/omarchy"
  cp "$src/clipboard-history.json" "$dest/.local/state/omarchy/clipboard-history.json"
else
  rm -f "$dest/Documents/Meetup/super-space-demo-run-sheet.md"
fi
echo "installed $(find "$src/home" -type f | wc -l | tr -d ' ') files into $dest"
