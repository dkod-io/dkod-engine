# Agent Protocol v0.1 Completion Design

*Date: 2026-02-28*
*Status: Approved*

## Overview

Complete the Agent Protocol to production-ready v0.1 across three phases. All 11 RPCs are already implemented. The remaining work is transport (WebSocket), security (JWT auth), protocol completeness (stubs), and infrastructure services.

## Current State

All 11 Agent Protocol RPCs are implemented and tested:
- CONNECT, CONTEXT, SUBMIT, VERIFY, MERGE, WATCH
- FileRead, FileWrite, FileList, PreSubmitCheck, GetSessionStatus

Gaps: WebSocket transport, JWT auth, dependency tracking in CONTEXT (stubbed), watch filtering (ignored), session resume (unused), infrastructure services (local-only).

## Phase 1: WebSocket Fallback + JWT Auth

### WebSocket Transport

**Approach:** `tonic-web` 0.12 wraps the existing tonic gRPC server with an HTTP/1.1 compatibility layer. Browsers and WebSocket clients use the same proto-generated stubs via gRPC-Web protocol.

**Changes:**
- Add `tonic-web` dependency to dk-server
- Wrap `AgentServiceServer` with `tonic_web::enable()` in main.rs
- Streaming RPCs (VERIFY, WATCH) work via server-sent events over HTTP/1.1
- No protocol or handler changes needed

**Why tonic-web:** Same proto stubs for all clients. Raw WebSocket would require custom framing and duplicate handler logic.

### JWT Auth

**Approach:** Dual-mode auth — JWT as primary, shared-secret as deprecated fallback.

**Changes:**
1. Add `jsonwebtoken` crate to dk-protocol
2. Move auth from message field (`ConnectRequest.auth_token`) to gRPC metadata (`authorization: Bearer <token>`)
3. Tonic interceptor validates JWT before any handler runs
4. JWT claims: `sub` (agent_id), `iss` (dkod), `exp`, `iat`, `scope` (repos/permissions)
5. Keep `auth_token` in ConnectRequest as deprecated fallback
6. Server config: `--jwt-secret` for HMAC signing key

**Auth flow:**
```
Agent → CONNECT with JWT in metadata → interceptor validates → handler creates session
Agent → subsequent RPCs with session_id + JWT in metadata → both validated
```

**Why not mTLS yet:** mTLS requires CA infrastructure. JWT gives per-agent identity and scoping now. mTLS layers on later.

## Phase 2: Fill Protocol Stubs

### Dependency Tracking in CONTEXT

**Current:** `dependencies: vec![]` (stubbed at context.rs:188)

**Fix:** When `include_dependencies` is set, query `dk-engine::graph::depgraph` for each returned symbol. Populate `DependencyRef` entries. Data model and store already exist.

### Watch Filtering

**Current:** `WatchRequest.filter` accepted but ignored.

**Fix:** Parse filter as glob pattern (e.g., `changeset.*`, `*.merged`). Filter events in WATCH streaming loop before sending. Simple string matching.

### Session Resume

**Current:** `resume_session_id` in WorkspaceConfig accepted but unused.

**Fix:** On CONNECT with `resume_session_id`, look up old session's workspace state from DB (overlay files, changeset). Restore into new session. Requires persisting workspace overlay to DB on session expiry.

## Phase 3: Infrastructure Services

All services are **optional** — engine runs fully functional with just PostgreSQL and local filesystem.

### Redis (Sessions + Cache + Pub/Sub)

**Current:** DashMap (in-memory), broadcast channel (single-process).

**Changes:**
- `redis` crate with tokio runtime
- `RedisSessionStore`: sessions persist across restarts
- `RedisEventBus`: distributed WATCH via Redis pub/sub
- Rate limiting via Redis counters
- Config: `--redis-url`

### S3-Compatible Object Storage

**Current:** Local filesystem (`./data`).

**Changes:**
- `opendal` or `aws-sdk-s3` crate
- `ObjectStore` trait abstraction (`get`, `put`, `delete`, `list`)
- Stream large blobs to/from S3
- Config: `--s3-endpoint`, `--s3-bucket`, `--s3-region`
- Fallback: local filesystem when S3 not configured

### Qdrant Vector Search

**Current:** Tantivy full-text search only.

**Changes:**
- `qdrant-client` crate
- Generate embeddings during symbol indexing
- Add `semantic_similarity` search mode to CONTEXT
- Config: `--qdrant-url`
- Graceful degradation to Tantivy-only when Qdrant not configured

## Execution Order

1. Phase 1: WebSocket + JWT Auth
2. Phase 2: Dependency tracking, watch filtering, session resume
3. Phase 3: Redis, S3, Qdrant

Tag `v0.2.0` after Phase 1+2. Tag `v0.3.0` after Phase 3.
