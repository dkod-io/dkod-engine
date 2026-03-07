#!/bin/bash
# Claude hook: run cargo test for the affected crate after editing a .rs file.
# Trigger: PostToolUse on Edit|Write

set -euo pipefail

# Only act on .rs file edits
TOOL_INPUT="${CLAUDE_TOOL_INPUT:-}"
# Extract file_path from JSON — handle both compact and pretty-printed formats
FILE_PATH=$(echo "$TOOL_INPUT" | tr -d '\n' | tr -s ' ' | sed -n 's/.*"file_path" *: *"\([^"]*\)".*/\1/p' 2>/dev/null || true)

if [ -z "$FILE_PATH" ]; then
    exit 0
fi

case "$FILE_PATH" in
    *.rs) ;;
    *) exit 0 ;;
esac

# Only act in Rust projects
if [ ! -f "Cargo.toml" ]; then
    exit 0
fi

# Dynamically detect which crate was edited from the file path
CRATE=""
case "$FILE_PATH" in
    *crates/*/*)
        # Extract crate directory name from path (e.g., "crates/dk-core/src/lib.rs" → "dk-core")
        CRATE=$(echo "$FILE_PATH" | sed -n 's|.*crates/\([^/]*\)/.*|\1|p')
        # Verify it's a real crate with a Cargo.toml
        if [ -n "$CRATE" ] && [ ! -f "crates/$CRATE/Cargo.toml" ]; then
            CRATE=""
        fi
        ;;
esac

if [ -z "$CRATE" ]; then
    exit 0
fi

# Skip tests that require DATABASE_URL if it's not set
if [ -z "${DATABASE_URL:-}" ]; then
    echo "HOOK: running cargo test -p $CRATE (skipping integration tests — DATABASE_URL not set)"
    if ! cargo test -p "$CRATE" --lib 2>&1; then
        echo "WARNING: unit tests failed for $CRATE" >&2
    fi
else
    echo "HOOK: running cargo test -p $CRATE"
    if ! cargo test -p "$CRATE" 2>&1; then
        echo "WARNING: tests failed for $CRATE" >&2
    fi
fi
