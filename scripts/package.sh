#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
exec "$C64_KEYBOARD_BRIDGE_PYTHON" "$C64_KEYBOARD_BRIDGE_ROOT/scripts/package.py" "$@"
