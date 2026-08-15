//! R1698 §5.38 §5.39 §5.40 — **the cursor inside a composite.**
//!
//! WAI-ARIA's composite widget pattern is two halves, and until this round the
//! framework only had one of them:
//!
//! 1. the composite owns **one** Tab stop, so a keyboard reaches it in one
//!    press rather than in as many presses as it has members — R1693 and R1696
//!    gave the two analysis screens that;
//! 2. **inside** the composite an arrow moves a cursor between the members, and
//!    the composite publishes where that cursor rests as
//!    `aria-activedescendant`.
//!
//! Without the second half a stop is a room with a door and no floor. Measured
//! on the two screens the day this round opened, by driving both running
//! applications: eleven stops, four arrow keys each, **forty-four presses that
//! moved nothing**, and an active descendant that was `None` at every one.
//!
//! ## What this module is, and what it is not
//!
//! It is the cursor and the policy — an ordered roster of members, an index
//! into it, the W3C key names that move that index, and the three declarations
//! that say *how*. It is deliberately not a widget: it paints nothing, owns no
//! tag, and has no opinion about what a member is. A screen that already has a
//! cursor (a selected row, a selected card) does not grow a second one — it
//! reports the one it has through this vocabulary.
//!
//! ## Why the policy is declared rather than fixed
//!
//! Every composite in a real application answers these questions differently,
//! and a framework that answers them once answers them wrongly for most of its
//! consumers. Measured against the reference toolkit at 6.11.1 by building a
//! probe and running it offscreen, rather than by reading its documentation:
//!
//! | question | there | here |
//! |---|---|---|
//! | which arrows move the cursor | fixed per widget class | [`Axis`] |
//! | what happens at the last member | fixed: it stops, and no property names that choice | [`Ends`] |
//! | does arriving also select | fixed: it always selects, in the one composite that has a cursor at all | [`Activation`] |
//! | may the cursor rest on a member that refuses | fixed: it may not, so a locked member is undiscoverable from the keyboard | it always may — see [`Member`] |
//!
//! The last row is the one worth being exact about, because it is the
//! difference between a policy and a defect. A composite there skips a disabled
//! member, so somebody driving it from the keyboard is never told the member
//! exists. A screen whose whole claim is that its locked seats are **heard**
//! (R1694) cannot use a cursor that hides them, so the cursor here rests on
//! every member and the member's own node carries the refusal and its reason.
//! WAI-ARIA APG allows either convention and recommends this one wherever the
//! existence of the disabled member is itself information.
//!
//! Two further floor measurements, because they are what this module has to
//! beat rather than match: a toolbar there is **not a keyboard destination at
//! all** (its buttons and the bar itself take no focus, so no arrow reaches
//! them), and making its buttons focusable turns one stop into five while the
//! arrows walk straight *out* of the bar — so the choice on offer is no
//! keyboard access or N tab stops with no containment, never one stop with a
//! cursor. And its tab list implements neither `Home` nor `End`.
//!
//! ## Why this is not [`toolbar`](crate::widgets::toolbar)
//!
//! [`Toolbar`](crate::widgets::toolbar::Toolbar) has had a roving cursor since
//! R692, and its own module documentation lists *vertical orientation* and the
//! rest as "future axes once a consumer needs them". Several consumers arrived
//! in one round. This is that lift: the toolbar's cursor is the
//! `Axis::Horizontal` + `Ends::Wrap` + `Activation::Explicit` case of it, and
//! nothing here paints or emits intents, which is what kept the two apart.

use core::fmt;

