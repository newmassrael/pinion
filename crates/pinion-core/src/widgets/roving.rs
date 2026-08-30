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

    /// R1699 — the keys that descend into a member which is itself a composite,
    /// in the order `[arrow…, Enter]`.
    ///
    /// The arrow is the **advancing** one of the perpendicular axis, which is
    /// WAI-ARIA APG's toolbar convention, and [`Axis::Both`] therefore offers
    /// none: an axis that navigates by all four arrows has no free one left,
    /// and a key that both moved the cursor and descended would be answering
    /// two questions at once. Derived here rather than declared per composite
    /// for the same reason [`Step::from_key`] is — an entry key that was not
    /// disjoint from [`keys`](Self::keys) is a contradiction the type should
    /// not be able to express, and
    /// `r1699_no_axis_navigates_by_a_key_it_also_enters_by` asserts the
    /// disjointness for every arm.
    #[must_use]
    pub const fn entry_keys(self) -> &'static [&'static str] {
        match self {
            Self::Horizontal => &["ArrowDown", "Enter"],
            Self::Vertical => &["ArrowRight", "Enter"],
            Self::Both => &["Enter"],
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
    /// R1699 — the W3C `KeyboardEvent.key` names that **choose** the member the
    /// cursor rests on.
    ///
    /// [`Explicit`](Self::Explicit) has always documented that "`Enter` or
    /// `Space` chooses" and until this round nothing anywhere implemented the
    /// sentence. Measured by driving both analysis screens: eleven Tab stops,
    /// `Enter` and `Space` at every one of them, **twenty-two presses that
    /// changed nothing painted** — a cursor a reader could walk and never act
    /// on. Publishing the keys from the declaration is what stops the two
    /// drifting apart again: a composite cannot say `Explicit` and answer a
    /// different key, because this list is where both the wire and the key
    /// handler read it.
    ///
    /// [`Follows`](Self::Follows) declares **none**, and that is not an
    /// omission: arriving already chose, so a key that chose again would be a
    /// second way to do what the arrow just did.
    #[must_use]
    pub const fn choose_keys(self) -> &'static [&'static str] {
        match self {
            Self::Follows => &[],
            Self::Explicit => &["Enter", "Space"],
        }
    }

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

/// ★★★★★ R1910 — **what a Tab stop holds INSIDE it**, in three arms, because
/// two of them had been sharing one.
///
/// # The failure this closes
///
/// A focus ring's stops used to declare `Option<RovingSpec>`, and `None` was
/// documented as *a single control, with nothing inside for a cursor to move
/// between*. It was never only that. A screen's board declares `None` too, for
/// the opposite reason: its cursor is SPATIAL — the arrows move to the
/// neighbouring card in a direction rather than to the next item in a list —
/// so it has no roster and very much has a cursor.
///
/// Two different facts, one value, and nothing said which. A client walking
/// the ring could only guess, and the guess held exactly as long as there was
/// one `None` in the table. Measured at R1910: the moment a second stop
/// declared `None` — a button that puts a panel away — the guess broke, and a
/// demo asserting *"no roster, so it must be the spatial one; it still names
/// its cursor"* went red and stayed red for three published rounds.
///
/// ⇒ ★★★★★ *an `Option` whose `None` means two things is a type that cannot be
/// read.* Three arms make the confusion unrepresentable, which is stronger
/// than the sentence in the doc comment that was supposed to prevent it — and
/// that sentence had been WRONG about one of its own two cases the whole time.
///
/// # Why not a bool beside the option
///
/// Because `Some(spec)` + `spatial: true` would be a state nobody means, and a
/// reader would have to know which field wins. Every combination of these
/// three arms is a thing an author can point at on a screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopInterior {
    /// A linear roster: the arrows walk its members, in order, under this
    /// declaration.
    Roster(RovingSpec),
    /// A cursor the arrows move that is **not** a list — a board's is spatial,
    /// moving to the neighbouring card in the direction pressed. There is no
    /// member order to declare, and there IS a cursor: such a stop still
    /// reports the member it rests on as its active descendant.
    Spatial,
    /// Nothing inside. The stop **is** the control, so no arrow does anything
    /// here and no active descendant is owed.
    Single,
}

