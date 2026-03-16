#!/bin/bash
# Claude hook: run cargo clippy with -D warnings before git commit.
# Catches CI failures locally before pushing (CI runs clippy, not just check).
# Trigger: PreToolUse on Bash (git commit commands only)

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

# Run cargo clippy with the same flags as CI
if ! cargo clippy --workspace -- -D warnings 2>&1; then
    echo "BLOCK: cargo clippy --workspace failed with -D warnings (same as CI)" >&2
    echo "Fix the clippy warnings before committing." >&2
    exit 1
fi
