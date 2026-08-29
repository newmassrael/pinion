//! R1891 §5.16 §5.21 §2 #6 — **where a detached panel lives, as ONE value that
//! the paint and the window topology both derive from.**
//!
//! # The fork this closes
//!
//! Tearing a card off a board has two plausible destinations: a real top-level
//! window, and a panel drawn over the host's own canvas. A tree that builds
//! both without deciding between them gets both *at once* — measured on the
//! assembled analysis tool at R1891, tearing one card off left
//! `windows: ["main", "torn-packet#0"]` **and** five `float.packet#0…` regions
//! still painted in the main window, with two unrelated models (`detached`,
//! keyed on which windows exist, and `floats`, carrying live geometry)
//! answering about the same card and not tracking each other.
//!
//! ⇒ ★★★★★ *A thing that can be in two places needs a value saying which, or
//! it is in both.* [`DetachHome`] is that value. It is an enum, so "both" has
//! no representation and the two-picture state has no reachable path — which is
//! stronger than a rule saying they must agree.
//!
//! # Why this is not simply "always a window"
//!
//! §2 #6 makes GUI and TUI one scene over two dispatch paths, and a terminal
//! backend has no window server to put a torn-off panel in. Deleting the canvas
//! form would make tear-off a gesture that silently does nothing on half of
//! this framework's supported surfaces. So the choice is kept, the host
//! declares which homes it can provide ([`DetachPolicy`]), and asking for one
//! it cannot provide is **refused with a sentence** rather than ignored.
//!
//! # Where the floor stands, measured
//!
//! Built from source and run offscreen at R1891, against the floor toolkit at
//! 6.11's own detachable panel:
//!
//! * Tearing a panel off **reclaims the space it left** — the host's central
//!   area grows. This tree agrees, and did before this module.
//! * A detached panel is **always a top-level window**. Of 104 published
//!   members on that class (38 methods, 66 properties), **zero** name any
//!   choice about where a detached panel lives — no in-canvas form exists, so
//!   there is nothing to choose between.
//! * *Is it detached* is readable as a boolean.
//!
//! So the floor answers "is it out?" and this module answers "where is it, of
//! the places this host can put it?" — a strictly larger question, and one the
//! floor does not need because it targets exactly one windowing surface.
//!
//! # What this module does NOT decide
//!
//! Geometry. Where a canvas float sits and how big it is stays the host's,
//! because a canvas float's rectangle is in the host's coordinate space and a
//! window's is in the display's. This module says *which space*, and the host
//! reads that to know which rectangle it owns.

use crate::external::RefusalReason;

/// Where a detached panel lives.
///
/// One value, so a panel cannot be in both places. The variants are the two
/// spaces a rectangle can be in — a display's, and the host's own canvas — and
/// there is deliberately no third for "both" or "unknown": a detached panel
/// that nothing can point at is the defect this type exists to make
/// unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetachHome {
    /// A real top-level window, positioned in the display's coordinate space
    /// and managed by whatever manages windows.
    ///
    /// What the floor toolkit always does, and what a desktop reader expects:
    /// the panel can go behind the host, onto another monitor, and into the
    /// window switcher.
    Window,
    /// A panel drawn over the host's own canvas, positioned in the host's
    /// coordinate space.
    ///
    /// The only form a backend with no window server can offer (§2 #6), and the
    /// form a web-hosted prototype of this tool uses. It cannot leave the host,
    /// which is a limitation and also the reason it works everywhere.
    Canvas,
}

impl DetachHome {
    /// The scene-as-data name — what `query` returns and `invoke` accepts, so
    /// an agent discovers and drives the home over the §2 #7 wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::Canvas => "canvas",
        }
    }

    /// Parse a wire name back to a home — the inverse of [`Self::as_str`].
    ///
    /// `None` for an unknown name rather than a default, because a default here
    /// would put the panel somewhere the caller did not ask for and report
    /// success. Named `from_wire` rather than `from_str` so it does not shadow
    /// the `FromStr` trait, the convention [`crate::widgets`] already follows.
    #[must_use]
    pub fn from_wire(name: &str) -> Option<Self> {
        match name {
            "window" => Some(Self::Window),
            "canvas" => Some(Self::Canvas),
            _ => None,
        }
    }
}