impl StopInterior {
    /// The word a client reads. One per arm, so a reader never has to infer an
    /// arm from the absence of a key.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Roster(_) => "roster",
            Self::Spatial => "spatial",
            Self::Single => "single",
        }
    }

    /// The roster declaration, for the one arm that has one.
    #[must_use]
    pub const fn roster(self) -> Option<RovingSpec> {
        match self {
            Self::Roster(spec) => Some(spec),
            Self::Spatial | Self::Single => None,
        }
    }

    /// Whether a reader standing here should be told which member the cursor
    /// is on.
    ///
    /// ★ TRUE for both cursor-bearing arms, and that is the predicate the demo
    /// could not ask for. A roster reports the member the arrows last reached;
    /// a spatial cursor reports the card it is over. Only [`Single`](Self::Single)
    /// owes nothing — and owing nothing is a claim, checkable in the same
    /// sweep as the other two rather than an exemption from it.
    #[must_use]
    pub const fn owes_an_active_descendant(self) -> bool {
        matches!(self, Self::Roster(_) | Self::Spatial)
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
    /// R1699 — the composite this member **is**, when it is one.
    ///
    /// Private because it is the one field with an invariant: a member the
    /// cursor has descended into must have somewhere to descend to, so it is
    /// set only through [`containing`](Self::containing) and read through
    /// [`inner`](Self::inner).
    inner: Option<Box<Roving>>,
}

impl Member {
    /// A member that can be chosen.
    #[must_use]
    pub fn new(tag: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            enabled: true,
            inner: None,
        }
    }

    /// A member that can be chosen when `enabled`.
    #[must_use]
    pub fn maybe(tag: impl Into<String>, enabled: bool) -> Self {
        Self {
            tag: tag.into(),
            enabled,
            inner: None,
        }
    }

    /// ★★★★★ R1699 — this member is **itself a composite**, and `inner` is the
    /// cursor that walks what is inside it.
    ///
    /// WAI-ARIA's nesting: the enclosing composite passes over this member in
    /// one step, because a bar containing a tab list should not make a reader
    /// arrow through every tab on the way to the control after it. What that
    /// costs — and what nothing in this module answered before this round — is
    /// a way **in**: measured on the two analysis screens, the one nested
    /// member each has was reachable and had no key that entered it, so the
    /// members inside were unreachable from a keyboard entirely.
    ///
    /// The nesting is recursive rather than one level deep because there is no
    /// non-arbitrary depth to stop at, and because the recursion is what lets
    /// [`Roving::active_descendant`] answer with one walk instead of the
    /// caller keeping a stack.
    #[must_use]
    pub fn containing(mut self, inner: Roving) -> Self {
        self.inner = Some(Box::new(inner));
        self
    }

    /// R1699 — whether this member is itself a composite.
    #[must_use]
    pub const fn is_composite(&self) -> bool {
        self.inner.is_some()
    }

    /// R1699 — the composite inside this member, if it is one.
    #[must_use]
    pub fn inner(&self) -> Option<&Roving> {
        self.inner.as_deref()
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
    /// ★★★★★ R1699 — the reader **chose** the member at this index, which is
    /// what [`Activation::Explicit`] has always promised `Enter` and `Space`
    /// would do.
    ///
    /// A separate arm from [`Self::Moved`]`{ choose: true }` because the two
    /// are different events with different repairs: that one is "the arrow
    /// arrived and the declaration says arriving chooses", this one is "the
    /// reader asked". A composite whose members do something expensive
    /// declares `Explicit` precisely so those are not the same key.
    Chosen(usize),
    /// R1699 — the reader chose a member that **refuses**, and the composite
    /// consumed the key.
    ///
    /// Its own arm rather than a silent no-op: a screen whose whole subject is
    /// that a locked seat is heard (R1694) must say why the seat refused, and
    /// an arm the caller has to match is what makes forgetting to visible. The
    /// key is still consumed, because a refusal is an answer — letting it fall
    /// through to whatever encloses the composite would act somewhere else.
    Refused(usize),
    /// ★★★★★ R1699 — the cursor **descended into** the member at this index,
    /// which is itself a composite. The arrows now move that composite's
    /// cursor, and [`Roving::active_descendant`] names a tag one level deeper.
    Entered(usize),
    /// R1699 — the cursor came back **out** to the member at this index.
    /// `Escape`, which is the key WAI-ARIA APG gives a nested composite for
    /// leaving without also leaving the enclosing one.
    Exited(usize),
}

