//! Real S3-backed `ObjectStore`. Tested against a local S3 stand-in
//! (`s3s-fs`, which checks request signatures) in
//! `tests/s3_object_store.rs`: the shared `ObjectStore` contract, presigned
//! URLs used by a plain HTTP client, and rejection of tampered and expired
//! URLs. Not yet run against real S3. Real S3 puts the bucket in the host
//! name, which the local tests can't, and `s3s-fs` doesn't report missing
//! buckets the way S3 does. The real-AWS run in the migration plan's §V2
//! is still needed before V2 can be called done.

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

fn backend_error(e: impl std::error::Error + Send + Sync + 'static) -> ObjectStoreError {
    ObjectStoreError::Backend(Box::new(e))
}

#[async_trait]
impl ObjectStore for S3ObjectStore {
    async fn presign_put(
        &self,
        key: &str,
        expires_in: Duration,
    ) -> Result<String, ObjectStoreError> {
        let config = PresigningConfig::expires_in(expires_in).map_err(backend_error)?;
        let presigned = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(config)
            .await
            .map_err(backend_error)?;
        Ok(presigned.uri().to_string())
    }

    async fn presign_get(
        &self,
        key: &str,
        expires_in: Duration,
    ) -> Result<String, ObjectStoreError> {
        let config = PresigningConfig::expires_in(expires_in).map_err(backend_error)?;
        let presigned = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(config)
            .await
            .map_err(backend_error)?;
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
                Some(GetObjectError::NoSuchKey(_)) => ObjectStoreError::NotFound,
                _ => backend_error(e),
            })?;
        let bytes = output.body.collect().await.map_err(backend_error)?;
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
            .map_err(backend_error)?;
        Ok(())
    }
}
