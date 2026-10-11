#!/usr/bin/env bash
# stdout contains only the selected source root; build diagnostics go to stderr.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

revision=555282497c3ecf8b313d87d5973093af19c35bd5
if [[ -d "$C64BRIDGE_ROOT/third-party/cc65" ]]; then
    cc65_source="$C64BRIDGE_ROOT/third-party/cc65"
    "$C64BRIDGE_PYTHON" "$C64BRIDGE_ROOT/scripts/verify-cc65-source.py" "$cc65_source" >&2
else
    cc65_source="$C64BRIDGE_ROOT/target/toolchains/cc65-V2.19"
    if [[ ! -d "$cc65_source/.git" ]]; then
        if [[ -e "$cc65_source" ]]; then
            echo "Refusing to replace an existing cc65 directory without pinned Git metadata: $cc65_source" >&2
            exit 1
        fi
        mkdir -p "$(dirname "$cc65_source")"
        git init --quiet "$cc65_source" >&2
        git -C "$cc65_source" remote add origin https://github.com/cc65/cc65.git
        git -C "$cc65_source" fetch --quiet --depth 1 origin "$revision" >&2
        git -C "$cc65_source" checkout --quiet --detach FETCH_HEAD >&2
    fi
    actual="$(git -C "$cc65_source" rev-parse HEAD)"
    if [[ "$actual" != "$revision" ]]; then
        echo "cc65 cache is not pinned to $revision; refusing to build it." >&2
        exit 1
    fi
    git -C "$cc65_source" diff --quiet
    git -C "$cc65_source" diff --cached --quiet
    printf '%s\n' "$revision" > "$cc65_source/.c64bridge-source-revision"
fi

jobs="$("$C64BRIDGE_PYTHON" -c 'import os; print(min(os.cpu_count() or 2, 8))')"
make -C "$cc65_source/src" -j "$jobs" BUILD_ID='Git 55528249' cc65 ca65 cl65 ar65 ld65 >&2
make -C "$cc65_source/libsrc" -j "$jobs" BUILD_ID='Git 55528249' c64 >&2
for tool in cc65 ca65 cl65 ar65 ld65; do
    [[ -x "$cc65_source/bin/$tool" ]] || { echo "Missing built cc65 tool: $tool" >&2; exit 1; }
done
[[ -f "$cc65_source/lib/c64.lib" ]] || { echo "Missing built cc65 C64 runtime." >&2; exit 1; }
printf '%s\n' "$cc65_source"
