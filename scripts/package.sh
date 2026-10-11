#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
exec "$C64BRIDGE_PYTHON" "$C64BRIDGE_ROOT/scripts/package.py" "$@"
