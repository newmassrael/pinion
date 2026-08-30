//! ★★★★★ R1916 §5.38 §5.40 — **a screen's marks carry sentences about
//! themselves, and one derivation says which one a reader is being shown.**
//!
//! # What forced this, measured
//!
//! The framework has had a tooltip widget since R695, and the assembled
//! analyzer mounted **zero** of them across six pages. Read to the end, R695's
//! own module docs say why in as many words:
//!
//! > the cross-widget "attach a tooltip to an arbitrary existing widget"
//! > primitive is a future axis once a 2nd consumer needs it
//!
//! ⇒ the tooltip is its **own anchor**. It knows when *it* is hovered, and
//! there was no way to say *this other mark, over there, has a sentence*. The
//! debt that recorded the absence wrote `standing_because: 프레임워크에 위젯이
//! 이미 있으므로 막는 것은 없다` — half true, and the half that was not is what
//! this module is.
//!
//! # What it is
//!
//! A [`Descriptions`] is a screen's map from a paint tag to the sentence that
//! mark carries. [`Descriptions::shown`] answers *which sentence a reader is
//! being shown right now*, given a [`Resting`] posture. That is the whole
//! contract, and it is deliberately small: the map is data a screen builds
//! while it builds its scene, and the posture is what the shell already knows.
//!
//! ## ⚠ Hover and focus are ONE vocabulary
//!
//! WCAG 2.2 SC 1.4.13 is *Content on Hover **or Focus***, and this answers both
//! from one call. A pointer-only version would have been shorter and would have
//! made the keyboard reader a second-class one — which matters more here than
//! in most trees, because the behaviour canon this screen reproduces has **zero
//! key bindings**, so copying it exactly is how a keyboard affordance quietly
//! stops existing.
//!
//! ## ⚠ And the dismissal is part of the posture, not of the map
//!
//! SC 1.4.13 also requires the content be *dismissible* without moving the
//! pointer. A latch belongs to the reader's session, not to the mark, so it
//! rides on [`Resting`] — which is what lets one `Descriptions` serve a screen
//! that is shown to two readers.

use std::collections::BTreeMap;

/// ★★★★★ R1916 — the sentences a screen's marks carry, by paint tag.
///
/// Built while the scene is built, so a mark and its sentence are written in
/// one place. A screen that kept a second table keyed by tag would have two
/// facts free to disagree — the class this tree has paid for repeatedly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Descriptions {
    by_tag: BTreeMap<String, String>,
}

impl Descriptions {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Say what the mark under `tag` is for.
    ///
    /// A later call for the same tag replaces the earlier one: the scene is
    /// built once per frame, so the last writer in a frame is the one that
    /// drew it.
    pub fn describe(&mut self, tag: impl Into<String>, sentence: impl Into<String>) {
        self.by_tag.insert(tag.into(), sentence.into());
    }

    /// The sentence `tag` carries, or `None`.
    #[must_use]
    pub fn of(&self, tag: &str) -> Option<&str> {
        self.by_tag.get(tag).map(String::as_str)
    }

    /// How many marks carry one.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_tag.len()
    }

    /// Whether nothing carries one.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_tag.is_empty()
    }

    /// Every described tag, ascending — what a census counts.
    pub fn tags(&self) -> impl Iterator<Item = &str> {
        self.by_tag.keys().map(String::as_str)
    }

    /// ★★★★★ R1916 — **which sentence a reader is being shown**, or `None`.
    ///
    /// The one derivation. A screen asks this and draws what comes back; it
    /// does not carry `hovered == tag` itself, which is the shape the debt this
    /// closes named as the thing to avoid.
    ///
    /// Hover wins over focus when both point somewhere, because a pointer that
    /// has come to rest is the more recent statement of what the reader is
    /// asking about. Both are reported through [`Shown::because`], so a
    /// consumer that wants to draw them differently can.
    #[must_use]
    pub fn shown<'a>(&'a self, resting: &Resting<'a>) -> Option<Shown<'a>> {
        if resting.dismissed {
            return None;
        }
        let hovered = resting
            .hovered
            .and_then(|tag| self.of(tag).map(|s| (tag, s, Because::Hovered)));
        let focused = resting
            .focused
            .and_then(|tag| self.of(tag).map(|s| (tag, s, Because::Focused)));
        let (tag, sentence, because) = hovered.or(focused)?;
        Some(Shown {
            tag,
            sentence,
            because,
        })
    }
}

