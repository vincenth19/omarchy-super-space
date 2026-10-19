#!/bin/bash
# The agent watcher under a real systemd user manager: an agent installed after
# setup gets the MCP server, a burst of installs is one run, a second setup
# does not deadlock with it, and uninstall removes it. Run as a lingering user
# in a container booted with systemd, after test-setup.sh (see README.md here).
REPO=$(cd "$(dirname "$0")/../.." && pwd) WORK=${WORK:-/work}
set -u
XDG_RUNTIME_DIR=/run/user/$(id -u)
export XDG_RUNTIME_DIR
P=$HOME/.config/omarchy/plugins/io.github.vincenth19.superspace L=/tmp/agents.log pass=0 fail=0
ok() { if (eval "$2"); then echo "PASS $1"; ((pass++)); else echo "FAIL $1"; ((fail++)); fi; }
agent() { printf '#!/bin/bash\necho "%s $*" >>%s\n' "${1##*/}" "$L" >"$1" && chmod +x "$1"; }
runs() { grep -c "^claude mcp add" "$L"; }
# wait_runs N: up to 40 s for the watcher's Nth run to finish.
wait_runs() { for _ in $(seq 40); do (($(runs) >= $1)) && [[ $(systemctl --user is-active superspace-agents.service) == inactive ]] && return; sleep 1; done; }
mkdir -p "$P" ~/.local/share/superspace/models && tar -C "$REPO" --exclude=./target --exclude=./.git --exclude=./demo/corpus -cf - . | tar -C "$P" -xf -
cp "$WORK"/keep/* ~/.local/share/superspace/models/
printf '#!/bin/sh\necho dev\n' >/tmp/ssdev && chmod +x /tmp/ssdev && : >"$L"
"$P/bin/superspace-setup" --binary /tmp/ssdev </dev/null >/tmp/w1.out 2>&1 || cat /tmp/w1.out
ok "watcher enabled and waiting" '[[ $(systemctl --user is-enabled superspace-agents.path) == enabled ]] && systemctl --user -q is-active superspace-agents.path'
mkdir -p ~/.local/bin && agent ~/.local/bin/claude && t0=$SECONDS && wait_runs 1
echo "claude in ~/.local/bin registered after $((SECONDS - t0)) s"
ok "claude installed later: registered" 'grep -qx "claude mcp add --scope user superspace -- $HOME/.local/bin/superspace mcp" $L'
mkdir -p ~/.local/share/mise/shims && for i in $(seq 300); do : >~/.local/share/mise/shims/tool$i; done && agent ~/.local/share/mise/shims/codex && wait_runs 2
ok "300-file reshim with codex: one run, codex registered" '[[ $(runs) == 2 ]] && grep -qx "codex mcp add superspace -- $HOME/.local/bin/superspace mcp" $L'
ok "watcher still waiting (no rate limit hit)" 'systemctl --user -q is-active superspace-agents.path'
t0=$SECONDS && timeout 120 "$P/bin/superspace-setup" --binary /tmp/ssdev </dev/null >/tmp/w2.out 2>&1; rc=$?
ok "setup again with the watcher on: no deadlock" '[[ $rc == 0 ]]'
wait_runs 4 && echo "setup + the run its links set off took $((SECONDS - t0)) s; runs so far: $(runs)"
ok "doctor sees the watcher" '"$P/bin/superspace-setup" doctor | grep -q "ok    watching for agents installed later"'
"$P/bin/superspace-setup" uninstall >/dev/null 2>&1
ok "uninstall: watcher and services stopped and gone" '! systemctl --user -q is-active superspace-agents.path superspace-embed.service superspace-embed-index.service && [[ ! -e ~/.config/systemd/user/superspace-agents.path && ! -e ~/.config/systemd/user/default.target.wants/superspace-agents.path ]]'
n=$(runs) && agent ~/.local/bin/gemini && sleep 15
ok "uninstall: nothing registered after" '[[ $(runs) == "$n" ]] && ! grep -q "^gemini" $L'
echo "RESULT pass=$pass fail=$fail"
((fail == 0))
