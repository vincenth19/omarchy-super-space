#!/bin/bash
# The x86_64 llama.cpp and whisper.cpp downloads unpack to the layout the
# daemon expects (uname faked). Run after test-setup.sh, in the same container.
REPO=$(cd "$(dirname "$0")/../.." && pwd) WORK=${WORK:-/work}
set -u
export HOME=/tmp/hx; mkdir -p $HOME/.local/share/superspace/models /tmp/st
P=$HOME/.config/omarchy/plugins/io.github.vincenth19.superspace; mkdir -p $P && tar -C "$REPO" --exclude=./target --exclude=./.git --exclude=./demo/corpus -cf - . | tar -C $P -xf -
ln $WORK/keep/*.gguf $WORK/keep/ggml-*.bin $HOME/.local/share/superspace/models/ 2>/dev/null || cp $WORK/keep/*.gguf $WORK/keep/ggml-*.bin $HOME/.local/share/superspace/models/
for c in systemctl hyprctl; do printf '#!/bin/sh\n[ "$2" = is-active ] && exit 3; exit 0\n' >/tmp/st/$c; chmod +x /tmp/st/$c; done
printf '#!/bin/sh\n[ "$1" = -m ] && echo x86_64 || /usr/bin/uname "$@"\n' >/tmp/st/uname; chmod +x /tmp/st/uname
printf '#!/bin/sh\necho dev\n' >/tmp/ssdev; chmod +x /tmp/ssdev
PATH=/tmp/st:$PATH $P/bin/superspace-setup --progress --binary /tmp/ssdev </dev/null >/tmp/x.out 2>&1; echo "rc=$?"
grep -v '"done"' /tmp/x.out
S=$HOME/.local/share/superspace
cat $S/llama/.build $S/whisper/.build
file $S/whisper/whisper-cli $S/llama/llama-server | sed 's/, BuildID.*//'
ls $S/whisper | grep -c 'libggml-cpu'
du -sh $S/whisper $S/llama
