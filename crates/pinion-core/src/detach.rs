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
//! # What this module decides about geometry, and what it still does not
//!
//! ⚠ **R1891 wrote here that this module does not decide geometry, and R1905
//! made that sentence false without touching it.** [`Transfer::cross`] converts
//! a panel's position between the two spaces and bounds where it may land,
//! which is geometry by any reading. It is corrected rather than quietly
//! deleted because the interesting part is *how* it went false: the round that
//! broke the claim had this paragraph in front of it, and a module header is
//! prose that nothing re-performs — the class this tree paid for four rounds
//! running (R1853-R1856), each time in the round that wrote the prose.
//!
//! It decides the **relation between the two spaces**: which space each home
//! measures its rectangle in ([`DetachHome::space`]), how a position crosses
//! between them and whether the crossing kept its place ([`Transfer::cross`],
//! [`Arrival`]), and that a panel arriving in a bounded space lands somewhere
//! its header can still be grabbed.
//!
//! It still does not decide where a float sits *within* a home or how big it
//! is — the host owns that and hands this module the pair, never asks it to
//! choose one. Nor does it hold the display topology, so a host on a monitor
//! left of or above the primary is a stated residue (see [`Transfer`]) rather
//! than a silent wrong answer.

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

    /// The coordinate space a panel living here has its rectangle in.
    ///
    /// **Derived, not declared.** The two facts would otherwise be two fields
    /// that can disagree, and the disagreement is silent: four numbers are four
    /// numbers whichever space they mean. This is [`DetachPolicy::for_host`]'s
    /// argument one level down.
    #[must_use]
    pub const fn space(self) -> Space {
        match self {
            Self::Window => Space::Display,
            Self::Canvas => Space::Host,
        }
    }
}

/// The coordinate space a detached panel's rectangle is measured in.
///
/// # Why this exists as a value at all
///
/// Measured on the assembled analysis tool at R1905, tearing one card off and
/// then sending it to the canvas:
///
/// ```text
/// floats: [{x: 120, y: 40, w: 520, h: 380, home: "window"}]
///         torn-packet#0 position [120, 40]        <- the DISPLAY's space
/// floats: [{x: 120, y: 40, w: 520, h: 380, home: "canvas"}]
///                                                 <- the HOST's space
/// ```
///
/// The identical four numbers, read against two different origins, with nothing
/// in the value saying which — and the panel therefore arriving somewhere
/// nobody chose. ⇒ ★★★★★ *A rectangle without its space is not a place.*
///
/// [`DetachHome`] already distinguishes the two homes, so this is derived from
/// it rather than stored beside it; what the type adds is a NAME for the thing
/// a transfer converts between, so [`Transfer`] can be written down and gated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Space {
    /// The display's, shared by every window on it — where a window manager
    /// places things and where a second monitor has coordinates of its own.
    Display,
    /// The host window's own client area, which is where a canvas float is
    /// painted and where the host's hit test asks its questions.
    Host,
}

impl Space {
    /// The wire word, so a client branches without parsing a sentence.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Display => "display",
            Self::Host => "host",
        }
    }
}

