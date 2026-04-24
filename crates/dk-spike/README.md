# dk-spike — backend crate evaluation

> **Not a product crate.** `dk-spike` exists only to force candidate
> dependencies into the workspace compile graph so we can confirm they
> co-habitate cleanly before wiring them into real crates.
> `publish = false`, `version = "0.0.0"`.

## Status of each candidate

All of the crates below pass `cargo check -p dk-spike` against the
existing dkod-engine workspace (tonic 0.14, prost 0.14, tokio 1.x,
sqlx 0.8, rustls/ring). No conflicts, no duplicate resolver picks that
bricked production crates.

| Crate | Version | Status | Planned home in dkod |
|---|---|---|---|
| `octocrab` | `0.49` | ✅ | `dk-github` — PR/issue ops for the review loop, wraps GitHub REST. |
| `genai` | `0.5.3` | ✅ | `dk-agent-sdk` / `dk-runner` — multi-provider LLM client (OpenAI, Anthropic, xAI, Ollama, Groq, DeepSeek). Sits underneath the BAML-generated clients for ad-hoc calls that don't fit a BAML function. |
| `enum_dispatch` | `0.3` | ✅ | `dk-runner` — zero-cost `dyn Trait` replacement for workflow step enums. |
| `typetag` | `0.2` | ✅ | `dk-protocol` / `dk-engine` — serde-friendly trait objects for persisted step payloads + analytics events. |
| `tracing-core` | `0.1` | ✅ | `dk-observability` — direct dep once we write a custom Langfuse subscriber. |
| `langfuse-sdk` | `0.1.1` | ✅ | `dk-observability` — typed Langfuse traces/generations/scores client. Complements the OTLP exporter already used for cloud.langfuse.com. |
| `rmcp-macros` | `1.5` | ✅ (duplicate of rmcp 0.16 already in dk-mcp — kept on request, will reconcile when dk-mcp is promoted to rmcp 1.x) | `dk-mcp` — proc-macros from the official rmcp SDK. |
| `connectrpc` | `0.3.2` (feat: `client`) | ✅ | `dk-server` / clients — **target runtime stack for tonic replacement (path B).** Connect-RPC (HTTP+protobuf, browser-native) supersedes tonic + tonic-web once dk-protocol is regenerated. |
| `buffa` | `0.3.0` (feat: `json`) | ✅ | `dk-protocol` — protobuf ↔ JSON for Connect clients without a separate code path. |
| `buffa-types` | `0.3.0` (feat: `json`) | ✅ | `dk-protocol` — type helpers for Buffa. |
| `swiftide` | `0.32.1` | ✅ | `dk-agent-sdk` — streaming RAG / ingestion pipelines. Uses rig under the hood; heavy but keeps retrieval code declarative. |
| `dspy-rs` | `0.7.3` | ✅ | `dk-agent-sdk` — prompt optimization. Does **not** conflict with `genai`: DSPy wraps its own LLM traits and BAML handles the typed-call layer on top. |
| `baml` | `0.221.0` | ✅ | `dk-agent-sdk` — runtime for the BAML-generated Rust client (see `baml_src/`). Agent + workflow graphs are authored in `.baml`, compiled to typed Rust via `baml-cli generate`. |

## Binary-only peers (installed to the dev environment, not a Cargo dep)

| Tool | How to get it | Role in dkod |
|---|---|---|
| `rtk` | `cargo install --git https://github.com/rtk-ai/rtk` | Repo toolkit; used by dev workflows, not linked into the engine. |
| `grit` | `cargo install --git https://github.com/rtk-ai/grit grit` | Code-search / structural rewrites CLI (the package has multiple bins, must specify `grit`). |
| `baml-cli` | `cargo install baml-cli` | Compiles `baml_src/*.baml` into `baml_client/` (Rust module). |
| `icm` | `cargo install --git https://github.com/rtk-ai/icm --bin icm` | "Infinite Context Memory" — single-binary memory/knowledge graph server. Exposes itself as an MCP server; dkod registers it as a managed server, does **not** vendor the code. |
| `gh aw` | `gh extension install githubnext/gh-aw` | GitHub Agent Workflows — compiles natural-language `.md` workflow specs into SHA-pinned `.lock.yml` with AWF firewall + MCP gateway + Serena LSP. Preferred over the `gh-workflow` Rust crate for agent-authored CI. |

