#!/bin/bash
# Claude hook: run cargo test for the affected crate after editing a .rs file.
# Trigger: PostToolUse on Edit

set -euo pipefail

# Only act on .rs file edits
TOOL_INPUT="${CLAUDE_TOOL_INPUT:-}"
FILE_PATH=$(echo "$TOOL_INPUT" | sed -n 's/.*"file_path" *: *"\([^"]*\)".*/\1/p' 2>/dev/null || true)

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

# Determine which crate was edited
CRATE=""
case "$FILE_PATH" in
    *crates/dk-core/*) CRATE="dk-core" ;;
    *crates/dk-engine/*) CRATE="dk-engine" ;;
    *crates/dk-protocol/*) CRATE="dk-protocol" ;;
    *crates/dk-server/*) CRATE="dk-server" ;;
    *crates/dk-cli/*) CRATE="dk-cli" ;;
    *crates/dk-agent-sdk/*) CRATE="dk-agent-sdk" ;;
    *crates/dk-runner/*) CRATE="dk-runner" ;;
esac

if [ -z "$CRATE" ]; then
    exit 0
fi

echo "HOOK: running cargo test -p $CRATE"
if ! cargo test -p "$CRATE" 2>&1; then
    echo "WARNING: tests failed for $CRATE" >&2
    # Don't block — just inform
fi
