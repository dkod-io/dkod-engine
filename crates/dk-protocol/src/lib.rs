#![allow(clippy::new_without_default, clippy::result_large_err)]

mod generated;

pub use generated::dkod::v1::*;

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
pub mod pre_submit;
pub mod push;
pub mod record_review;
pub mod resolve;
pub mod review;
pub mod server;
pub mod session;
#[cfg(feature = "redis")]
pub mod session_redis;
pub mod session_status;
pub mod session_store;
pub mod submit;
pub mod validation;
pub mod verify;
pub mod watch;

pub use server::ProtocolServer;
