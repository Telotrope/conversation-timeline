//! In-memory `ObjectStore` -- real bytes in a `Mutex<HashMap>`. Presigned
//! URLs are real, relative HTTP paths under `/_dev/local-storage/...`, not
//! an inert placeholder scheme -- see the migration plan's §V2a: a real
//! browser needs an actual URL it can `PUT`/`GET`, and `timeline-api`'s
//! `_dev`-namespaced routes are what serve that path in local dev, backed
//! by this same adapter. A relative path (no scheme/host) resolves against
//! whatever origin the page fetching it is served from, so it works the
//! same whether the local dev server is on port 3000 or anything else.

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
        Ok(format!("/_dev/local-storage/put/{key}"))
    }

    async fn presign_get(
        &self,
        key: &str,
        _expires_in: Duration,
    ) -> Result<String, ObjectStoreError> {
        Ok(format!("/_dev/local-storage/get/{key}"))
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
