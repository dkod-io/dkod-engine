# `@dkod/baml-client` — generated TypeScript client

**Do not edit files in this directory by hand.** They are regenerated from
`baml_src/` via `./scripts/baml-generate.sh`.

## What this is

The same `baml_src/agents.baml` graph that powers `dk-runner` / `dk-agent-sdk`
/ `dk-server` (via the Rust client at `baml_client/`) is also compiled to a
typed TypeScript client here. Browsers and Node runtimes can import this to:

- Call `PlanWorkflow()`, `JudgeChangeset()`, `DeepReview()` with typed args
  and return types identical to the Rust side.
- Use `streamGenerator()` to render partial `WorkflowPlan` / `ReviewVerdict`
  structs as they arrive from the LLM (drives streaming UI in
  `clickhouse-monitoring`'s `/dkod/agents` page).
- Keep symbol-tuned enums (`VerdictCode::K1/K2/K3`, `AgentRole::K1..K5`)
  aligned with the Rust side — no duplicate Zod / manual type definitions.

## Why this is committed (not gitignored)

Unlike `baml_client/` (Rust, regenerated on each `cargo build`), the
TypeScript client is committed so that downstream repos — mainly
[`clickhouse-monitoring`](https://github.com/sjoerd2025/clickhouse-monitoring)
— can pull it in via git submodule without needing `baml-cli` installed.

## Consumer setup (git submodule)

From a consumer repo:

```bash
git submodule add \
  https://github.com/sjoerd2025/dkod-engine.git \
  vendor/dkod-engine

# Resolve @dkod/baml-client to the submodule path
cat >> package.json <<'JSON'
{
  "dependencies": {
    "@dkod/baml-client": "file:./vendor/dkod-engine/baml_client_ts"
  }
}
JSON

bun install
```

Then import like any typed module:

```ts
import { b } from "@dkod/baml-client"
const plan = await b.PlanWorkflow({ goal: "review changeset", repo_id: "r_1" })
```

## Regenerating

From the `dkod-engine` repo root:

```bash
./scripts/baml-generate.sh
```

The script runs `baml-cli generate` and renames the intermediate output
directory so the final path stays stable at `baml_client_ts/`.

## Runtime dependency

Requires `@boundaryml/baml@0.221.0` in the consuming project. Pin the same
version that `baml_src/generators.baml` declares so the generated code and
runtime stay compatible.
