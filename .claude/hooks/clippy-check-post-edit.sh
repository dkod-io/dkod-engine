#!/usr/bin/env bash
# Claude hook: run cargo clippy on the affected crate after editing .rs files.
# Targets just the crate containing the edited file (fast incremental) rather
# than the full workspace (slow). Matches CI's `-D warnings` flag.
# Trigger: PostToolUse on Edit, Write

set -euo pipefail

# Read tool input from stdin (Claude Code passes JSON on stdin)
INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('file_path',''))" 2>/dev/null || true)

# Only act on .rs files
if [[ ! "$FILE_PATH" == *.rs ]]; then
    exit 0
fi

# Only act if the file exists and we're in a Rust project
if [ ! -f "$FILE_PATH" ] || [ ! -f "Cargo.toml" ]; then
    exit 0
fi

# Determine the crate containing the edited file by walking up to find
# the nearest Cargo.toml with a [package] section.
CRATE_PKG=""
DIR=$(dirname "$FILE_PATH")
while [[ "$DIR" != "/" && "$DIR" != "." ]]; do
    if [[ -f "$DIR/Cargo.toml" ]] && grep -q '^\[package\]' "$DIR/Cargo.toml" 2>/dev/null; then
        CRATE_PKG=$(grep '^name\s*=' "$DIR/Cargo.toml" | head -1 | sed 's/^name[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/')
        break
    fi
    DIR=$(dirname "$DIR")
done

# Run clippy on just the affected crate (fast) or workspace (fallback)
if [[ -n "$CRATE_PKG" ]]; then
    if ! cargo clippy -p "$CRATE_PKG" -- -D warnings 2>&1; then
        echo "HOOK: cargo clippy -p $CRATE_PKG found warnings/errors — fix them before committing."
        exit 2
    fi
else
    if ! cargo clippy --workspace -- -D warnings 2>&1; then
        echo "HOOK: cargo clippy found warnings/errors — fix them before committing."
        exit 2
    fi
fi
