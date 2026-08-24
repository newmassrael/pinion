//! R1712 §5.16 §5.32 §5.12 §2 #3 §2 #7 — **what a window gives up to get
//! smaller**, declared once and checked against the screen.
//!
//! # The two numbers this tree spelled as one
//!
//! A screen has two minimums and they are not the same fact:
//!
//! * the size below which its **layout stops reflowing** — below it the window
//!   clips instead of rearranging, which is what
//!   [`external::layout_size`](crate::external::layout_size) implements;
//! * the size below which the **window refuses to shrink** — what the window
//!   system is told, and what [`SizeBounds`](crate::size_grant::SizeBounds)
//!   enforces since R1710.
//!
//! Measured before this module existed, all three screens of the analysis tool
//! passed **one constant to both**. That is not a tidy coincidence: one number
//! cannot express the decision "let the reader make this window smaller than
//! the size it lays out at, and here is what that costs", so the decision could
//! not be made. The node lab's constant is 1625 wide, and a 1600-pixel display
//! therefore could not open the screen at all — a loss R1689 wrote down and
//! nothing could act on.
//!
//! # What the reference does, measured
//!
//! Built as a probe against 6.11.1 and run offscreen, on the analysis tool's
//! own shape (fixed rail, fixed palette, stretchy middle, fixed inspector):
//!
//! | question | answer |
//! |---|---|
//! | is there a layout minimum, derived from the content? | **yes** — 1008x120 here |
//! | is it enforced? | **yes**, client side: a resize 119 short lands back at 1008 |
//! | can the window floor be put **below** it? | ★ **yes** — declaring the window's own minimum suppresses the layout's, and the window really goes to 889 while the layout keeps saying 1008 |
//! | is a contradictory pair refused? | **no** — a floor of 889 and a ceiling of 800 are both held and both reported |
//! | does anything say what the band costs? | **no** — the pane that gets sliced is reported visible, and no accessor counts or names what is clipped |
//! | can a caller ask about a size the window is not at? | **no** — the only way is to resize, look, and put it back |
//! | is the concession announced? | **no** — crossing the boundary delivers a move and a paint, and nothing that means "you are being given up" |
//! | members whose name carries the idea | **0** of 139 properties + 73 methods on its widget, layout and scroll-area classes |
//!
//! So the *capability* is parity — the reference has two numbers and this tree
//! had one — and everything about **the band being a declaration that can be
//! checked** is new here. That split is why this module exists at all rather
//! than a second field on a window spec: a floor below the layout minimum is a
//! promise about what the reader loses, and a promise nothing checks is the
//! shape this project keeps finding rotted.
//!
//! # The shape
//!
//! [`ShrinkPolicy`] holds **both** numbers plus the names of what the band
//! gives up, and it is the only place either number is written: the layout
//! clamp reads [`ShrinkPolicy::comfortable`] and the window floor reads
//! [`ShrinkPolicy::floor`], so they cannot drift. A pair that contradicts
//! itself is refused, and — because the constructors are `const` — a binding
//! that declares one **fails to compile** rather than opening a window whose
//! floor is above the size it lays out at.
//!
//! [`audit`] is the other half: given what the floor actually does to the
//! screen, it says whether the declaration was honest. A screen that starts
//! giving up something it never admitted to reads `surprised`, which is a
//! defect; one whose declaration has outlived what it describes reads `stale`;
//! and a floor that puts something out of reach altogether reads `unreachable`,
//! which no concession can excuse — clipping is a decision a screen is allowed
//! to make, and losing is not.
//!
//! # What this module deliberately does not decide
//!
//! It does not choose a screen's floor. That is a product decision — how small
//! is *usable* is not derivable from geometry — and the measurement that
//! informs it lives in [`size_floor`](crate::size_floor). What this module does
//! is make the decision writable and then keep it honest.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::reach::{Cut, OutOfSight};
use crate::scene::{Rect, Scene, ScrollAxis, ScrollNode};
use crate::size_floor::Axis;
use crate::widgets::scroll::ScrollState;

/// Why a declaration was refused.
///
/// Each arm is a statement a screen cannot mean, not a value out of range —
/// which is why they are refused rather than normalised. A caller that wrote
/// one has a bug in its declaration, and both defensible repairs would be
/// guesses about which half it meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The window floor is **above** the size the layout stops reflowing at, on
    /// this axis. A window that cannot reach its own layout minimum has a band
    /// pointing the wrong way: there is no size at which the layout is clamped
    /// and the window is not.
    FloorAboveComfortable {
        /// Which axis contradicts.
        axis: Axis,
        /// The declared window floor on it.
        floor: u32,
        /// The declared layout minimum on it.
        comfortable: u32,
    },
    /// There is a band and nothing is named as given up in it.
    ///
    /// Refused rather than allowed-as-empty because a band that costs nothing
    /// is not a band: if the reader loses nothing between the two sizes, the
    /// layout minimum was simply the lower number all along, and saying so is
    /// both shorter and true. Allowing it would also make the honest case and
    /// the un-audited case indistinguishable.
    BandNamesNothing,
    /// Something is named as given up and there is no band to give it up in.
    ///
    /// The mirror, and the likelier of the two in practice: a screen whose
    /// floor was raised back up to its layout minimum, leaving the list behind.
    NamesWithoutBand,
    /// (R1714) The band is served by a pan, and something is named as given up
    /// in it.
    ///
    /// A contradiction rather than a redundancy: a pan is the claim that the
    /// band costs the reader *nothing but simultaneity*, so a list of what it
    /// takes away is a screen saying both things at once. The likely author
    /// error is a [`Self::Pan`](Recourse::Pan) written over a clip declaration
    /// whose list was left behind, and normalising it either way would pick one
    /// of the two halves at random.
    PanNamesWhatItKeeps,
    /// (R1714) A pan is declared and there is no band to pan in.
    ///
    /// [`Self::BandNamesNothing`]'s sibling under the other recourse, and the
    /// same argument: a window that stops exactly where its layout does never
    /// pans, so a policy saying it does describes a state the screen cannot
    /// enter. [`ShrinkPolicy::rigid`] is the shorter and true spelling.
    PanWithoutBand,
    /// ★★★★★ (R1798) A name in `gives_up` **cannot be a tag**, so nothing it
    /// says can ever match a mark.
    ///
    /// [`ShrinkPolicy::covers`] compares each name against a cut mark's own tag
    /// or a step of its path. A name that is not shaped like one is therefore
    /// not a coarse declaration or a stale one — it is a declaration that
    /// **cannot be true of anything**, and the audit's arithmetic silently
    /// answers `covered = 0`, `stale = <the whole list>`, and `unnamed = <every
    /// mark that was cut>`. All three read exactly as a screen that gives up
    /// something it never admitted to.
    ///
    /// Measured at R1798: two of the five shipped screens declared their
    /// concession as an English sentence — *"the columns right of the message
    /// clip before the decode pane narrows"* — and both had been reporting
    /// `unreachable` on the wire since the round that gave them a policy, which
    /// this module's own documentation calls the one verdict no concession can
    /// excuse. Nothing failed, because the gate that reads the verdict runs
    /// over a hand-written list of screens that those two were never added to.
    ///
    /// The rule is a space: a tag has none, and prose cannot avoid one. It is
    /// deliberately crude — it will not catch a single misspelled word — but it
    /// separates the two KINDS of wrong, and the kind it catches is the one
    /// that cannot be right.
    NameIsNotATag {
        /// The name that cannot match a mark.
        name: &'static str,
    },
}