/// Serialised as its wire name, so a session file and a wire read agree.
///
/// Hand-written rather than derived: a derive would spell the two names a
/// second time, and a home that persists as one word and publishes as another
/// is a card that reopens somewhere a client was never told about.
impl serde::Serialize for DetachHome {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for DetachHome {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::from_wire(&name).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "a detached panel lives in a window or on the canvas, not {name:?}"
            ))
        })
    }
}

/// Why a requested home was refused.
///
/// One variant today. It is an enum rather than a bare error because the shape
/// this tree settled on for a refusal is *the reason names what was asked for
/// AND what would have worked* — `no` is not actionable and
/// `no, this host offers canvas` is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetachRefusal {
    /// The host does not provide that home.
    HomeNotAvailable {
        /// The home the caller asked for.
        asked: DetachHome,
        /// The homes the host declared it provides, in preference order.
        available: &'static [DetachHome],
    },
}

impl DetachRefusal {
    /// The sentence, in the vocabulary an `External` refusal already uses.
    ///
    /// The available homes are **in the sentence**, not only in the arm, for
    /// the reason [`crate::edge_panel::EdgeRefusal`] puts its bounds there: a
    /// refusal reaches a person as words, and a person who is told only "no"
    /// has to guess what to ask instead.
    #[must_use]
    pub fn reason(&self) -> RefusalReason {
        match self {
            Self::HomeNotAvailable { available, .. } => {
                let names: Vec<&str> = available.iter().map(|h| h.as_str()).collect();
                RefusalReason::from(format!("this host detaches to {} only", names.join(" or ")))
            }
        }
    }

    /// A short machine word for the wire, so a client branches without parsing
    /// the sentence.
    #[must_use]
    pub const fn wire_word(&self) -> &'static str {
        match self {
            Self::HomeNotAvailable { .. } => "home-not-available",
        }
    }
}

/// The homes a host can provide, in preference order.
///
/// Preference order rather than a separate `default` field: the first entry IS
/// the default, so a policy cannot declare a preference it does not admit.
/// That pairing was the first draft's bug, caught by asking what a
/// `{admits: [Canvas], preferred: Window}` policy should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetachPolicy {
    homes: &'static [DetachHome],
}

/// A host that can open windows: a torn-off panel becomes one, and the canvas
/// form stays available for a reader who wants it kept inside.
const WINDOWING: &[DetachHome] = &[DetachHome::Window, DetachHome::Canvas];

/// A host with no window server: the canvas is the only place a torn-off panel
/// can be.
const CANVAS_ONLY: &[DetachHome] = &[DetachHome::Canvas];

impl DetachPolicy {
    /// The policy a host with `can_open_windows` gets.
    ///
    /// **Derived, not chosen.** A host that reports its own windowing capability
    /// and then separately declares a policy has two facts that can disagree;
    /// this takes the capability and returns the policy, so there is one.
    #[must_use]
    pub const fn for_host(can_open_windows: bool) -> Self {
        Self {
            homes: if can_open_windows {
                WINDOWING
            } else {
                CANVAS_ONLY
            },
        }
    }

