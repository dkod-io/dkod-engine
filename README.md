<div align="center">

# dkod

**The agent-native code platform.**

AI agents write, review, and ship code — together, without conflicts.

[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Discord](https://img.shields.io/badge/Discord-Join-5865F2.svg)](https://discord.gg/q2xzuNDJ)
[![Twitter](https://img.shields.io/badge/Twitter-@dkod__io-1DA1F2.svg)](https://twitter.com/dkod_io)

[Website](https://dkod.io) · [Docs](https://dkod.io/docs) · [Discord](https://discord.gg/q2xzuNDJ) · [Twitter](https://twitter.com/dkod_io)

</div>

---

## What is dkod?

dkod lets multiple AI coding agents work on the same repository simultaneously — without merge conflicts, file locks, or broken builds.

- **Session Isolation** — Each agent gets an isolated workspace overlay. Writes go to the overlay, reads fall through to the base. No clones, no locks.
- **Semantic Merging** — Conflicts detected at the symbol level (functions, types, constants), not line-by-line. Two agents editing different functions in the same file? No conflict.
- **Verification Pipeline** — Every changeset runs through lint, type-check, and test gates before merge. Agents get structured feedback and fix issues autonomously.
- **Agent Protocol** — A purpose-built gRPC protocol for AI agents: `CONNECT → CONTEXT → SUBMIT → VERIFY → MERGE`.

Works with **Cursor**, **Claude Code**, **Cline**, **Windsurf**, **Codex**, and any MCP-compatible agent.

## Quick Start

### Install the CLI

```bash
cargo install --git https://github.com/dkod-io/dkod-engine dk-cli
```

### Connect and code

```bash
dk login
dk init my-org/my-repo --intent "add new feature"
dk cat src/main.rs
dk add src/main.rs --content "..."
dk commit -m "add feature"
dk check
dk push
```

### Or use with Claude Code (MCP)

Add to your MCP settings:

```json
{
  "mcpServers": {
    "dkod": {
      "command": "dk",
      "args": ["mcp"]
    }
  }
}
```

Then use `dk_connect`, `dk_file_write`, `dk_submit`, `dk_verify`, `dk_merge` from your agent.

## Architecture

```
crates/
├── dk-core       # Shared types and error handling
├── dk-engine     # Storage engine: Git layer + semantic graph
├── dk-protocol   # Agent Protocol gRPC server
├── dk-runner     # Verification pipeline runner
├── dk-agent-sdk  # Rust SDK for AI agents
├── dk-cli        # CLI (drop-in git alternative)
└── dk-server     # Reference server binary
```

## Build from source

**Requirements:** Rust 1.88+, PostgreSQL 16+, protoc

```bash
cargo build --workspace
```

## Run tests

```bash
cargo test --workspace
```

## Community

- [Discord](https://discord.gg/q2xzuNDJ) — Chat, questions, feedback
- [Twitter / X](https://twitter.com/dkod_io) — Updates and deep dives
- [GitHub Issues](https://github.com/dkod-io/dkod-engine/issues) — Bugs and feature requests

## License

[MIT](LICENSE)
