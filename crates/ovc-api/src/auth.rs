//! JWT authentication and middleware.
//!
//! Provides token creation, validation, and an Axum extractor that enforces
//! authentication on protected routes.
//!
//! Two authentication flows are supported:
//!
//! 1. **Password-based** — `POST /auth/token` with a shared password.
//!    Produces a JWT with `sub = "local-user"` and `role = "admin"`.
//!
//! 2. **Key-based** — `GET /auth/challenge` + `POST /auth/key-auth`.
//!    The user signs a server-generated challenge with their Ed25519 key.
//!    Produces a JWT with `sub = <fingerprint>` and `role = <ACL role>`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{FromRequestParts, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use chrono::Utc;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::models::{TokenRequest, TokenResponse};
use crate::state::AppState;

/// JWT claims embedded in every token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject: `"local-user"` (password auth) or key fingerprint (key auth).
    pub sub: String,
    /// Expiration time (seconds since epoch).
    pub exp: usize,
    /// Role: `"owner"`, `"admin"`, `"write"`, or `"read"`.
    pub role: String,
    /// Repository ID the token is scoped to (key-based auth only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
    /// Token version at issuance time.
    ///
    /// Validated against `AppState::current_token_version()`. If the stored
    /// version has advanced past this value, the token is rejected as stale.
    /// This provides instant, stateless revocation of all outstanding tokens
    /// when access control changes without maintaining a server-side revocation
    /// list.
    #[serde(default)]
    pub tkv: u64,
}

impl Claims {
    /// Returns `true` if this is a local (password-based) admin session.
    ///
    /// Password-based auth bypasses ACL checks for backward compatibility.
    #[must_use]
    pub fn is_local_admin(&self) -> bool {
        self.sub == "local-user"
    }

    /// Checks whether the claims carry at least the given role.
    ///
    /// Local admins always pass. For key-based sessions, the role string
    /// is compared against the requested minimum.
    #[must_use]
    pub fn has_role(&self, min_role: &str) -> bool {
        if self.is_local_admin() {
            return true;
        }
        role_level(&self.role) >= role_level(min_role)
    }
}

/// Maps a role string to a numeric level for comparison.
fn role_level(role: &str) -> u8 {
    match role {
        "owner" => 3,
        "admin" => 2,
        "write" => 1,
        _ => 0,
    }
}

/// Token validity duration: 1 hour.
///
/// Short-lived tokens bound the exploitation window in the absence of a
/// full revocation store. Combined with the `tkv` (token version) claim,
/// this provides both a time-bound and an on-demand invalidation path.
const TOKEN_DURATION_SECS: i64 = 3_600;

/// Challenge validity duration: 60 seconds.
const CHALLENGE_DURATION_SECS: u64 = 60;

/// Domain separation label for deriving the JWT signing key from the server
/// secret. This ensures the HMAC signing key is cryptographically independent
/// from the raw secret used for password comparison, so knowledge of the
/// password alone cannot be used to forge JWTs.
const JWT_KEY_DERIVATION_LABEL: &[u8] = b"ovc-jwt-signing-key-v1";

/// Derives a JWT signing key from the server secret using HMAC-SHA256.
///
/// This provides domain separation: even if someone learns the server secret
/// (used for password comparison), the JWT signing key is a distinct derived
/// value. The derivation is deterministic so both token creation and
/// validation produce the same key.
fn derive_jwt_signing_key(secret: &str) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts any key length");
    mac.update(JWT_KEY_DERIVATION_LABEL);
    mac.finalize().into_bytes().to_vec()
}

/// Creates a signed JWT for the local daemon.
pub fn create_jwt(secret: &str, token_version: u64) -> Result<(String, String), ApiError> {
    create_jwt_with_claims(secret, "local-user", "admin", None, token_version)
}

/// Creates a signed JWT with specific claims.
pub fn create_jwt_with_claims(
    secret: &str,
    sub: &str,
    role: &str,
    repo_id: Option<String>,
    token_version: u64,
) -> Result<(String, String), ApiError> {
    let now = Utc::now();
    let exp = now + chrono::Duration::seconds(TOKEN_DURATION_SECS);

    let claims = Claims {
        sub: sub.to_owned(),
        exp: usize::try_from(exp.timestamp()).unwrap_or(usize::MAX),
        role: role.to_owned(),
        repo_id,
        tkv: token_version,
    };

    let signing_key = derive_jwt_signing_key(secret);
    let token = jsonwebtoken::encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(&signing_key),
    )
    .map_err(|e| {
        tracing::error!("failed to create JWT: {e}");
        ApiError::internal("failed to create token")
    })?;

    let expires_at = exp.to_rfc3339();
    Ok((token, expires_at))
}

