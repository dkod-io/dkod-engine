---
name: proto-sync
description: Validate protobuf changes, regenerate Rust code, and check for breaking API changes
disable-model-invocation: true
---

Validate and synchronize protobuf definitions with generated Rust code.

## Steps

1. Check for uncommitted proto changes:
   ```bash
   git diff --name-only -- 'proto/**/*.proto'
   git diff --cached --name-only -- 'proto/**/*.proto'
   ```

2. If proto files changed, check for breaking changes by comparing with the last committed version:
   - Removed fields or messages
   - Changed field numbers
   - Changed field types
   - Renamed services or RPCs

   ```bash
   git diff HEAD -- proto/dekode/v1/*.proto
   ```

   Flag any breaking changes and warn the user.

3. If no proto files changed (both lists from step 1 are empty), report
   "No proto changes detected -- workspace is in sync." and **stop here** (do not
   execute steps 4-7).

4. Rebuild the protocol crate to regenerate Rust code:
   ```bash
   cargo build -p dk-protocol
   ```

5. Check that the rest of the workspace compiles with the new proto:
   ```bash
   cargo check --workspace
   ```

6. Run clippy on the workspace:
   ```bash
   cargo clippy --workspace -- -D warnings
   ```

7. If everything passes, report success and summarize what changed. If anything fails, show the errors and suggest fixes.

## Output

```
## Proto Sync Results

### Changed Files
- proto/dekode/v1/types.proto: added field `foo` to `Bar`

### Breaking Changes
- None (or list them)

### Build Status
- dk-protocol: OK
- workspace check: OK
- clippy: OK
```
