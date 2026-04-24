// Generated from proto/dkod/v1/agent.proto — messages + gRPC client/server stubs.
pub mod agent {
    include!("agent.rs");
}

// Generated from proto/dkod/v1/types.proto — shared message types.
pub mod types {
    include!("types.rs");
}

// Re-export everything flat so callers can use `super::*` directly.
// Generated protos duplicate some type names between `agent` and `types`
// (e.g. `SymbolRef`, `CallEdgeRef`) — the ambiguity is benign because
// both sides are structurally identical, but rustc is about to promote
// the warning to a hard error. Silencing here keeps the generated layer
// hermetic; callers that need a specific import can reach into the
// submodule directly.
#[allow(ambiguous_glob_reexports)]
pub use agent::*;
#[allow(ambiguous_glob_reexports, unused_imports)]
pub use types::*;
