//! # dk-agent-sdk
//!
//! Typed Rust client for the dkod Agent Protocol.
//!
//! This crate wraps the tonic-generated gRPC client from `dk-protocol` and
//! provides a clean, session-oriented API for AI agents to interact with a
//! dkod server.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use dk_agent_sdk::{AgentClient, Change, Depth};
//!
//! # async fn example() -> dk_agent_sdk::Result<()> {
//! let mut client = AgentClient::connect("http://localhost:50051", "my-token").await?;
//! let mut session = client.init("my-repo", "fix auth bug").await?;
//!
//! let ctx = session.context("auth middleware", Depth::Full, 4000).await?;
//! session.submit(vec![Change::modify("src/auth.rs", "// fixed")]).await?;
//! let steps = session.verify().await?;
//! let result = session.merge("fix: auth bypass", false).await?;
//! # Ok(())
//! # }
//! ```

pub mod client;
pub mod error;
pub mod session;
pub mod tools;
pub mod types;

pub use client::AgentClient;
pub use error::{Result, SdkError};
pub use session::Session;
pub use types::*;
