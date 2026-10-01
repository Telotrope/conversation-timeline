//! Flag handles: proof that a flag save names a message the server actually
//! sent. See the migration plan's §V2c "Flag handles".
//!
//! Messages aren't stored individually -- they exist only inside the
//! uploaded `conversations.json` -- so the server can't cheaply look up
//! whether a message id named in a `PATCH .../flags` request is real.
//! Instead, `GET /export` hands the page one handle per user message: an
//! HMAC-SHA256 signature over the user id, conversation id and message id,
//! made with a key only the server holds. A save must carry the handle; the
//! server recomputes it from the ids in the request and compares. A made-up
//! id won't match, and nothing has to be stored or looked up. This is the
//! same idea as a presigned S3 URL, which carries its own proof.
//!
//! It does not stop the user from scripting saves: they can read their own
//! handles in the browser. Such a script can only do what clicking already
//! does -- save flags on their own, real messages.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use timeline_core::model::{ConversationId, MessageId};
use timeline_core::ports::ids::UserId;

type HmacSha256 = Hmac<Sha256>;

/// Names the signed format, so a future change to what's signed can't
/// accept handles made under the old one.
const LABEL: &[u8] = b"flag-handle-v1";

/// The environment variable the Lambda reads the key from; see
/// `infra/template.yaml`.
pub const KEY_ENV_VAR: &str = "TIMELINE_FLAG_HANDLE_KEY";

/// Shorter keys are refused: 32 bytes is HMAC-SHA256's own output size, the
/// usual floor for its key.
pub const MIN_KEY_BYTES: usize = 32;

/// A handle as the page sends it back: unpadded base64url text. Kept as its
/// own type so it can't be confused with an id or any other string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FlagHandle(pub String);

/// The server's secret signing key. Deliberately has no `Debug` impl, so it
/// can't end up in a log line by accident.
pub struct FlagHandleKey {
    bytes: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FlagHandleKeyError {
    Missing,
    TooShort { bytes: usize },
}

impl std::fmt::Display for FlagHandleKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlagHandleKeyError::Missing => {
                write!(f, "{KEY_ENV_VAR} is not set; the flag-handle key is required")
            }
            FlagHandleKeyError::TooShort { bytes } => write!(
                f,
                "{KEY_ENV_VAR} is {bytes} bytes; it must be at least {MIN_KEY_BYTES}"
            ),
        }
    }
}

impl std::error::Error for FlagHandleKeyError {}

impl FlagHandleKey {
    /// A fresh random key, for the local dev server. Handles made with it
    /// stop working when the server restarts -- which also wipes the
    /// in-memory data, so the page has to reload anyway.
    pub fn generate() -> Self {
        let mut bytes = vec![0u8; MIN_KEY_BYTES];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        Self { bytes }
    }

    /// The key from the environment variable's value, for the Lambda.
    /// Missing or too short is an error, never a silent fallback to a
    /// generated key: separate Lambda instances would then each have their
    /// own key and reject each other's handles.
    pub fn from_env_value(value: Option<&str>) -> Result<Self, FlagHandleKeyError> {
        let text = value.ok_or(FlagHandleKeyError::Missing)?;
        if text.len() < MIN_KEY_BYTES {
            return Err(FlagHandleKeyError::TooShort { bytes: text.len() });
        }
        Ok(Self {
            bytes: text.as_bytes().to_vec(),
        })
    }

    fn mac_for(&self, user_id: &UserId, conversation_id: ConversationId, message_id: MessageId) -> HmacSha256 {
        let mut mac = HmacSha256::new_from_slice(&self.bytes).expect("HMAC accepts keys of any length");
        mac.update(LABEL);
        // Each field is preceded by its length, so two different id
        // combinations can never produce the same signed bytes -- even if a
        // user id contains whatever character a separator would have used.
        for field in [
            user_id.0.as_str(),
            &conversation_id.to_string(),
            &message_id.to_string(),
        ] {
            mac.update(&(field.len() as u64).to_be_bytes());
            mac.update(field.as_bytes());
        }
        mac
    }

    pub fn handle_for(&self, user_id: &UserId, conversation_id: ConversationId, message_id: MessageId) -> FlagHandle {
        let tag = self.mac_for(user_id, conversation_id, message_id).finalize().into_bytes();
        FlagHandle(URL_SAFE_NO_PAD.encode(tag))
    }

    /// True only if `handle` is the one this key issues for exactly these
    /// ids. The comparison is constant-time (`verify_slice`), so response
    /// timing reveals nothing about how close a guess was.
    pub fn verify(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
        message_id: MessageId,
        handle: &FlagHandle,
    ) -> bool {
        let Ok(tag) = URL_SAFE_NO_PAD.decode(handle.0.as_bytes()) else {
            return false;
        };
        self.mac_for(user_id, conversation_id, message_id)
            .verify_slice(&tag)
            .is_ok()
    }
}
