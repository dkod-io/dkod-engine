#!/bin/bash
# Claude hook: run cargo check with -D warnings before git commit.
# Catches CI failures locally before pushing.
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

# Run cargo check with the same flags as CI
if ! RUSTFLAGS="-D warnings" cargo check --workspace 2>&1; then
    echo "BLOCK: cargo check --workspace failed with -D warnings (same as CI)" >&2
    echo "Fix the warnings before committing." >&2
    exit 1
fi