/// How a window serves the band between its layout minimum and its own floor.
///
/// # ★★★★★ R1714 — the answer this tree could not write down
///
/// R1712 gave a screen two floors and a list of what the space between them
/// costs. What it could not express is *how* the space is served, and there are
/// two answers, not one:
///
/// * [`Self::Clip`] — the window cuts, and what it cuts is **gone**. The reader
///   reaches it by making the window bigger and by nothing else, which is why
///   the declaration has to name what goes.
/// * [`Self::Pan`] — the window becomes a viewport onto the layout, and what it
///   cuts is **one gesture away**. Nothing is given up, so there is nothing to
///   name; what the band costs is seeing two things at once.
///
/// Until this round every screen here had the first, by construction rather
/// than by decision: `layout_size` clamps the layout at the comfortable size
/// and the window clipped the result, with no range anywhere to move it.
/// Measured on the analysis tool's node lab, that cost the screen its whole
/// concession — the band bottomed out 24 pixels down, at 1601, one pixel above
/// the 1600-wide display R1689 asked for, and the last pixel could not be
/// bought at all because below the comfortable size the layout stops reflowing
/// and the window simply removes content. With a pan the same screen keeps
/// **everything reachable at 400 pixels wide** (measured, this round).
///
/// The mature retained-mode toolkits this project is judged against reach the
/// same answer by wrapping a window's central widget in a scrolling area, and
/// their applications do it by hand, one application at a time. Here it is the
/// policy's own word, so the screen declares the intent and the framework does
/// the wrapping — a screen cannot declare a pan and forget to build one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recourse {
    /// The window clips, and [`ShrinkPolicy::gives_up`] names what goes.
    Clip,
    /// The window pans over the layout: everything stays reachable, and the
    /// band costs simultaneity rather than content.
    Pan,
}

impl Recourse {
    /// The word that rides on the wire.
    #[must_use]
    pub const fn wire_word(self) -> &'static str {
        match self {
            Self::Clip => "clip",
            Self::Pan => "pan",
        }
    }

    /// Whether the band is served by moving rather than by cutting.
    #[must_use]
    pub const fn pans(self) -> bool {
        matches!(self, Self::Pan)
    }
}

/// A screen's two floors, and what the reader gives up between them.
///
/// `Copy`, and its names are `'static`, so a binding declares one as a `const`
/// and every reader of either number reads *that* value. There is deliberately
/// no way to build one whose halves came from different places.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShrinkPolicy {
    comfortable: (u32, u32),
    floor: (u32, u32),
    gives_up: &'static [&'static str],
    recourse: Recourse,
}

impl ShrinkPolicy {
    /// A floor that concedes nothing: the window stops exactly where the layout
    /// does.
    ///
    /// The honest spelling of what a bare minimum size means, and the majority
    /// case. It is a *declaration*, not a default — a binding that has never
    /// thought about the question declares no policy at all, and this module
    /// keeps those two apart on purpose.
    #[must_use]
    pub const fn rigid(size: (u32, u32)) -> Self {
        Self {
            comfortable: size,
            floor: size,
            gives_up: &[],
            recourse: Recourse::Clip,
        }
    }

