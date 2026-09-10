//! Black-box tests for `CognitoVerifier`, using a self-signed test RSA
//! keypair -- there is no real Cognito user pool available in this
//! environment, so these prove the verification *logic* (signature check,
//! issuer, token_use, client_id) is correct; verifying against a real
//! user pool's actual tokens is still needed before V2 can be called done
//! (see the migration plan's V2 test list).

use std::sync::LazyLock;

use base64::Engine;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use rsa::pkcs8::{EncodePrivateKey, LineEnding};
use rsa::traits::PublicKeyParts;
use rsa::RsaPrivateKey;
use serde::Serialize;
use serde_json::json;
use timeline_auth::cognito::{AuthError, CognitoVerifier};

// A throwaway RSA keypair generated once for this whole test binary --
// never written to disk, never checked into git. Committing any private
// key to source control, even one that was never valid for anything real,
// trains a bad habit and trips automated secret-scanners into flagging an
// apparent leaked credential -- generating it at runtime removes the
// concern entirely.
static TEST_KEYPAIR: LazyLock<(String, JwkSet)> = LazyLock::new(generate_test_keypair);
const TEST_KID: &str = "test-key-1";
// A fixed RSA public exponent (65537, the universal default -- not part of
// the generated key, and not a secret), used only by the malformed-JWK
// test below to build a syntactically-plausible-but-corrupt JWK.
const TEST_KEY_E: &str = "AQAB";

const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_testpool";
const CLIENT_ID: &str = "test-client-id";

fn generate_test_keypair() -> (String, JwkSet) {
    let mut rng = rand::rngs::OsRng;
    let private_key =
        RsaPrivateKey::new(&mut rng, 2048).expect("RSA key generation should not fail");
    let pem = private_key
        .to_pkcs8_pem(LineEnding::LF)
        .expect("PKCS#8 PEM encoding should not fail")
        .to_string();

    let public_key = private_key.to_public_key();
    let n = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public_key.n().to_bytes_be());
    let e = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public_key.e().to_bytes_be());

    let jwks: JwkSet = serde_json::from_value(json!({
        "keys": [{"kty": "RSA", "kid": TEST_KID, "use": "sig", "alg": "RS256", "n": n, "e": e}]
    }))
    .expect("hand-built JWKS is always well-formed");

    (pem, jwks)
}

fn test_jwks() -> JwkSet {
    TEST_KEYPAIR.1.clone()
}

fn verifier() -> CognitoVerifier {
    CognitoVerifier::new(test_jwks(), ISSUER, CLIENT_ID)
}

#[derive(Serialize)]
struct TestClaims<'a> {
    sub: &'a str,
    iss: &'a str,
    client_id: &'a str,
    token_use: &'a str,
    exp: i64,
}

fn sign(claims: &TestClaims, kid: &str) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(kid.to_string());
    let key = EncodingKey::from_rsa_pem(TEST_KEYPAIR.0.as_bytes()).expect("test PEM is valid");
    encode(&header, claims, &key).expect("signing a well-formed token must succeed")
}

fn far_future_exp() -> i64 {
    // year 2286 -- comfortably "not expired" without depending on the
    // current wall-clock time in a test.
    9_999_999_999
}

fn valid_claims() -> TestClaims<'static> {
    TestClaims {
        sub: "user-123",
        iss: ISSUER,
        client_id: CLIENT_ID,
        token_use: "access",
        exp: far_future_exp(),
    }
}

#[test]
fn valid_access_token_verifies_to_the_correct_user() {
    let token = sign(&valid_claims(), TEST_KID);
    let user_id = verifier()
        .verify(&token)
        .expect("a well-formed, correctly-signed token must verify");
    assert_eq!(user_id.0, "user-123");
}

#[test]
fn wrong_client_id_is_rejected() {
    let claims = TestClaims {
        client_id: "some-other-client",
        ..valid_claims()
    };
    let token = sign(&claims, TEST_KID);
    assert!(matches!(
        verifier().verify(&token),
        Err(AuthError::WrongClientId)
    ));
}

#[test]
fn id_token_is_rejected_as_the_wrong_token_use() {
    let claims = TestClaims {
        token_use: "id",
        ..valid_claims()
    };
    let token = sign(&claims, TEST_KID);
    assert!(matches!(verifier().verify(&token), Err(AuthError::WrongTokenUse(u)) if u == "id"));
}

#[test]
fn unknown_key_id_is_rejected() {
    let token = sign(&valid_claims(), "some-other-key");
    assert!(
        matches!(verifier().verify(&token), Err(AuthError::UnknownKeyId(kid)) if kid == "some-other-key")
    );
}

#[test]
fn wrong_issuer_is_rejected() {
    let claims = TestClaims {
        iss: "https://not-the-real-issuer.example",
        ..valid_claims()
    };
    let token = sign(&claims, TEST_KID);
    assert!(matches!(
        verifier().verify(&token),
        Err(AuthError::InvalidToken(_))
    ));
}

#[test]
fn expired_token_is_rejected() {
    let claims = TestClaims {
        exp: 1,
        ..valid_claims()
    }; // 1970, long expired
    let token = sign(&claims, TEST_KID);
    assert!(matches!(
        verifier().verify(&token),
        Err(AuthError::InvalidToken(_))
    ));
}

#[test]
fn garbage_is_rejected_not_panicked_on() {
    assert!(verifier().verify("not.a.jwt").is_err());
    assert!(verifier().verify("").is_err());
}

#[test]
fn auth_error_variants_have_readable_messages() {
    assert_eq!(
        AuthError::MissingKeyId.to_string(),
        "token header has no key id"
    );
    assert_eq!(
        AuthError::UnknownKeyId("abc".to_string()).to_string(),
        "no signing key found for key id \"abc\""
    );
    assert_eq!(
        AuthError::WrongTokenUse("id".to_string()).to_string(),
        "expected an access token, got token_use \"id\""
    );
    assert_eq!(
        AuthError::WrongClientId.to_string(),
        "token was issued for a different app client"
    );
}

#[test]
fn invalid_token_error_preserves_its_source() {
    use std::error::Error;
    let token = sign(&valid_claims(), TEST_KID);
    // A token that's simply garbled produces an InvalidToken with a real
    // jsonwebtoken source error underneath.
    let garbled = format!("{token}garbage");
    let err = CognitoVerifier::new(test_jwks(), ISSUER, CLIENT_ID)
        .verify(&garbled)
        .unwrap_err();
    assert!(matches!(err, AuthError::InvalidToken(_)));
    assert!(err.source().is_some());
    assert!(!err.to_string().is_empty());
}

#[test]
fn a_malformed_jwk_is_rejected_as_an_invalid_token_not_a_panic() {
    let bad_jwks: JwkSet = serde_json::from_value(json!({
        "keys": [{
            "kty": "RSA",
            "kid": TEST_KID,
            "use": "sig",
            "alg": "RS256",
            "n": "not-valid-base64url-!!!",
            "e": TEST_KEY_E,
        }]
    }))
    .expect("still well-formed enough to deserialize as a Jwk");
    let token = sign(&valid_claims(), TEST_KID);
    let verifier = CognitoVerifier::new(bad_jwks, ISSUER, CLIENT_ID);
    assert!(matches!(
        verifier.verify(&token),
        Err(AuthError::InvalidToken(_))
    ));
}