/// Which arrow keys move a composite's cursor (WAI-ARIA `aria-orientation`).
///
/// A composite laid out left-to-right navigates by `ArrowLeft` / `ArrowRight`
/// and must **ignore** `ArrowUp` / `ArrowDown`, so the key falls through to
/// whatever encloses it — a vertical list of horizontal toolbars is the shape
/// that requires this, and a composite that consumed all four would trap the
/// cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// `ArrowRight` advances, `ArrowLeft` retreats. Lowers to
    /// `aria-orientation="horizontal"`.
    Horizontal,
    /// `ArrowDown` advances, `ArrowUp` retreats. Lowers to
    /// `aria-orientation="vertical"`.
    Vertical,
    /// All four arrows move the cursor along the one linear roster: right and
    /// down advance, left and up retreat. For a composite whose members wrap
    /// across lines, where neither axis alone is the reading order.
    ///
    /// Lowers to **no** `aria-orientation` at all, which is exactly what the
    /// attribute's absence means in ARIA: the orientation is undefined rather
    /// than horizontal.
    Both,
}

impl Axis {
    /// The W3C `KeyboardEvent.key` names this axis navigates by, in the order
    /// `[advance…, retreat…]`.
    ///
    /// Published so a client can be told which keys reach a composite instead
    /// of discovering it by pressing all of them, and so the wire surface and
    /// the key mapping cannot disagree — they are the same list.
    #[must_use]
    pub const fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Horizontal => &["ArrowRight", "ArrowLeft"],
            Self::Vertical => &["ArrowDown", "ArrowUp"],
            Self::Both => &["ArrowRight", "ArrowDown", "ArrowLeft", "ArrowUp"],
        }
    }

    /// The `aria-orientation` value, or `None` when the axis is undefined.
    #[must_use]
    pub const fn aria(self) -> Option<&'static str> {
        match self {
            Self::Horizontal => Some("horizontal"),
            Self::Vertical => Some("vertical"),
            Self::Both => None,
        }
    }

    /// The wire spelling.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
            Self::Both => "both",
        }
    }
}

impl fmt::Display for Axis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.wire())
    }
}

/// What an advance past the last member does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ends {
    /// The cursor holds on the last member. The right answer where the roster
    /// has a meaningful first and last — a byte offset, a document outline —
    /// because wrapping there reads as a jump backwards.
    Stop,
    /// The cursor continues at the first member. The right answer for a ring
    /// of peers: a set of filter chips has no last one.
    Wrap,
}

impl Ends {
    /// The wire spelling.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Wrap => "wrap",
        }
    }
}

/// Whether reaching a member also chooses it.
///
/// WAI-ARIA APG names this distinction for tab lists and leaves it to the
/// author; the reference toolkit does not have it — measured, its tab list
/// changes the current tab on every arrow press and exposes no property that
/// would let an author say otherwise. The difference is not cosmetic: a rail
/// whose selection followed its cursor would navigate away from the page a
/// reader is trying to leave, four times, on the way to the fifth destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    /// Arriving selects. For a cursor that *is* the selection — a message list
    /// where moving down means reading the next message.
    Follows,
    /// Arriving only moves the cursor; `Enter` or `Space` chooses. For a
    /// composite whose members do something when chosen.
    Explicit,
}

impl Activation {
    /// The wire spelling.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Follows => "follows",
            Self::Explicit => "explicit",
        }
    }
}

/// The three declarations that make a composite's keyboard behaviour a
/// specification rather than an accident of whoever wrote its key handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RovingSpec {
    /// Which arrows move the cursor.
    pub axis: Axis,
    /// What an advance past the last member does.
    pub ends: Ends,
    /// Whether arriving at a member also chooses it.
    pub activation: Activation,
}

impl RovingSpec {
    /// A composite that navigates along `axis`, stops at its ends, and requires
    /// an explicit choice — the conservative default, since both other arms
    /// take an action the author did not ask for.
    #[must_use]
    pub const fn new(axis: Axis) -> Self {
        Self {
            axis,
            ends: Ends::Stop,
            activation: Activation::Explicit,
        }
    }

    /// Declare what happens at the ends.
    #[must_use]
    pub const fn with_ends(mut self, ends: Ends) -> Self {
        self.ends = ends;
        self
    }

    /// Declare whether arriving chooses.
    #[must_use]
    pub const fn with_activation(mut self, activation: Activation) -> Self {
        self.activation = activation;
        self
    }
}

