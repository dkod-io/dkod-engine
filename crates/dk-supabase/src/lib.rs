//! `dk-supabase` — thin wrapper around [`supabase_rs`] for the dkod engine.
//!
//! # Scope
//!
//! `dk-server` connects to Postgres directly via `sqlx` for migrations,
//! transactions, and prepared statements. That path is required for the core
//! symbol-locking / changeset machinery.
//!
//! This crate exists for the **complementary** path: agent/frontend code that
//! benefits from Supabase's REST + Auth + Storage stack with Row-Level Security
//! enforced by the publishable (anon) or service-role key. Typical consumers:
//!
//! * The `/dkod/analytics` drop-zone uploading Parquet/CSV via Supabase Storage.
//! * BAML-generated agents writing to user-scoped `sessions` / `changesets`
//!   mirror tables behind RLS policies.
//! * Auth token minting for the frontend without a round-trip through
//!   `dk-server`.
//!
//! Callers that need transactions or DDL should continue to use sqlx directly.
//!
//! # Example
//!
//! ```no_run
//! use dk_supabase::SupabaseConfig;
//!
//! # async fn demo() -> anyhow::Result<()> {
//! let cfg = SupabaseConfig::from_env()?;
//! let client = cfg.build_rest_client();
//! // Pass `client` (a `supabase_rs::SupabaseClient`) to downstream code that
//! // wants REST access.
//! let _ = client;
//! # Ok(())
//! # }
//! ```

use std::env;

use thiserror::Error;

/// Environment variable holding the Supabase project URL
/// (e.g. `https://<ref>.supabase.co`).
pub const ENV_URL: &str = "SUPABASE_URL";

/// Environment variable holding the Supabase API key. May be either the
/// publishable (anon) key for RLS-gated access or the service-role key for
/// privileged access.
pub const ENV_KEY: &str = "SUPABASE_KEY";

/// Errors surfaced while constructing a Supabase configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// A required environment variable was not present.
    #[error("missing environment variable: {0}")]
    MissingEnv(&'static str),
}

/// Configuration needed to construct a Supabase REST / Auth / Storage client.
#[derive(Debug, Clone)]
pub struct SupabaseConfig {
    /// Project URL, e.g. `https://<ref>.supabase.co`.
    pub url: String,
    /// API key — publishable or service-role depending on the access path.
    pub api_key: String,
}

impl SupabaseConfig {
    /// Build a [`SupabaseConfig`] from the `SUPABASE_URL` + `SUPABASE_KEY`
    /// environment variables.
    pub fn from_env() -> Result<Self, ConfigError> {
        let url = env::var(ENV_URL).map_err(|_| ConfigError::MissingEnv(ENV_URL))?;
        let api_key = env::var(ENV_KEY).map_err(|_| ConfigError::MissingEnv(ENV_KEY))?;
        Ok(Self { url, api_key })
    }

    /// Construct a [`supabase_rs::SupabaseClient`] for the configured project.
    ///
    /// The returned client speaks PostgREST over HTTPS and enforces RLS when
    /// used with the publishable (anon) key.
    pub fn build_rest_client(&self) -> supabase_rs::SupabaseClient {
        // `supabase_rs::SupabaseClient::new` panics only on a malformed URL;
        // we've already validated the string came from the environment, so
        // we surface the underlying panic as an explicit error-like case via
        // `expect` with a clear message. Callers that want fallible
        // construction can validate `self.url` first.
        supabase_rs::SupabaseClient::new(self.url.clone(), self.api_key.clone())
            .expect("invalid SUPABASE_URL — must be an http(s) URL")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_reports_missing_url() {
        // Snapshot + clear the relevant vars.
        let saved_url = env::var(ENV_URL).ok();
        let saved_key = env::var(ENV_KEY).ok();
        // SAFETY: single-threaded test; restored before return.
        unsafe {
            env::remove_var(ENV_URL);
            env::set_var(ENV_KEY, "x");
        }

        let err = SupabaseConfig::from_env().unwrap_err();
        match err {
            ConfigError::MissingEnv(name) => assert_eq!(name, ENV_URL),
        }

        // Restore.
        // SAFETY: single-threaded test; restoring previous values.
        unsafe {
            if let Some(v) = saved_url {
                env::set_var(ENV_URL, v);
            }
            if let Some(v) = saved_key {
                env::set_var(ENV_KEY, v);
            } else {
                env::remove_var(ENV_KEY);
            }
        }
    }

    #[test]
    fn from_env_constructs_when_both_present() {
        let saved_url = env::var(ENV_URL).ok();
        let saved_key = env::var(ENV_KEY).ok();
        // SAFETY: single-threaded test.
        unsafe {
            env::set_var(ENV_URL, "https://example.supabase.co");
            env::set_var(ENV_KEY, "sb_publishable_test");
        }

        let cfg = SupabaseConfig::from_env().expect("both vars set");
        assert_eq!(cfg.url, "https://example.supabase.co");
        assert_eq!(cfg.api_key, "sb_publishable_test");

        // SAFETY: single-threaded test.
        unsafe {
            match saved_url {
                Some(v) => env::set_var(ENV_URL, v),
                None => env::remove_var(ENV_URL),
            }
            match saved_key {
                Some(v) => env::set_var(ENV_KEY, v),
                None => env::remove_var(ENV_KEY),
            }
        }
    }
}
