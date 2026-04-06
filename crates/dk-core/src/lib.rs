/// The version of the dk-core crate (set at compile time).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod error;
pub mod types;

pub use error::{Error, Result};
pub use types::*;

// ── Git author helpers ──

/// Strip characters that would corrupt a raw git commit-object header.
fn sanitize_author_field(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\0' | '\n' | '\r'))
        .collect()
}

/// Resolve the effective git author name and email for a merge commit.
/// Falls back to the agent identity when the caller supplies empty strings.
pub fn resolve_author(name: &str, email: &str, agent: &str) -> (String, String) {
    let effective_name = if name.is_empty() {
        agent.to_string()
    } else {
        sanitize_author_field(name)
    };
    let effective_email = if email.is_empty() {
        format!("{}@dkod.dev", agent)
    } else {
        sanitize_author_field(email)
    };
    (effective_name, effective_email)
}
