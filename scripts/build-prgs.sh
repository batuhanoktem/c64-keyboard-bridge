#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
if [[ $# -ne 0 ]]; then
    echo "Usage: scripts/build-prgs.sh" >&2
    exit 2
fi
exec "$C64_KEYBOARD_BRIDGE_PYTHON" "$C64_KEYBOARD_BRIDGE_ROOT/scripts/build-prgs.py"
