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
    let safe_agent = sanitize_author_field(agent);
    let effective_name = if name.is_empty() {
        safe_agent.clone()
    } else {
        sanitize_author_field(name)
    };
    let effective_email = if email.is_empty() {
        format!("{}@dkod.dev", safe_agent)
    } else {
        sanitize_author_field(email)
    };
    (effective_name, effective_email)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_author_uses_supplied_values() {
        let (name, email) = resolve_author("Alice", "alice@example.com", "agent-1");
        assert_eq!(name, "Alice");
        assert_eq!(email, "alice@example.com");
    }

    #[test]
    fn resolve_author_falls_back_to_agent() {
        let (name, email) = resolve_author("", "", "agent-1");
        assert_eq!(name, "agent-1");
        assert_eq!(email, "agent-1@dkod.dev");
    }

    #[test]
    fn resolve_author_sanitizes_newlines_and_nulls() {
        let (name, email) = resolve_author("Al\nice\0", "al\rice@\nex.com", "agent-1");
        assert_eq!(name, "Alice");
        assert_eq!(email, "alice@ex.com");
    }
}
