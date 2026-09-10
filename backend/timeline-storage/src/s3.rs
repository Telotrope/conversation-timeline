//! Real S3-backed `ObjectStore`. Compiles, but has zero test coverage --
//! not even private-function tests, since this file has no logic of its
//! own to unit-test (keys are passed in by the caller; there's no
//! key-naming logic here to verify in isolation). Has not been run
//! against real or LocalStack S3 in this environment -- no AWS
//! credentials or Docker were available. See the migration plan V2 test
//! list: running this against real S3 is one of the steps still needed
//! before V2 can be called done.

use std::time::Duration;

use async_trait::async_trait;
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
            .map_err(backend_error)?;
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
