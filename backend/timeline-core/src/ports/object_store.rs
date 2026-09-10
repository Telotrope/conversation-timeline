//! Blob storage port (S3-shaped): the client uploads/downloads large
//! `conversations.json` files and exports directly via presigned URLs,
//! never routing the bytes through a Lambda — see the migration plan §1.6.

use std::time::Duration;

use async_trait::async_trait;

use super::errors::ObjectStoreError;

#[async_trait]
pub trait ObjectStore: Send + Sync {
    /// A URL the client can `PUT` an object to directly, valid for
    /// `expires_in`.
    async fn presign_put(
        &self,
        key: &str,
        expires_in: Duration,
    ) -> Result<String, ObjectStoreError>;

    /// A URL the client can `GET` an object from directly, valid for
    /// `expires_in`.
    async fn presign_get(
        &self,
        key: &str,
        expires_in: Duration,
    ) -> Result<String, ObjectStoreError>;

    /// Reads an object's full contents — used server-side by the upload
    /// processing step, not by anything the client calls directly.
    async fn get(&self, key: &str) -> Result<Vec<u8>, ObjectStoreError>;

    /// Writes an object — used server-side to produce a generated export.
    async fn put(&self, key: &str, data: Vec<u8>) -> Result<(), ObjectStoreError>;
}