## BAML workflow (source of truth for agents + graphs)

See `baml_src/` at the repo root. The project already has:

- `generators.baml` — Rust generator pointing at `baml_client/` (gitignored; regenerate with `baml-cli generate`).
- `clients.baml` — `DkodDefault`, `DkodJudge`, `DkodAnthropic` LLM clients. All secrets read from env vars (`DK_LLM_*`, `CLAUDE_API_KEY`) — none embedded.
- `agents.baml` — the actual graph-building functions:
  - `PlanWorkflow(intent, repo_context) -> WorkflowPlan` — orchestrator that emits a `{steps, edges, entry}` DAG. dk-runner walks the graph at runtime.
  - `JudgeChangeset(summary, review_notes) -> ReviewVerdict` — LLM-as-judge approval gate.
  - `DeepReview(diff, context) -> ReviewVerdict` — reviewer pass used before judge.

Regenerate the Rust client after any `.baml` edit:

```bash
baml-cli generate  # from repo root; writes baml_client/
cargo check        # confirms the generated module type-checks
```

## What's explicitly out of scope in this PR

- No production crate is touched. `dk-core`, `dk-engine`, `dk-server`, `dk-runner`,
  `dk-mcp`, `dk-cli`, `dk-agent-sdk`, `dk-protocol`, `dk-analytics`: untouched.
- No runtime integration of BAML — only the `.baml` source files + the
  `baml` crate in `dk-spike`. The generated `baml_client/` is gitignored.
- No dk-server Langfuse exporter yet — that lands in the follow-up PR
  together with `dk-observability`.
- No icm server wiring. This PR only documents how to install the
  `icm` binary; actual registration in `dk-mcp` lands in the follow-up.

## Follow-up PR plan

**Path B — tonic → connectrpc replacement (multi-PR series):**

1. `dk-protocol-connect` — emit Connect-RPC stubs via `buffa` alongside the existing `tonic-build` output (dual-transport phase).
2. `dk-server` — port handlers from `tonic::Status` to connectrpc equivalents; run both transports in parallel.
3. Port `dk-cli`, `dk-agent-sdk`, `dk-mcp` clients to the connectrpc client.
4. Remove `tonic`, `tonic-web`, `tonic-build` once all call sites are on Connect.

**Non-blocking follow-ups:**

1. Promote passing crates out of `dk-spike` into real crates:
   - `dk-github` (octocrab; `gh aw` invoked as a peer binary rather than linked)
   - `dk-observability` (langfuse-sdk, tracing-core, opentelemetry-otlp)
   - `dk-agents` (baml, genai, dspy-rs, swiftide)
   - `dk-embedded-ch` (chdb-rust, for local ClickHouse in verification pipelines)
2. Wire `baml-cli generate` into a `cargo xtask` or `justfile` target so CI
   regenerates the client from `baml_src/`.
3. Register `icm` in `dk-mcp`'s managed-server registry.
4. Seed the clickhouse-monitoring "managed MCP servers" page from the
   platform.csv the user attached (Linear, Notion, Stripe, Figma, etc.).
5. Delete `dk-spike` once all its deps have found a real home.

## Compatibility audit summary

Ran `cargo tree -d` on the full workspace with all 15 candidate crates present. Findings:

- **No conflicts with production pins.** tonic 0.12, prost 0.13, sqlx 0.8, tokio 1.x, rustls 0.23 all resolve cleanly.
- **Duplicate versions in the tree** (all either pre-existing or build-time only, none break runtime):
  - `imara-diff 0.1 + 0.2` — pre-existing from gix.
  - `reqwest 0.11 + 0.12 + 0.13`, `hyper 0.14 + 1.8`, `rustls 0.21 + 0.23` — 0.x chain was pulled by `cargo-issue-lib` (now removed) and `chdb-rust` build-dep only; not linked into runtime binaries.
  - `jsonwebtoken 9 + 10` — octocrab pulls 10, production `dk-protocol` pins 9. Harmless coexistence; reconcile when `dk-github` is extracted.
  - `rmcp-macros 0.16 + 1.5` — existing rmcp 0.16 in dk-mcp vs new 1.5 added here; kept intentionally per owner decision.
- **`cargo-issue-lib` dropped** — was the main source of ancient deps (reqwest 0.11, rustls 0.21).
- **`gh-workflow` Rust crate dropped** — superseded by `gh aw` peer binary (see table above).