/// One member of a composite, in cursor order.
///
/// `enabled` is **not** whether the cursor may rest here — it always may — but
/// whether choosing it does anything. The distinction is the whole point of
/// [`Activation`]: a cursor resting on a locked seat is how a reader is told
/// the seat exists, and choosing it is what refuses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    /// The painted tag, which must also be a node in the accessibility tree —
    /// it becomes this composite's `aria-activedescendant` while the cursor is
    /// here, and a descendant that is not in the tree names nothing.
    pub tag: String,
    /// Whether choosing this member does anything.
    pub enabled: bool,
}

impl Member {
    /// A member that can be chosen.
    #[must_use]
    pub fn new(tag: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            enabled: true,
        }
    }

    /// A member the cursor reaches and choosing refuses.
    #[must_use]
    pub fn locked(tag: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            enabled: false,
        }
    }

    /// A member that can be chosen when `enabled`.
    #[must_use]
    pub fn maybe(tag: impl Into<String>, enabled: bool) -> Self {
        Self {
            tag: tag.into(),
            enabled,
        }
    }
}

/// A cursor movement, named rather than spelled as a signed delta so `First`
/// and `Last` are the same kind of thing as `Next` (WAI-ARIA APG requires
/// `Home` / `End` on a composite; the reference toolkit's tab list implements
/// neither — measured, both keys leave the current tab where it was).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// One member along the axis' advancing direction.
    Next,
    /// One member against it.
    Previous,
    /// The first member (`Home`).
    First,
    /// The last member (`End`).
    Last,
}

impl Step {
    /// Which step a W3C `KeyboardEvent.key` name means to a composite on
    /// `axis`, or `None` when the composite does not navigate by that key.
    ///
    /// The mapping is derived from [`Axis::keys`] rather than restated, so a
    /// composite cannot publish one set of keys and answer another.
    #[must_use]
    pub fn from_key(axis: Axis, chord: &str) -> Option<Self> {
        match chord {
            "Home" => return Some(Self::First),
            "End" => return Some(Self::Last),
            _ => {}
        }
        let keys = axis.keys();
        let half = keys.len() / 2;
        keys.iter()
            .position(|k| *k == chord)
            .map(|i| if i < half { Self::Next } else { Self::Previous })
    }
}

/// Where a [`Step`] left the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landing {
    /// The cursor moved. `choose` is `true` when the composite declared
    /// [`Activation::Follows`], and is the caller's instruction to select what
    /// the cursor reached — the one place the declaration turns into an action,
    /// so an unread arm here is a composite that declared `Follows` and did
    /// not follow.
    Moved {
        /// The index the cursor left.
        from: usize,
        /// The index the cursor reached.
        to: usize,
        /// Select the member at `to`.
        choose: bool,
    },
    /// The cursor was already where the step would take it — at an end under
    /// [`Ends::Stop`], or `Home` when it was already first. Distinct from
    /// [`Self::Nowhere`] because the composite *did* consume the key: nothing
    /// enclosing it should also act on it.
    Held(usize),
    /// The composite has no members, so it has no cursor.
    Nowhere,
}

impl Landing {
    /// The index the cursor rests on afterwards.
    #[must_use]
    pub const fn cursor(self) -> Option<usize> {
        match self {
            Self::Moved { to, .. } => Some(to),
            Self::Held(at) => Some(at),
            Self::Nowhere => None,
        }
    }

    /// Whether the cursor changed position.
    #[must_use]
    pub const fn moved(self) -> bool {
        matches!(self, Self::Moved { .. })
    }
}

/// A composite's roster and the cursor moving over it.
///
/// The roster is re-seated every frame from whatever the composite paints (see
/// [`Roving::seat`]) — membership is dynamic in every real consumer, and a
/// composite that cached a roster would publish members that are no longer on
/// screen.
///
/// ## Why the roster is not the accessibility children
///
/// A container's children are its **structure**; this is what its **arrows
/// reach**, and they are not the same list. Measured on this project's own
/// widget palette: its accessibility children are three section groups and two
/// status readouts, while the thing a cursor walks is the thirteen catalogue
/// entries inside those groups. And measured at the floor for the same
/// distinction: a tab bar of three tabs reports **five** accessible children,
/// so a client there that read the child list to learn what the arrows reach
/// would be told two things that are not members and miss nothing that is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Roving {
    spec: RovingSpec,
    members: Vec<Member>,
    cursor: Option<usize>,
}

