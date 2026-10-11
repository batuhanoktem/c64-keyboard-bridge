#!/usr/bin/env bash
# Shared by the checked-in build scripts; do not source user configuration.
set -euo pipefail

C64BRIDGE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PYTHONUTF8=1
if command -v python3 >/dev/null 2>&1; then
    C64BRIDGE_PYTHON="$(command -v python3)"
elif command -v python >/dev/null 2>&1; then
    C64BRIDGE_PYTHON="$(command -v python)"
else
    echo "Python 3.11 or newer is required." >&2
    exit 1
fi
"$C64BRIDGE_PYTHON" -c 'import sys; assert sys.version_info >= (3, 11), "Python 3.11 or newer is required"'
export C64BRIDGE_ROOT C64BRIDGE_PYTHON
export PYTHONDONTWRITEBYTECODE=1
export CARGO_TARGET_DIR="$("$C64BRIDGE_PYTHON" -c 'from pathlib import Path; import sys; print(Path(sys.argv[1]).resolve() / "target")' "$C64BRIDGE_ROOT")"

c64bridge_cc65() {
    CC65_HOME="$("$C64BRIDGE_ROOT/scripts/bootstrap-cc65.sh")"
    export CC65_HOME
    export PATH="$CC65_HOME/bin:$PATH"
    export CL65="$CC65_HOME/bin/cl65"
}
