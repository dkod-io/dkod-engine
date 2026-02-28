# dkod-engine

The open-source foundation of [dkod.io](https://dkod.io) — an agent-native code storage engine that combines Git-compatible storage with a semantic code graph.

## Components

- **dk-core** — Shared types (Symbol, CallEdge, Dependency, TypeInfo) and error types
- **dk-engine** — Dual storage engine: Git-compatible layer (gitoxide) + semantic graph (tree-sitter, Tantivy)
- **dk-protocol** — Agent Protocol gRPC server (CONNECT, CONTEXT, SUBMIT, VERIFY, MERGE, WATCH)
- **dk-agent-sdk** — Rust SDK for AI agents
- **dk-cli** — Human-facing CLI, drop-in git replacement
- **dk-runner** — Verification pipeline runner
- **dk-server** — Reference server binary

## Requirements

- Rust 1.88+
- PostgreSQL 16+
- protobuf compiler (`protoc`)

## Build

```bash
cargo build --workspace
```

## Run

```bash
createdb dkod
cargo run --bin dk-server -- --database-url postgres://localhost/dkod --auth-token <your-token>
```

## Test

```bash
# Unit tests (no DB required)
cargo test --workspace

# With database
DATABASE_URL=postgres://localhost/dekode_test cargo test --workspace
```

## License

MIT
