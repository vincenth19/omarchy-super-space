#!/bin/bash
# bin/superspace-setup end to end on arm64 Linux, with real downloads and
# stubbed systemctl, hyprctl, gum and agent CLIs (see README.md here). Only in
# a container or CI: it serves fake model servers on ports 47801 and 47802.
# WORK is kept between runs: the fake HOME, and keep/ so the models survive.
set -uo pipefail
REPO=$(cd "$(dirname "$0")/../.." && pwd)
WORK=${WORK:-/work}
export HOME=$WORK/home
P=$HOME/.config/omarchy/plugins/io.github.vincenth19.superspace
STUBS=/tmp/stubs
LOG=/tmp/calls.log
FIX=$REPO/tests/setup/fixtures
CURL=$(command -v curl)
pass=0 fail=0
ok() { if (eval "$2"); then echo "PASS $1"; ((pass++)); else echo "FAIL $1"; ((fail++)); fi; }

# --- stubs: every outside command the setup touches, logged ---
mkdir -p "$STUBS"
stub() { printf '#!/bin/bash\necho "%s $*" >>%s\n%s\n' "$1" "$LOG" "${2:-}" >"$STUBS/$1"; chmod +x "$STUBS/$1"; }
stub systemctl '[[ $* == *is-active* ]] && exit "${ACTIVE:-3}"; exit 0'
stub hyprctl 'exit 0'
stub gum 'exit "${GUM_ANSWER:-1}"'
stub claude 'exit 0'                       # a real install
stub copilot 'exit 0'
stub opencode 'exit 0'
# Omarchy install-on-first-run stubs: codex is installed behind it (mise knows it), gemini is not.
for a in codex gemini; do
  printf '#!/bin/bash\necho "%s RAN-STUB $*" >>%s\nexport MISE_MINIMUM_RELEASE_AGE=0\nmise use -g --quiet "%s" || exit 1\nexec mise x "%s" -- "%s" "$@"\n' "$a" "$LOG" "$a" "$a" "$a" >"$STUBS/$a"
  chmod +x "$STUBS/$a"
done
printf '#!/bin/bash\necho "mise $*" >>%s\n[[ ($1 == which && $2 == codex) || ($1 == use && $4 == codex) ]] && exit 0\n[[ $1 == x && $2 == codex ]] && { shift 4; echo "codex-real $*" >>%s; exit 0; }\nexit 1\n' "$LOG" "$LOG" >"$STUBS/mise"
chmod +x "$STUBS/mise"
printf '#!/bin/bash\nif [[ " $* " == *" -o "* ]]; then %s -w "curl-downloaded %%{size_download}\\n" "$@" >>%s; else exec %s "$@"; fi\n' "$CURL" "$LOG" "$CURL" >"$STUBS/curl"
chmod +x "$STUBS/curl"
export PATH=$STUBS:$PATH

# fake superspace build
cat >/tmp/superspace-dev <<'EOF'
#!/bin/sh
case $1 in
'') echo "superspace: usage: superspace serve | search ..." >&2; exit 1 ;;
search) [ -e /tmp/answering ] || { echo "superspace: cannot reach the daemon" >&2; exit 1; }; echo '{"rows":[{"key":"web:doctor"}],"empty":true}' ;;
*) echo superspace-dev "$@" ;;
esac
EOF
chmod +x /tmp/superspace-dev

