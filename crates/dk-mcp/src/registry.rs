//! dkod-managed integrations registry.
//!
//! Loads `registry.toml` (co-located with this crate) at startup and exposes
//! the parsed [`Registry`] to the MCP surface. The `dk_list_integrations` tool
//! reads it to advertise available third-party MCP servers and peer binaries
//! to connected agents.
//!
//! The registry is intentionally static-at-compile-time (`include_str!`) so the
//! binary is self-contained — no file IO at runtime, no deployment drift.
//! Adding, removing, or retagging an integration is a one-commit change.
//!
//! # Example
//!
//! ```
//! use dk_mcp::registry::Registry;
//!
//! let reg = Registry::load_embedded();
//! assert!(reg.mcp_servers().any(|s| s.name == "supabase"));
//! assert!(reg.peers().any(|p| p.name == "gh-aw"));
//! ```
use serde::{Deserialize, Serialize};

const EMBEDDED_REGISTRY_TOML: &str = include_str!("../registry.toml");

/// Parsed view of the registry.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Registry {
    #[serde(default, rename = "mcp")]
    pub mcp: Vec<McpEntry>,

    #[serde(default, rename = "peer")]
    pub peer: Vec<PeerEntry>,
}

/// A third-party MCP server that dkod agents may call as a tool source.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct McpEntry {
    /// Canonical short name (e.g. `supabase`, `linear`).
    pub name: String,

    /// Wire transport the server speaks. `stdio` or `http`.
    pub transport: String,

    /// Free-form category tags (e.g. `auth`, `db`, `docs`).
    #[serde(default)]
    pub categories: Vec<String>,

    /// Env vars required to configure this server. Empty means no auth.
    #[serde(default)]
    pub auth_env_vars: Vec<String>,

    /// Canonical upstream homepage / docs URL.
    pub homepage: String,

    /// One-line description shown in the /dkod/integrations UI.
    pub description: String,
}

/// An external binary dkod delegates to (not an MCP server).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PeerEntry {
    /// Canonical short name (e.g. `gh-aw`, `mdbook`).
    pub name: String,

    /// Copy-paste install command.
    pub install: String,

    /// Free-form category tags.
    #[serde(default)]
    pub categories: Vec<String>,

    /// Canonical upstream homepage / docs URL.
    pub homepage: String,

    /// One-line description.
    pub description: String,
}

impl Registry {
    /// Parse the compile-time embedded `registry.toml`.
    ///
    /// Panics only if the embedded TOML is malformed — which would fail CI
    /// before ever shipping, so this is safe to call from `Default` impls.
    pub fn load_embedded() -> Self {
        toml::from_str(EMBEDDED_REGISTRY_TOML)
            .expect("embedded registry.toml is malformed; fix the file and rebuild")
    }

    /// Parse a caller-supplied TOML string. Used in tests and for overrides.
    pub fn from_toml_str(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    pub fn mcp_servers(&self) -> impl Iterator<Item = &McpEntry> {
        self.mcp.iter()
    }

    pub fn peers(&self) -> impl Iterator<Item = &PeerEntry> {
        self.peer.iter()
    }

    /// Whether a given MCP entry looks configured in the current environment
    /// (all listed auth env vars are set, or the entry requires none).
    pub fn is_mcp_configured(entry: &McpEntry) -> bool {
        entry
            .auth_env_vars
            .iter()
            .all(|var| std::env::var(var).is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_registry_parses() {
        let reg = Registry::load_embedded();
        assert!(
            reg.mcp.len() >= 11,
            "expected 11+ MCP entries, got {}",
            reg.mcp.len()
        );
        assert!(
            reg.peer.len() >= 7,
            "expected 7+ peer entries, got {}",
            reg.peer.len()
        );
    }

    #[test]
    fn every_mcp_has_required_fields() {
        let reg = Registry::load_embedded();
        for m in &reg.mcp {
            assert!(!m.name.is_empty(), "mcp entry missing name");
            assert!(
                matches!(m.transport.as_str(), "stdio" | "http"),
                "mcp {}: invalid transport {:?}",
                m.name,
                m.transport
            );
            assert!(
                m.homepage.starts_with("http"),
                "mcp {}: homepage must be URL",
                m.name
            );
            assert!(
                !m.description.is_empty(),
                "mcp {}: description required",
                m.name
            );
        }
    }

    #[test]
    fn every_peer_has_required_fields() {
        let reg = Registry::load_embedded();
        for p in &reg.peer {
            assert!(!p.name.is_empty(), "peer entry missing name");
            assert!(
                !p.install.is_empty(),
                "peer {}: install command required",
                p.name
            );
            assert!(
                p.homepage.starts_with("http"),
                "peer {}: homepage must be URL",
                p.name
            );
        }
    }

    #[test]
    fn names_are_unique_within_each_kind() {
        let reg = Registry::load_embedded();
        let mut mcp_names: Vec<&str> = reg.mcp.iter().map(|m| m.name.as_str()).collect();
        mcp_names.sort();
        let orig = mcp_names.len();
        mcp_names.dedup();
        assert_eq!(orig, mcp_names.len(), "duplicate MCP names");

        let mut peer_names: Vec<&str> = reg.peer.iter().map(|p| p.name.as_str()).collect();
        peer_names.sort();
        let orig = peer_names.len();
        peer_names.dedup();
        assert_eq!(orig, peer_names.len(), "duplicate peer names");
    }

    #[test]
    fn parses_from_toml_str() {
        let s = r#"
[[mcp]]
name = "test"
transport = "stdio"
homepage = "https://example.com"
description = "test entry"
"#;
        let reg = Registry::from_toml_str(s).unwrap();
        assert_eq!(reg.mcp.len(), 1);
        assert_eq!(reg.mcp[0].name, "test");
    }
}
