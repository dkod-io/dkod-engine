//! JWT and shared-secret authentication for the Agent Protocol.
//!
//! [`AuthConfig`] supports four modes:
//! - **Jwt** -- HMAC-SHA256 JWT validation/issuance.
//! - **SharedSecret** -- simple string comparison (legacy).
//! - **Dual** -- try JWT first, fall back to shared secret.
//! - **External** -- trust an outer layer (e.g. tonic interceptor) that already
//!   validated the token; pass through the token value as agent id.

use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use tonic::Status;

// ── Claims ──────────────────────────────────────────────────────────

/// JWT claims carried inside every agent token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkodClaims {
    /// Subject -- the agent identity (e.g. "agent-42").
    pub sub: String,
    /// Issuer -- always "dkod".
    pub iss: String,
    /// Expiration (UTC epoch seconds).
    pub exp: usize,
    /// Issued-at (UTC epoch seconds).
    pub iat: usize,
    /// Permission scope (e.g. "read", "read+write", "admin").
    pub scope: String,
}

// ── AuthConfig ──────────────────────────────────────────────────────

/// Authentication configuration supporting JWT, shared-secret, or both.
#[derive(Clone, Debug)]
pub enum AuthConfig {
    /// Pure JWT mode -- validate/issue using HMAC-SHA256.
    Jwt { secret: String },
    /// Legacy shared-secret mode -- simple string comparison.
    SharedSecret { token: String },
    /// Dual mode -- try JWT first, fall back to shared secret.
    Dual {
        jwt_secret: String,
        shared_token: String,
    },
    /// External mode -- an outer authentication layer (e.g. a tonic
    /// interceptor) has already validated the token. The raw token value
    /// is passed through as the agent id. Useful for managed platforms
    /// where a gateway handles JWT verification before the request
    /// reaches the protocol server.
    External,
}

impl AuthConfig {
    /// Validate an incoming bearer token.
    ///
    /// Returns the agent id on success:
    /// - JWT modes: the `sub` claim from the decoded token.
    /// - SharedSecret mode: the literal `"anonymous"`.
    ///
    /// Empty tokens are always rejected regardless of auth mode.
    pub fn validate(&self, token: &str) -> Result<String, Status> {
        if token.is_empty() {
            return Err(Status::unauthenticated("Auth token must not be empty"));
        }

        match self {
            AuthConfig::Jwt { secret } => validate_jwt(token, secret),

            AuthConfig::SharedSecret {
                token: expected_token,
            } => {
                if token == expected_token {
                    Ok("anonymous".to_string())
                } else {
                    Err(Status::unauthenticated("Invalid auth token"))
                }
            }

            AuthConfig::Dual {
                jwt_secret,
                shared_token,
            } => {
                // Try JWT first; if that fails, try shared-secret.
                match validate_jwt(token, jwt_secret) {
                    Ok(agent_id) => Ok(agent_id),
                    Err(_jwt_err) => {
                        if token == shared_token {
                            Ok("anonymous".to_string())
                        } else {
                            Err(Status::unauthenticated("Invalid auth token"))
                        }
                    }
                }
            }

            AuthConfig::External => {
                // The outer layer already validated the token; pass it
                // through as the agent id (typically a user_id).
                Ok(token.to_string())
            }
        }
    }

    /// Issue a new JWT for the given agent.
    ///
    /// Only available when a JWT secret is configured (Jwt or Dual mode).
    /// Returns `Status::failed_precondition` if called in SharedSecret-only
    /// mode.
    pub fn issue_token(
        &self,
        agent_id: &str,
        scope: &str,
        ttl_secs: usize,
    ) -> Result<String, Status> {
        let secret = match self {
            AuthConfig::Jwt { secret } => secret,
            AuthConfig::Dual { jwt_secret, .. } => jwt_secret,
            AuthConfig::SharedSecret { .. } | AuthConfig::External => {
                return Err(Status::failed_precondition(
                    "Cannot issue JWT tokens without a JWT secret",
                ));
            }
        };

        if secret.len() < 32 {
            tracing::error!("JWT secret is too short (< 32 bytes); check server configuration");
            return Err(Status::internal("server misconfiguration"));
        }

        let now = jsonwebtoken::get_current_timestamp() as usize;
        let claims = DkodClaims {
            sub: agent_id.to_string(),
            iss: "dkod".to_string(),
            exp: now + ttl_secs,
            iat: now,
            scope: scope.to_string(),
        };

        encode(
            &Header::default(), // HS256
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .map_err(|e| Status::internal(format!("Failed to encode JWT: {e}")))
    }
}

// ── Private helpers ─────────────────────────────────────────────────

/// Decode and validate a JWT using HMAC-SHA256.
///
/// Validates:
/// - Algorithm: HS256
/// - Issuer: "dkod"
/// - Required claims: sub, exp, iss
///
/// Returns the `sub` claim (agent id) on success.
fn validate_jwt(token: &str, secret: &str) -> Result<String, Status> {
    if secret.len() < 32 {
        tracing::error!("JWT secret is too short (< 32 bytes); check server configuration");
        return Err(Status::unauthenticated("JWT validation failed"));
    }

    let mut validation = Validation::new(jsonwebtoken::Algorithm::HS256);
    validation.set_issuer(&["dkod"]);
    validation.set_required_spec_claims(&["sub", "exp", "iss"]);

    let token_data = decode::<DkodClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|e| Status::unauthenticated(format!("JWT validation failed: {e}")))?;

