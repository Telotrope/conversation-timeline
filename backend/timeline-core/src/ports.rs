//! Trait "ports" this crate defines and infrastructure crates (`timeline-storage`)
//! implement — see the migration plan §6.2 ("ports and adapters"). Domain
//! code depends only on these traits, never on a concrete AWS SDK type,
//! which is what lets `timeline-api`'s route handlers be unit-tested with
//! in-memory fakes instead of real AWS.

pub mod analyses;
pub mod conversations;
pub mod errors;
pub mod ids;
pub mod messages;
pub mod object_store;
pub mod sessions;
pub mod uploads;
pub mod user_record;
