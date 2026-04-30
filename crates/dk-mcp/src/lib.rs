pub mod proto {
    pub mod dkod {
        pub mod v1 {
            tonic::include_proto!("dkod.v1");
        }
    }
}

pub use proto::dkod::v1::*;

pub mod auth;
pub mod gateway;
pub mod grpc;
pub mod registry;
pub mod retry;
pub mod review_gate;
#[cfg(any(test, feature = "mock-review"))]
pub mod review_gate_mock;
pub mod server;
pub mod state;