impl Landing {
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
    /// R1699 — the cursor has descended into the member it rests on.
    ///
    /// State of the **enclosing** composite rather than of the member, because
    /// it is the enclosing one whose arrows stop applying: exactly one member
    /// can be entered at a time, and hanging the flag off the member would let
    /// two of them claim it.
    entered: bool,
}

impl Roving {
    /// R1699 — the key that leaves a nested composite, WAI-ARIA APG's.
    ///
    /// A constant rather than a declaration because a composite that could
    /// choose its own exit key would make leaving unguessable, which is the
    /// one thing an escape hatch must never be.
    pub const EXIT_KEY: &'static str = "Escape";

    /// An empty composite declaring `spec`. It has no cursor until it is
    /// [`seat`](Self::seat)ed.
    #[must_use]
    pub fn new(spec: RovingSpec) -> Self {
        Self {
            spec,
            members: Vec::new(),
            cursor: None,
            entered: false,
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

    /// Where the cursor rests **at this level**, as the member's tag.
    ///
    /// Not the `aria-activedescendant` since R1699 — that is
    /// [`active_descendant`](Self::active_descendant), which answers one level
    /// deeper when the cursor has descended.
    #[must_use]
    pub fn cursor_tag(&self) -> Option<&str> {
        self.cursor
            .and_then(|i| self.members.get(i))
            .map(|m| m.tag.as_str())
    }

    /// R1699 — whether the cursor has descended into the member it rests on.
    #[must_use]
    pub const fn entered(&self) -> bool {
        self.entered
    }

    /// R1699 — the composite the cursor rests on, when that member is one.
    ///
    /// Published so the accessibility tree can give a nested composite its own
    /// `aria-orientation` and roster, which is what lets a client learn what
    /// the inner arrows reach without descending first.
    #[must_use]
    pub fn inner_at_cursor(&self) -> Option<&Roving> {
        self.members.get(self.cursor?)?.inner()
    }

    /// ★★★★★ R1699 — the composite's `aria-activedescendant`: the **innermost**
    /// tag the cursor names.
    ///
    /// One walk rather than a stack the caller keeps, and the reason the
    /// nesting is modelled recursively. While the cursor is at this level it is
    /// [`cursor_tag`](Self::cursor_tag); once it has descended it is whatever
    /// the entered composite's own cursor names, however deep that goes. ARIA
    /// permits exactly this — the attribute addresses any descendant of the
    /// element owning the Tab stop, not only a child.
    #[must_use]
    pub fn active_descendant(&self) -> Option<&str> {
        let member = self.members.get(self.cursor?)?;
        match (self.entered, member.inner()) {
            (true, Some(inner)) => inner.active_descendant().or(Some(&member.tag)),
            _ => Some(&member.tag),
        }
    }

    /// R1699 — the tag the cursor rests on at every level, outermost first.
    ///
    /// What a screen reads to act on a nested cursor: the last element is the
    /// thing chosen and the ones before it say which composites it is inside.
    #[must_use]
    pub fn tag_path(&self) -> Vec<&str> {
        let mut path = Vec::new();
        let mut here = self;
        loop {
            let Some(member) = here.cursor.and_then(|i| here.members.get(i)) else {
                return path;
            };
            path.push(member.tag.as_str());
            match (here.entered, member.inner()) {
                (true, Some(inner)) => here = inner,
                _ => return path,
            }
        }
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
        let previous = core::mem::replace(&mut self.members, members);
        let was = self
            .cursor
            .and_then(|i| previous.get(i))
            .map(|m| m.tag.clone());
        // ★ R1699 — a nested composite's own cursor is state too, and the outer
        // roster is rebuilt every frame. Without this, descending into a tab
        // list and moving to its second tab would be undone by the next paint,
        // which is the same property `seat` already keeps for this level.
        for fresh in &mut self.members {
            if let Some(before) = previous.iter().find(|old| old.tag == fresh.tag)
                && let (Some(now), Some(then)) = (fresh.inner.as_mut(), before.inner())
            {
                now.adopt_cursor_of(then);
            }
        }
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
        // A cursor that had to move to a different member cannot still be
        // inside the one it left.
        if self.cursor_tag() != was.as_deref() {
            self.entered = false;
        }
        if !self
            .members
            .get(self.cursor.unwrap_or(0))
            .is_some_and(Member::is_composite)
        {
            self.entered = false;
        }
    }

    /// R1699 — take `other`'s cursor position and descent, recursively.
    ///
    /// The half of [`seat`](Self::seat) that keeps a nested composite's own
    /// cursor across a re-seat. By tag rather than by index, for the reason
    /// `seat` itself is: the inner roster can change too.
    fn adopt_cursor_of(&mut self, other: &Self) {
        if let Some(tag) = other.cursor_tag() {
            self.point_at(tag);
        }
        self.entered = other.entered
            && self
                .members
                .get(self.cursor.unwrap_or(usize::MAX))
                .is_some_and(Member::is_composite);
        if let (Some(mine), Some(theirs)) = (self.inner_at_cursor_mut(), other.inner_at_cursor()) {
            mine.adopt_cursor_of(theirs);
        }
    }

    /// R1699 — the composite the cursor rests on, mutably.
    ///
    /// Public because a screen that PROJECTS its cursor rather than owning one
    /// has to seat the inner cursor from the same state every frame, which is
    /// what keeps a nested cursor from becoming a second copy of a fact the
    /// screen already holds.
    #[must_use]
    pub fn inner_at_cursor_mut(&mut self) -> Option<&mut Roving> {
        let index = self.cursor?;
        self.members.get_mut(index)?.inner.as_deref_mut()
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
                // R1699 — a pointer that moves the cursor to a different member
                // also brings it back out: a descent belongs to the member it
                // descended into.
                if self.cursor != Some(i) {
                    self.entered = false;
                }
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

    /// ★★★★★ R1699 — descend into the member the cursor rests on, reporting
    /// whether there was anything to descend into.
    pub fn enter(&mut self) -> bool {
        let entering = self
            .cursor
            .and_then(|i| self.members.get(i))
            .is_some_and(Member::is_composite);
        if entering {
            self.entered = true;
        }
        entering
    }

    /// Deliver a W3C `KeyboardEvent.key` name, returning `None` when this
    /// composite does not navigate by that key — the caller must then let the
    /// key fall through rather than swallowing it.
    ///
    /// ★★★★★ R1699 — **innermost first**, which is the whole of the nesting
    /// rule. A key is offered to the composite the cursor has descended into
    /// before this one looks at it, so a vertical list inside a horizontal bar
    /// answers `ArrowDown` while the bar still answers `ArrowRight`, and
    /// `Escape` leaves one level rather than all of them.
    ///
    /// The order after that is not arbitrary either:
    ///
    /// 1. **navigate** — an arrow on this composite's own axis;
    /// 2. **enter** — a key from [`Axis::entry_keys`], and only when the cursor
    ///    rests on a member that is a composite. This is what keeps R1698's
    ///    invariant intact: an off-axis arrow still falls through everywhere
    ///    else, so a vertical gesture enclosing a horizontal bar still works;
    /// 3. **choose** — a key from [`Activation::choose_keys`], which is where
    ///    `Explicit` stops being a word and starts being a behaviour.
    ///
    /// `Enter` appears in both 2 and 3 and the order settles it: you cannot
    /// choose a composite, you go into it.
    pub fn key(&mut self, chord: &str) -> Option<Landing> {
        if self.entered {
            let index = self.cursor?;
            if let Some(inner) = self.inner_at_cursor_mut() {
                if let Some(landing) = inner.key(chord) {
                    return Some(landing);
                }
                if chord == Self::EXIT_KEY {
                    self.entered = false;
                    return Some(Landing::Exited(index));
                }
                return None;
            }
            // The roster changed under the cursor and the member it rests on is
            // no longer a composite. `seat` clears this, so reaching here means
            // somebody mutated the roster another way; recover rather than
            // route a key into nothing.
            self.entered = false;
        }
        if let Some(step) = Step::from_key(self.spec.axis, chord) {
            return Some(self.step(step));
        }
        let index = self.cursor?;
        let member = self.members.get(index)?;
        if member.is_composite() && self.spec.axis.entry_keys().contains(&chord) {
            self.entered = true;
            return Some(Landing::Entered(index));
        }
        if self.spec.activation.choose_keys().contains(&chord) {
            return Some(if member.enabled {
                Landing::Chosen(index)
            } else {
                Landing::Refused(index)
            });
        }
        None
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
        assert!(r.key("End").is_some_and(Landing::moved));
        assert_eq!(r.cursor(), Some(2));
        assert_eq!(r.cursor_tag(), Some("c"));
        assert!(r.key("Home").is_some_and(Landing::moved));
        assert_eq!(r.cursor(), Some(0));
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
            Member::maybe("booked", false),
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
        assert!(r.step(Step::Previous).moved());
        assert_eq!(
            r.cursor(),
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

    /// The inner composite the nesting tests descend into.
    fn tabs() -> Roving {
        let mut inner = Roving::new(RovingSpec::new(Axis::Horizontal).with_ends(Ends::Wrap));
        inner.seat(vec![Member::new("tab.one"), Member::new("tab.two")]);
        inner
    }

    /// A horizontal bar whose middle member is the tab list.
    fn bar() -> Roving {
        let mut outer = Roving::new(RovingSpec::new(Axis::Horizontal));
        outer.seat(vec![
            Member::new("before"),
            Member::new("tabs").containing(tabs()),
            Member::new("after"),
        ]);
        outer
    }

    #[test]
    fn r1699_no_axis_navigates_by_a_key_it_also_enters_by() {
        // The property that lets `entry_keys` be derived instead of declared:
        // a key cannot both move this cursor and descend into a member, or a
        // press would be answering two questions.
        for axis in [Axis::Horizontal, Axis::Vertical, Axis::Both] {
            for key in axis.entry_keys() {
                assert_eq!(
                    Step::from_key(axis, key),
                    None,
                    "{axis} enters by {key} and must not navigate by it"
                );
            }
            assert!(
                axis.entry_keys().contains(&"Enter"),
                "{axis} must always offer a key that does not depend on a free arrow"
            );
        }
        assert_eq!(
            Axis::Both.entry_keys(),
            ["Enter"],
            "an axis that takes all four arrows has none left to enter by"
        );
    }

    #[test]
    fn r1699_the_cross_axis_arrow_enters_a_nested_member_and_escape_leaves() {
        let mut outer = bar();
        outer.key("ArrowRight");
        assert_eq!(outer.cursor_tag(), Some("tabs"));
        assert!(!outer.entered());
        assert_eq!(outer.active_descendant(), Some("tabs"));

        assert_eq!(outer.key("ArrowDown"), Some(Landing::Entered(1)));
        assert!(outer.entered());
        assert_eq!(
            outer.active_descendant(),
            Some("tab.one"),
            "the active descendant is the innermost tag, which is what ARIA addresses"
        );
        assert_eq!(outer.tag_path(), vec!["tabs", "tab.one"]);

        // The inner axis now answers, and the outer one does not.
        assert!(outer.key("ArrowRight").is_some_and(Landing::moved));
        assert_eq!(outer.active_descendant(), Some("tab.two"));
        assert_eq!(
            outer.cursor_tag(),
            Some("tabs"),
            "the enclosing cursor did not move while the reader was inside"
        );

        assert_eq!(outer.key("Escape"), Some(Landing::Exited(1)));
        assert!(!outer.entered());
        assert_eq!(outer.active_descendant(), Some("tabs"));
        assert!(
            outer.key("ArrowRight").is_some_and(Landing::moved),
            "and the outer axis answers again"
        );
        assert_eq!(outer.cursor_tag(), Some("after"));
    }

    #[test]
    fn r1699_enter_descends_into_a_composite_and_chooses_anything_else() {
        let mut outer = bar();
        assert_eq!(
            outer.key("Enter"),
            Some(Landing::Chosen(0)),
            "a plain member is chosen"
        );
        outer.key("ArrowRight");
        assert_eq!(
            outer.key("Enter"),
            Some(Landing::Entered(1)),
            "a composite is entered — you cannot choose one"
        );
        assert_eq!(
            outer.key("Enter"),
            Some(Landing::Chosen(0)),
            "and inside, Enter chooses the inner member"
        );
    }

    #[test]
    fn r1699_a_key_neither_composite_navigates_by_falls_all_the_way_through() {
        let mut outer = bar();
        outer.key("ArrowRight");
        outer.key("ArrowDown");
        assert!(outer.entered());
        assert_eq!(
            outer.key("PageDown"),
            None,
            "an enclosing gesture must still see a key nobody claimed"
        );
        assert!(outer.entered(), "and declining it did not leave");
    }

    #[test]
    fn r1699_an_off_axis_arrow_is_consumed_only_where_there_is_something_to_enter() {
        // R1698's invariant, which this round had to narrow rather than break:
        // the off-axis arrow still falls through at every member that is not a
        // composite.
        let mut outer = bar();
        assert_eq!(outer.cursor_tag(), Some("before"));
        assert_eq!(outer.key("ArrowDown"), None, "nothing to enter here");
        outer.key("ArrowRight");
        assert_eq!(outer.key("ArrowDown"), Some(Landing::Entered(1)));
    }

    #[test]
    fn r1699_explicit_chooses_and_a_refusing_member_says_so() {
        let mut r = Roving::new(RovingSpec::new(Axis::Vertical));
        r.seat(vec![Member::new("open"), Member::maybe("booked", false)]);
        assert_eq!(r.key("Enter"), Some(Landing::Chosen(0)));
        assert_eq!(r.key("Space"), Some(Landing::Chosen(0)));
        r.key("ArrowDown");
        assert_eq!(
            r.key("Enter"),
            Some(Landing::Refused(1)),
            "a locked seat refuses rather than doing nothing quietly"
        );
        assert_eq!(
            r.key("Space"),
            Some(Landing::Refused(1)),
            "and the refusal consumes the key, so nothing enclosing acts instead"
        );
    }

    #[test]
    fn r1699_a_cursor_that_follows_does_not_also_choose_on_enter() {
        let mut r =
            Roving::new(RovingSpec::new(Axis::Vertical).with_activation(Activation::Follows));
        r.seat(vec![Member::new("a"), Member::new("b")]);
        assert!(Activation::Follows.choose_keys().is_empty());
        assert_eq!(
            r.key("Enter"),
            None,
            "arriving already chose, so Enter belongs to whatever encloses this"
        );
        assert_eq!(r.key("Space"), None);
    }

    #[test]
    fn r1699_reseating_keeps_the_inner_cursor_and_the_descent() {
        let mut outer = bar();
        outer.key("ArrowRight");
        outer.key("ArrowDown");
        outer.key("ArrowRight");
        assert_eq!(outer.tag_path(), vec!["tabs", "tab.two"]);

        // The frame repaints and the roster is rebuilt from scratch.
        outer.seat(vec![
            Member::new("before"),
            Member::new("tabs").containing(tabs()),
            Member::new("after"),
        ]);
        assert_eq!(
            outer.tag_path(),
            vec!["tabs", "tab.two"],
            "a repaint must not undo a descent, the way it does not undo a cursor"
        );
        assert!(outer.entered());
    }

    #[test]
    fn r1699_a_cursor_dragged_off_the_entered_member_comes_back_out() {
        let mut outer = bar();
        outer.key("ArrowRight");
        outer.key("ArrowDown");
        assert!(outer.entered());
        assert!(outer.point_at("after"), "a pointer press elsewhere");
        assert!(!outer.entered(), "cannot still be inside what it left");
        assert_eq!(outer.active_descendant(), Some("after"));

        // And the same when the member stops being a composite entirely.
        let mut again = bar();
        again.key("ArrowRight");
        again.key("ArrowDown");
        again.seat(vec![
            Member::new("before"),
            Member::new("tabs"),
            Member::new("after"),
        ]);
        assert!(!again.entered(), "the tab list is gone; there is no inside");
        assert_eq!(again.active_descendant(), Some("tabs"));
    }

    #[test]
    fn r1699_entering_reports_whether_there_was_anywhere_to_go() {
        // `enter` exists for a screen that PROJECTS its descent from state it
        // already holds rather than keeping a `Roving` between frames — the
        // capture viewer, whose row cursor is rebuilt every paint. Leaving has
        // no such caller: `key` clears the descent on `Escape` and `point_at`
        // clears it when the cursor moves off the member, so a public `leave`
        // would be an API whose only user is the test of itself (R1698.1).
        let mut outer = bar();
        assert!(!outer.enter(), "the first member is not a composite");
        assert!(!outer.entered());
        outer.key("ArrowRight");
        assert!(outer.enter());
        assert!(outer.entered());
        assert!(outer.enter(), "entering twice is entering once");
        assert_eq!(outer.key("Escape"), Some(Landing::Exited(1)));
        assert!(!outer.entered(), "and Escape is what comes back out");
    }

    #[test]
    fn r1699_the_nesting_is_recursive_rather_than_one_level_deep() {
        let mut innermost = Roving::new(RovingSpec::new(Axis::Vertical));
        innermost.seat(vec![Member::new("leaf.a"), Member::new("leaf.b")]);
        let mut middle = Roving::new(RovingSpec::new(Axis::Horizontal));
        middle.seat(vec![Member::new("mid").containing(innermost)]);
        let mut outer = Roving::new(RovingSpec::new(Axis::Vertical));
        outer.seat(vec![Member::new("top").containing(middle)]);

        assert_eq!(outer.key("ArrowRight"), Some(Landing::Entered(0)));
        assert_eq!(outer.key("ArrowDown"), Some(Landing::Entered(0)));
        assert_eq!(outer.tag_path(), vec!["top", "mid", "leaf.a"]);
        assert_eq!(outer.active_descendant(), Some("leaf.a"));
        assert!(outer.key("ArrowDown").is_some_and(Landing::moved));
        assert_eq!(outer.active_descendant(), Some("leaf.b"));
        assert_eq!(outer.key("Escape"), Some(Landing::Exited(0)), "one level");
        assert_eq!(outer.tag_path(), vec!["top", "mid"]);
        assert_eq!(outer.key("Escape"), Some(Landing::Exited(0)));
        assert_eq!(outer.tag_path(), vec!["top"]);
        assert_eq!(outer.key("Escape"), None, "and then it falls through");
    }

    #[test]
    fn r1699_a_composite_publishes_what_is_inside_it_before_anybody_descends() {
        let outer = bar();
        assert!(
            outer.inner_at_cursor().is_none(),
            "the first member is plain"
        );
        let mut at_tabs = bar();
        at_tabs.key("ArrowRight");
        let inner = at_tabs.inner_at_cursor().expect("the tab list");
        assert_eq!(inner.members().len(), 2);
        assert_eq!(inner.spec().ends, Ends::Wrap);
        assert!(at_tabs.members()[1].is_composite());
        assert!(!at_tabs.members()[0].is_composite());
    }

    #[test]
    fn r1698_the_wire_spelling_is_distinct_for_every_arm() {
        // The wire is a vocabulary a client matches on, so two arms sharing a
        // word would make a policy unaskable rather than merely ugly.
        let axes: Vec<&str> = [Axis::Horizontal, Axis::Vertical, Axis::Both]
            .iter()
            .map(|a| a.wire())
            .collect();
        assert_eq!(axes.len(), 3);
        assert_eq!(
            axes.iter().collect::<std::collections::BTreeSet<_>>().len(),
            3
        );
        assert_ne!(Ends::Stop.wire(), Ends::Wrap.wire());
        assert_ne!(Activation::Follows.wire(), Activation::Explicit.wire());
    }

    /// ★★★★★ R1910 — **the two cursor-bearing arms are one answer to "is a
    /// cursor owed" and two answers to "is there a roster"**, and it is the
    /// pair of questions that the old `Option` could not tell apart.
    ///
    /// Asserted as a partition rather than three spot checks: every arm is
    /// classified by both predicates, and the two predicates disagree on
    /// exactly one arm. A fourth arm added later has to be given an answer
    /// here, which is the point — an unclassified arm is a red, not a pass.
    #[test]
    fn r1910_a_stops_interior_answers_two_questions_and_they_differ() {
        let roster = StopInterior::Roster(RovingSpec::new(Axis::Vertical));
        let all = [roster, StopInterior::Spatial, StopInterior::Single];

        // Every arm has its own word — a client never infers an arm from a
        // missing key, which is precisely what the `Option` forced.
        let words: std::collections::BTreeSet<&str> = all.iter().map(|i| i.wire()).collect();
        assert_eq!(words.len(), all.len(), "one word per arm: {words:?}");

        // Question one: is there a roster to walk?
        assert!(roster.roster().is_some());
        assert!(StopInterior::Spatial.roster().is_none());
        assert!(StopInterior::Single.roster().is_none());

        // Question two: is an active descendant owed? ★ The answers SPLIT the
        // arms differently, which is the whole reason three arms exist.
        assert!(roster.owes_an_active_descendant());
        assert!(
            StopInterior::Spatial.owes_an_active_descendant(),
            "★ a spatial cursor has no roster and still rests on a member — \
             the case the old `None` shared with a plain button, and the case a \
             demo asserted about a button for three published rounds"
        );
        assert!(
            !StopInterior::Single.owes_an_active_descendant(),
            "★ a single control owes nothing, and that is a CLAIM checked in \
             the same sweep as the other two rather than an exemption from it"
        );

        // The two questions genuinely differ: exactly one arm answers them
        // differently, so neither predicate is the other under another name.
        let split: Vec<&str> = all
            .iter()
            .filter(|i| i.roster().is_some() != i.owes_an_active_descendant())
            .map(|i| i.wire())
            .collect();
        assert_eq!(
            split,
            vec!["spatial"],
            "★ if no arm split them, one predicate would be redundant and the \
             `Option` would have been enough after all"
        );
    }
}
