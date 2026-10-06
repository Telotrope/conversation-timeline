//! Your flags and the automatic ones, and the rules that decide what a
//! message counts as under the two show switches (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §5b).

#[path = "support/entries.rs"]
mod entries;

use entries::*;
use timeline_core::flag_values::{FlagKind, FlagOverrides, MessageFlags};
use timeline_core::flag_view::FlagView;

/// Replaces `flags.test.js` L79 ("only messages with a value under the
/// switches count toward rates"), under all four views.
#[test]
fn counts_toward_rates_under_each_view() {
    let unreviewed = auto(true, false, false);
    let reviewed_one = reviewed(None, Some(false), None);
    let never_scanned = MessageFlags::default();
    for view in [FlagView::Automatic, FlagView::Both] {
        assert!(view.counts_toward_rates(&unreviewed));
        assert!(view.counts_toward_rates(&reviewed_one));
        assert!(view.counts_toward_rates(&never_scanned));
    }
    assert!(!FlagView::Yours.counts_toward_rates(&unreviewed));
    assert!(FlagView::Yours.counts_toward_rates(&reviewed_one));
    assert!(!FlagView::Yours.counts_toward_rates(&never_scanned));
    for flags in [unreviewed, reviewed_one, never_scanned] {
        assert!(!FlagView::Neither.counts_toward_rates(&flags));
    }
}

#[test]
fn which_value_is_in_effect_under_each_view() {
    // Automatic caps; you said it isn't caps, and that it is angry.
    let flags = MessageFlags {
        user: FlagOverrides {
            caps: Some(false),
            critical: None,
            angry: Some(true),
        },
        ..auto(true, true, false)
    };
    let shown = |view: FlagView| {
        FlagKind::ALL
            .iter()
            .map(|k| view.shows(&flags, *k))
            .collect::<Vec<_>>()
    };
    // Order: caps, critical, angry.
    assert_eq!(shown(FlagView::Automatic), vec![true, true, false]);
    assert_eq!(shown(FlagView::Yours), vec![false, false, true]);
    assert_eq!(shown(FlagView::Both), vec![false, true, true]);
    assert_eq!(shown(FlagView::Neither), vec![false, false, false]);
    assert!(FlagView::Both.is_flagged(&flags));
    assert!(!FlagView::Neither.is_flagged(&flags));
    assert!(!FlagView::Yours.is_flagged(&reviewed(Some(false), Some(false), None)));
}

#[test]
fn a_never_scanned_message_has_no_automatic_flags() {
    let flags = MessageFlags::default();
    for kind in FlagKind::ALL {
        assert!(!flags.auto_value(kind));
        assert_eq!(flags.user_value(kind), None);
    }
    assert!(!FlagView::Automatic.is_flagged(&flags));
}

#[test]
fn a_later_review_replaces_only_the_flags_it_names() {
    let earlier = FlagOverrides {
        caps: Some(true),
        critical: Some(false),
        angry: None,
    };
    let later = FlagOverrides {
        caps: None,
        critical: Some(true),
        angry: Some(false),
    };
    assert_eq!(
        earlier.updated_by(later),
        FlagOverrides {
            caps: Some(true),
            critical: Some(true),
            angry: Some(false),
        }
    );
    assert!(earlier.is_review());
    assert!(!FlagOverrides::default().is_review());
    assert!(FlagOverrides {
        angry: Some(false),
        ..FlagOverrides::default()
    }
    .is_review());
}