    Ok(token_data.claims.sub)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-key-for-unit-tests!!";
    const TEST_AGENT: &str = "agent-42";
    const TEST_SCOPE: &str = "read+write";
    const TTL: usize = 3600; // 1 hour

    #[test]
    fn jwt_roundtrip() {
        let config = AuthConfig::Jwt {
            secret: TEST_SECRET.to_string(),
        };
        let token = config
            .issue_token(TEST_AGENT, TEST_SCOPE, TTL)
            .expect("issue_token should succeed");
        let agent_id = config.validate(&token).expect("validate should succeed");
        assert_eq!(agent_id, TEST_AGENT);
    }

    #[test]
    fn jwt_rejects_bad_token() {
        let config = AuthConfig::Jwt {
            secret: TEST_SECRET.to_string(),
        };
        let result = config.validate("not-a-jwt");
        assert!(result.is_err(), "should reject garbage token");
    }

    #[test]
    fn jwt_rejects_wrong_secret() {
        let config1 = AuthConfig::Jwt {
            secret: "secret-one-padding-for-32-bytes!".to_string(),
        };
        let config2 = AuthConfig::Jwt {
            secret: "secret-two-padding-for-32-bytes!".to_string(),
        };
        let token = config1
            .issue_token(TEST_AGENT, TEST_SCOPE, TTL)
            .expect("issue_token should succeed");
        let result = config2.validate(&token);
        assert!(
            result.is_err(),
            "should reject token signed with different secret"
        );
    }

    #[test]
    fn shared_secret_accepts_correct_token() {
        let config = AuthConfig::SharedSecret {
            token: "my-shared-token".to_string(),
        };
        let agent_id = config
            .validate("my-shared-token")
            .expect("should accept correct token");
        assert_eq!(agent_id, "anonymous");
    }

    #[test]
    fn shared_secret_rejects_wrong_token() {
        let config = AuthConfig::SharedSecret {
            token: "correct-token".to_string(),
        };
        let result = config.validate("wrong-token");
        assert!(result.is_err(), "should reject wrong token");
    }

    #[test]
    fn dual_mode_accepts_jwt() {
        let config = AuthConfig::Dual {
            jwt_secret: TEST_SECRET.to_string(),
            shared_token: "fallback-token".to_string(),
        };
        let token = config
            .issue_token(TEST_AGENT, TEST_SCOPE, TTL)
            .expect("issue_token should succeed");
        let agent_id = config.validate(&token).expect("should accept valid JWT");
        assert_eq!(agent_id, TEST_AGENT);
    }

    #[test]
    fn dual_mode_falls_back_to_shared_secret() {
        let config = AuthConfig::Dual {
            jwt_secret: TEST_SECRET.to_string(),
            shared_token: "fallback-token".to_string(),
        };
        let agent_id = config
            .validate("fallback-token")
            .expect("should fall back to shared secret");
        assert_eq!(agent_id, "anonymous");
    }

    #[test]
    fn dual_mode_rejects_invalid() {
        let config = AuthConfig::Dual {
            jwt_secret: TEST_SECRET.to_string(),
            shared_token: "fallback-token".to_string(),
        };
        let result = config.validate("garbage-that-matches-nothing");
        assert!(result.is_err(), "should reject invalid token in dual mode");
    }

    #[test]
    fn empty_token_rejected_in_all_modes() {
        let jwt = AuthConfig::Jwt {
            secret: TEST_SECRET.to_string(),
        };
        assert!(
            jwt.validate("").is_err(),
            "JWT mode should reject empty token"
        );

        let shared = AuthConfig::SharedSecret {
            token: "my-token".to_string(),
        };
        assert!(
            shared.validate("").is_err(),
            "SharedSecret should reject empty token"
        );

        let dual = AuthConfig::Dual {
            jwt_secret: TEST_SECRET.to_string(),
            shared_token: "fallback".to_string(),
        };
        assert!(
            dual.validate("").is_err(),
            "Dual mode should reject empty token"
        );

        let external = AuthConfig::External;
        assert!(
            external.validate("").is_err(),
            "External mode should reject empty token"
        );
    }

    #[test]
    fn empty_shared_secret_never_matches() {
        // Even if someone constructs SharedSecret with an empty token,
        // empty incoming tokens are rejected before comparison.
        let config = AuthConfig::SharedSecret {
            token: "".to_string(),
        };
        assert!(
            config.validate("").is_err(),
            "empty token should be rejected even if shared secret is empty"
        );
    }

    #[test]
    fn external_passes_through_token_as_agent_id() {
        let config = AuthConfig::External;
        let agent_id = config
            .validate("user_abc123")
            .expect("External should accept any non-empty token");
        assert_eq!(agent_id, "user_abc123");
    }

    #[test]
    fn external_rejects_empty_token() {
        let config = AuthConfig::External;
        assert!(
            config.validate("").is_err(),
            "External should reject empty token"
        );
    }

    #[test]
    fn external_cannot_issue_tokens() {
        let config = AuthConfig::External;
        assert!(
            config.issue_token("agent", "read", 3600).is_err(),
            "External mode should not issue JWT tokens"
        );
    }

    #[test]
    fn rejects_short_jwt_secret() {
        let config = AuthConfig::Jwt {
            secret: "short".to_string(),
        };
        let result = config.issue_token("agent-1", "full", 3600);
        assert!(result.is_err());
    }
}
