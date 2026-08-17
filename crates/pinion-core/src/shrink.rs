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

use crate::reach::{Cut, OutOfSight};
use crate::size_floor::Axis;

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
        match Self::fault(comfortable, floor, gives_up) {
            None => Self {
                comfortable,
                floor,
                gives_up,
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
    ) -> Result<Self, Fault> {
        match Self::fault(comfortable, floor, gives_up) {
            Some(fault) => Err(fault),
            None => Ok(Self {
                comfortable,
                floor,
                gives_up,
            }),
        }
    }

    /// The whole rule, in one place, so the two doors above cannot disagree
    /// about what is legal.
    const fn fault(
        comfortable: (u32, u32),
        floor: (u32, u32),
        gives_up: &'static [&'static str],
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
        let band = floor.0 < comfortable.0 || floor.1 < comfortable.1;
        match (band, gives_up.is_empty()) {
            (true, true) => Some(Fault::BandNamesNothing),
            (false, false) => Some(Fault::NamesWithoutBand),
            _ => None,
        }
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
#[must_use]
pub fn audit(policy: ShrinkPolicy, cut: &[Cut], out_of_sight: &[OutOfSight]) -> Audit {
    let unreachable = out_of_sight
        .iter()
        .filter(|mark| mark.reach.is_lost())
        .map(|mark| mark.tag.clone().unwrap_or_else(|| mark.path.join("/")))
        .collect();
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
    use super::{Audit, Fault, ShrinkPolicy, audit};
    use crate::containment::Overhang;
    use crate::reach::{Cut, OutOfSight, Reach, Viewport};
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
            ShrinkPolicy::checked((1625, 360), (1700, 333), &["x"]),
            Err(Fault::FloorAboveComfortable {
                axis: Axis::Width,
                floor: 1700,
                comfortable: 1625,
            })
        );
        assert_eq!(
            ShrinkPolicy::checked((1625, 360), (1506, 400), &["x"]),
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
            ShrinkPolicy::checked((1625, 360), (1506, 360), &[]),
            Err(Fault::BandNamesNothing)
        );
    }

    /// And the mirror — the shape a screen leaves behind when its floor is
    /// raised back up and the list is not.
    #[test]
    fn r1712_names_with_no_band_are_refused() {
        assert_eq!(
            ShrinkPolicy::checked((1625, 360), (1625, 360), &["lab.inspector"]),
            Err(Fault::NamesWithoutBand)
        );
    }

    /// One axis conceding and the other not is legal, and is the common shape.
    #[test]
    fn r1712_a_band_on_one_axis_only_is_legal() {
        let policy = ShrinkPolicy::checked((1625, 360), (1506, 360), &["lab.inspector"])
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
            ShrinkPolicy::checked((1625, 360), (1506, 333), &["lab.inspector"])
        );
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
                Reach::Scrollable { to: (0, 120) },
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
