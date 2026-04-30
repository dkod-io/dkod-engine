pub mod ast_merge;
mod claim_tracker;
pub mod payload;
#[cfg(feature = "valkey")]
mod valkey_claim_tracker;

pub use ast_merge::{ast_merge, MergeResult, MergeStatus, SymbolConflict};
pub use claim_tracker::{
    AcquireOutcome, ClaimTracker, ConflictInfo, LocalClaimTracker, ReleasedLock, SymbolClaim,
    SymbolLocked,
};
pub use payload::{
    build_conflict_block, build_conflict_detail, ConflictBlock, SymbolConflictDetail, SymbolVersion,
};
#[cfg(feature = "valkey")]
pub use valkey_claim_tracker::ValkeyClaimTracker;
