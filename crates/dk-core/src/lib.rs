/// The version of the dk-core crate (set at compile time).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod error;
pub mod types;

pub use error::{Error, Result};
pub use types::*;

// ── Git author helpers ──

/// Strip characters that would corrupt a raw git commit-object header.
/// Removes null bytes, newlines, and angle brackets (git author/email delimiters).
fn sanitize_author_field(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\0' | '\n' | '\r' | '<' | '>'))
        .collect()
}

/// Resolve the effective git author name and email for a merge commit.
/// Falls back to the agent identity when the caller supplies empty or
/// all-stripped strings. Sanitization runs BEFORE the emptiness check
/// so that inputs like "\n" correctly fall back to the agent identity.
pub fn resolve_author(name: &str, email: &str, agent: &str) -> (String, String) {
    let safe_agent = sanitize_author_field(agent);
    let sanitized_name = sanitize_author_field(name);
    let effective_name = if sanitized_name.is_empty() {
        safe_agent.clone()
    } else {
        sanitized_name
    };
    let sanitized_email = sanitize_author_field(email);
    let effective_email = if sanitized_email.is_empty() {
        format!("{}@dkod.dev", safe_agent)
    } else {
        sanitized_email
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

    #[test]
    fn resolve_author_falls_back_when_input_is_only_stripped_chars() {
        let (name, email) = resolve_author("\n", "\r\0", "agent-1");
        assert_eq!(name, "agent-1");
        assert_eq!(email, "agent-1@dkod.dev");
    }

    #[test]
    fn resolve_author_strips_angle_brackets() {
        let (name, email) = resolve_author("Alice <hacker>", "a<b>c@ex.com", "agent-1");
        assert_eq!(name, "Alice hacker");
        assert_eq!(email, "abc@ex.com");
    }
}
