# dkod-engine

Rust workspace for the dkod platform — a code intelligence engine with gRPC API.

## Workspace Structure

| Crate | Role |
|-------|------|
| `dk-core` | Domain types, traits, and shared logic |
| `dk-engine` | Core engine logic (analysis, diffing, graph) |
| `dk-protocol` | gRPC/protobuf generated code (`proto/dkod/v1/`) |
| `dk-server` | gRPC server binary (tonic) |
| `dk-cli` | CLI binary (`dk`) |
| `dk-agent-sdk` | SDK for building agents |
| `dk-runner` | Task/agent runner |

## Build & Test

```bash
cargo build --workspace
cargo test --workspace          # requires DATABASE_URL for integration tests
cargo clippy --workspace -- -D warnings   # must pass (enforced by CI + hooks)
cargo fmt --all                 # auto-run by pre-commit hook
```

Tests that need Postgres: `DATABASE_URL=postgres://dkod:dkod@localhost:5432/dkod_test`

## Key Dependencies

- **tonic** / **prost** — gRPC server + protobuf codegen
- **sqlx** — async Postgres (with migrations)
- **tokio** — async runtime
- **redis** — caching / pub-sub
- **qdrant-client** — vector search
- **opendal** — object storage (S3, local FS)
- **jsonwebtoken** — JWT auth

## Proto

Canonical definitions live in `proto/dkod/v1/`. A copy exists at
`crates/dk-protocol/proto/dkod/v1/` for crates.io packaging (so the crate is
self-contained). **Both directories must stay in sync** — CI runs `diff -r` and
fails if they diverge. When editing proto files, update the canonical source in
`proto/dkod/v1/` and copy to `crates/dk-protocol/proto/dkod/v1/`.
Generated Rust code is in `dk-protocol`.
Protobuf compiler (`protoc`) is required at build time.

## CI

GitHub Actions (`ci.yml`): check → clippy → test → cross-compile release (linux-amd64/arm64, darwin-amd64/arm64).
Merges to `main` auto-tag and create GitHub Releases.

## Conventions

- Edition 2021, resolver v2
- `clippy.toml`: `too-many-arguments-threshold = 10`
- All warnings are errors in CI (`RUSTFLAGS=-D warnings`)
