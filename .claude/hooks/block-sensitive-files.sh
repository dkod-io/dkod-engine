#!/bin/bash
# Claude hook: block edits to .env files and credential-like files.
# Trigger: PreToolUse on Edit|Write

set -euo pipefail

TOOL_INPUT="${CLAUDE_TOOL_INPUT:-}"
# Extract file_path from JSON — handle both compact and pretty-printed formats
# by collapsing all whitespace/newlines first
FILE_PATH=$(echo "$TOOL_INPUT" | tr -d '\n' | tr -s ' ' | sed -n 's/.*"file_path" *: *"\([^"]*\)".*/\1/p' 2>/dev/null || true)

if [ -z "$FILE_PATH" ]; then
    exit 0
fi

BASENAME=$(basename "$FILE_PATH")

# Block .env files
case "$BASENAME" in
    .env|.env.*|*.env)
        echo "BLOCK: refusing to edit sensitive file: $BASENAME" >&2
        echo "Environment files should be edited manually." >&2
        exit 1
        ;;
esac

# Block credential/secret files
case "$BASENAME" in
    credentials*|secrets*|*.pem|*.key|*.p12|*.pfx)
        echo "BLOCK: refusing to edit credential file: $BASENAME" >&2
        echo "Credential files should be edited manually." >&2
        exit 1
        ;;
esac