/// Serialised as its wire name, for [`DetachHome`]'s reason.
impl serde::Serialize for Space {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
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

/// How a rectangle arrived in the space it crossed into.
///
/// Three arms and no catch-all, for [`crate::crossing::CrossingPolicy`]'s
/// reason: "the caller did not think about it" must not be spellable as one of
/// the honest answers. A caller that has an [`Arrived`] has been told which of
/// the three happened.
/// ⚠ **No `Default`.** "Nothing has crossed yet" is a different statement from
/// any of these three, and a holder that needs to say it says it with `None` —
/// a default arm here would be an escape hatch that lets an unasked question
/// look like an answered one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Arrival {
    /// The same place on the reader's screen, named in the other space.
    ///
    /// Reachable only when the host can say where its own space begins — see
    /// [`Transfer::new`]. That is the whole point of the seam this arm forced:
    /// an arm no input can produce is decoration, and this tree has shipped one
    /// before and had to withdraw it before release (R1898).
    Kept,
    /// The same place would have left the panel where nothing could reach it,
    /// so it was pulled back by this much.
    ///
    /// The destination is bounded (a host canvas has an extent), and a panel
    /// whose header is outside it cannot be picked up again. R1903 measured the
    /// cost of the weaker rule: a reachability check satisfied by a point
    /// *outside the window* is not a check.
    PulledIn {
        /// How far the panel was moved back horizontally.
        dx: i32,
        /// How far vertically.
        dy: i32,
    },
    /// The offset between the two spaces is not known here, so "the same place"
    /// names no value. The numbers cross unconverted and say so.
    ///
    /// ⚠ This is the honest arm, not the escape hatch: it is what the assembled
    /// tool did *silently* before this type existed. A gate that lets a real
    /// host sit in this arm for ever has stopped testing anything — see the
    /// walk, which asserts the assembled tool is NOT adrift.
    Adrift,
}

impl Arrival {
    /// The wire word, so a client branches without parsing a sentence.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Kept => "kept",
            Self::PulledIn { .. } => "pulled-in",
            Self::Adrift => "adrift",
        }
    }

    /// Whether the panel is where the reader last saw it.
    #[must_use]
    pub const fn is_exact(self) -> bool {
        matches!(self, Self::Kept)
    }
}

/// Where a panel landed, and how it got there.
///
/// The pair rather than a bare point, because a caller that gets only the point
/// cannot tell a converted place from an unconverted one — which is exactly the
/// state R1905 found and the reason this module grew a transfer at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arrived {
    at: (i32, i32),
    how: Arrival,
}

impl Arrived {
    /// The panel's position, in the destination home's space.
    #[must_use]
    pub const fn at(self) -> (i32, i32) {
        self.at
    }

    /// How it got there.
    #[must_use]
    pub const fn how(self) -> Arrival {
        self.how
    }
}

/// The relation between the two spaces a detached panel can live in.
///
/// # What this decides, and why R1891 could not
///
/// R1891 gave a panel a [`DetachHome`] and deliberately left the geometry
/// alone, writing down that "a canvas float's rectangle is in the host's
/// coordinate space and a window's is in the display's" and that the transfer
/// between them was undecided. It was undecided because **the one input it
/// needs did not exist**: the host's own origin. Measured at R1905 through
/// `scene/windows` on the assembled tool, the main window answered
/// `position: None` — a window-manager-placed window, and the framework
/// published no live origin for it, so nothing in the tree *could* convert.
///
/// [`crate::external::window_origin`] is the seam that closed that, and this is
/// the value that consumes it.
///
/// # Why the host's extent is part of the relation
///
/// Because a crossing into the host's space has to land somewhere a reader can
/// still grab. The display's side needs no such bound — where a window may sit
/// is the window manager's judgement and this value does not hold the display
/// topology — so the asymmetry is real and is stated rather than smoothed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transfer {
    origin: Option<(i32, i32)>,
    host: (u32, u32),
}

impl Transfer {
    /// The relation for a host that knows where it is: `origin` is the host
    /// space's top-left corner in the display's space, and `host` is how far
    /// the host's space runs.
    #[must_use]
    pub const fn new(origin: (i32, i32), host: (u32, u32)) -> Self {
        Self {
            origin: Some(origin),
            host,
        }
    }

    /// The relation for a host that cannot say where it is.
    ///
    /// `None` rather than `(0, 0)`: a host at an unknown position is not a host
    /// at the display's corner, and a default here would make every crossing
    /// report [`Arrival::Kept`] while landing the panel wherever the numbers
    /// happened to fall. **A default is an escape hatch that disables its own
    /// gate**, which is the failure this tree names in its standing rules.
    #[must_use]
    pub const fn adrift(host: (u32, u32)) -> Self {
        Self { origin: None, host }
    }

    /// Whether this host can say where its own space begins.
    #[must_use]
    pub const fn knows_offset(self) -> bool {
        self.origin.is_some()
    }

