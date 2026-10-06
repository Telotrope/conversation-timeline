//! Real S3-backed `ObjectStore`. Tested against a local S3 stand-in
//! (`s3s-fs`, which checks request signatures) in
//! `tests/s3_object_store.rs`: the shared `ObjectStore` contract, presigned
//! URLs used by a plain HTTP client, and rejection of tampered and expired
//! URLs. Not yet run against real S3. Real S3 puts the bucket in the host
//! name, which the local tests can't, and `s3s-fs` doesn't report missing
//! buckets the way S3 does. The real-AWS run in the migration plan's §V2
//! is still needed before V2 can be called done.

use crate::aws_failure::report;
use std::time::Duration;

use async_trait::async_trait;
use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::Client;
use timeline_core::ports::errors::ObjectStoreError;
use timeline_core::ports::object_store::ObjectStore;

pub struct S3ObjectStore {
    client: Client,
    bucket: String,
}

impl S3ObjectStore {
    pub fn new(client: Client, bucket: impl Into<String>) -> Self {
        Self {
            client,
            bucket: bucket.into(),
        }
    }
}

/// Maps a failed call to `operation` to a backend error, reporting it for
/// the request's log line (`crate::aws_failure`).
fn backend_error<E: std::error::Error + Send + Sync + 'static>(
    operation: &'static str,
) -> impl FnOnce(E) -> ObjectStoreError {
    move |e| {
        report(operation, &e);
        ObjectStoreError::Backend(Box::new(e))
    }
}

#[async_trait]
impl ObjectStore for S3ObjectStore {
    async fn presign_put(
        &self,
        key: &str,
        expires_in: Duration,
    ) -> Result<String, ObjectStoreError> {
        // Presigning signs a link here, without calling AWS; a failure is
        // still reported, under a name that says so.
        let config = PresigningConfig::expires_in(expires_in)
            .map_err(backend_error("S3.PutObject presign"))?;
        let presigned = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(config)
            .await
            .map_err(backend_error("S3.PutObject presign"))?;
        Ok(presigned.uri().to_string())
    }

    async fn presign_get(
        &self,
        key: &str,
        expires_in: Duration,
    ) -> Result<String, ObjectStoreError> {
        let config = PresigningConfig::expires_in(expires_in)
            .map_err(backend_error("S3.GetObject presign"))?;
        let presigned = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(config)
            .await
            .map_err(backend_error("S3.GetObject presign"))?;
        Ok(presigned.uri().to_string())
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, ObjectStoreError> {
        let output = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| match e.as_service_error() {
                // A missing key is the caller's "no such object" case; any
                // other failure -- including a missing bucket, which is a
                // misconfiguration -- stays a backend error.
                // Reported too: AWS answered the call with an error.
                Some(GetObjectError::NoSuchKey(_)) => {
                    report("S3.GetObject", &e);
                    ObjectStoreError::NotFound
                }
                _ => backend_error("S3.GetObject")(e),
            })?;
        // Reading the body is still part of the GetObject call.
        let bytes = output
            .body
            .collect()
            .await
            .map_err(backend_error("S3.GetObject"))?;
        Ok(bytes.to_vec())
    }

    async fn put(&self, key: &str, data: Vec<u8>) -> Result<(), ObjectStoreError> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(data.into())
            .send()
            .await
            .map_err(backend_error("S3.PutObject"))?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), ObjectStoreError> {
        // S3 answers a delete of a missing key with success, as the port
        // promises.
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(backend_error("S3.DeleteObject"))?;
        Ok(())
    }
}
