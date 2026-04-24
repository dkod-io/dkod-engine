#![allow(clippy::new_without_default, clippy::result_large_err)]

// Exposed so sibling crates (e.g. dk-agent-sdk) can reach into a
// specific submodule and disambiguate proto re-exports. The flat
// re-export below is still the primary public API.
pub mod generated;

#[allow(ambiguous_glob_reexports, unused_imports)]
pub use generated::dkod::v1::*;

pub(crate) mod abandon;
pub mod approve;
pub mod auth;
pub mod close;
pub mod connect;
pub mod context;
pub mod events;
pub mod file_list;
pub mod file_read;
pub mod file_write;
pub mod merge;
pub mod metrics;
pub mod pre_submit;
pub mod push;
pub mod record_review;
pub(crate) mod require_live_session;
pub mod resolve;
pub mod review;
pub mod server;
pub mod session;
#[cfg(feature = "redis")]
pub mod session_redis;
pub mod session_status;
pub mod session_store;
pub mod stale_overlay;
pub mod submit;
pub mod validation;
pub mod verify;
pub mod watch;

pub use server::ProtocolServer;