    /// A window floor below the layout minimum, and the regions that band
    /// clips.
    ///
    /// # Panics
    ///
    /// On any [`Fault`]. In a `const` context — which is where a binding
    /// declares its policy — that panic is a **compile error**, so a screen
    /// cannot ship a contradictory declaration. [`Self::checked`] is the same
    /// rule with the fault as a value, for callers that need to handle one.
    #[must_use]
    pub const fn conceding(
        comfortable: (u32, u32),
        floor: (u32, u32),
        gives_up: &'static [&'static str],
    ) -> Self {
        Self::declared(comfortable, floor, gives_up, Recourse::Clip)
    }

    /// (R1714) A window floor below the layout minimum, served by **panning**
    /// over the layout rather than cutting into it.
    ///
    /// Nothing is given up, so nothing is named — see
    /// [`Fault::PanNamesWhatItKeeps`]. What the band costs the reader is
    /// simultaneity: below the comfortable size the screen is bigger than the
    /// window and the window moves over it.
    ///
    /// The declaration is what *causes* the pan: the framework wraps a binding
    /// whose policy says this in the viewport its window describes, so a screen
    /// cannot declare a pan and ship a clip. That is the direction this tree
    /// keeps finding rotted the other way round — R1699's `Activation::Explicit`
    /// declared a keyboard that was never implemented, and every gate read the
    /// declaration.
    ///
    /// # Panics
    ///
    /// On any [`Fault`], which in the `const` context a binding declares its
    /// policy in is a **compile error**.
    #[must_use]
    pub const fn panning(comfortable: (u32, u32), floor: (u32, u32)) -> Self {
        Self::declared(comfortable, floor, &[], Recourse::Pan)
    }

    /// The one door the two public constructors go through, so a fault is
    /// spelled once however a policy was written.
    const fn declared(
        comfortable: (u32, u32),
        floor: (u32, u32),
        gives_up: &'static [&'static str],
        recourse: Recourse,
    ) -> Self {
        match Self::fault(comfortable, floor, gives_up, recourse) {
            None => Self {
                comfortable,
                floor,
                gives_up,
                recourse,
            },
            Some(Fault::FloorAboveComfortable { .. }) => {
                panic!("shrink policy: the window floor is above the layout minimum")
            }
            Some(Fault::BandNamesNothing) => {
                panic!("shrink policy: the band names nothing it gives up")
            }
            Some(Fault::NamesWithoutBand) => {
                panic!("shrink policy: names are given up but there is no band")
            }
            Some(Fault::PanNamesWhatItKeeps) => {
                panic!("shrink policy: a pan gives nothing up, so it names nothing")
            }
            Some(Fault::PanWithoutBand) => {
                panic!("shrink policy: a pan is declared and there is no band to pan in")
            }
            Some(Fault::NameIsNotATag { .. }) => {
                panic!(
                    "shrink policy: a name in `gives_up` is not shaped like a tag, so it \
                     cannot match any mark — name the region, not what happens to it"
                )
            }
        }
    }

    /// [`Self::conceding`] with the refusal as a value.
    ///
    /// ★ R1712.1 — kept with **no production caller**, which every other member
    /// the close audit found in that state was deleted for. The reason it
    /// survives: the `const` door's refusal is a *compile error*, so this is the
    /// only way the rule can be tested at all. A rule with no testable door is
    /// a rule nobody has run.
    ///
    /// # Errors
    ///
    /// The [`Fault`] the declaration commits.
    pub const fn checked(
        comfortable: (u32, u32),
        floor: (u32, u32),
        gives_up: &'static [&'static str],
        recourse: Recourse,
    ) -> Result<Self, Fault> {
        match Self::fault(comfortable, floor, gives_up, recourse) {
            Some(fault) => Err(fault),
            None => Ok(Self {
                comfortable,
                floor,
                gives_up,
                recourse,
            }),
        }
    }

    /// The whole rule, in one place, so the doors above cannot disagree about
    /// what is legal.
    const fn fault(
        comfortable: (u32, u32),
        floor: (u32, u32),
        gives_up: &'static [&'static str],
        recourse: Recourse,
    ) -> Option<Fault> {
        if floor.0 > comfortable.0 {
            return Some(Fault::FloorAboveComfortable {
                axis: Axis::Width,
                floor: floor.0,
                comfortable: comfortable.0,
            });
        }
        if floor.1 > comfortable.1 {
            return Some(Fault::FloorAboveComfortable {
                axis: Axis::Height,
                floor: floor.1,
                comfortable: comfortable.1,
            });
        }
        // ★ R1798 — before any question about the band: can these names match
        // a mark at all? A `const fn` cannot use an iterator, so this is a
        // hand-rolled walk over the bytes, which is also why the rule is one
        // byte: a tag has no space and a sentence cannot avoid one.
        let mut i = 0;
        while i < gives_up.len() {
            let bytes = gives_up[i].as_bytes();
            let mut j = 0;
            while j < bytes.len() {
                if bytes[j] == b' ' {
                    return Some(Fault::NameIsNotATag { name: gives_up[i] });
                }
                j += 1;
            }
            if bytes.is_empty() {
                return Some(Fault::NameIsNotATag { name: gives_up[i] });
            }
            i += 1;
        }
        let band = floor.0 < comfortable.0 || floor.1 < comfortable.1;
        // ★ R1714 — the recourse decides what the names mean, so it decides
        // which pairings are contradictions. A pan keeps everything, so a list
        // beside one is two statements; a clip loses the band, so a band with no
        // list is an unaudited claim.
        match recourse {
            Recourse::Pan => match (band, gives_up.is_empty()) {
                (_, false) => Some(Fault::PanNamesWhatItKeeps),
                (false, true) => Some(Fault::PanWithoutBand),
                (true, true) => None,
            },
            Recourse::Clip => match (band, gives_up.is_empty()) {
                (true, true) => Some(Fault::BandNamesNothing),
                (false, false) => Some(Fault::NamesWithoutBand),
                _ => None,
            },
        }
    }

    /// (R1714) How the band between the two floors is served.
    #[must_use]
    pub const fn recourse(self) -> Recourse {
        self.recourse
    }

    /// The size below which the layout stops reflowing and the window clips.
    ///
    /// What [`external::layout_size`](crate::external::layout_size) clamps at.
    #[must_use]
    pub const fn comfortable(self) -> (u32, u32) {
        self.comfortable
    }

    /// The size below which the window refuses to shrink.
    ///
    /// What the window system is told, and what a programmatic resize resolves
    /// against.
    #[must_use]
    pub const fn floor(self) -> (u32, u32) {
        self.floor
    }

    /// The regions the band clips, by the name a reader addresses them with.
    ///
    /// Empty exactly when there is no band — see [`Fault::BandNamesNothing`].
    #[must_use]
    pub const fn gives_up(self) -> &'static [&'static str] {
        self.gives_up
    }

    /// How much smaller than its layout minimum this window may go, per axis.
    ///
    /// `(0, 0)` for [`Self::rigid`], and the two axes are independent: a screen
    /// may concede width and not height, which is the common case on a design
    /// whose side panes are fixed and whose middle stretches.
    #[must_use]
    pub const fn band(self) -> (u32, u32) {
        (
            self.comfortable.0 - self.floor.0,
            self.comfortable.1 - self.floor.1,
        )
    }

    /// Whether this policy concedes anything at all.
    #[must_use]
    pub const fn concedes(self) -> bool {
        self.floor.0 < self.comfortable.0 || self.floor.1 < self.comfortable.1
    }

    /// Whether `name` is one this policy admits to giving up — the mark's own
    /// tag, or any ancestor on its path.
    ///
    /// Ancestry is the point: a screen concedes *regions*, not the individual
    /// runs inside them, so `lab.inspector` covers the label three levels down
    /// without the declaration having to enumerate it. The cost is stated
    /// rather than hidden — a coarse name also covers something new that
    /// appears inside it later, which is why [`Audit`] publishes what each name
    /// actually covered instead of only whether it did.
    #[must_use]
    pub fn covers(self, cut: &Cut) -> bool {
        self.gives_up.iter().any(|name| {
            cut.tag.as_deref() == Some(*name) || cut.path.iter().any(|step| step == name)
        })
    }
}

/// The tag the window's own pan node carries.
///
/// A name rather than an anonymous node because §2 #7 makes every scene fact
/// addressable: `scene/scroll {path: "window.pan"}` drives it on any screen
/// that declares one, and `scene/scroll_reach` reports it by this name, so an
/// agent that finds a mark out of sight can move the thing that reaches it
/// without knowing which screen it is on.
pub const PAN_TAG: &str = "window.pan";

thread_local! {
    /// Surface tag -> the pan its window is using.
    ///
    /// Keyed by the surface's tag, the same key
    /// [`external::surface_size`](crate::external::surface_size) uses and for
    /// the same reason: the two facts are read together, by a hit test that has
    /// a tag and no reactive scope. It carries that key's limit too — one
    /// surface tag is one pan — which is the limit `SURFACE_SIZES` already has
    /// and not a new one.
    static PANS: RefCell<BTreeMap<String, Rc<ScrollState>>> =
        const { RefCell::new(BTreeMap::new()) };
}

