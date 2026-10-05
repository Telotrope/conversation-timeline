//! Pure domain logic for the conversation-timeline tool: no I/O, no AWS SDK,
//! no HTTP — a straight, tested port of the non-presentation logic that used
//! to live in `timeline.html`'s inline `<script>`. See
//! [docs/plans/2026-09-09-rust-aws-backend-migration.md](../../docs/plans/2026-09-09-rust-aws-backend-migration.md)
//! for the version this crate implements (V1) and the architecture it's part of.

pub mod conversation_metadata;
pub mod dedup;
pub mod flags;
pub mod format;
pub mod labels;
pub mod model;
pub mod ports;
pub mod sessions;
pub mod vader;

pub use dedup::{dedup_chat_messages, dedup_conversations, extract_text};
pub use format::{unwrap_uploaded_json, unwrap_uploaded_value, FormatError, UnwrapResult};
pub use model::{
    ChatMessage, ContentPiece, Conversation, ConversationId, ConversationName, MessageId,
    PieceType, Sender,
};
pub use sessions::{build_blocks, SessionBlock};
