#!/usr/bin/env bash
# Regenerate both BAML clients (Rust + TypeScript) from baml_src/.
#
# - Rust client lands at `baml_client/` (gitignored; consumers in this workspace
#   regenerate on build).
# - TypeScript client lands at `baml_client_ts_raw/baml_client/` first because
#   baml-cli always appends a `baml_client/` subdirectory to its output_dir.
#   We rename it to `baml_client_ts/` so (a) it doesn't collide with the Rust
#   module and (b) downstream consumers (clickhouse-monitoring) can pull it in
#   via git submodule without needing baml-cli installed.
#
# Usage: ./scripts/baml-generate.sh  (run from repo root)

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

if ! command -v baml-cli >/dev/null 2>&1; then
    echo "error: baml-cli not found. Install with:" >&2
    echo "  cargo install baml-cli" >&2
    exit 1
fi

echo "Running baml-cli generate..."
baml-cli generate

if [ -d "baml_client_ts_raw/baml_client" ]; then
    echo "Renaming baml_client_ts_raw/baml_client -> baml_client_ts/"
    rm -rf baml_client_ts
    mv baml_client_ts_raw/baml_client baml_client_ts
    rm -rf baml_client_ts_raw
fi

echo "Generated:"
echo "  - baml_client/      (Rust, gitignored)"
echo "  - baml_client_ts/   (TypeScript, committed for submodule consumers)"