/// The pan a surface's window uses, made on the first ask.
///
/// Held outside any [`Owner`](crate::reactive::Owner) rather than through
/// `use_scroll_state`, because the hit test that reads the offset runs outside
/// every scope — the same reason `surface_size` is a plain cache. The offsets
/// inside are still signals, so a view that reads one re-runs when the wire
/// moves it.
#[must_use]
pub fn pan_state(tag: &str) -> Rc<ScrollState> {
    PANS.with(|pans| {
        Rc::clone(
            pans.borrow_mut()
                .entry(tag.to_owned())
                .or_insert_with(|| Rc::new(ScrollState::with_tag(PAN_TAG))),
        )
    })
}

/// Where a surface's window is panned to, in the frame the screen's layout is
/// stated in.
///
/// `(0, 0)` for a screen that does not pan and for one that has not been
/// panned. Never negative: an offset is a distance the content has moved up and
/// left, which is what a hit test adds back.
#[must_use]
pub fn window_pan(tag: &str) -> (u32, u32) {
    PANS.with(|pans| {
        pans.borrow().get(tag).map_or((0, 0), |state| {
            let (x, y) = state.offset();
            (x.max(0).unsigned_abs(), y.max(0).unsigned_abs())
        })
    })
}

/// Forget a surface's pan, so a screen that is gone cannot hand its offset to
/// the next one with the same tag.
pub fn forget_pan(tag: &str) {
    PANS.with(|pans| {
        pans.borrow_mut().remove(tag);
    });
}

/// (R1714.1) Put an existing pan back to zero, because there is no range to
/// hold an offset in.
///
/// Deliberately does NOT create one: a screen that has never panned has no
/// offset to clamp, and [`pan_state`] would leave a state behind for every
/// binding in the tree on every frame.
fn clamp_existing_pan(tag: &str) {
    PANS.with(|pans| {
        if let Some(state) = pans.borrow().get(tag) {
            state.set_max(0, 0);
        }
    });
}

/// ★★★★★ R1714 — wrap a screen's laid-out root in the pan its policy declares.
///
/// The whole behaviour of [`Recourse::Pan`], in one place the framework calls
/// for every binding, so the declaration is what produces it.
///
/// Identity — the very same `Scene` back — for all three cases where there is
/// no pan to make:
///
/// * a binding that declares no policy, or one that clips;
/// * a window at or above the comfortable size on both axes, where there is
///   nothing to pan over. A pan with no range is not a pan, the same argument
///   [`Fault::BandNamesNothing`] makes about a band that costs nothing, and it
///   is what keeps this round's blast radius to the screens that opted in;
/// * a window of no extent, which is the "nothing has painted yet" case
///   [`external::layout_size`](crate::external::layout_size) already names.
///
/// The content is the size the layout was actually laid out at —
/// `max(window, comfortable)` per axis, which is `layout_size`'s own answer, so
/// the pan's range is exactly what the window is short by and cannot drift from
/// what the screen painted. The viewport is the window.
#[must_use]
pub fn pan(policy: Option<ShrinkPolicy>, tag: &str, window: (u32, u32), root: Scene) -> Scene {
    let Some(policy) = policy.filter(|p| p.recourse().pans()) else {
        return root;
    };
    if window.0 == 0 || window.1 == 0 {
        return root;
    }
    let comfortable = policy.comfortable();
    let content = (window.0.max(comfortable.0), window.1.max(comfortable.1));
    if content == window {
        // ★★★★★ R1714.1 — a pan with no range is a pan AT ZERO, and saying only
        // the first half is what the close audit of this round caught.
        //
        // The offset outlives the node. A reader pans this screen and then
        // makes the window big enough again: the pan is not built, so nothing
        // paints at an offset — and `window_pan` went on answering the offset it
        // was left at, so `into_layout` added it to every press. Measured:
        // `scene/pointer_target` went from 61 addressable rectangles to **one**,
        // with 61 unreachable, on a window at its full comfortable size.
        //
        // The runtime does this for every real scroll node — the layout pass
        // publishes the range and `set_max` clamps the offset into it — so this
        // is that rule reaching the one viewport the layout pass never sees.
        // Only for a pan that exists: asking creates one, and a screen that has
        // never panned has nothing to clamp.
        clamp_existing_pan(tag);
        return root;
    }
    let state = pan_state(tag);
    let node = ScrollNode::from_state(state, Rect::new(0, 0, window.0, window.1), root)
        .with_axis(ScrollAxis::Both);
    let layout = node.layout.clone().with_absolute_position(0, 0);
    // ★★★★★ R1724 — **the pan is a clip, not a thing on the screen**, and it
    // has to say so or it is a painted addressable region nobody gave a voice
    // and nobody declared quiet.
    //
    // Nothing noticed until a screen was mounted inside another one. Every
    // screen that declares `Recourse::Pan` opens at its own comfortable size,
    // so the pan node does not exist at boot and the voice census never met
    // it; place that same screen in a REGION smaller than its layout minimum
    // and it exists from the first frame. Measured the day this landed: the
    // analysis-tool shell at Catalog reported `unvoiced: 1` and the tag was
    // this node.
    //
    // The declaration belongs here rather than in each screen for the reason
    // the whole function does: a viewport the framework mints is not something
    // an application should have to remember to describe. It is the same
    // silence a scrolling pane's viewport carries — what a reader walks is the
    // layout inside it.
    Scene::Scroll(node.with_layout(layout)).silenced(crate::voice::Silence::layout(
        "the window's panning viewport",
    ))
}

/// What a declaration and a measurement said about each other.
///
/// Three lists rather than one verdict because they mean different things:
/// something the floor puts **out of reach entirely** breaks the promise the
/// floor itself is; something cut that nothing declared is a screen giving up
/// more than it admits; and a name covering nothing is a declaration that has
/// outlived its screen. None is derivable from the others.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Audit {
    unreachable: Vec<String>,
    unnamed: Vec<String>,
    stale: Vec<String>,
    covered: usize,
}

impl Audit {
    /// Marks nothing can bring into view at the floor.
    ///
    /// ★ The safety property, and the one a concession can never excuse: a
    /// floor is the claim that the screen still *works* there, and clipped
    /// content a reader can scroll to is a different statement from content no
    /// scroll reaches. A screen may decide to clip its inspector; it may not
    /// decide to make it unreachable and call that a concession.
    #[must_use]
    pub fn unreachable(&self) -> &[String] {
        &self.unreachable
    }