/// ★ R1916 — where a reader's attention is, and whether they have waved the
/// description away.
///
/// Three fields and not two: SC 1.4.13's *dismissible* obligation is a fact
/// about the reader's session rather than about the mark, so it rides here
/// where a screen shown to two readers can hold two of them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Resting<'a> {
    /// The tag the pointer has come to rest on.
    pub hovered: Option<&'a str>,
    /// The tag holding keyboard focus.
    pub focused: Option<&'a str>,
    /// Whether the reader dismissed the description without moving.
    pub dismissed: bool,
}

impl<'a> Resting<'a> {
    /// A pointer resting on `tag`, nothing focused.
    #[must_use]
    pub const fn hovering(tag: &'a str) -> Self {
        Self {
            hovered: Some(tag),
            focused: None,
            dismissed: false,
        }
    }

    /// Keyboard focus on `tag`, no pointer.
    #[must_use]
    pub const fn focusing(tag: &'a str) -> Self {
        Self {
            hovered: None,
            focused: Some(tag),
            dismissed: false,
        }
    }

    /// This posture with the description waved away.
    #[must_use]
    pub const fn dismissed(mut self) -> Self {
        self.dismissed = true;
        self
    }
}

/// ★ R1916 — the description a reader is being shown, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shown<'a> {
    /// The mark the sentence belongs to.
    pub tag: &'a str,
    /// What it says.
    pub sentence: &'a str,
    /// Which posture is showing it.
    pub because: Because,
}

/// ★ R1916 — why a description is on screen.
///
/// Named rather than left as a boolean, because the two are drawn differently
/// in every toolkit that draws them at all: a hover description follows the
/// pointer and a focus description is anchored to the focused control. A
/// consumer told only "shown" would have to ask a second question to place it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Because {
    /// The pointer is resting on it.
    Hovered,
    /// It holds the keyboard focus.
    Focused,
}

impl Because {
    /// The word this reason is published under.
    #[must_use]
    pub const fn wire_word(self) -> &'static str {
        match self {
            Self::Hovered => "hovered",
            Self::Focused => "focused",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Because, Descriptions, Resting};

    fn two() -> Descriptions {
        let mut d = Descriptions::new();
        d.describe("save", "Saves the current file");
        d.describe("open", "Opens a file");
        d
    }

    #[test]
    fn r1916_a_mark_with_no_sentence_shows_nothing() {
        let d = two();
        assert_eq!(d.shown(&Resting::hovering("quit")), None);
        assert_eq!(d.of("quit"), None);
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn r1916_hover_and_focus_are_one_vocabulary() {
        let d = two();
        let by_pointer = d.shown(&Resting::hovering("save")).expect("shown");
        assert_eq!(by_pointer.sentence, "Saves the current file");
        assert_eq!(by_pointer.because, Because::Hovered);

        // ★★★★★ The keyboard reader gets the same sentence from the same call.
        // A pointer-only derivation would have been shorter and would have made
        // this reader a second-class one.
        let by_key = d.shown(&Resting::focusing("save")).expect("shown");
        assert_eq!(by_key.sentence, by_pointer.sentence);
        assert_eq!(by_key.because, Because::Focused);
        assert_eq!(Because::Focused.wire_word(), "focused");
    }

    #[test]
    fn r1916_a_resting_pointer_wins_over_a_focus_elsewhere() {
        let d = two();
        let both = Resting {
            hovered: Some("open"),
            focused: Some("save"),
            dismissed: false,
        };
        let shown = d.shown(&both).expect("shown");
        assert_eq!(shown.tag, "open", "the pointer is the more recent ask");
        assert_eq!(shown.because, Because::Hovered);

        // ★ And a pointer resting on something UNDESCRIBED does not mask a
        // described focus: the derivation falls through rather than answering
        // nothing, which is what a caller reading `hovered.is_some()` would do.
        let over_nothing = Resting {
            hovered: Some("quit"),
            focused: Some("save"),
            dismissed: false,
        };
        let shown = d.shown(&over_nothing).expect("shown");
        assert_eq!(shown.tag, "save");
        assert_eq!(shown.because, Because::Focused);
    }

    #[test]
    fn r1916_a_dismissal_hides_it_without_moving() {
        let d = two();
        assert_eq!(d.shown(&Resting::hovering("save").dismissed()), None);
        // ★ And it is the POSTURE that is dismissed, not the mark: the same
        // map answers a second reader who has not waved it away.
        assert!(d.shown(&Resting::hovering("save")).is_some());
    }
}
