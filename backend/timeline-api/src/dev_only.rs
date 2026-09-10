//! Constants for the local-only, `_dev`-namespaced testing surface: a
//! fixed, checked-in, throwaway RSA keypair used both to build the local
//! dev server's `CognitoVerifier` (see `main.rs::build_local_state`) and to
//! mint tokens for `POST /_dev/login` (see `routes::dev_login`) -- there is
//! no real Cognito pool to log in against locally. Never valid for
//! anything real; never present in the Lambda/production build (see
//! `main.rs` for how the two routers are kept separate).

/// The public half -- what `CognitoVerifier` checks signatures against.
pub const DEV_ONLY_JWKS_JSON: &str = include_str!("../dev_only_test_jwks.json");
/// The private half -- what `POST /_dev/login` signs new tokens with.
/// Matches `DEV_ONLY_JWKS_JSON`'s one key exactly (same modulus/exponent as
/// `timeline-auth/tests/cognito.rs`'s test keypair, just tagged with this
/// crate's own `kid` instead of that test's `test-key-1`).
pub const DEV_ONLY_PRIVATE_KEY_PEM: &str = include_str!("../dev_only_test_private_key.pem");
pub const DEV_ONLY_KID: &str = "dev-only-key-1";
pub const DEV_ONLY_ISSUER: &str = "https://dev-only.invalid/local-testing";
pub const DEV_ONLY_CLIENT_ID: &str = "dev-only-local-client";