fresh_home() {
  # keep downloaded models between test sessions, wipe everything else
  mkdir -p $WORK/keep
  [[ -d $HOME/.local/share/superspace/models ]] && cp -rn "$HOME/.local/share/superspace/models/." $WORK/keep/ 2>/dev/null
  rm -rf "$HOME"
  mkdir -p "$HOME/.config/hypr" "$HOME/.config/opencode" "$(dirname "$P")" \
    "$HOME/.agents/skills" "$HOME/.claude/skills" "$HOME/.codex/skills" "$HOME/.pi/agent/skills" "$HOME/.hermes/skills" \
    "$HOME/.hermes/profiles/james/skills" "$HOME/.config/superspace"
  cp "$FIX/bindings.lua" "$HOME/.config/hypr/bindings.lua"
  cp "$FIX/opencode.json" "$HOME/.config/opencode/opencode.json"
  echo 'prefer = ["keep me"]' >"$HOME/.config/superspace/preferences.toml"
  mkdir -p "$HOME/.claude/skills/omarchy"   # an unrelated skill that must survive
  # the plugin as `omarchy plugin add` leaves it: a git clone, no symlinks
  mkdir -p "$P" && tar -C "$REPO" --exclude=./target --exclude=./.git --exclude=./demo/corpus -cf - . | tar -C "$P" -xf -
  if [[ "${KEEP_MODELS:-1}" == 1 && -n "$(ls $WORK/keep 2>/dev/null)" ]]; then
    mkdir -p "$HOME/.local/share/superspace/models" && cp -r $WORK/keep/. "$HOME/.local/share/superspace/models/"
  fi
}

SETUP=$P/bin/superspace-setup
S=$HOME/.local/share/superspace
fresh_home
: >"$LOG"

echo "== symlinks in plugin folder (must be none)"
ok "no symlinks in plugin" '[[ -z $(find "$P" -type l -print -quit) ]]'

