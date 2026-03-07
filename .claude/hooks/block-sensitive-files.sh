#!/bin/bash
# Claude hook: block access to .env files and credential-like files.
# Trigger: PreToolUse on Edit|Write|Bash

set -euo pipefail

TOOL_INPUT="${CLAUDE_TOOL_INPUT:-}"
TOOL_NAME="${CLAUDE_TOOL_NAME:-}"
FLAT_INPUT=$(echo "$TOOL_INPUT" | tr -d '\n' | tr -s ' ')

# For Bash tool: check if the command references sensitive files
if [ "$TOOL_NAME" = "Bash" ]; then
    # Use python3 for robust JSON parsing; fall back to greedy sed if unavailable
    COMMAND=$(echo "$FLAT_INPUT" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('command',''))" 2>/dev/null || \
              echo "$FLAT_INPUT" | sed -n 's/.*"command" *: *"\(.*\)"/\1/p' | sed 's/\\\"/"/g' | head -1)
    if [ -z "$COMMAND" ]; then
        exit 0
    fi
    # Block commands that write to .env or credential files
    if echo "$COMMAND" | grep -qE '(>|>>|tee|cp|mv|cat\s*>)\s*\S*(\.env(\s|$|\.)|credentials|secrets|id_rsa|id_ed25519|id_ecdsa|private.*\.(pem|key)|\.p12|\.pfx)|echo\s.*>\s*\S*(\.env|credentials|secrets|id_rsa|id_ed25519)'; then
        echo "BLOCK: refusing to write to sensitive file via shell command" >&2
        exit 1
    fi
    exit 0
fi

# For Edit/Write tool: check the file_path
FILE_PATH=$(echo "$FLAT_INPUT" | sed -n 's/.*"file_path" *: *"\([^"]*\)".*/\1/p' 2>/dev/null || true)

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

# Block private key and credential files (public certs like *.crt are allowed)
case "$BASENAME" in
    credentials*|secrets*|*.p12|*.pfx|*private*.pem|*private*.key|id_rsa|id_ed25519|id_ecdsa)
        echo "BLOCK: refusing to edit credential file: $BASENAME" >&2
        echo "Credential files should be edited manually." >&2
        exit 1
        ;;
esac
