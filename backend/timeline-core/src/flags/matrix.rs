//! The four-visibility-state matrix that decides which value — the
//! automatic (heuristic or LLM) detection, or the user's own override — is
//! "effective" for a given flag, given the two global `show_auto`/`show_user`
//! toggles. Exact behavior specified in
//! [timeline-project-decisions.md:266-276](../../../../timeline-project-decisions.md#L266).
//!
//! Port of `hasUserValue`/`effectiveFlag`/`isOverridden` at
//! [timeline.html:65188-65211](../../../../timeline.html#L65188), simplified
//! to operate on already-extracted values rather than a message object and a
//! field-name string — the auto value, the optional user override, and the
//! two toggles are all a caller needs to supply.

/// `user_value` is `Some(_)` exactly when the user has explicitly set this
/// flag on this message (`hasUserValue` in the original); `None` means no
/// opinion stated yet.
pub fn effective_flag(
    auto_value: bool,
    user_value: Option<bool>,
    show_auto: bool,
    show_user: bool,
) -> bool {
    match (show_auto, show_user) {
        // Auto-only view: overrides are ignored entirely (not deleted, just not shown).
        (true, false) => auto_value,
        // Your-tags-only view: auto is not used as a fallback; no stated
        // preference just means "nothing", not "whatever auto thinks".
        (false, true) => user_value.unwrap_or(false),
        (false, false) => false,
        // Both on: normal behavior — your override wins if you've stated one.
        (true, true) => user_value.unwrap_or(auto_value),
    }
}

/// "Overridden" in the ON/ON sense (used for the "auto"/"you" source label).
pub fn is_overridden(user_value: Option<bool>) -> bool {
    user_value.is_some()
}
