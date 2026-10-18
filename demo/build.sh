#!/usr/bin/env bash
# Build the demo corpus into demo/corpus/ (gitignored). Needs a Mac (macOS `say`),
# ffmpeg and uv; downloads about 80 MB once (cached in demo/corpus/.cache).
set -euo pipefail
cd "$(dirname "$0")"
uv run make_docs.py
uv run make_media.py
