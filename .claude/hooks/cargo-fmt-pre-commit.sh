#!/bin/bash
# Claude hook: run cargo fmt --all before git commit commands.
# Trigger: PreToolUse on Bash

set -euo pipefail

# Only act on git commit commands
TOOL_INPUT="${CLAUDE_TOOL_INPUT:-}"
if ! echo "$TOOL_INPUT" | grep -qE '"command".*git\s+commit'; then
    exit 0
fi

# Only act in Rust projects
if [ ! -f "Cargo.toml" ]; then
    exit 0
fi

# Run cargo fmt
if ! cargo fmt --all 2>/dev/null; then
    echo "cargo fmt failed" >&2
    exit 0  # Don't block the commit, just warn
fi

# Stage any formatting changes
CHANGED=$(git diff --name-only -- '*.rs' 2>/dev/null || true)
if [ -n "$CHANGED" ]; then
    echo "$CHANGED" | xargs git add
    echo "HOOK: cargo fmt applied and staged formatting fixes for: $CHANGED"
fi