echo "== run 1"
t0=$(date +%s)
"$SETUP" --yes --binary /tmp/superspace-dev 2>&1 | tr '\r' '\n' | grep -v '^#\|^ *$\|%$' | tee /tmp/run1.out
rc1=${PIPESTATUS[0]}
echo "run 1 took $(( $(date +%s) - t0 )) s"
ok "run 1 exit 0" '[[ $rc1 == 0 ]]'
mkdir -p $WORK/keep && cp -rn "$S/models/." $WORK/keep/
ok "binary installed" '[[ -x $S/bin/superspace && $($S/bin/superspace x) == "superspace-dev x" ]]'
ok "llama-server present + build stamp" '[[ -x $S/llama/llama-server && $(cat $S/llama/.build) == b11718 ]]'
ok "llama-server runs (--version)" '$S/llama/llama-server --version 2>&1 | grep -q "version: 11718\|b11718\|11718"'
ok "no tarball left" '[[ -z $(ls $S/*.tar.gz $S/*.part $S/models/*.part 2>/dev/null) ]]'
ok "model sha256" 'echo "2188ac1deca4b77dffefd603c2776a9d76d9d74ec01841392982ebb840b09135  $S/models/embeddinggemma-2-Q8_0.gguf" | sha256sum -c --status'
ok "mmproj sha256" 'echo "c4a8a52691ecef40618438928bdf9e68379b854e24166f292592353db0aab64f  $S/models/mmproj-embeddinggemma-2-Q8_0.gguf" | sha256sum -c --status'
ok "model sizes" '[[ $(stat -c %s $S/models/embeddinggemma-2-Q8_0.gguf) == 309855456 && $(stat -c %s $S/models/mmproj-embeddinggemma-2-Q8_0.gguf) == 554821024 ]]'
ok "whisper build stamp + cli" '[[ -x $S/whisper/whisper-cli && $(cat $S/whisper/.build) == b5454 ]]'
ok "whisper libs beside cli, none missing" '[[ -e $S/whisper/libwhisper.so.1 ]] && ! ldd $S/whisper/whisper-cli | grep -q "not found"'
ok "whisper-cli -h exits 0" '$S/whisper/whisper-cli -h >/dev/null 2>&1'
ok "whisper model sha256" 'echo "c577b9a86e7e048a0b7eada054f4dd79a56bbfa911fbdacf900ac5b567cbb7d9  $S/models/ggml-base-q8_0.bin" | sha256sum -c --status'
ok "vad model sha256" 'echo "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987  $S/models/ggml-silero-v6.2.0.bin" | sha256sum -c --status'
"$CURL" -fsSL -o /tmp/jfk.wav https://raw.githubusercontent.com/ggml-org/whisper.cpp/b5454/samples/jfk.wav
t0=$(date +%s%N)
$S/whisper/whisper-cli -m $S/models/ggml-base-q8_0.bin -f /tmp/jfk.wav -t 4 -l auto -bs 1 -bo 1 -sns -np -oj --vad -vt 0.1 -vm $S/models/ggml-silero-v6.2.0.bin -of /tmp/stt >/dev/null 2>&1
echo "whisper on 11 s of speech: $(( ($(date +%s%N) - t0) / 1000000 )) ms: $(jq -r '[.transcription[].text] | join("")' /tmp/stt.json 2>/dev/null)"
ok "whisper transcribes the installed way" 'jq -r "[.transcription[].text] | join(\"\")" /tmp/stt.json | grep -qi "what your country"'
ok "~/.local/bin/superspace -> share bin" '[[ $(readlink $HOME/.local/bin/superspace) == $S/bin/superspace ]]'
ok "~/.local/bin/superspace-setup -> plugin" '[[ $(readlink $HOME/.local/bin/superspace-setup) == $P/bin/superspace-setup ]]'
ok "units linked from plugin" 'for u in superspace superspace-embed superspace-embed-index; do grep -qx "systemctl --user -q link --force $P/systemd/$u.service" $LOG || exit 1; done'
ok "enable + restart" 'grep -qx "systemctl --user -q enable superspace.service" $LOG && grep -qx "systemctl --user restart superspace.service superspace-embed.service superspace-embed-index.service" $LOG'
ok "skill linked in 6 dirs" 'for d in .agents/skills .claude/skills .codex/skills .pi/agent/skills .hermes/skills .hermes/profiles/james/skills; do [[ $(readlink $HOME/$d/superspace) == $P/skills/superspace ]] || exit 1; done'
ok "unrelated skill untouched" '[[ -d $HOME/.claude/skills/omarchy && ! -L $HOME/.claude/skills/omarchy ]]'
ok "claude mcp add" 'grep -qx "claude mcp add --scope user superspace -- $HOME/.local/bin/superspace mcp" $LOG'
ok "copilot mcp add" 'grep -qx "copilot mcp add superspace -- $HOME/.local/bin/superspace mcp" $LOG'
ok "codex (installed behind stub) mcp add" 'grep -qx "codex-real mcp add superspace -- $HOME/.local/bin/superspace mcp" $LOG'
ok "gemini stub NOT run (would install it)" '! grep -q "^gemini" $LOG'
ok "opencode.json merged, original keys kept" 'jq -e ".theme == \"system\" and .autoupdate == false and .mcp.superspace.type == \"local\" and .mcp.superspace.command == [\"$HOME/.local/bin/superspace\",\"mcp\"] and .mcp.superspace.enabled == true" $HOME/.config/opencode/opencode.json >/dev/null'
ok "opencode backup = original" 'cmp -s $HOME/.config/opencode/opencode.json.bak-superspace $FIX/opencode.json'
ok "binding block appended once" '[[ $(grep -c "^-- <<< superspace-setup$" $HOME/.config/hypr/bindings.lua) == 1 ]]'
ok "binding text" 'tail -3 $HOME/.config/hypr/bindings.lua | grep -qxF "o.bind(\"SUPER + SPACE\", \"Super Space\", \"omarchy-shell shell toggle io.github.vincenth19.superspace \\\"{}\\\"\")"'
ok "unbind before bind" 'tail -3 $HOME/.config/hypr/bindings.lua | head -1 | grep -qxF "hl.unbind(\"SUPER + SPACE\")"'
ok "user lines unchanged (prefix identical)" 'cmp -s <(head -c $(stat -c %s $FIX/bindings.lua) $HOME/.config/hypr/bindings.lua) $FIX/bindings.lua'
ok "hyprctl reload" 'grep -qx "hyprctl reload" $LOG'
echo "--- bindings tail:"; tail -5 "$HOME/.config/hypr/bindings.lua"

