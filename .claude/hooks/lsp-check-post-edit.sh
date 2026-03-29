#!/usr/bin/env bash
# Claude hook: remind to use LSP after editing .rs files.
# Trigger: PostToolUse on Edit, Write
# Outputs a message that Claude sees as hook feedback, prompting LSP usage.

set -euo pipefail

FILE_PATH=$(echo "${CLAUDE_TOOL_INPUT:-}" | tr -d '\n' | tr -s ' ' | sed -n 's/.*"file_path" *: *"\([^"]*\)".*/\1/p' 2>/dev/null || true)

# Only act on .rs files
if [[ ! "$FILE_PATH" == *.rs ]]; then
    exit 0
fi

echo "[lsp-check] Rust file modified: $FILE_PATH"
echo "Run LSP documentSymbol on this file to verify code structure, then hover on changed symbols to confirm types."
