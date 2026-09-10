//! Black-box tests for `CognitoVerifier`, using a self-signed test RSA
//! keypair -- there is no real Cognito user pool available in this
//! environment, so these prove the verification *logic* (signature check,
//! issuer, token_use, client_id) is correct; verifying against a real
//! user pool's actual tokens is still needed before V2 can be called done
//! (see the migration plan's V2 test list).

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;
use serde_json::json;
use timeline_auth::cognito::{AuthError, CognitoVerifier};

// A throwaway RSA keypair generated solely for this test file -- never
// used for anything real.
const TEST_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEvwIBADANBgkqhkiG9w0BAQEFAASCBKkwggSlAgEAAoIBAQCj31aKAeOwf1pk
K2LN/lopW9mSXmYnNPxFpG2RyEu4MPXJWSuGkXfiBGdU1hqjrg9rZuuFvSPOnyQJ
/sPJhPx71BtWe9vy7gg5tPIoK6/leV0WCEZOuZfpNEy1gAabSOkrOaiyDfEjw11Q
uadi8+eg/SktgGJhyk2K+RM0AfgLCxZU+e011xshkDZ7JsvskSR65NWrxS7hBWWx
f0IcMRE1XrrDHm9Xt2xNPZZZPkHFkITxlVajZC/6/0yPeQtjiTeclL6CNDblKbAl
VGIIoUIRXtN8vTSe6/PxaELaAPqrGK73ClE13k7ZO8+IlrO9Ww9t9BHq9p+hWNx4
bGQ1mQJfAgMBAAECggEAAVDQtQLcpmpuRt50QuTSsnLu6llC915rRRL7mWyd8u02
U8uQmCnn5Vf9N/OvlJRPoBfWg3NZQniJsN6WGlHrPWMY7c2sPb1Wy9WAkWDXtEqp
6ZjWyc2ZcKK6dchH5JZMhFR19YR0j5QwxRMmvKty3rnYjLOBtf6pxUAYHbst9cyi
5adyJ4s+t0HRED4kupDR8WpXqzz3MS7O6GD9p8AVmSC1ZZY8VOnITcY7Dpb2PR06
RwehMXbdK9b69BVd/rf6IOGAsJefKMJMjiwb87PkhRhJqfRd7p5asrdm27nn8lhQ
ZUVzlkqRmiJ2Ig7d3WMfO8egGFePV9dQAicvfxa6MQKBgQDV7z1E/4vs13PmV7bl
djag5oU/PWGlYakJJtOwLP8mJqLkeum2vdyQaaFd5PIC0ne6FEu8SkIbWxqwmYhR
TVOaVUTb8C17LeIb0tKfFOxI2sX1WStB6PikD5mmAJ4mNOQOWcPXdhKWld83ctr0
xZCXUjLRSMIuth8ZjtYTGcdWxQKBgQDEGCLxUKnqn5EeSgKNsvIbN+cBtu30p+0b
p+7NZ+Td0W7yxIvswo9BTW9QpBkdPrFLvqCdg3c0oYGDI25zYSt+SRz/acNjlpFC
fG+PGp5EyFBXVB8TGVJ8JK7cMpmhrQYfZlbyJNpTARkg4azR8Bw03/WDKcdaRI60
5qY6rnJm0wKBgQCFpsvBQmE5WrTGj7/shLjGNp3CD2fkeSmwVPhlFQdl3zdexEck
amLUOZmdXj2vc6tmre1OuZmpG3aGI7TdDhEP1vuI5/iR/u1GcqQwzFJ9hWesysNS
juhfHnvgEHy848giCwRlpBciyojETFXsG00krC6hPvJJWm/9eJXXIwC8/QKBgQCG
dzae231o0fqlFoMhv6+dUnwqBNKvjeddq45pc/DQ2qiF+JkqxU+OrBbE6YH/N9pD
4ngpCtlXUdiJoGZA4ET+2Av2aQP+6mS5frLRIqOc7u+IsrqMUjTpxA3UGS6YWxlz
tq2wZe0ANiSRE696VnhBGcI1KxT0pUZmbjNW0gDI2QKBgQCztwMys0ILsuhfWrZk
Ee6K5exfbmGTTyuZJY6bOpT/Gn/iPb1oh4cPvQAeMMo+t9WJNUumjUpWv7XPNFom
sLk3FL95Owdyct1cu33lfcm/9/qriAzucNEZ3z4cRA3ivgn4JTxhVJh5K6XLXsl6
yxaADll3PS6Ln8CszrSkfm54Pg==
-----END PRIVATE KEY-----";

const TEST_KEY_N: &str = "o99WigHjsH9aZCtizf5aKVvZkl5mJzT8RaRtkchLuDD1yVkrhpF34gRnVNYao64Pa2brhb0jzp8kCf7DyYT8e9QbVnvb8u4IObTyKCuv5XldFghGTrmX6TRMtYAGm0jpKzmosg3xI8NdULmnYvPnoP0pLYBiYcpNivkTNAH4CwsWVPntNdcbIZA2eybL7JEkeuTVq8Uu4QVlsX9CHDERNV66wx5vV7dsTT2WWT5BxZCE8ZVWo2Qv-v9Mj3kLY4k3nJS-gjQ25SmwJVRiCKFCEV7TfL00nuvz8WhC2gD6qxiu9wpRNd5O2TvPiJazvVsPbfQR6vafoVjceGxkNZkCXw";
const TEST_KEY_E: &str = "AQAB";
const TEST_KID: &str = "test-key-1";

const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_testpool";
const CLIENT_ID: &str = "test-client-id";

fn test_jwks() -> JwkSet {
    serde_json::from_value(json!({
        "keys": [{
            "kty": "RSA",
            "kid": TEST_KID,
            "use": "sig",
            "alg": "RS256",
            "n": TEST_KEY_N,
            "e": TEST_KEY_E,
        }]
    }))
    .expect("test JWK is well-formed")
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
    let key = EncodingKey::from_rsa_pem(TEST_KEY_PEM.as_bytes()).expect("test PEM is valid");
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
