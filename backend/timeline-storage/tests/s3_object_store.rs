//! `S3ObjectStore` against a local S3 stand-in (`s3s-fs`, which checks
//! request signatures). Runs the shared `ObjectStore` contract, then the
//! checks only a real HTTP endpoint makes possible: presigned URLs used by
//! a plain HTTP client, the way the browser uses them. See the migration
//! plan's §V2b.

#[macro_use]
#[path = "support/object_store_contract.rs"]
mod object_store_contract;
#[path = "support/s3_local.rs"]
mod s3_local;

use std::time::Duration;

use s3_local::LocalS3;
use timeline_core::ports::errors::ObjectStoreError;
use timeline_core::ports::object_store::ObjectStore;
use timeline_storage::s3::S3ObjectStore;

mod contract {
    async fn make() -> (timeline_storage::s3::S3ObjectStore, super::LocalS3) {
        let local = super::LocalS3::start().await;
        (local.object_store(), local)
    }
    object_store_contract!(make);
}

fn http() -> reqwest::Client {
    reqwest::Client::new()
}

#[tokio::test]
async fn a_presigned_put_url_accepts_an_upload_from_a_plain_http_client() {
    let local = LocalS3::start().await;
    let store = local.object_store();
    let url = store
        .presign_put("raw/alice/u1.json", Duration::from_secs(300))
        .await
        .unwrap();
    let res = http()
        .put(&url)
        .body(b"{\"conversations\":[]}".to_vec())
        .send()
        .await
        .unwrap();
    assert!(
        res.status().is_success(),
        "PUT to presigned URL failed: {}",
        res.status()
    );
    assert_eq!(
        store.get("raw/alice/u1.json").await.unwrap(),
        b"{\"conversations\":[]}"
    );
}

#[tokio::test]
async fn a_presigned_get_url_serves_the_object_to_a_plain_http_client() {
    let local = LocalS3::start().await;
    let store = local.object_store();
    store
        .put("export/alice/e1.json", b"exported".to_vec())
        .await
        .unwrap();
    let url = store
        .presign_get("export/alice/e1.json", Duration::from_secs(300))
        .await
        .unwrap();
    let res = http().get(&url).send().await.unwrap();
    assert!(
        res.status().is_success(),
        "GET of presigned URL failed: {}",
        res.status()
    );
    assert_eq!(res.bytes().await.unwrap().as_ref(), b"exported");
}

/// Proves the stand-in really checks signatures, so the two tests above
/// passing means our presigned URLs are correctly signed -- not just that
/// the stand-in accepts anything.
#[tokio::test]
async fn a_presigned_url_with_a_changed_signature_is_rejected() {
    let local = LocalS3::start().await;
    let store = local.object_store();
    store.put("k", b"secret".to_vec()).await.unwrap();
    let url = store
        .presign_get("k", Duration::from_secs(300))
        .await
        .unwrap();
    let tampered = flip_last_signature_char(&url);
    assert_ne!(tampered, url);
    let res = http().get(&tampered).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_presigned_put_url_cannot_be_used_to_read() {
    let local = LocalS3::start().await;
    let store = local.object_store();
    store.put("k", b"secret".to_vec()).await.unwrap();
    let put_url = store
        .presign_put("k", Duration::from_secs(300))
        .await
        .unwrap();
    let res = http().get(&put_url).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_expired_presigned_url_is_rejected() {
    let local = LocalS3::start().await;
    let store = local.object_store();
    store.put("k", b"secret".to_vec()).await.unwrap();
    let url = store
        .presign_get("k", Duration::from_secs(1))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    let res = http().get(&url).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);
}

/// S3 refuses presigned URLs valid for more than 7 days; the adapter must
/// report that as an error rather than hand out an unusable URL.
#[tokio::test]
async fn presigning_for_longer_than_a_week_is_a_backend_error() {
    let local = LocalS3::start().await;
    let store = local.object_store();
    let eight_days = Duration::from_secs(8 * 24 * 60 * 60);
    assert!(matches!(
        store.presign_put("k", eight_days).await,
        Err(ObjectStoreError::Backend(_))
    ));
    assert!(matches!(
        store.presign_get("k", eight_days).await,
        Err(ObjectStoreError::Backend(_))
    ));
}

/// Only a missing *key* is `NotFound`; any other failure must come back as
/// `Backend`, never as `NotFound` (which the API would turn into a
/// misleading 404).
///
/// The natural case to test is a missing bucket, but `s3s-fs` 0.17 can't
/// produce it: its `GetObject` and `PutObject` don't check that the bucket
/// exists (`GetObject` reports `NoSuchKey`, `PutObject` creates the
/// folder), though real S3 returns `NoSuchBucket`. That case is only
/// covered by the real-AWS run in the migration plan's §V2. An unreachable
/// server is the failure this stand-in can produce.
#[tokio::test]
async fn an_unreachable_server_is_a_backend_error_not_not_found() {
    // Bind then release a port, so nothing is listening on it.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let client = s3_local::client_for(&format!("http://127.0.0.1:{port}"));
    let store = S3ObjectStore::new(client, s3_local::BUCKET);
    let get = store.get("k").await;
    assert!(
        matches!(get, Err(ObjectStoreError::Backend(_))),
        "got {get:?}"
    );
    let put = store.put("k", b"x".to_vec()).await;
    assert!(
        matches!(put, Err(ObjectStoreError::Backend(_))),
        "got {put:?}"
    );
}

fn flip_last_signature_char(url: &str) -> String {
    let marker = "X-Amz-Signature=";
    let start = url.find(marker).expect("presigned URL has a signature") + marker.len();
    let end = url[start..].find('&').map_or(url.len(), |i| start + i);
    let last = url.as_bytes()[end - 1];
    let replacement = if last == b'0' { '1' } else { '0' };
    format!("{}{}{}", &url[..end - 1], replacement, &url[end..])
}
