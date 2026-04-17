#![allow(clippy::new_without_default, clippy::result_large_err)]

pub mod proto {
    pub mod dkod {
        pub mod v1 {
            tonic::include_proto!("dkod.v1");
        }
    }
}

pub use proto::dkod::v1::*;

pub mod auth;
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