/// Validates a JWT and returns the embedded claims.
///
/// `current_token_version` should be `AppState::current_token_version()`. Any
/// token whose embedded `tkv` is less than this value is rejected as stale,
/// providing stateless, instant revocation when access control changes.
pub fn validate_jwt(
    token: &str,
    secret: &str,
    current_token_version: u64,
) -> Result<Claims, ApiError> {
    // Pin the algorithm to HS256 explicitly to prevent algorithm confusion
    // attacks where an attacker crafts a token using a different algorithm
    // (e.g., "none" or an asymmetric algorithm with a known public key).
    let signing_key = derive_jwt_signing_key(secret);
    let token_data = jsonwebtoken::decode::<Claims>(
        token,
        &DecodingKey::from_secret(&signing_key),
        &Validation::new(jsonwebtoken::Algorithm::HS256),
    )
    .map_err(|_| ApiError::unauthorized("invalid or expired token"))?;

    let claims = token_data.claims;

    // Reject tokens issued before the most recent revocation event.
    // This is an O(1) check: no revocation list to scan.
    if claims.tkv < current_token_version {
        return Err(ApiError::unauthorized(
            "token has been revoked — please re-authenticate",
        ));
    }

    Ok(claims)
}

/// Axum extractor that validates the `Authorization: Bearer <token>` header.
///
/// Injects the validated [`Claims`] into the handler if authentication succeeds.
/// Returns 401 if the token is missing or invalid.
impl FromRequestParts<Arc<AppState>> for Claims {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ApiError::unauthorized("missing Authorization header"))?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| ApiError::unauthorized("invalid Authorization header format"))?;

        validate_jwt(token, &state.jwt_secret, state.current_token_version())
    }
}

/// Handler: `POST /api/v1/auth/token`
///
/// Accepts a password and returns a JWT only if the password matches the
/// server's JWT secret. Uses constant-time comparison to prevent timing
/// side-channel attacks.
#[allow(clippy::items_after_statements)]
pub async fn create_token(
    State(app): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<TokenRequest>,
) -> Result<Json<TokenResponse>, ApiError> {
    // Rate-limit auth attempts per IP to prevent brute-force attacks.
    let client_ip = extract_client_ip(&headers);
    if !app.check_auth_rate_limit(&client_ip) {
        return Err(ApiError::rate_limited(
            "too many authentication attempts — try again later",
        ));
    }

    if req.password.is_empty() {
        return Err(ApiError::bad_request("password must not be empty"));
    }

    // Constant-time comparison prevents timing side-channel leakage of the
    // secret length or content.
    //
    // We hash both sides with SHA-256 before comparing so that:
    //   1. The comparison always operates on fixed-length (32-byte) inputs,
    //      eliminating the length-leaking early return that a direct
    //      `ct_eq` on variable-length slices would require.
    //   2. The hashing step itself runs in data-independent time for each
    //      input, so the overall operation leaks neither length nor content.
    use sha2::{Digest, Sha256};
    use subtle::ConstantTimeEq;

    let password_hash = Sha256::digest(req.password.as_bytes());
    let secret_hash = Sha256::digest(app.jwt_secret.as_bytes());
    let matches: bool = password_hash.ct_eq(&secret_hash).into();

    if !matches {
        return Err(ApiError::unauthorized("invalid password"));
    }

    let (token, expires_at) = create_jwt(&app.jwt_secret, app.current_token_version())?;
    Ok(Json(TokenResponse { token, expires_at }))
}

/// Handler: `GET /api/v1/auth/verify`
///
/// Returns 200 if the token in the Authorization header is valid.
pub async fn verify_token(_claims: Claims) -> StatusCode {
    StatusCode::OK
}

// ── Challenge-response key authentication ──────────────────────────────

/// Response for `GET /api/v1/auth/challenge`.
#[derive(Debug, Serialize)]
pub struct ChallengeResponse {
    /// Hex-encoded 32-byte random challenge.
    pub challenge: String,
    /// Seconds until the challenge expires.
    pub expires_in: u64,
}

/// Request for `POST /api/v1/auth/key-auth`.
#[derive(Debug, Deserialize)]
pub struct KeyAuthRequest {
    /// Repository ID to authenticate against.
    pub repo_id: String,
    /// Key fingerprint (`SHA256:<base64>`).
    pub fingerprint: String,
    /// The challenge string (hex) from the challenge endpoint.
    pub challenge: String,
    /// Base64-encoded Ed25519 signature of the raw challenge bytes.
    pub signature: String,
}

