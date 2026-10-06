//! A generic, domain-agnostic blob store: arbitrary bytes kept under a
//! caller-chosen string key. It knows nothing about conversations,
//! uploads, or any other domain type — it's infrastructure, not a domain
//! concept (see `timeline-core/README.md`'s "Domain model" section if
//! `Upload`, `Conversation`, and this port's relationship isn't clear —
//! `Object` here does *not* mean "supertype of `Conversation`").
//!
//! Its one distinguishing capability, `presign_put`/`presign_get`, is what
//! lets a client upload/download a large file (a `conversations.json`
//! export, potentially tens of MB) directly, never routing the bytes
//! through a Lambda — see the migration plan §1.6. That's a real
//! capability every adapter must provide, not an S3-specific detail: the
//! in-memory adapter has to offer it too, as a real local HTTP path a
//! browser can `PUT`/`GET` against (see
//! `timeline_storage::memory::object_store`'s module doc), not a
//! placeholder string.
//!
//! **Key namespacing is a caller convention, not something this trait
//! enforces or even knows about.** The two prefixes this codebase uses:
//! `raw/{user_id}/{upload_id}.json` for an upload's submitted bytes, deleted
//! once processed, and `files/{user_id}/{conversation_id}/{message_id}/{n}`
//! for the files kept from a conversation (see `crate::ports::uploads`).

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

    /// Writes an object, replacing any under the same key.
    async fn put(&self, key: &str, data: Vec<u8>) -> Result<(), ObjectStoreError>;

    /// Removes an object; a key with no object is not an error.
    async fn delete(&self, key: &str) -> Result<(), ObjectStoreError>;
}