    /// Marks the size cuts that no declared name covers — the defect
    /// direction, by the tag or path a reader addresses them with.
    #[must_use]
    pub fn unnamed(&self) -> &[String] {
        &self.unnamed
    }

    /// Declared names that covered nothing at the floor.
    #[must_use]
    pub fn stale(&self) -> &[String] {
        &self.stale
    }

    /// How many cut marks the declaration accounted for.
    ///
    /// Published because a declaration made of region names covers many marks
    /// with few words, and a reader judging whether that is too coarse needs
    /// the number it bought.
    #[must_use]
    pub const fn covered(&self) -> usize {
        self.covered
    }

    /// The word that rides on the wire — **derived**, never stored, so it
    /// cannot disagree with the lists it summarises.
    ///
    /// Ranked by what a reader has to do about it: a floor that loses content
    /// is broken, a screen giving up something undeclared is a fact to act on,
    /// and a stale name beside either is bookkeeping. Folding to the worst is
    /// what stops one honest half from hiding the other — the rule
    /// `size_floor`'s own verdict already uses across its two axes.
    #[must_use]
    pub fn wire_word(&self) -> &'static str {
        if !self.unreachable.is_empty() {
            return "unreachable";
        }
        match (self.unnamed.is_empty(), self.stale.is_empty()) {
            (false, _) => "surprised",
            (true, false) => "stale",
            (true, true) => "honoured",
        }
    }
}

