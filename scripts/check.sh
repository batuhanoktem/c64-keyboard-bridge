#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
host_only=false
if [[ $# -eq 1 && "$1" == "--host-only" ]]; then
    host_only=true
elif [[ $# -ne 0 ]]; then
    echo "Usage: scripts/check.sh [--host-only]" >&2
    exit 2
fi

if "$host_only"; then
    : "${C64_PRG_DIR:?--host-only requires genuine unconfigured PRGs plus prg-manifest.json from the pinned C64 build}"
    C64_PRG_DIR="$("$C64BRIDGE_PYTHON" -c 'from pathlib import Path; import sys; print(Path(sys.argv[1]).resolve())' "$C64_PRG_DIR")"
else
    "$C64BRIDGE_ROOT/scripts/build-prgs.sh"
    C64_PRG_DIR="$C64BRIDGE_ROOT/target/c64-prgs"
    make -C "$C64BRIDGE_ROOT/c64" BUILD=../target/c64-tests -B test
    make -C "$C64BRIDGE_ROOT/c64" BUILD=../target/c64-tests-sanitized -B \
        HOST_CC='cc -fsanitize=address,undefined -fno-omit-frame-pointer' test
fi
export C64_PRG_DIR
"$C64BRIDGE_PYTHON" "$C64BRIDGE_ROOT/scripts/verify-prgs.py" "$C64_PRG_DIR"
cd "$C64BRIDGE_ROOT"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo build --locked --bin c64bridge --bin c64bridge-gui
binary="$C64BRIDGE_ROOT/target/debug/c64bridge"
if [[ "${OS:-}" == "Windows_NT" ]]; then
    binary="${binary}.exe"
fi
"$C64BRIDGE_PYTHON" "$C64BRIDGE_ROOT/scripts/cli-smoke.py" --binary "$binary"
"$C64BRIDGE_PYTHON" "$C64BRIDGE_ROOT/scripts/package-tests.py"