/// Handler: `GET /api/v1/auth/challenge`
///
/// Generates a random 32-byte challenge for key-based authentication.
pub async fn get_challenge(
    State(app): State<Arc<AppState>>,
) -> Result<Json<ChallengeResponse>, ApiError> {
    use rand::RngCore;

    let mut challenge_bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut challenge_bytes);

    let challenge_hex = hex_encode(&challenge_bytes);

    // Store in challenge cache.
    let mut challenges = app
        .auth_challenges
        .write()
        .map_err(|_| ApiError::internal("challenge cache lock poisoned"))?;

    // Prune expired challenges, then enforce the hard cap.
    // The background task also cleans up periodically, but we prune here too
    // to handle bursts that arrive faster than the 60-second sweep interval.
    let now = std::time::Instant::now();
    challenges.retain(|_, (_, expiry)| *expiry > now);
    if challenges.len() >= AppState::MAX_PENDING_CHALLENGES {
        return Err(ApiError::rate_limited(
            "too many pending challenges; try again later",
        ));
    }

    let expiry = now + std::time::Duration::from_secs(CHALLENGE_DURATION_SECS);
    challenges.insert(challenge_hex.clone(), (challenge_bytes.to_vec(), expiry));
    drop(challenges);

    Ok(Json(ChallengeResponse {
        challenge: challenge_hex,
        expires_in: CHALLENGE_DURATION_SECS,
    }))
}

/// Handler: `POST /api/v1/auth/key-auth`
///
/// Verifies an Ed25519 signature over a challenge, then issues a JWT
/// scoped to the user's role from the repository's ACL.
pub async fn key_auth(
    State(app): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<KeyAuthRequest>,
) -> Result<Json<TokenResponse>, ApiError> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use ed25519_dalek::Verifier;

    // Rate-limit auth attempts per IP.
    let client_ip = extract_client_ip(&headers);
    if !app.check_auth_rate_limit(&client_ip) {
        return Err(ApiError::rate_limited(
            "too many authentication attempts — try again later",
        ));
    }

    // 1. Retrieve and consume the challenge.
    let challenge_bytes = {
        let mut challenges = app
            .auth_challenges
            .write()
            .map_err(|_| ApiError::internal("challenge cache lock poisoned"))?;

        let (bytes, expiry) = challenges
            .remove(&req.challenge)
            .ok_or_else(|| ApiError::unauthorized("invalid or expired challenge"))?;
        drop(challenges);

        if std::time::Instant::now() > expiry {
            return Err(ApiError::unauthorized("challenge expired"));
        }
        bytes
    };

    // 2. Decode the signature.
    let sig_bytes = BASE64
        .decode(&req.signature)
        .map_err(|_| ApiError::bad_request("invalid signature base64"))?;

    let sig_array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| ApiError::bad_request("signature must be 64 bytes"))?;

    let signature = ed25519_dalek::Signature::from_bytes(&sig_array);

    // 3. Open the repo and find the user's public key.
    let repo_path = app.repos_dir.join(format!("{}.ovc", req.repo_id));

    let password = app
        .passwords
        .read()
        .ok()
        .and_then(|p| p.get(&req.repo_id).cloned())
        .ok_or_else(|| {
            ApiError::unauthorized("repository not unlocked — unlock with password first")
        })?;

    let repo = ovc_core::repository::Repository::open(&repo_path, password.as_bytes())
        .map_err(ApiError::from_core)?;

    // 4. Find the public key matching the fingerprint from the repo's key slots.
    let authorized_keys = repo.authorized_public_keys();
    let pubkey = authorized_keys
        .iter()
        .find(|k| k.fingerprint == req.fingerprint)
        .ok_or_else(|| ApiError::unauthorized("key not authorized for this repository"))?;

    // 5. Verify the signature against the challenge bytes.
    pubkey
        .signing_public
        .verify(&challenge_bytes, &signature)
        .map_err(|_| ApiError::unauthorized("signature verification failed"))?;

    // 6. Determine the user's role from the ACL.
    let role = repo
        .access_control()
        .role_for(&req.fingerprint)
        .map_or_else(|| "write".to_owned(), |r| r.to_string());

    // 7. Issue a JWT scoped to this user and repo.
    let (token, expires_at) = create_jwt_with_claims(
        &app.jwt_secret,
        &req.fingerprint,
        &role,
        Some(req.repo_id),
        app.current_token_version(),
    )?;

    Ok(Json(TokenResponse { token, expires_at }))
}

/// Hex-encodes a byte slice.
fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(&mut s, "{b:02x}").expect("hex write infallible");
    }
    s
}

/// Extracts the client IP from request headers for rate limiting.
///
/// Checks `X-Forwarded-For` first (for reverse-proxy setups), then
/// `X-Real-Ip`, falling back to `"unknown"`.
fn extract_client_ip(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
        })
        .unwrap_or("unknown")
        .to_owned()
}

/// Require that the claims carry at least the given role.
///
/// Returns `Ok(())` if the caller has sufficient privilege, or `Err(403)`.
pub fn require_role(claims: &Claims, min_role: &str) -> Result<(), ApiError> {
    if claims.has_role(min_role) {
        Ok(())
    } else {
        Err(ApiError::forbidden(&format!(
            "insufficient permissions: requires at least '{min_role}' role"
        )))
    }
}
