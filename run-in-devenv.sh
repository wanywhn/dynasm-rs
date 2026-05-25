#!/usr/bin/env bash
# Wrapper script to run arbitrary commands inside the devenv environment.
# Usage:
#   ./run-in-devenv.sh <command>
# Examples:
#   ./run-in-devenv.sh test-loongarch
#   ./run-in-devenv.sh gen-opmap
#   ./run-in-devenv.sh "cd testing && cargo test -j 1 --test loongarch_0"
#   ./run-in-devenv.sh "cargo build --target loongarch64-unknown-linux-gnu"
#   ./run-in-devenv.sh qemu-loongarch64 -L /nix/store/xxx-glibc... /path/to/binary

set -euo pipefail

if [ $# -eq 0 ]; then
  echo "Usage: run-in-devenv.sh <command>"
  echo "  Runs the command inside the devenv shell environment."
  exit 1
fi

# Write the command to a temporary script so devenv shell can execute it.
# This is necessary because "devenv shell CMD" only accepts a single
# executable name — it cannot handle shell expressions like "cd x && cmd".
TMPSCRIPT=$(mktemp /tmp/run-in-devenv-XXXXXX.sh)
trap 'rm -f "$TMPSCRIPT"' EXIT

cat > "$TMPSCRIPT" <<'HEADER'
#!/usr/bin/env bash
set -euo pipefail
HEADER

# Append the user's command
printf '%s\n' "$*" >> "$TMPSCRIPT"
chmod +x "$TMPSCRIPT"

exec devenv shell bash "$TMPSCRIPT"