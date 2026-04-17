// Generated from proto/dkod/v1/agent.proto — messages + gRPC client/server stubs.
pub mod agent {
    include!("agent.rs");
}

// Generated from proto/dkod/v1/types.proto — shared message types.
pub mod types {
    include!("types.rs");
}

// Re-export everything flat so callers can use `super::*` directly.
pub use agent::*;
pub use types::*;
