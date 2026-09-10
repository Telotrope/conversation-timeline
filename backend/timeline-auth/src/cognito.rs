//! Verifies a Cognito *access* token (not an ID token -- access tokens are
//! what API Gateway/a route handler should check, since they're what
//! carries authorization for calling an API). Access tokens use `client_id`
//! rather than the more familiar `aud` claim, and carry `token_use: "access"`
//! -- both checked explicitly here, not left to `jsonwebtoken`'s generic
//! audience validation, which doesn't know about this Cognito-specific shape.

use std::fmt;

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, decode_header, DecodingKey, Validation};
use serde::Deserialize;
use timeline_core::ports::ids::UserId;

#[derive(Debug, Deserialize)]
struct AccessTokenClaims {
    sub: String,
    client_id: String,
    token_use: String,
}

#[derive(Debug)]
pub enum AuthError {
    /// The token's header didn't name which signing key it used.
    MissingKeyId,
    /// The named key isn't in this verifier's JWKS -- e.g. the user pool
    /// rotated its signing keys and the cached set is stale.
    UnknownKeyId(String),
    /// Signature verification or claim validation (issuer, expiry, ...)
    /// failed; the original `jsonwebtoken` error is preserved.
    InvalidToken(jsonwebtoken::errors::Error),
    /// A valid, correctly-signed token, but not an *access* token -- e.g.
    /// someone sent an ID token instead.
    WrongTokenUse(String),
    /// A valid access token, but issued for a different app client.
    WrongClientId,
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthError::MissingKeyId => write!(f, "token header has no key id"),
            AuthError::UnknownKeyId(kid) => write!(f, "no signing key found for key id {kid:?}"),
            AuthError::InvalidToken(e) => write!(f, "invalid token: {e}"),
            AuthError::WrongTokenUse(use_) => {
                write!(f, "expected an access token, got token_use {use_:?}")
            }
            AuthError::WrongClientId => write!(f, "token was issued for a different app client"),
        }
    }
}

impl std::error::Error for AuthError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AuthError::InvalidToken(e) => Some(e),
            _ => None,
        }
    }
}

pub struct CognitoVerifier {
    jwks: JwkSet,
    issuer: String,
    client_id: String,
}

impl CognitoVerifier {
    pub fn new(jwks: JwkSet, issuer: impl Into<String>, client_id: impl Into<String>) -> Self {
        Self {
            jwks,
            issuer: issuer.into(),
            client_id: client_id.into(),
        }
    }

    /// Verifies `token`'s signature and claims, returning the caller's
    /// identity if (and only if) it's a valid, unexpired access token
    /// issued by this user pool for this app client.
    pub fn verify(&self, token: &str) -> Result<UserId, AuthError> {
        let header = decode_header(token).map_err(AuthError::InvalidToken)?;
        let kid = header.kid.ok_or(AuthError::MissingKeyId)?;
        let jwk = self
            .jwks
            .find(&kid)
            .ok_or_else(|| AuthError::UnknownKeyId(kid.clone()))?;
        let decoding_key = DecodingKey::from_jwk(jwk).map_err(AuthError::InvalidToken)?;

        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[&self.issuer]);
        // Cognito access tokens carry `client_id`, not the generic `aud`
        // claim jsonwebtoken's built-in audience check expects -- checked
        // manually below instead.
        validation.validate_aud = false;

        let token_data = decode::<AccessTokenClaims>(token, &decoding_key, &validation)
            .map_err(AuthError::InvalidToken)?;
        let claims = token_data.claims;

        if claims.token_use != "access" {
            return Err(AuthError::WrongTokenUse(claims.token_use));
        }
        if claims.client_id != self.client_id {
            return Err(AuthError::WrongClientId);
        }
        Ok(UserId(claims.sub))
    }
}
