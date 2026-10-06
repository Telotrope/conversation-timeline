//! Pure domain logic for the conversation-timeline tool: no I/O, no AWS SDK,
//! no HTTP — a straight, tested port of the non-presentation logic that used
//! to live in `timeline.html`'s inline `<script>`. See
//! [docs/plans/2026-09-09-rust-aws-backend-migration.md](../../docs/plans/2026-09-09-rust-aws-backend-migration.md)
//! for the version this crate implements (V1) and the architecture it's part of.

pub mod branches;
pub mod conversation_metadata;
pub mod dedup;
pub mod flag_values;
pub mod flag_view;
pub mod flags;
pub mod format;
pub mod keep;
pub mod kept_files;
pub mod labels;
pub mod merge;
pub mod message_filter;
pub mod message_time;
pub mod model;
pub mod ports;
pub mod server_analyses;
pub mod sessions;
pub mod stored_message;
pub mod stored_session;
pub mod vader;
pub mod walk_cursor;
pub mod work_budget;

pub use dedup::{dedup_chat_messages, dedup_conversations, extract_text};
pub use format::{unwrap_uploaded_json, unwrap_uploaded_value, FormatError, UnwrapResult};
pub use message_time::{MessageTime, UNKNOWN_TIME};
pub use model::{
    ChatMessage, ContentPiece, Conversation, ConversationId, ConversationName, MessageId,
    PieceType, Sender,
};
pub use sessions::{build_blocks, SessionBlock};