impl Roving {
    /// An empty composite declaring `spec`. It has no cursor until it is
    /// [`seat`](Self::seat)ed.
    #[must_use]
    pub fn new(spec: RovingSpec) -> Self {
        Self {
            spec,
            members: Vec::new(),
            cursor: None,
        }
    }

    /// What this composite declared.
    #[must_use]
    pub const fn spec(&self) -> RovingSpec {
        self.spec
    }

    /// The roster, in cursor order.
    #[must_use]
    pub fn members(&self) -> &[Member] {
        &self.members
    }

    /// Where the cursor rests, as an index into [`members`](Self::members).
    #[must_use]
    pub const fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    /// Where the cursor rests, as the member's tag — the composite's
    /// `aria-activedescendant`.
    #[must_use]
    pub fn cursor_tag(&self) -> Option<&str> {
        self.cursor
            .and_then(|i| self.members.get(i))
            .map(|m| m.tag.as_str())
    }

    /// Replace the roster, **keeping the cursor on the same member**.
    ///
    /// The cursor is an identity, not an index: a composite whose roster grows
    /// at the front would otherwise silently move the cursor to a different
    /// member without a key being pressed. If the member the cursor was on is
    /// gone, the cursor takes its old index clamped into the new roster — the
    /// nearest surviving neighbour, which is what a list does when the selected
    /// row is deleted — and an empty roster clears it.
    pub fn seat(&mut self, members: Vec<Member>) {
        let was = self
            .cursor
            .and_then(|i| self.members.get(i))
            .map(|m| m.tag.clone());
        self.members = members;
        self.cursor = if self.members.is_empty() {
            None
        } else if let Some(tag) = was
            .as_deref()
            .and_then(|tag| self.members.iter().position(|m| m.tag == tag))
        {
            Some(tag)
        } else {
            Some(self.cursor.unwrap_or(0).min(self.members.len() - 1))
        };
    }

    /// Put the cursor on `tag`, reporting whether the roster has it.
    ///
    /// This is what a pointer press does: clicking a member is also a way of
    /// moving the cursor there, and a composite whose mouse and keyboard
    /// disagreed about where the cursor is would frame the wrong member the
    /// next time an arrow arrived.
    pub fn point_at(&mut self, tag: &str) -> bool {
        match self.members.iter().position(|m| m.tag == tag) {
            Some(i) => {
                self.cursor = Some(i);
                true
            }
            None => false,
        }
    }

    /// Move the cursor.
    pub fn step(&mut self, step: Step) -> Landing {
        let n = self.members.len();
        if n == 0 {
            self.cursor = None;
            return Landing::Nowhere;
        }
        let from = self.cursor.unwrap_or(0).min(n - 1);
        let to = match step {
            Step::First => 0,
            Step::Last => n - 1,
            Step::Next => match self.spec.ends {
                Ends::Wrap => (from + 1) % n,
                Ends::Stop => (from + 1).min(n - 1),
            },
            Step::Previous => match self.spec.ends {
                Ends::Wrap => (from + n - 1) % n,
                Ends::Stop => from.saturating_sub(1),
            },
        };
        self.cursor = Some(to);
        if to == from {
            Landing::Held(to)
        } else {
            Landing::Moved {
                from,
                to,
                choose: self.spec.activation == Activation::Follows,
            }
        }
    }