/// Check a declaration against what a size actually does to the screen.
///
/// Both arguments are measured at [`ShrinkPolicy::floor`]:
///
/// * `cut` is what [`reach::cut`](crate::reach::cut) reported — the marks that
///   size can never show **whole**, however the reader scrolls. A policy that
///   concedes nothing expects that list to be empty, and every row in it is
///   `unnamed`.
/// * `out_of_sight` is what [`reach::out_of_sight`](crate::reach::out_of_sight)
///   reported. Only its `lost` rows are read here — a mark one scroll away is
///   exactly what a floor is allowed to have, and counting those would make
///   every scrolling screen fail.
///
/// Two predicates rather than one because they answer the two halves of what a
/// floor promises, and the sharper one alone would answer neither: `cut` says
/// what the reader cannot see at once, `lost` says what the reader cannot see
/// at all.
///
/// ★★★★★ R1713 — that last sentence was written here before `lost` meant it.
/// [`Reach::Lost`](crate::reach::Reach::Lost) used to mean *not fully
/// containable*, so a form row whose right edge a narrowed pane cuts off came
/// back `lost` and this rule failed a concession for content nearly all of which
/// the reader reaches. Measured on the node lab at 1595x360: 19 `lost`, of which
/// **6** were marks no pixel of which is reachable and **13** were wide rows.
/// [`Reach::Clipped`](crate::reach::Reach::Clipped) is now the middle answer, in
/// the word this rule is written in, and this
/// filter — unchanged — finally reads what it always said it read.
/// ★★★★★ R1714 — and the `cut` half is read only under
/// [`Recourse::Clip`]. A pan gives nothing up, so there is no list to check a
/// cut against: what a pan's band costs is seeing two things **at once**, which
/// is the very thing `cut` reports and the very thing the declaration permits.
/// Reading it there would fail every panning screen for doing exactly what it
/// said it would do.
///
/// The half that stays is the one that can still fail, and it is the one that
/// matters: `lost` is empty under a working pan at every size, so a screen that
/// declares a pan and does not get one — the framework not wrapping it, a pan
/// whose content is the window rather than the layout — reads `unreachable`
/// here. That is what keeps this from being a check that cannot fail.
#[must_use]
pub fn audit(policy: ShrinkPolicy, cut: &[Cut], out_of_sight: &[OutOfSight]) -> Audit {
    let unreachable: Vec<String> = out_of_sight
        .iter()
        .filter(|mark| mark.reach.is_lost())
        .map(|mark| mark.tag.clone().unwrap_or_else(|| mark.path.join("/")))
        .collect();
    if policy.recourse().pans() {
        return Audit {
            unreachable,
            unnamed: Vec::new(),
            stale: Vec::new(),
            covered: 0,
        };
    }
    let mut unnamed = Vec::new();
    let mut covered = 0;
    for mark in cut {
        if policy.covers(mark) {
            covered += 1;
        } else {
            unnamed.push(mark.tag.clone().unwrap_or_else(|| mark.path.join("/")));
        }
    }
    let stale = policy
        .gives_up()
        .iter()
        .filter(|name| {
            !cut.iter().any(|mark| {
                mark.tag.as_deref() == Some(**name) || mark.path.iter().any(|step| step == *name)
            })
        })
        .map(|name| (*name).to_owned())
        .collect();
    Audit {
        unreachable,
        unnamed,
        stale,
        covered,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Audit, Fault, PAN_TAG, PANS, Rc, Recourse, Scene, ShrinkPolicy, audit, forget_pan, pan,
        pan_state, window_pan,
    };
    use crate::containment::Overhang;
    use crate::reach::{Cut, Move, OutOfSight, Reach, Viewport};
    use crate::scene::Rect;
    use crate::size_floor::Axis;

    fn viewport() -> Viewport {
        Viewport {
            name: "<window>".to_owned(),
            origin: (0, 0),
            size: (100, 100),
            declared: Rect::new(0, 0, 100, 100),
            content: (100, 100),
            at: (0, 0),
            max: (0, 0),
        }
    }

    fn rect() -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        }
    }

    fn mark(tag: Option<&str>, path: &[&str]) -> Cut {
        Cut {
            tag: tag.map(str::to_owned),
            path: path.iter().map(|s| (*s).to_owned()).collect(),
            content: None,
            rect: rect(),
            viewport: viewport(),
            short_by: Overhang {
                left: 0,
                top: 0,
                right: 5,
                bottom: 0,
            },
        }
    }

    fn sighting(tag: &str, reach: Reach) -> OutOfSight {
        OutOfSight {
            tag: Some(tag.to_owned()),
            path: vec![tag.to_owned()],
            content: None,
            rect: rect(),
            viewport: viewport(),
            reach,
        }
    }

    #[test]
    fn r1712_a_rigid_policy_puts_both_floors_at_one_size() {
        let policy = ShrinkPolicy::rigid((1440, 900));
        assert_eq!(policy.comfortable(), (1440, 900));
        assert_eq!(policy.floor(), (1440, 900));
        assert_eq!(policy.band(), (0, 0));
        assert!(!policy.concedes());
        assert!(policy.gives_up().is_empty());
    }

    #[test]
    fn r1712_a_conceding_policy_keeps_the_two_floors_apart() {
        let policy = ShrinkPolicy::conceding((1625, 360), (1506, 333), &["lab.inspector"]);
        assert_eq!(policy.comfortable(), (1625, 360));
        assert_eq!(policy.floor(), (1506, 333));
        assert_eq!(policy.band(), (119, 27));
        assert!(policy.concedes());
    }

    /// ★★★★★ The refusal that makes the type worth having: a floor above the
    /// layout minimum is not a size to normalise, it is a declaration that
    /// cannot mean anything, and it names the axis it contradicts on.
    #[test]
    fn r1712_a_floor_above_the_layout_minimum_is_refused_by_axis() {
        assert_eq!(
            ShrinkPolicy::checked((1625, 360), (1700, 333), &["x"], Recourse::Clip),
            Err(Fault::FloorAboveComfortable {
                axis: Axis::Width,
                floor: 1700,
                comfortable: 1625,
            })
        );
        assert_eq!(
            ShrinkPolicy::checked((1625, 360), (1506, 400), &["x"], Recourse::Clip),
            Err(Fault::FloorAboveComfortable {
                axis: Axis::Height,
                floor: 400,
                comfortable: 360,
            })
        );
    }

    /// A band that costs nothing is a lower layout minimum written twice.
    #[test]
    fn r1712_a_band_that_names_nothing_is_refused() {
        assert_eq!(
            ShrinkPolicy::checked((1625, 360), (1506, 360), &[], Recourse::Clip),
            Err(Fault::BandNamesNothing)
        );
    }

    /// ★★★★★ R1714 — a pan keeps everything, so naming what it gives up is a
    /// screen saying two things at once.
    #[test]
    fn r1714_a_pan_that_names_what_it_gives_up_is_refused() {
        assert_eq!(
            ShrinkPolicy::checked((1625, 360), (1024, 360), &["lab.inspector"], Recourse::Pan),
            Err(Fault::PanNamesWhatItKeeps)
        );
    }

    /// And the sibling of [`Fault::BandNamesNothing`]: a window that stops where
    /// its layout does never pans, so a policy saying it does describes a state
    /// the screen cannot enter.
    #[test]
    fn r1714_a_pan_with_no_band_is_refused() {
        assert_eq!(
            ShrinkPolicy::checked((1625, 360), (1625, 360), &[], Recourse::Pan),
            Err(Fault::PanWithoutBand)
        );
    }

    /// The legal pan, and the two facts a reader gets from it.
    #[test]
    fn r1714_a_pan_declares_a_band_and_gives_nothing_up() {
        const POLICY: ShrinkPolicy = ShrinkPolicy::panning((1625, 360), (1024, 360));
        assert_eq!(POLICY.recourse(), Recourse::Pan);
        assert_eq!(POLICY.recourse().wire_word(), "pan");
        assert_eq!(POLICY.band(), (601, 0));
        assert!(POLICY.gives_up().is_empty());
        assert!(POLICY.concedes());
        assert_eq!(
            Ok(POLICY),
            ShrinkPolicy::checked((1625, 360), (1024, 360), &[], Recourse::Pan)
        );
    }

    /// ★★ And the default is still a clip, so the 224 bindings that never heard
    /// of this round keep the recourse they were written under.
    #[test]
    fn r1714_a_policy_written_before_this_round_still_clips() {
        assert_eq!(ShrinkPolicy::rigid((800, 600)).recourse(), Recourse::Clip);
        assert_eq!(
            ShrinkPolicy::conceding((1625, 360), (1506, 360), &["lab.inspector"]).recourse(),
            Recourse::Clip
        );
        assert_eq!(Recourse::Clip.wire_word(), "clip");
        assert!(!Recourse::Clip.pans());
        assert!(Recourse::Pan.pans());
    }

    /// And the mirror — the shape a screen leaves behind when its floor is
    /// raised back up and the list is not.
    #[test]
    fn r1712_names_with_no_band_are_refused() {
        assert_eq!(
            ShrinkPolicy::checked((1625, 360), (1625, 360), &["lab.inspector"], Recourse::Clip),
            Err(Fault::NamesWithoutBand)
        );
    }

    /// One axis conceding and the other not is legal, and is the common shape.
    #[test]
    fn r1712_a_band_on_one_axis_only_is_legal() {
        let policy =
            ShrinkPolicy::checked((1625, 360), (1506, 360), &["lab.inspector"], Recourse::Clip)
                .expect("width-only band is legal");
        assert_eq!(policy.band(), (119, 0));
        assert!(policy.concedes());
    }

    /// ★ The declaration is checked at compile time where a binding writes it:
    /// this is the `const` path, and a contradictory pair here would not build.
    #[test]
    fn r1712_a_const_declaration_is_the_same_value_as_the_checked_one() {
        const POLICY: ShrinkPolicy =
            ShrinkPolicy::conceding((1625, 360), (1506, 333), &["lab.inspector"]);
        assert_eq!(
            Ok(POLICY),
            ShrinkPolicy::checked((1625, 360), (1506, 333), &["lab.inspector"], Recourse::Clip)
        );
    }

    /// A root laid out at `size`, the shape every screen here hands the shell.
    fn root(size: (u32, u32)) -> Scene {
        Scene::Container(
            crate::scene::ContainerNode::new(Vec::new())
                .with_tag("screen")
                .with_layout(
                    crate::style::LayoutStyle::new()
                        .with_size(crate::style::Size::px(size.0, size.1)),
                ),
        )
    }

    fn pan_node(scene: &Scene) -> Option<&crate::scene::ScrollNode> {
        match scene {
            Scene::Scroll(node) => Some(node),
            _ => None,
        }
    }

    /// ★★★★★ R1714 — the declaration is what makes the pan, and the three cases
    /// where there is nothing to make are the SAME scene back.
    ///
    /// Asserted as identity of the root's tag rather than "is not a scroll",
    /// because a wrap that produced some other container would also not be a
    /// scroll and would still have moved every rectangle on the screen.
    #[test]
    fn r1714_a_screen_with_nothing_to_pan_over_is_handed_back_unchanged() {
        let clip = ShrinkPolicy::conceding((1625, 360), (1506, 360), &["lab.inspector"]);
        let pans = ShrinkPolicy::panning((1625, 360), (1024, 360));
        for (what, policy, window) in [
            ("no policy at all", None, (800, 300)),
            ("a policy that clips", Some(clip), (800, 300)),
            ("a window at the comfortable size", Some(pans), (1625, 360)),
            ("a window above it", Some(pans), (1920, 1080)),
            ("nothing painted yet", Some(pans), (0, 0)),
        ] {
            let out = pan(policy, "screen", window, root((1625, 360)));
            assert!(
                pan_node(&out).is_none() && out.tag() == Some("screen"),
                "{what}: the root came back wrapped",
            );
        }
    }

    /// ★★★★★ And the case that is the round: the window becomes a viewport onto
    /// the layout, with the range it is short by.
    #[test]
    fn r1714_a_window_below_its_layout_becomes_a_viewport_onto_it() {
        let policy = ShrinkPolicy::panning((1625, 360), (1024, 360));
        forget_pan("screen");
        let out = pan(Some(policy), "screen", (1200, 360), root((1625, 360)));
        let node = pan_node(&out).expect("the window pans");
        assert_eq!(
            node.tag.as_deref(),
            Some(PAN_TAG),
            "the pan is addressable by name",
        );
        assert_eq!(
            node.viewport,
            Rect::new(0, 0, 1200, 360),
            "the viewport is the window",
        );
        assert_eq!(
            node.content.tag(),
            Some("screen"),
            "and the screen is what it looks onto, unchanged",
        );
        // ★ The content is what `layout_size` laid out — max(window, comfortable)
        // per axis — so the range is exactly the shortfall and nothing here
        // writes a second opinion about the layout's size.
        assert_eq!(
            crate::widgets::scroll::max_scroll_offset(1625, 1200),
            425,
            "the range the window is short by",
        );
        // ★★★★★ R1724 — and it declares itself a clip rather than a thing on
        // the screen. Without this the pan is a painted, addressable region
        // nobody gave a voice and nobody declared quiet, which is the exact
        // arm `scene/voice` calls `unvoiced`. Nothing met it for ten rounds
        // because a screen that declares `Recourse::Pan` opens at its own
        // comfortable size, so the node does not exist at boot; the day a
        // screen was placed in a REGION smaller than its layout minimum it
        // existed from the first frame and the shell reported `unvoiced: 1`.
        let silence = node
            .layout
            .silence
            .as_ref()
            .expect("the pan viewport declares why it is quiet");
        assert_eq!(
            silence.kind(),
            crate::voice::SilenceKind::Layout,
            "a viewport is a clip: what a reader walks is the layout inside it",
        );
    }

    /// ★★ An axis the window is big enough for pans on the other one only, which
    /// is the common shape on a design whose height fits and whose width does
    /// not.
    #[test]
    fn r1714_a_pan_on_one_axis_still_wraps() {
        let policy = ShrinkPolicy::panning((1625, 360), (1024, 360));
        forget_pan("screen");
        let out = pan(Some(policy), "screen", (1200, 900), root((1625, 900)));
        let node = pan_node(&out).expect("the width is short, so it pans");
        assert_eq!(node.viewport, Rect::new(0, 0, 1200, 900));
    }

    /// ★★★★★ R1714.1 — a window that grows back past its layout leaves no
    /// offset behind.
    ///
    /// Found by the round's own close audit, and it is the round's own defect
    /// class turned on itself: the pan node is not built once the window can
    /// show the whole layout, so nothing paints at an offset — and `window_pan`
    /// went on answering the offset the reader had left it at, which
    /// `into_layout` then added to every press. Measured on the node lab:
    /// `scene/pointer_target` fell from 61 addressable rectangles to **one**,
    /// with 61 unreachable, at the screen's full comfortable size.
    #[test]
    fn r1714_1_a_pan_with_no_range_left_holds_no_offset() {
        let policy = ShrinkPolicy::panning((1625, 360), (1024, 360));
        forget_pan("screen");
        // Pan it, the way a reader does, while there is range to pan in.
        let _ = pan(Some(policy), "screen", (1200, 360), root((1625, 360)));
        let state = pan_state("screen");
        state.set_max(425, 0);
        state.scroll_to(400, 0);
        assert_eq!(window_pan("screen"), (400, 0));
        // Then grow the window back past the layout. No pan is built…
        let out = pan(Some(policy), "screen", (1625, 360), root((1625, 360)));
        assert!(pan_node(&out).is_none(), "there is nothing to pan over");
        // …and the offset a hit test reads is gone with it.
        assert_eq!(window_pan("screen"), (0, 0));
        // ★ And a screen that has never panned is not given a pan by asking:
        // this runs for every binding on every frame.
        forget_pan("never");
        let _ = pan(Some(policy), "never", (1625, 360), root((1625, 360)));
        assert!(
            !PANS.with(|pans| pans.borrow().contains_key("never")),
            "the identity path must not leave a pan behind for every binding",
        );
        forget_pan("screen");
    }

    /// ★★★ The offset a hit test reads is the offset the pan is at — one fact,
    /// read by the paint through the scroll node and by the press through here.
    #[test]
    fn r1714_the_pan_offset_is_readable_outside_every_scope() {
        forget_pan("screen");
        assert_eq!(window_pan("screen"), (0, 0), "nothing has panned");
        let state = pan_state("screen");
        state.set_max(425, 0);
        state.scroll_to(24, 0);
        assert_eq!(window_pan("screen"), (24, 0));
        assert_eq!(
            Rc::as_ptr(&pan_state("screen")),
            Rc::as_ptr(&state),
            "asking twice gives the same pan, or the paint and the press would \
             be reading two",
        );
        forget_pan("screen");
        assert_eq!(window_pan("screen"), (0, 0), "and a screen that is gone");
    }

    /// A declared region covers what is inside it, addressed either way.
    #[test]
    fn r1712_a_region_covers_its_own_tag_and_everything_on_its_path() {
        let policy = ShrinkPolicy::conceding((1625, 360), (1506, 333), &["lab.inspector"]);
        assert!(policy.covers(&mark(Some("lab.inspector"), &["lab.inspector"])));
        assert!(policy.covers(&mark(
            Some("lab.inspector.body"),
            &["lab.inspector", "lab.inspector.body"]
        )));
        assert!(policy.covers(&mark(None, &["lab.inspector", "7"])));
        assert!(!policy.covers(&mark(Some("lab.appbar"), &["lab.appbar"])));
    }

    #[test]
    fn r1712_an_honoured_declaration_says_what_it_covered() {
        let policy = ShrinkPolicy::conceding((1625, 360), (1506, 333), &["lab.inspector"]);
        let report = audit(
            policy,
            &[
                mark(Some("lab.inspector"), &["lab.inspector"]),
                mark(None, &["lab.inspector", "3"]),
            ],
            &[],
        );
        assert_eq!(report.wire_word(), "honoured");
        assert_eq!(report.covered(), 2);
        assert!(report.unnamed().is_empty());
        assert!(report.stale().is_empty());
    }

    /// ★★★★★ The direction that is a defect: the screen gives up something it
    /// never admitted to, and the row is named rather than counted.
    #[test]
    fn r1712_an_undeclared_loss_is_named() {
        let policy = ShrinkPolicy::conceding((1625, 360), (1506, 333), &["lab.inspector"]);
        let report = audit(
            policy,
            &[
                mark(Some("lab.inspector"), &["lab.inspector"]),
                mark(Some("lab.appbar"), &["lab.appbar"]),
                mark(None, &["lab.canvas", "9"]),
            ],
            &[],
        );
        assert_eq!(report.wire_word(), "surprised");
        assert_eq!(report.unnamed(), ["lab.appbar", "lab.canvas/9"]);
        assert_eq!(report.covered(), 1);
    }

    /// The other direction: the declaration outlived what it described.
    #[test]
    fn r1712_a_name_that_covers_nothing_is_stale() {
        let policy =
            ShrinkPolicy::conceding((1625, 360), (1506, 333), &["lab.inspector", "lab.gate"]);
        let report = audit(
            policy,
            &[mark(Some("lab.inspector"), &["lab.inspector"])],
            &[],
        );
        assert_eq!(report.wire_word(), "stale");
        assert_eq!(report.stale(), ["lab.gate"]);
        assert!(report.unnamed().is_empty());
    }

    /// ★ Both at once, and the defect direction is what the word reports —
    /// a screen is not told "your list is out of date" while it is also
    /// clipping something nobody declared.
    #[test]
    fn r1712_the_defect_direction_wins_the_word() {
        let policy =
            ShrinkPolicy::conceding((1625, 360), (1506, 333), &["lab.inspector", "lab.gate"]);
        let report = audit(policy, &[mark(Some("lab.appbar"), &["lab.appbar"])], &[]);
        assert_eq!(report.wire_word(), "surprised");
        assert_eq!(report.unnamed(), ["lab.appbar"]);
        assert_eq!(report.stale(), ["lab.inspector", "lab.gate"]);
    }

    /// A rigid policy expects to cut nothing, so anything cut at its floor is
    /// undeclared by construction — there is no list it could be on.
    #[test]
    fn r1712_a_rigid_policy_names_everything_its_floor_cuts() {
        let report = audit(
            ShrinkPolicy::rigid((1440, 900)),
            &[mark(Some("shell.appbar"), &["shell.appbar"])],
            &[],
        );
        assert_eq!(report.wire_word(), "surprised");
        assert_eq!(report.unnamed(), ["shell.appbar"]);
    }

    #[test]
    fn r1712_a_floor_that_cuts_nothing_is_honoured() {
        assert_eq!(
            audit(ShrinkPolicy::rigid((1440, 900)), &[], &[]),
            Audit::default()
        );
        assert_eq!(
            audit(ShrinkPolicy::rigid((1440, 900)), &[], &[]).wire_word(),
            "honoured"
        );
    }

    /// ★★★★★ The safety property a concession can never buy: content the floor
    /// puts out of reach entirely. The mark here is also *declared*, and the
    /// declaration does not save it — which is the whole point, because the
    /// cheap wrong design lets a screen name a region and then lose it.
    #[test]
    fn r1712_a_declared_region_that_becomes_unreachable_is_still_a_defect() {
        let policy = ShrinkPolicy::conceding((1625, 360), (1506, 333), &["lab.inspector"]);
        let report = audit(
            policy,
            &[mark(Some("lab.inspector"), &["lab.inspector"])],
            &[sighting(
                "lab.inspector",
                Reach::Lost {
                    short_by: Overhang {
                        left: 0,
                        top: 0,
                        right: 9,
                        bottom: 0,
                    },
                },
            )],
        );
        assert_eq!(report.wire_word(), "unreachable");
        assert_eq!(report.unreachable(), ["lab.inspector"]);
        // ★ And the concession still reads as honoured on its own terms — the
        // two facts are kept apart rather than folded, so a reader repairing
        // the floor is not also told the list is wrong.
        assert!(report.unnamed().is_empty());
        assert_eq!(report.covered(), 1);
    }

    /// A mark one scroll away is exactly what a floor is allowed to have —
    /// counting it would fail every screen that scrolls, which is all of them.
    #[test]
    fn r1712_a_mark_one_scroll_away_is_not_a_defect() {
        let report = audit(
            ShrinkPolicy::rigid((1440, 900)),
            &[],
            &[sighting(
                "shell.canvas.row.7",
                Reach::Scrollable {
                    moves: vec![Move {
                        viewport: "shell.canvas".to_owned(),
                        to: (0, 120),
                    }],
                },
            )],
        );
        assert_eq!(report.wire_word(), "honoured");
        assert!(report.unreachable().is_empty());
    }

    /// ★★★★★ R1713 — and a mark whose EDGE is unreachable is what a concession
    /// buys, not what it may never buy.
    ///
    /// This rule's doc always said it read "what the reader cannot see at all",
    /// and until [`Reach::Clipped`] existed the value it read was "what the reader
    /// cannot see at once" — so a declared band failed on the wide rows inside
    /// the very region it declared. Measured on the node lab at 1595x360: of 19
    /// `lost`, 13 were rows like this one.
    #[test]
    fn r1713_a_mark_whose_edge_is_unreachable_is_conceded_not_lost() {
        let policy = ShrinkPolicy::conceding((1625, 360), (1600, 360), &["lab.inspector"]);
        let edge = Overhang {
            left: 0,
            top: 0,
            right: 28,
            bottom: 0,
        };
        let report = audit(
            policy,
            &[mark(Some("lab.inspector"), &["lab.inspector"])],
            &[sighting(
                "lab.form.control.listen",
                Reach::Clipped { short_by: edge },
            )],
        );
        assert_eq!(report.wire_word(), "honoured");
        assert!(
            report.unreachable().is_empty(),
            "a cut edge is a clip, and clipping is what the band is for"
        );
        // ★ The counterfactual that keeps the two apart: the same overhang, on a
        // mark no pixel of which is reachable, is still the severe verdict.
        let lost = audit(
            policy,
            &[mark(Some("lab.inspector"), &["lab.inspector"])],
            &[sighting(
                "lab.form.remove.id.glyph",
                Reach::Lost { short_by: edge },
            )],
        );
        assert_eq!(lost.wire_word(), "unreachable");
    }
}
