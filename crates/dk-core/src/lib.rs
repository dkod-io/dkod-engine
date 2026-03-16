/// The version of the dk-core crate (set at compile time).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod error;
pub mod types;

pub use error::{Error, Result};
pub use types::*;