    /// Deliver a W3C `KeyboardEvent.key` name, returning `None` when this
    /// composite does not navigate by that key — the caller must then let the
    /// key fall through rather than swallowing it.
    pub fn key(&mut self, chord: &str) -> Option<Landing> {
        Step::from_key(self.spec.axis, chord).map(|step| self.step(step))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn three() -> Roving {
        let mut r = Roving::new(RovingSpec::new(Axis::Horizontal));
        r.seat(vec![Member::new("a"), Member::new("b"), Member::new("c")]);
        r
    }

    #[test]
    fn r1698_a_seated_composite_puts_its_cursor_on_the_first_member() {
        let r = three();
        assert_eq!(r.cursor(), Some(0));
        assert_eq!(r.cursor_tag(), Some("a"));
    }

    #[test]
    fn r1698_an_unseated_composite_has_no_cursor() {
        let r = Roving::new(RovingSpec::new(Axis::Vertical));
        assert_eq!(r.cursor(), None);
        assert_eq!(r.cursor_tag(), None);
        assert!(r.members().is_empty());
    }

    #[test]
    fn r1698_an_arrow_moves_the_cursor_along_the_declared_axis() {
        let mut r = three();
        assert_eq!(
            r.key("ArrowRight"),
            Some(Landing::Moved {
                from: 0,
                to: 1,
                choose: false
            })
        );
        assert_eq!(r.cursor_tag(), Some("b"));
        assert!(r.key("ArrowLeft").is_some_and(Landing::moved));
        assert_eq!(r.cursor_tag(), Some("a"));
    }

    #[test]
    fn r1698_an_arrow_off_the_axis_is_not_consumed() {
        let mut r = three();
        assert_eq!(r.key("ArrowDown"), None, "horizontal ignores the vertical");
        assert_eq!(r.key("ArrowUp"), None);
        assert_eq!(r.cursor_tag(), Some("a"), "and moved nothing doing it");

        let mut v = Roving::new(RovingSpec::new(Axis::Vertical));
        v.seat(vec![Member::new("a"), Member::new("b")]);
        assert_eq!(v.key("ArrowRight"), None, "vertical ignores the horizontal");
        assert!(v.key("ArrowDown").is_some());
    }

    #[test]
    fn r1698_both_axes_navigate_one_linear_roster() {
        let mut r = Roving::new(RovingSpec::new(Axis::Both));
        r.seat(vec![Member::new("a"), Member::new("b"), Member::new("c")]);
        assert!(r.key("ArrowDown").is_some_and(Landing::moved));
        assert_eq!(r.cursor_tag(), Some("b"));
        assert!(r.key("ArrowRight").is_some_and(Landing::moved));
        assert_eq!(r.cursor_tag(), Some("c"));
        assert!(r.key("ArrowUp").is_some_and(Landing::moved));
        assert_eq!(r.cursor_tag(), Some("b"));
        assert!(r.key("ArrowLeft").is_some_and(Landing::moved));
        assert_eq!(r.cursor_tag(), Some("a"));
    }

    #[test]
    fn r1698_the_ends_are_a_declaration_and_both_arms_are_reachable() {
        let mut stops = three();
        stops.step(Step::Last);
        assert_eq!(stops.step(Step::Next), Landing::Held(2), "stop holds");
        assert_eq!(stops.cursor_tag(), Some("c"));

        let mut wraps = Roving::new(RovingSpec::new(Axis::Horizontal).with_ends(Ends::Wrap));
        wraps.seat(vec![Member::new("a"), Member::new("b"), Member::new("c")]);
        wraps.step(Step::Last);
        assert!(wraps.step(Step::Next).moved(), "wrap continues");
        assert_eq!(wraps.cursor_tag(), Some("a"));
        assert!(wraps.step(Step::Previous).moved());
        assert_eq!(wraps.cursor_tag(), Some("c"), "and backwards too");
    }

    #[test]
    fn r1698_home_and_end_reach_the_first_and_last_member() {
        let mut r = three();
        assert_eq!(r.key("End").and_then(Landing::cursor), Some(2));
        assert_eq!(r.cursor_tag(), Some("c"));
        assert_eq!(r.key("Home").and_then(Landing::cursor), Some(0));
        assert_eq!(r.cursor_tag(), Some("a"));
        assert_eq!(r.key("Home"), Some(Landing::Held(0)), "already first");
    }

    #[test]
    fn r1698_arriving_chooses_only_where_the_composite_declared_it() {
        let mut explicit = three();
        assert_eq!(
            explicit.key("ArrowRight"),
            Some(Landing::Moved {
                from: 0,
                to: 1,
                choose: false
            })
        );

        let mut follows =
            Roving::new(RovingSpec::new(Axis::Vertical).with_activation(Activation::Follows));
        follows.seat(vec![Member::new("a"), Member::new("b")]);
        assert_eq!(
            follows.key("ArrowDown"),
            Some(Landing::Moved {
                from: 0,
                to: 1,
                choose: true
            })
        );
    }

    #[test]
    fn r1698_the_cursor_rests_on_a_member_that_refuses() {
        let mut r = Roving::new(RovingSpec::new(Axis::Vertical));
        r.seat(vec![
            Member::new("open"),
            Member::locked("booked"),
            Member::new("also_open"),
        ]);
        assert!(r.key("ArrowDown").is_some_and(Landing::moved));
        assert_eq!(
            r.cursor_tag(),
            Some("booked"),
            "the locked member is reachable, which is how a reader learns it is there"
        );
        assert!(!r.members()[1].enabled, "and it still says it refuses");
    }

    #[test]
    fn r1698_reseating_keeps_the_cursor_on_the_member_and_not_the_index() {
        let mut r = three();
        r.step(Step::Next);
        assert_eq!(r.cursor_tag(), Some("b"));
        r.seat(vec![
            Member::new("new_at_front"),
            Member::new("a"),
            Member::new("b"),
            Member::new("c"),
        ]);
        assert_eq!(
            r.cursor_tag(),
            Some("b"),
            "a member added in front must not drag the cursor with it"
        );
        assert_eq!(r.cursor(), Some(2), "the index followed the member");
    }

    #[test]
    fn r1698_a_cursor_whose_member_is_gone_takes_the_nearest_survivor() {
        let mut r = three();
        r.step(Step::Last);
        assert_eq!(r.cursor(), Some(2));
        r.seat(vec![Member::new("a"), Member::new("b")]);
        assert_eq!(r.cursor(), Some(1), "clamped into the shorter roster");
        assert_eq!(r.cursor_tag(), Some("b"));
        r.seat(Vec::new());
        assert_eq!(r.cursor(), None, "an empty roster has no cursor");
        assert_eq!(r.step(Step::Next), Landing::Nowhere);
    }

    #[test]
    fn r1698_a_press_moves_the_cursor_the_keyboard_will_use_next() {
        let mut r = three();
        assert!(r.point_at("c"));
        assert_eq!(r.cursor_tag(), Some("c"));
        assert_eq!(
            r.step(Step::Previous).cursor(),
            Some(1),
            "the arrow continues from where the pointer left it"
        );
        assert!(!r.point_at("absent"), "a tag off the roster is refused");
        assert_eq!(r.cursor_tag(), Some("b"), "and moves nothing");
    }

    #[test]
    fn r1698_the_published_keys_are_the_keys_that_work() {
        for axis in [Axis::Horizontal, Axis::Vertical, Axis::Both] {
            for key in axis.keys() {
                assert!(
                    Step::from_key(axis, key).is_some(),
                    "{axis} publishes {key} and must navigate by it"
                );
            }
            for key in ["Home", "End"] {
                assert!(Step::from_key(axis, key).is_some(), "{axis} needs {key}");
            }
            for key in ["Enter", " ", "Escape", "Tab", "PageDown"] {
                assert_eq!(
                    Step::from_key(axis, key),
                    None,
                    "{axis} must let {key} fall through"
                );
            }
        }
        assert_eq!(Step::from_key(Axis::Horizontal, "ArrowDown"), None);
        assert_eq!(Step::from_key(Axis::Vertical, "ArrowRight"), None);
    }

    #[test]
    fn r1698_the_aria_orientation_is_absent_exactly_when_it_is_undefined() {
        assert_eq!(Axis::Horizontal.aria(), Some("horizontal"));
        assert_eq!(Axis::Vertical.aria(), Some("vertical"));
        assert_eq!(
            Axis::Both.aria(),
            None,
            "ARIA has no 'both'; the attribute's absence IS undefined"
        );
    }
}
