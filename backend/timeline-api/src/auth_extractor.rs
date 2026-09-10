//! Extracts an authenticated caller's identity from the `Authorization`
//! header, verifying it with `timeline-auth`'s `CognitoVerifier`. Any route
//! handler that takes `AuthenticatedUser` as a parameter gets a real,
//! verified `UserId` or the request never reaches the handler body at all.

use std::sync::Arc;

use axum::extract::{FromRef, FromRequestParts};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use timeline_auth::cognito::CognitoVerifier;
use timeline_core::ports::ids::UserId;

pub struct AuthenticatedUser(pub UserId);

#[derive(Debug)]
pub enum AuthRejection {
    MissingHeader,
    MalformedHeader,
    InvalidToken,
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        let message = match self {
            AuthRejection::MissingHeader => "missing Authorization header",
            AuthRejection::MalformedHeader => "Authorization header must be 'Bearer <token>'",
            AuthRejection::InvalidToken => "invalid or expired token",
        };
        (StatusCode::UNAUTHORIZED, message).into_response()
    }
}

impl<S> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
    Arc<CognitoVerifier>: FromRef<S>,
{
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let verifier = Arc::<CognitoVerifier>::from_ref(state);
        let header = parts
            .headers
            .get(AUTHORIZATION)
            .ok_or(AuthRejection::MissingHeader)?;
        let header_str = header
            .to_str()
            .map_err(|_| AuthRejection::MalformedHeader)?;
        let token = header_str
            .strip_prefix("Bearer ")
            .ok_or(AuthRejection::MalformedHeader)?;
        let user_id = verifier
            .verify(token)
            .map_err(|_| AuthRejection::InvalidToken)?;
        Ok(AuthenticatedUser(user_id))
    }
}