echo "== run 2 (idempotent)"
cp "$HOME/.config/hypr/bindings.lua" /tmp/b1; cp "$HOME/.config/opencode/opencode.json" /tmp/o1
: >"$LOG"
t0=$(date +%s)
out2=$("$SETUP" --yes --binary /tmp/superspace-dev 2>&1); rc2=$?
echo "$out2"
echo "run 2 took $(( $(date +%s) - t0 )) s"
ok "run 2 exit 0" '[[ $rc2 == 0 ]]'
ok "run 2 downloads nothing" '[[ $out2 != *Downloading* ]]'
ok "bindings unchanged" 'cmp -s /tmp/b1 $HOME/.config/hypr/bindings.lua'
ok "opencode.json unchanged" 'cmp -s /tmp/o1 $HOME/.config/opencode/opencode.json'
ok "no hyprctl reload when already bound" '! grep -q hyprctl $LOG'
ok "claude re-registered via remove+add" 'grep -qx "claude mcp remove --scope user superspace" $LOG && grep -qx "claude mcp add --scope user superspace -- $HOME/.local/bin/superspace mcp" $LOG'

echo "== a second setup waits for the one running"
flock "$S/.setup.lock" sleep 3 & sleep 0.5
t0=$(date +%s)
outl=$("$SETUP" --yes --binary /tmp/superspace-dev 2>&1); rcl=$?
ok "second setup waited, then finished" '[[ $rcl == 0 && $(( $(date +%s) - t0 )) -ge 2 && $outl == *"Another setup is running"* ]]'

echo "== doctor with nothing running"
"$SETUP" doctor; rcs=$?
ok "doctor: no servers -> exit 1" '[[ $rcs == 1 ]]'
ok "old status command is gone" '! "$SETUP" status >/dev/null 2>&1'

echo "== resume a half-downloaded model"
M=$S/models/embeddinggemma-2-Q8_0.gguf
head -c 100000000 "$M" >"$M.part" && rm "$M"
: >"$LOG"
out3=$("$SETUP" --yes --binary /tmp/superspace-dev 2>&1 | tr '\r' '\n'); rc3=$?
ok "resume exit 0" '[[ $rc3 == 0 ]]'
ok "curl resumed: fetched only the rest" 'grep -qx "curl-downloaded $((309855456 - 100000000))" $LOG'
ok "resumed model sha256" 'echo "2188ac1deca4b77dffefd603c2776a9d76d9d74ec01841392982ebb840b09135  $M" | sha256sum -c --status'

echo "== complete .part left by an interrupted run (download done, not moved)"
mv "$M" "$M.part"
out4=$("$SETUP" --yes --binary /tmp/superspace-dev 2>&1); rc4=$?
ok "complete .part accepted without curl" '[[ $rc4 == 0 && -f $M && ! -f $M.part ]]'

echo "== corrupt partial llama download"
rm "$S/llama/.build"
T=$S/llama-b11718-bin-ubuntu-arm64.tar.gz
head -c 1000000 /dev/urandom >"$T.part"
out5=$("$SETUP" --yes --binary /tmp/superspace-dev 2>&1); rc5=$?
ok "corrupt -> fails with checksum message" '[[ $rc5 != 0 && $out5 == *"failed its checksum; run setup again"* ]]'
ok "corrupt .part deleted" '[[ ! -e $T.part ]]'
out6=$("$SETUP" --yes --binary /tmp/superspace-dev 2>&1); rc6=$?
ok "rerun after corrupt succeeds" '[[ $rc6 == 0 && $(cat $S/llama/.build) == b11718 && -x $S/llama/llama-server ]]'

echo "== declining the binding (no --yes, gum says no)"
"$SETUP" uninstall >/dev/null 2>&1
KEEP_MODELS=1 fresh_home; : >"$LOG"
GUM_ANSWER=1 script -qec "$SETUP --binary /tmp/superspace-dev" /dev/null >/tmp/run7.out 2>&1; rc7=$?
ok "declined: exit 0" '[[ $rc7 == 0 ]]'
ok "declined: gum asked" 'grep -q "^gum confirm" $LOG'
ok "declined: bindings untouched" 'cmp -s $FIX/bindings.lua $HOME/.config/hypr/bindings.lua'
: >"$LOG"
"$SETUP" --binary /tmp/superspace-dev </dev/null >/tmp/run8.out 2>&1; rc8=$?
ok "no TTY, no --yes: not asked, untouched" '[[ $rc8 == 0 ]] && ! grep -q "^gum" $LOG && cmp -s $FIX/bindings.lua $HOME/.config/hypr/bindings.lua'
: >"$LOG"
GUM_ANSWER=0 script -qec "$SETUP --binary /tmp/superspace-dev" /dev/null >/tmp/run9.out 2>&1
ok "accepted via gum: bound" 'grep -qx -- "-- <<< superspace-setup" $HOME/.config/hypr/bindings.lua'

