//! `POST /_dev/login` -- mints a bearer token signed with the checked-in
//! throwaway keypair from [`crate::dev_only`], since there is no real
//! Cognito pool to log in against locally. Takes only a display name, no
//! password: this is not an authentication mechanism, it's a stand-in for
//! one, and it must never be reachable in the Lambda/production build (see
//! `main.rs`).

use axum::http::StatusCode;
use axum::Json;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

use crate::dev_only::{DEV_ONLY_CLIENT_ID, DEV_ONLY_ISSUER, DEV_ONLY_KID, DEV_ONLY_PRIVATE_KEY_PEM};

/// Far enough out that a local testing session never has to log in twice;
/// meaningless as a real security boundary, same as the rest of this file.
const DEV_ONLY_TOKEN_EXPIRY: i64 = 9_999_999_999;

#[derive(Deserialize)]
pub struct DevLoginRequest {
    pub sub: String,
}

#[derive(Serialize)]
pub struct DevLoginResponse {
    pub token: String,
}

#[derive(Serialize)]
struct Claims<'a> {
    sub: &'a str,
    iss: &'a str,
    client_id: &'a str,
    token_use: &'a str,
    exp: i64,
}

pub async fn login(
    Json(req): Json<DevLoginRequest>,
) -> Result<Json<DevLoginResponse>, (StatusCode, String)> {
    if req.sub.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "sub must not be empty".to_string()));
    }
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(DEV_ONLY_KID.to_string());
    let claims = Claims {
        sub: &req.sub,
        iss: DEV_ONLY_ISSUER,
        client_id: DEV_ONLY_CLIENT_ID,
        token_use: "access",
        exp: DEV_ONLY_TOKEN_EXPIRY,
    };
    let key = EncodingKey::from_rsa_pem(DEV_ONLY_PRIVATE_KEY_PEM.as_bytes())
        .expect("the checked-in dev-only private key is always well-formed");
    let token = encode(&header, &claims, &key).map_err(|e| {
        eprintln!("failed to sign dev-only token: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to sign token".to_string(),
        )
    })?;
    Ok(Json(DevLoginResponse { token }))
}
