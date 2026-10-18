#!/bin/bash
# The real daemon (an aarch64 build, $1) and llama-servers from the unit files,
# then doctor and --progress. Run after test-setup.sh, in the same container.
B=${1:?usage: doctor-e2e.sh SUPERSPACE_BINARY}
REPO=$(cd "$(dirname "$0")/../.." && pwd) WORK=${WORK:-/work}
set -u
export HOME=/tmp/h XDG_RUNTIME_DIR=/tmp/xdg; mkdir -p -m 700 $XDG_RUNTIME_DIR; mkdir -p $HOME/.local/share/superspace/models /tmp/st $HOME/Documents $HOME/.config/superspace
P=$HOME/.config/omarchy/plugins/io.github.vincenth19.superspace; mkdir -p $P && tar -C "$REPO" --exclude=./target --exclude=./.git --exclude=./demo/corpus -cf - . | tar -C $P -xf -
cp $WORK/keep/*.gguf $WORK/keep/ggml-*.bin $HOME/.local/share/superspace/models/
printf '#!/bin/sh\necho "systemctl $*" >>/tmp/sc.log\n[ "$2" = is-active ] && exit "${ACTIVE:-3}"; exit 0\n' >/tmp/st/systemctl; chmod +x /tmp/st/systemctl
printf '#!/bin/sh\nexit 0\n' >/tmp/st/hyprctl; chmod +x /tmp/st/hyprctl
export PATH=/tmp/st:$PATH
printf '[[exclude]]\nkind = "emoji"\n' >$HOME/.config/superspace/preferences.toml
echo "The lease for the flat on Elm Street runs until March. Rent is due on the first." >$HOME/Documents/lease-notes.txt
$P/bin/superspace-setup --binary "$B" </dev/null >/tmp/setup.out 2>&1 || { echo SETUP-FAILED; cat /tmp/setup.out; }
echo "== doctor, nothing started"; $P/bin/superspace-setup doctor; echo "rc=$?"
for u in superspace-embed superspace-embed-index; do $(sed -n 's/^ExecStart=//p' $P/systemd/$u.service | sed "s#%h#$HOME#g") >/tmp/$u.log 2>&1 & done
$(sed -n 's/^ExecStart=//p' $P/systemd/superspace.service | sed "s#%h#$HOME#g") >/tmp/daemon.log 2>&1 &
for i in $(seq 120); do curl -sf 127.0.0.1:47802/health >/dev/null && [ -S $XDG_RUNTIME_DIR/superspace.sock ] && break; sleep 0.5; done
echo "== doctor, all started"; t0=$(date +%s%N); $P/bin/superspace-setup doctor; echo "rc=$? in $(( ($(date +%s%N) - t0) / 1000000 )) ms"
echo "== --progress with the daemon up"; : >/tmp/sc.log; t0=$(date +%s%N)
ACTIVE=0 $P/bin/superspace-setup --progress </dev/null; echo "rc=$? in $(( ($(date +%s%N) - t0) / 1000000 )) ms; systemctl calls: $(tr '\n' ';' </tmp/sc.log)"
sleep 8; $HOME/.local/bin/superspace search "when is the rent due" --json | jq -c '[.rows[] | {key, why}]'