echo "== release binary path (local server standing in for GitHub releases)"
mkdir -p /tmp/rel/v0.1.0 && cd /tmp/rel
python3 -m http.server 8765 --bind 127.0.0.1 >/dev/null 2>&1 &
HTTP=$!; cd /; sleep 1
sed 's#https://github.com/$REPO/releases/download#http://127.0.0.1:8765#' "$SETUP" >"$P/bin/setup-rel" && chmod +x "$P/bin/setup-rel"
out=$("$P/bin/setup-rel" --yes 2>&1); rc=$?
ok "release missing -> clear error" '[[ $rc -ne 0 && $out == *"no superspace v0.1.0 release for arm64 yet"* ]]'
printf '#!/bin/sh\necho release-one\n' >/tmp/rel/v0.1.0/superspace-linux-arm64
(cd /tmp/rel/v0.1.0 && sha256sum superspace-linux-arm64 >superspace-linux-arm64.sha256)
: >"$LOG"
"$P/bin/setup-rel" --yes >/tmp/rel1.out 2>&1; rcr=$?
ok "release: installs downloaded binary" '[[ $rcr == 0 && $($S/bin/superspace) == release-one ]]'
ok "release: binary fetched" 'grep -q "curl-downloaded" $LOG'
: >"$LOG"
"$P/bin/setup-rel" --yes >/tmp/rel2.out 2>&1
ok "release: same checksum -> not fetched again" '! grep -q "curl-downloaded" $LOG'
printf '#!/bin/sh\necho release-two\n' >/tmp/rel/v0.1.0/superspace-linux-arm64
(cd /tmp/rel/v0.1.0 && sha256sum superspace-linux-arm64 >superspace-linux-arm64.sha256)
"$P/bin/setup-rel" --yes >/tmp/rel3.out 2>&1
ok "release: new checksum -> replaced" '[[ $($S/bin/superspace) == release-two ]]'
printf "tampered\n" >/tmp/rel/v0.1.0/superspace-linux-arm64; rm -f "$S/bin/superspace"
"$P/bin/setup-rel" --yes >/tmp/rel4.out 2>&1; rct=$?
ok "release: tampered binary refused" '[[ $rct != 0 ]] && grep -q "failed its checksum" /tmp/rel4.out'
kill $HTTP; rm -f "$P/bin/setup-rel"
"$SETUP" --yes --binary /tmp/superspace-dev >/dev/null 2>&1