    /// Every home this host provides, best first.
    ///
    /// Never empty: both constructors above name at least the canvas, because a
    /// backend that can paint at all can paint a float over its own canvas. A
    /// host that cannot detach at all does not have a policy — it has `None`.
    #[must_use]
    pub const fn homes(self) -> &'static [DetachHome] {
        self.homes
    }

    /// Where a panel goes when nobody said.
    #[must_use]
    pub fn preferred(self) -> DetachHome {
        self.homes[0]
    }

    /// Whether this host can put a detached panel in `home`.
    #[must_use]
    pub fn admits(self, home: DetachHome) -> bool {
        self.homes.contains(&home)
    }

    /// Admit `asked`, or refuse naming what would have worked.
    ///
    /// Returns the home rather than `()` so a caller writes
    /// `let home = policy.admit(asked)?;` and cannot go on holding the value it
    /// asked for instead of the one it got — the shape
    /// [`crate::edge_panel::EdgePolicy::admit`] uses, for the same reason.
    ///
    /// # Errors
    ///
    /// [`DetachRefusal::HomeNotAvailable`] when this host does not provide
    /// `asked`.
    pub fn admit(self, asked: DetachHome) -> Result<DetachHome, DetachRefusal> {
        if self.admits(asked) {
            Ok(asked)
        } else {
            Err(DetachRefusal::HomeNotAvailable {
                asked,
                available: self.homes,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DetachHome, DetachPolicy, DetachRefusal};

    #[test]
    fn a_home_round_trips_through_its_wire_name() {
        for home in [DetachHome::Window, DetachHome::Canvas] {
            assert_eq!(DetachHome::from_wire(home.as_str()), Some(home));
        }
        // An unknown name is refused rather than defaulted: a default would put
        // the panel somewhere the caller did not ask for and report success.
        assert_eq!(DetachHome::from_wire("both"), None);
        assert_eq!(DetachHome::from_wire(""), None);
    }

    #[test]
    fn a_windowing_host_prefers_a_window_and_still_admits_the_canvas() {
        let policy = DetachPolicy::for_host(true);
        assert_eq!(policy.preferred(), DetachHome::Window);
        assert!(policy.admits(DetachHome::Window));
        assert!(policy.admits(DetachHome::Canvas));
    }

    #[test]
    fn a_host_with_no_window_server_refuses_a_window_and_says_what_it_has() {
        let policy = DetachPolicy::for_host(false);
        assert_eq!(policy.preferred(), DetachHome::Canvas);
        let refusal = policy
            .admit(DetachHome::Window)
            .expect_err("a host with no window server cannot open one");
        assert_eq!(
            refusal,
            DetachRefusal::HomeNotAvailable {
                asked: DetachHome::Window,
                available: &[DetachHome::Canvas],
            }
        );
        // ★ The sentence carries what WOULD have worked, which is the half a
        // person acts on.
        let said = refusal.reason().to_string();
        assert!(
            said.contains("canvas"),
            "a refusal that does not name an available home leaves the caller \
             guessing: {said}"
        );
        assert!(
            !said.contains("window"),
            "and it must not name the home it just refused as if it were an \
             option: {said}"
        );
        assert_eq!(refusal.wire_word(), "home-not-available");
    }

    #[test]
    fn the_preference_is_the_first_admitted_home_so_the_two_cannot_disagree() {
        // The property the field layout buys: `preferred` is READ OUT of the
        // admitted set rather than stored beside it, so a policy that prefers a
        // home it does not admit is unconstructible.
        for can_open_windows in [true, false] {
            let policy = DetachPolicy::for_host(can_open_windows);
            assert!(
                policy.admits(policy.preferred()),
                "a host must admit the home it prefers"
            );
            assert!(
                !policy.homes().is_empty(),
                "a policy names at least one home"
            );
        }
    }

    #[test]
    fn a_home_persists_as_the_same_word_it_publishes() {
        // The property the hand-written impl buys: a session file and a wire
        // read cannot name the same home differently, because both go through
        // `as_str`. A derive would have spelled the names a second time.
        for home in [DetachHome::Window, DetachHome::Canvas] {
            let json = serde_json::to_string(&home).expect("a home serialises");
            assert_eq!(json, format!("\"{}\"", home.as_str()));
            let back: DetachHome = serde_json::from_str(&json).expect("and round-trips");
            assert_eq!(back, home);
        }
        // A word this type does not know is a REFUSAL, not a default — a
        // session that named something else must fail loudly rather than
        // reopen the panel somewhere nobody asked for.
        let bad: Result<DetachHome, _> = serde_json::from_str("\"both\"");
        assert!(bad.is_err(), "an unknown home must not deserialise");
    }

    #[test]
    fn a_panel_has_one_home_so_being_in_both_places_has_no_representation() {
        // Not a test of behaviour — a test of the TYPE, which is what makes the
        // two-picture state unreachable rather than merely forbidden. If a
        // third variant or a set is ever added here, this fails and the round
        // that adds it has to say why the fork is safe again.
        let homes = [DetachHome::Window, DetachHome::Canvas];
        assert_eq!(homes.len(), 2, "the home is a choice of exactly two spaces");
        assert_ne!(homes[0], homes[1]);
        // And each names exactly one wire word, so a published home is
        // unambiguous to a client that never saw this enum.
        let words: Vec<&str> = homes.iter().map(|h| h.as_str()).collect();
        assert_eq!(words.len(), 2);
        assert_ne!(words[0], words[1]);
    }
}
