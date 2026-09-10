//! Constants and key material for the local-only, `_dev`-namespaced
//! testing surface. The signing keypair is generated fresh, in memory,
//! once per process -- never written to disk, never checked into git.
//!
//! An earlier version of this module `include_str!`'d a checked-in PEM
//! file and JWKS. That was a bad idea even though the key was always
//! throwaway and never valid for anything real: committing any private
//! key to source control, fake or not, trains a bad habit and trips
//! automated secret-scanners (GitHub's, TruffleHog, etc.) into flagging
//! an apparent leaked credential. Generating it at runtime removes the
//! concern entirely instead of just explaining it away.

use std::sync::LazyLock;

use jsonwebtoken::jwk::JwkSet;
use rsa::pkcs8::{EncodePrivateKey, LineEnding};
use rsa::traits::PublicKeyParts;
use rsa::RsaPrivateKey;

pub const DEV_ONLY_KID: &str = "dev-only-key-1";
pub const DEV_ONLY_ISSUER: &str = "https://dev-only.invalid/local-testing";
pub const DEV_ONLY_CLIENT_ID: &str = "dev-only-local-client";

/// Generated once, lazily, on first access -- shared by both the local
/// server's `CognitoVerifier` (`main.rs::build_local_state`) and
/// `POST /_dev/login`'s token signing (`routes::dev_login`), so the two
/// sides of one running process always agree on the same key.
pub static DEV_KEYPAIR: LazyLock<(String, JwkSet)> = LazyLock::new(generate_dev_keypair);

fn base64_url(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Generates a throwaway 2048-bit RSA keypair and a matching single-key
/// JWKS (`DEV_ONLY_KID`). Also called directly (not via `DEV_KEYPAIR`) by
/// tests that want their own fresh, self-consistent pair rather than the
/// one shared process-wide static -- see `timeline-api/tests/app.rs`.
pub fn generate_dev_keypair() -> (String, JwkSet) {
    let mut rng = rand::rngs::OsRng;
    let private_key =
        RsaPrivateKey::new(&mut rng, 2048).expect("RSA key generation should not fail");
    let pem = private_key
        .to_pkcs8_pem(LineEnding::LF)
        .expect("PKCS#8 PEM encoding should not fail")
        .to_string();

    let public_key = private_key.to_public_key();
    let n = base64_url(&public_key.n().to_bytes_be());
    let e = base64_url(&public_key.e().to_bytes_be());

    let jwks: JwkSet = serde_json::from_value(serde_json::json!({
        "keys": [{
            "kty": "RSA",
            "kid": DEV_ONLY_KID,
            "use": "sig",
            "alg": "RS256",
            "n": n,
            "e": e,
        }]
    }))
    .expect("hand-built JWKS is always well-formed");

    (pem, jwks)
}
