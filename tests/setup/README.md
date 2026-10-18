# Setup tests

`test-setup.sh` runs `bin/superspace-setup` end to end with real downloads
(about 1 GB the first time, kept in `$WORK/keep`) and stubbed systemctl,
hyprctl, gum and agent CLIs. CI runs it (`.github/workflows/setup.yml`). By
hand: arm64 Linux, a normal user, bash, curl, jq, python3 and libgomp, in a
container (it serves fake model servers on ports 47801 and 47802):

```bash
docker run --rm -v "$PWD:/repo:ro" -v ~/superspace-setup-test:/work IMAGE bash /repo/tests/setup/test-setup.sh
```

With the same `/work`, `doctor-e2e.sh BINARY` runs a real aarch64 daemon and
the model servers from the unit files and checks `doctor`, and `x64.sh` checks
that the x86_64 downloads unpack.