echo "== --progress (JSON lines) with a fresh llama download, not bound"
"$SETUP" unbind >/dev/null; rm -rf "$S/llama" "$S/whisper" "$S/models/ggml-base-q8_0.bin" $WORK/keep/ggml-base-q8_0.bin
"$SETUP" --progress --binary /tmp/superspace-dev </dev/null >/tmp/prog.out 2>/tmp/prog.err; rcp=$?
head -3 /tmp/prog.out; echo ...; tail -6 /tmp/prog.out
ok "progress exit 0" '[[ $rcp == 0 ]]'
ok "every stdout line is JSON with a step" 'jq -e -s "all(.[]; has(\"step\"))" /tmp/prog.out >/dev/null'
ok "download progress lines with total" 'jq -e -s "any(.[]; .file == \"llama-b11718-bin-ubuntu-arm64.tar.gz\" and .total == 14020840)" /tmp/prog.out >/dev/null'
ok "whisper tarball and model progress with totals" 'jq -e -s "any(.[]; .file == \"whisper-bin-ubuntu-arm64.tar.gz\" and .total == 4608377) and any(.[]; .file == \"ggml-base-q8_0.bin\" and .total == 81768585 and .done == 81768585)" /tmp/prog.out >/dev/null'
ok "whisper back after progress run" '[[ -x $S/whisper/whisper-cli && -s $S/models/ggml-base-q8_0.bin ]]'
ok "ask-bind emitted, then done last" 'jq -e -s "any(.[]; .step == \"ask-bind\") and (last.step == \"ready\")" /tmp/prog.out >/dev/null'
ok "nothing on stderr" '[[ ! -s /tmp/prog.err ]]'
"$SETUP" --progress --nope >/tmp/prog3.out 2>/dev/null
ok "unknown arg after --progress: error as JSON" 'jq -e ".step == \"error\"" /tmp/prog3.out >/dev/null'
: >"$LOG"
"$SETUP" bind >/tmp/bind.out; rcb=$?
ok "bind subcommand binds" '[[ $rcb == 0 ]] && grep -qx -- "-- <<< superspace-setup" $HOME/.config/hypr/bindings.lua && grep -qx "hyprctl reload" $LOG'
"$SETUP" bind >/dev/null
ok "bind twice: one block" '[[ $(grep -c "^-- <<< superspace-setup$" $HOME/.config/hypr/bindings.lua) == 1 ]]'
"$SETUP" unbind >/dev/null
ok "unbind restores byte-identical" 'cmp -s $FIX/bindings.lua $HOME/.config/hypr/bindings.lua'
"$SETUP" unbind >/dev/null; rcu2=$?
ok "unbind twice ok" '[[ $rcu2 == 0 ]] && cmp -s $FIX/bindings.lua $HOME/.config/hypr/bindings.lua'
"$SETUP" bind >/dev/null

echo "== --progress with the daemon up: ready, nothing restarted"
touch /tmp/answering; : >"$LOG"
t0=$(date +%s%N)
ACTIVE=0 "$SETUP" --progress </dev/null >/tmp/up.out 2>/tmp/up.err; rcup=$?
echo "took $(( ($(date +%s%N) - t0) / 1000000 )) ms: $(cat /tmp/up.out)"
ok "up: exit 0, one ready line" '[[ $rcup == 0 && $(wc -l </tmp/up.out) == 1 ]] && jq -e ".step == \"ready\"" /tmp/up.out >/dev/null'
ok "up: no restart, no link, no download" '! grep -qE "restart|link|enable|curl-downloaded" $LOG'
ok "up: nothing on stderr" '[[ ! -s /tmp/up.err ]]'
: >"$LOG"
"$SETUP" --binary /tmp/superspace-dev </dev/null >/dev/null 2>&1
ok "without --progress a running daemon is still restarted" 'grep -q "systemctl --user restart" $LOG'
rm /tmp/answering; : >"$LOG"
t0=$(date +%s)
ACTIVE=0 "$SETUP" --progress --binary /tmp/superspace-dev </dev/null >/tmp/up2.out 2>&1; rcup2=$?
echo "active but not answering took $(( $(date +%s) - t0 )) s"
ok "active but not answering: waits, then full setup" '[[ $rcup2 == 0 ]] && grep -q "systemctl --user restart" $LOG && jq -e -s "last.step == \"ready\"" /tmp/up2.out >/dev/null'

