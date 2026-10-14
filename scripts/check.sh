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
    C64_PRG_DIR="$("$C64_KEYBOARD_BRIDGE_PYTHON" -c 'from pathlib import Path; import sys; print(Path(sys.argv[1]).resolve())' "$C64_PRG_DIR")"
else
    "$C64_KEYBOARD_BRIDGE_ROOT/scripts/build-prgs.sh"
    C64_PRG_DIR="$C64_KEYBOARD_BRIDGE_ROOT/target/c64-prgs"
    make -C "$C64_KEYBOARD_BRIDGE_ROOT/c64" BUILD=../target/c64-tests -B test
    make -C "$C64_KEYBOARD_BRIDGE_ROOT/c64" BUILD=../target/c64-tests-sanitized -B \
        HOST_CC='cc -fsanitize=address,undefined -fno-omit-frame-pointer' test
fi
export C64_PRG_DIR
"$C64_KEYBOARD_BRIDGE_PYTHON" "$C64_KEYBOARD_BRIDGE_ROOT/scripts/verify-prgs.py" "$C64_PRG_DIR"
cd "$C64_KEYBOARD_BRIDGE_ROOT"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo build --locked --bin c64-keyboard-bridge --bin c64-keyboard-bridge-gui
binary="$C64_KEYBOARD_BRIDGE_ROOT/target/debug/c64-keyboard-bridge"
if [[ "${OS:-}" == "Windows_NT" ]]; then
    binary="${binary}.exe"
fi
"$C64_KEYBOARD_BRIDGE_PYTHON" "$C64_KEYBOARD_BRIDGE_ROOT/scripts/cli-smoke.py" --binary "$binary"
"$C64_KEYBOARD_BRIDGE_PYTHON" "$C64_KEYBOARD_BRIDGE_ROOT/scripts/package-tests.py"
"$C64_KEYBOARD_BRIDGE_PYTHON" "$C64_KEYBOARD_BRIDGE_ROOT/scripts/release-tests.py"
"$C64_KEYBOARD_BRIDGE_PYTHON" "$C64_KEYBOARD_BRIDGE_ROOT/scripts/gui-smoke-tests.py"
"$C64_KEYBOARD_BRIDGE_PYTHON" "$C64_KEYBOARD_BRIDGE_ROOT/scripts/c64-screenshots-tests.py"
if [[ "$(uname -s)" == "Darwin" ]]; then
    # Native, read-only service/worker queries. Headless runners may report an
    # unknown source, but must never assert, retain cancelled work, or hang.
    cargo run --locked -p bridge-host --example macos-layout-probe
    cargo run --locked -p bridge-gui --example macos-service-probe
elif [[ "$(uname -s)" == "Linux" ]]; then
    cargo build --locked -p bridge-gui --example linux-credential-probe
    "$C64_KEYBOARD_BRIDGE_PYTHON" "$C64_KEYBOARD_BRIDGE_ROOT/scripts/credential-smoke.py" \
        --binary "$C64_KEYBOARD_BRIDGE_ROOT/target/debug/examples/linux-credential-probe"
fi