    /// Cross a panel's position from one home's space into another's.
    ///
    /// `size` is the panel's own extent, needed because staying reachable is
    /// about the whole rectangle and not about its corner.
    ///
    /// Crossing to the home it is already in is the identity and reports
    /// [`Arrival::Kept`] — there is no conversion to get wrong, and refusing it
    /// would make every caller special-case a no-op.
    #[must_use]
    pub fn cross(
        self,
        from: DetachHome,
        to: DetachHome,
        at: (i32, i32),
        size: (u32, u32),
    ) -> Arrived {
        if from.space() == to.space() {
            return Arrived {
                at,
                how: Arrival::Kept,
            };
        }
        let Some((ox, oy)) = self.origin else {
            // Unconverted, but still reachable: the bound below is about where
            // a panel can be picked up, and that is true whether or not the
            // offset is known.
            let (at, _) = self.reachable_in(to, at, size);
            return Arrived {
                at,
                how: Arrival::Adrift,
            };
        };
        let crossed = match to.space() {
            Space::Host => (at.0 - ox, at.1 - oy),
            Space::Display => (at.0 + ox, at.1 + oy),
        };
        let (at, moved) = self.reachable_in(to, crossed, size);
        let how = match moved {
            Some((dx, dy)) => Arrival::PulledIn { dx, dy },
            None => Arrival::Kept,
        };
        Arrived { at, how }
    }

    /// Pull `at` back until a panel of `size` can still be grabbed in `to`.
    ///
    /// Returns the landing and how far it had to move, `None` when it did not.
    /// Only the host's space is bounded — see this type's header.
    fn reachable_in(
        self,
        to: DetachHome,
        at: (i32, i32),
        size: (u32, u32),
    ) -> ((i32, i32), Option<(i32, i32)>) {
        if to.space() != Space::Host {
            return (at, None);
        }
        // A panel wider than the host still has to be grabbable, so the upper
        // bound never falls below zero: its corner stays in and the overflow
        // hangs off the far edge. Clamping to a negative would push the header
        // out of reach, which is the thing being prevented.
        let limit =
            |extent: u32, span: u32| i32::try_from(extent.saturating_sub(span)).unwrap_or(0);
        let x = at.0.clamp(0, limit(self.host.0, size.0));
        let y = at.1.clamp(0, limit(self.host.1, size.1));
        let moved = (x != at.0 || y != at.1).then_some((x - at.0, y - at.1));
        ((x, y), moved)
    }
}

#[cfg(test)]
mod tests {
    use super::{Arrival, DetachHome, DetachPolicy, DetachRefusal, Space, Transfer};

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

    /// The two homes are two SPACES, and the mapping is derived from the home
    /// rather than stored beside it.
    #[test]
    fn r1905_each_home_names_the_space_its_rectangle_is_measured_in() {
        assert_eq!(DetachHome::Window.space(), Space::Display);
        assert_eq!(DetachHome::Canvas.space(), Space::Host);
        // Two homes, two spaces: a crossing always has something to convert.
        assert_ne!(DetachHome::Window.space(), DetachHome::Canvas.space());
        assert_eq!(Space::Display.as_str(), "display");
        assert_eq!(Space::Host.as_str(), "host");
    }

    /// ★★★★★ The defect this module grew a transfer for, as an assertion.
    ///
    /// The numbers measured off the running analysis tool at R1905: a card torn
    /// off sits at `(120, 40)`, its window opens at display `(120, 40)`, and
    /// sending it to the canvas left the same pair meaning a host coordinate.
    /// With the host at `(300, 150)` those are two places 335 px apart.
    #[test]
    fn r1905_a_crossing_converts_rather_than_relabelling() {
        let transfer = Transfer::new((300, 150), (1440, 900));
        let arrived = transfer.cross(
            DetachHome::Window,
            DetachHome::Canvas,
            (420, 190),
            (520, 380),
        );
        // Display (420, 190) is host (120, 40) — the same place on the screen.
        assert_eq!(arrived.at(), (120, 40));
        assert_eq!(arrived.how(), Arrival::Kept);
        // And back the other way, which must be the inverse or a card sent to
        // the canvas and returned would drift every trip.
        let back = transfer.cross(
            DetachHome::Canvas,
            DetachHome::Window,
            arrived.at(),
            (520, 380),
        );
        assert_eq!(back.at(), (420, 190));
        assert_eq!(back.how(), Arrival::Kept);
    }

