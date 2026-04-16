pub mod proto {
    pub mod dkod {
        pub mod v1 {
            tonic::include_proto!("dkod.v1");
        }
    }
}

pub use proto::dkod::v1::*;

pub mod auth;
pub mod grpc;
pub mod retry;
pub mod review_gate;
pub mod server;
pub mod state;
