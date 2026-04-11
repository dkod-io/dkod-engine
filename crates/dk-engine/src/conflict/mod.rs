pub mod ast_merge;
mod claim_tracker;
pub mod payload;

pub use ast_merge::{ast_merge, MergeResult, MergeStatus, SymbolConflict};
pub use claim_tracker::{AcquireOutcome, ConflictInfo, ReleasedLock, SymbolClaim, SymbolClaimTracker, SymbolLocked};
pub use payload::{
    build_conflict_block, build_conflict_detail, ConflictBlock, SymbolConflictDetail,
    SymbolVersion,
};