    /// A crossing into the host's space lands somewhere a reader can grab.
    #[test]
    fn r1905_a_panel_crossing_into_the_host_stays_reachable() {
        // The host is at (300, 150) and 1440x900; a window near the far corner
        // of a wider desktop converts to a host coordinate past its edge.
        let transfer = Transfer::new((300, 150), (1440, 900));
        let arrived = transfer.cross(
            DetachHome::Window,
            DetachHome::Canvas,
            (2400, 1200),
            (520, 380),
        );
        let (x, y) = arrived.at();
        assert!(
            x >= 0 && y >= 0 && x + 520 <= 1440 && y + 380 <= 900,
            "the whole panel must be inside the host it crossed into; got {:?}",
            arrived.at()
        );
        assert!(
            matches!(arrived.how(), Arrival::PulledIn { .. }),
            "and it must SAY it was moved, not report the place it was asked for"
        );
        // A panel wider than the host keeps its corner in rather than being
        // pushed out the other side — the header is what gets grabbed.
        let big = transfer.cross(
            DetachHome::Window,
            DetachHome::Canvas,
            (2400, 1200),
            (2000, 1600),
        );
        assert_eq!(big.at(), (0, 0));
    }

    /// The honest arm: a host that cannot say where it is says so.
    #[test]
    fn r1905_a_host_that_cannot_place_itself_reports_an_unconverted_crossing() {
        let transfer = Transfer::adrift((1440, 900));
        assert!(!transfer.knows_offset());
        let arrived = transfer.cross(
            DetachHome::Window,
            DetachHome::Canvas,
            (420, 190),
            (520, 380),
        );
        assert_eq!(arrived.how(), Arrival::Adrift);
        assert!(
            !arrived.how().is_exact(),
            "an unconverted crossing is not the same place"
        );
        // Unconverted is not unbounded: reachability is about where a panel can
        // be picked up and holds whether or not the offset is known.
        let far = transfer.cross(
            DetachHome::Window,
            DetachHome::Canvas,
            (5000, 5000),
            (520, 380),
        );
        assert_eq!(far.at(), (920, 520));
        assert_eq!(far.how(), Arrival::Adrift);
    }

    /// Crossing to the home it is already in is the identity.
    #[test]
    fn r1905_a_crossing_to_the_same_home_moves_nothing() {
        let transfer = Transfer::new((300, 150), (1440, 900));
        for home in [DetachHome::Window, DetachHome::Canvas] {
            let arrived = transfer.cross(home, home, (77, 88), (520, 380));
            assert_eq!(arrived.at(), (77, 88));
            assert_eq!(arrived.how(), Arrival::Kept);
        }
    }

    /// Every arm has a wire word, and they are distinct.
    #[test]
    fn r1905_every_arrival_names_itself_on_the_wire() {
        let arms = [
            Arrival::Kept,
            Arrival::PulledIn { dx: -1, dy: -2 },
            Arrival::Adrift,
        ];
        let words: Vec<&str> = arms.iter().map(|a| a.as_str()).collect();
        assert_eq!(words.len(), 3);
        assert_ne!(words[0], words[1]);
        assert_ne!(words[1], words[2]);
        assert_ne!(words[0], words[2]);
        // Exactly one of them means "where the reader last saw it".
        assert_eq!(arms.iter().filter(|a| a.is_exact()).count(), 1);
        // And it round-trips, so a session or a client reading it back gets the
        // arm that was written and not a default.
        for arm in arms {
            let json = serde_json::to_string(&arm).expect("an arrival serialises");
            let back: Arrival = serde_json::from_str(&json).expect("and round-trips");
            assert_eq!(back, arm);
        }
    }
}
