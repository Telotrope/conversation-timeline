//! Cognito access-token verification, per the migration plan section 1.4.
//! Fetching and caching a user pool's JWKS (JSON Web Key Set) from its real
//! `.well-known/jwks.json` endpoint is deliberately not built here yet --
//! there's no real Cognito user pool in this environment to fetch from or
//! verify against. `CognitoVerifier` takes an already-loaded `JwkSet`, so
//! it's fully testable now with a self-signed test key, and only needs a
//! thin "fetch JWKS over HTTP and cache it" wrapper added once a real user
//! pool exists.

pub mod cognito;