echo "== doctor"
mkdir -p /tmp/www && echo '{"status":"ok"}' >/tmp/www/health
(cd /tmp/www && exec python3 -m http.server 47801 --bind 127.0.0.1) >/dev/null 2>&1 & Q=$!
(cd /tmp/www && exec python3 -m http.server 47802 --bind 127.0.0.1) >/dev/null 2>&1 & I=$!
sleep 1; touch /tmp/answering
SSB=$HOME/.local/bin/superspace
# what the real CLIs write (checked with claude, codex and gemini in a fake HOME)
echo "{\"mcpServers\":{\"superspace\":{\"command\":\"$SSB\",\"args\":[\"mcp\"]}}}" >$HOME/.claude.json
mkdir -p $HOME/.codex $HOME/.copilot && printf '[mcp_servers.superspace]\ncommand = "%s"\nargs = ["mcp"]\n' "$SSB" >$HOME/.codex/config.toml
cp $HOME/.claude.json $HOME/.copilot/mcp-config.json
"$SETUP" doctor >/tmp/doc1.out 2>&1; rcd1=$?
cat /tmp/doc1.out
ok "doctor all ok: exit 0" '[[ $rcd1 == 0 ]] && ! grep -q FAIL /tmp/doc1.out'
ok "doctor rows" '[[ $(grep -c "^  ok" /tmp/doc1.out) == 10 ]] && grep -q "MCP server registered with opencode" /tmp/doc1.out && ! grep -q gemini /tmp/doc1.out'
kill $I; rm /tmp/answering $HOME/.claude.json; mv $S/whisper/whisper-cli /tmp/wcli; rm $HOME/.claude/skills/superspace
"$SETUP" doctor >/tmp/doc2.out 2>&1; rcd2=$?
cat /tmp/doc2.out
ok "doctor failures: exit 1" '[[ $rcd2 == 1 ]] && grep -q "To repair: superspace-setup" /tmp/doc2.out'
ok "doctor names each failure" 'for l in "index model server" "daemon answers" "speech to text" "agent skill" "MCP server registered with claude"; do grep -q "FAIL  $l" /tmp/doc2.out || exit 1; done'
ok "doctor keeps the rest ok" 'for l in "superspace runs" "query model server" "MCP server registered with codex"; do grep -q "ok    $l" /tmp/doc2.out || exit 1; done'
kill $Q; mv /tmp/wcli $S/whisper/whisper-cli
"$SETUP" --binary /tmp/superspace-dev >/dev/null 2>&1

echo "== uninstall"
: >"$LOG"
"$SETUP" uninstall; rcu=$?
ok "uninstall exit 0" '[[ $rcu == 0 ]]'
ok "bindings restored byte-identical" 'cmp -s $FIX/bindings.lua $HOME/.config/hypr/bindings.lua'
ok "opencode.json superspace removed, rest kept" 'jq -e "(.mcp.superspace == null) and .theme == \"system\"" $HOME/.config/opencode/opencode.json >/dev/null'
ok "skills unlinked, unrelated kept" '[[ -z $(find $HOME/.agents $HOME/.claude $HOME/.codex $HOME/.pi $HOME/.hermes -name superspace) && -d $HOME/.claude/skills/omarchy ]]'
ok "share/state/cache removed" '[[ ! -e $S && ! -e $HOME/.local/state/superspace && ! -e $HOME/.cache/superspace ]]'
ok "bin links removed" '[[ ! -e $HOME/.local/bin/superspace && ! -L $HOME/.local/bin/superspace && ! -L $HOME/.local/bin/superspace-setup ]]'
ok "preferences kept" '[[ -f $HOME/.config/superspace/preferences.toml ]]'
ok "units disabled+stopped" 'grep -qx "systemctl --user disable --now superspace.service superspace-embed.service superspace-embed-index.service" $LOG'
ok "agents unregistered" 'grep -qx "claude mcp remove --scope user superspace" $LOG && grep -qx "copilot mcp remove superspace" $LOG && grep -qx "codex-real mcp remove superspace" $LOG && ! grep -q "claude mcp add" $LOG'
ok "plugin folder untouched" '[[ -x $P/bin/superspace-setup ]]'
"$SETUP" uninstall >/dev/null 2>&1; ok "second uninstall exit 0" '[[ $? == 0 ]]'

echo "== help / bad args"
ok "--help prints usage" '"$SETUP" --help | grep -q "superspace-setup doctor" && [[ $("$SETUP" --help | tail -1) == *"--binary PATH"* ]]'
ok "bad arg fails" '! "$SETUP" --nope 2>/dev/null'
ok "bad --binary fails" '! "$SETUP" --binary /nonexistent 2>/dev/null'

echo "RESULT pass=$pass fail=$fail"
((fail == 0))
