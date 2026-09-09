//! The three flag types — ALL-CAPS, criticism, anger — plus the four-state
//! visibility matrix that decides which value (automatic or user-overridden)
//! is effective. See
//! [timeline-project-decisions.md §5](../../timeline-project-decisions.md#L216)
//! for why these are three very different detection mechanisms.

pub mod anger;
pub mod caps;
pub mod criticism;
pub mod matrix;
