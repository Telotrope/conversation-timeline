//! In-memory `ObjectStore` -- real bytes in a `Mutex<HashMap>`, "presigned"
//! URLs that are just the key itself prefixed with a fake scheme (nothing
//! actually reads them as URLs in tests; they only need to be distinct,
//! round-trippable strings).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use timeline_core::ports::errors::ObjectStoreError;
use timeline_core::ports::object_store::ObjectStore;

#[derive(Default)]
pub struct InMemoryObjectStore {
    objects: Mutex<HashMap<String, Vec<u8>>>,
}

impl InMemoryObjectStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ObjectStore for InMemoryObjectStore {
    async fn presign_put(
        &self,
        key: &str,
        _expires_in: Duration,
    ) -> Result<String, ObjectStoreError> {
        Ok(format!("memory://put/{key}"))
    }

    async fn presign_get(
        &self,
        key: &str,
        _expires_in: Duration,
    ) -> Result<String, ObjectStoreError> {
        Ok(format!("memory://get/{key}"))
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, ObjectStoreError> {
        self.objects
            .lock()
            .expect("in-memory store mutex poisoned")
            .get(key)
            .cloned()
            .ok_or(ObjectStoreError::NotFound)
    }

    async fn put(&self, key: &str, data: Vec<u8>) -> Result<(), ObjectStoreError> {
        self.objects
            .lock()
            .expect("in-memory store mutex poisoned")
            .insert(key.to_string(), data);
        Ok(())
    }
}
