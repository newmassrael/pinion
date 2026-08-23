//! R1791 §5.38 — **a row that runs out of room says what it moved**, and never
//! paints past its own edge.
//!
//! # The defect this exists for, measured on a running window
//!
//! A reader opened the assembled analysis tool and reported that the node lab's
//! inspector was cut off. Measured: the shipped window is 1440 wide, the lab's
//! page gets 1388, and the lab's declared minimum is 1625 — short by 237. And
//! 1029 of that 1625 is the **toolbar**, in a rigid row, so what cuts the
//! inspector is not the inspector.
//!
//! The screen's own source had already written down the answer and could not
//! take it: *"what would take it back is an overflow affordance on the toolbar,
//! which this tree does not have and which is a round of its own; until then a
//! screen whose chrome outgrows its window clips"*. This is that round.
//!
//! # The floor, built and run at 6.11
//!
//! The reference **has** an overflow affordance, so by this project's rule a
//! consumer for one exists. Measured with ten actions in a tool bar squeezed
//! from 1200px to 220px:
//!
//! | asked | what it does |
//! |---|---|
//! | how many stay visible | **1 of 10**, the rest behind an extension button |
//! | which ones did it hide | **there is no member that says** — the class has `setKeyValueAt`-style accessors for the actions and nothing for the extension |
//! | is a hidden action still "visible" | **`isVisible()` answers true** — the ACTION reports itself visible while its widget is not |
//!
//! The third row is the one that decides this module's shape. A reader asking
//! *can I reach this?* gets `true` for something a person cannot see. Here a
//! moved item is **not in [`Row::shown`]**, it **is** in [`Row::moved`], and
//! both are values a caller can publish.
//!
//! # What it does not decide
//!
//! Geometry. This answers *which items are in the row*; where they go is the
//! caller's, exactly as `pinion_node_graph::Document::deployment` leaves the
//! rendering to the screen that owns the pixels. A row laid out right-to-left
//! and one laid out left-to-right ask the same question of this and place the
//! answer differently.

use core::fmt;

/// What a row does with an item when it runs out of room.
///
/// Three words rather than a number, because a priority order invites arithmetic
/// nobody can read at the call site — *is 3 more important than 7?* — while
/// these say what happens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WhenTight {
    /// Never moves. The row moves something else, or reports it cannot fit.
    ///
    /// For the thing a person came to the row for: a launch button, a title.
    /// Marking everything `Keep` is legal and makes [`Row::short_by`] the
    /// honest answer rather than a hidden clip.
    Keep,
    /// Moves if the row needs the room, later ones first.
    ///
    /// Later first because a row is read in order and the reader's eye is at
    /// the start; taking from the end keeps the beginning where it was.
    Move,
    /// Moves before anything else, wherever it sits in the row.
    ///
    /// For an item that is present for completeness rather than for use.
    MoveFirst,
}

/// One item competing for room, with the width it needs and how hard it holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Item<T> {
    /// What it needs, in the row's own units.
    pub width: u32,
    /// What the row does with it when room runs out.
    pub when_tight: WhenTight,
    /// The caller's own handle on it — a tag, an index, a seat.
    pub value: T,
}

impl<T> Item<T> {
    /// An ordinary item: moves if the row needs the room.
    pub const fn new(width: u32, value: T) -> Self {
        Self {
            width,
            when_tight: WhenTight::Move,
            value,
        }
    }

    /// This item, but one the row will not move.
    #[must_use]
    pub const fn kept(mut self) -> Self {
        self.when_tight = WhenTight::Keep;
        self
    }

    /// This item, but the first the row gives up.
    #[must_use]
    pub const fn first_to_move(mut self) -> Self {
        self.when_tight = WhenTight::MoveFirst;
        self
    }
}

/// A row, decided: what stays, what moved, and whether it still does not fit.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row<T> {
    shown: Vec<T>,
    moved: Vec<T>,
    affordance: bool,
    short_by: u32,
}

impl<T> Row<T> {
    /// The items that stay in the row, in the order they were given.
    #[must_use]
    pub fn shown(&self) -> &[T] {
        &self.shown
    }

    /// The items behind the affordance, in the order they were given.
    ///
    /// ★ The list the floor does not have. A consumer publishes this so a
    /// reader — a person opening the menu, or an agent asking what a screen can
    /// do — is told what moved rather than finding out by not seeing it.
    #[must_use]
    pub fn moved(&self) -> &[T] {
        &self.moved
    }

    /// Whether the row needs to draw the affordance.
    ///
    /// False when nothing moved, so a row that fits does not carry a control
    /// that opens onto nothing — which is its own small lie.
    #[must_use]
    pub const fn needs_affordance(&self) -> bool {
        self.affordance
    }

    /// How much room the row still does not have, after moving everything it
    /// is allowed to.
    ///
    /// Zero when the row fits. Non-zero means the items that may not move,
    /// plus the affordance, are wider than what was given — which is a fact a
    /// screen can assert on at its shipped size, and the reason "it must never
    /// be cut" is checkable rather than a hope.
    #[must_use]
    pub const fn short_by(&self) -> u32 {
        self.short_by
    }

    /// Whether the row fits: nothing moved and nothing is short.
    #[must_use]
    pub const fn fits(&self) -> bool {
        !self.affordance && self.short_by == 0
    }
}

/// Why a row could not be laid out at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unroomed {
    /// The affordance is wider than the whole row.
    ///
    /// Not a shortfall but a contradiction: there is no arrangement, because
    /// the thing that would hold the overflow does not fit either.
    AffordanceTooWide {
        /// What the affordance costs.
        affordance: u32,
        /// What the row was given.
        available: u32,
    },
}

impl fmt::Display for Unroomed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AffordanceTooWide {
                affordance,
                available,
            } => write!(
                f,
                "the overflow control needs {affordance} and the row has {available}, \
                 so there is no arrangement — not a shortfall, a contradiction"
            ),
        }
    }
}

impl std::error::Error for Unroomed {}

/// Fit `items` into `available`, moving what has to move.
///
/// `affordance` is what the overflow control itself costs, and it is **charged
/// only when something moves**. Charging it always would shrink a row that fits;
/// forgetting it is the naive bug — show N items, then add a control that does
/// not fit either.
///
/// # Errors
/// [`Unroomed::AffordanceTooWide`] when the control could not be drawn even in
/// an empty row. Every other outcome is a [`Row`], including one that is short:
/// a row that cannot fit its kept items still has to say so, and refusing would
/// leave the caller with nothing to draw and nothing to report.
pub fn lay<T>(available: u32, affordance: u32, items: Vec<Item<T>>) -> Result<Row<T>, Unroomed> {
    let total: u32 = items.iter().map(|item| item.width).sum();
    if total <= available {
        return Ok(Row {
            shown: items.into_iter().map(|item| item.value).collect(),
            moved: Vec::new(),
            affordance: false,
            short_by: 0,
        });
    }
    if affordance > available {
        return Err(Unroomed::AffordanceTooWide {
            affordance,
            available,
        });
    }

    // Which to give up, in the order they are given up: the eager ones in row
    // order, then the ordinary ones from the end. `Keep` is never in this list,
    // which is what makes `short_by` mean something.
    let order: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.when_tight == WhenTight::MoveFirst)
        .map(|(i, _)| i)
        .chain(
            items
                .iter()
                .enumerate()
                .rev()
                .filter(|(_, item)| item.when_tight == WhenTight::Move)
                .map(|(i, _)| i),
        )
        .collect();

    let room = available - affordance;
    let mut width = total;
    let mut gone: Vec<usize> = Vec::new();
    for &at in &order {
        if width <= room {
            break;
        }
        width -= items[at].width;
        gone.push(at);
    }

    let short_by = width.saturating_sub(room);
    let mut shown = Vec::with_capacity(items.len() - gone.len());
    let mut moved = Vec::with_capacity(gone.len());
    for (i, item) in items.into_iter().enumerate() {
        if gone.contains(&i) {
            moved.push(item.value);
        } else {
            shown.push(item.value);
        }
    }
    Ok(Row {
        shown,
        moved,
        affordance: true,
        short_by,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(widths: &[u32]) -> Vec<Item<&'static str>> {
        const NAMES: [&str; 6] = ["a", "b", "c", "d", "e", "f"];
        widths
            .iter()
            .zip(NAMES)
            .map(|(w, name)| Item::new(*w, name))
            .collect()
    }

    #[test]
    fn r1791_a_row_that_fits_moves_nothing_and_draws_no_control() {
        let laid = lay(100, 20, row(&[10, 20, 30])).expect("room");
        assert_eq!(laid.shown(), ["a", "b", "c"]);
        assert!(laid.moved().is_empty());
        assert!(!laid.needs_affordance(), "a control onto nothing is a lie");
        assert_eq!(laid.short_by(), 0);
        assert!(laid.fits());
        // Exactly full still fits, and is the boundary a naive `<` gets wrong.
        assert!(lay(60, 20, row(&[10, 20, 30])).expect("room").fits());
    }

    #[test]
    fn r1791_the_control_costs_room_and_is_charged_only_when_it_appears() {
        // 60 of items into 61: fits, no control.
        assert!(lay(61, 20, row(&[10, 20, 30])).expect("room").fits());
        // Into 59 it does not, and now the control takes 20 of the 59, leaving
        // 39 — so `c` (30) has to go, not merely the 1 that was over.
        let laid = lay(59, 20, row(&[10, 20, 30])).expect("room");
        assert_eq!(laid.shown(), ["a", "b"]);
        assert_eq!(laid.moved(), ["c"]);
        assert!(laid.needs_affordance());
        assert_eq!(laid.short_by(), 0);
    }

    #[test]
    fn r1791_ordinary_items_are_given_up_from_the_end() {
        // 40 of items into 35: the control takes 5, leaving 30, so exactly the
        // last 10 goes. (45 would have FIT — the first draft of this test had
        // that arithmetic wrong and the run said so.)
        let laid = lay(35, 5, row(&[10, 10, 10, 10])).expect("room");
        assert_eq!(
            laid.shown(),
            ["a", "b", "c"],
            "the reader's eye is at the start"
        );
        assert_eq!(laid.moved(), ["d"]);
    }

    #[test]
    fn r1791_an_eager_item_goes_first_wherever_it_sits() {
        let items = vec![
            Item::new(10, "a"),
            Item::new(10, "b").first_to_move(),
            Item::new(10, "c"),
        ];
        let laid = lay(25, 5, items).expect("room");
        assert_eq!(
            laid.moved(),
            ["b"],
            "★ before `c`, which is later in the row"
        );
        assert_eq!(laid.shown(), ["a", "c"]);
    }

    #[test]
    fn r1791_a_kept_item_never_moves_and_the_shortfall_is_reported() {
        let items = vec![
            Item::new(40, "a").kept(),
            Item::new(40, "b").kept(),
            Item::new(10, "c"),
        ];
        let laid = lay(50, 5, items).expect("room");
        assert_eq!(laid.shown(), ["a", "b"], "neither may move");
        assert_eq!(laid.moved(), ["c"], "and everything movable did");
        // 80 kept + 5 control against 50: short by 35, said rather than clipped.
        assert_eq!(laid.short_by(), 35);
        assert!(!laid.fits());
    }

    #[test]
    fn r1791_a_moved_item_is_not_in_shown() {
        // ★★★★★ The floor's third answer, inverted. Measured at 6.11: a hidden
        // action's `isVisible()` still answers true, so a reader asking "can I
        // reach this?" is told yes about something a person cannot see. Here
        // the two lists are disjoint and their union is everything.
        let laid = lay(25, 5, row(&[10, 10, 10])).expect("room");
        for name in laid.moved() {
            assert!(!laid.shown().contains(name), "{name} is in both lists");
        }
        let mut all: Vec<&str> = laid.shown().iter().chain(laid.moved()).copied().collect();
        all.sort_unstable();
        assert_eq!(all, ["a", "b", "c"], "nothing was lost between them");
    }

    #[test]
    fn r1791_a_control_wider_than_the_row_is_a_contradiction_not_a_shortfall() {
        let why = lay(10, 20, row(&[10, 10])).expect_err("no arrangement");
        assert_eq!(
            why,
            Unroomed::AffordanceTooWide {
                affordance: 20,
                available: 10
            }
        );
        assert!(why.to_string().contains("contradiction"), "{why}");
        // But a control exactly as wide as the row is legal: everything moves.
        // Three items, not two — with two the total is 20 and the row FITS, so
        // the control never appears. The first draft asserted an empty `shown`
        // against a row that fitted, and the run said so.
        let laid = lay(20, 20, row(&[10, 10, 10])).expect("room");
        assert!(
            laid.shown().is_empty(),
            "no room for a single item beside it"
        );
        assert_eq!(laid.moved(), ["a", "b", "c"]);
        assert_eq!(laid.short_by(), 0, "nothing was kept, so nothing is short");
    }

    #[test]
    fn r1791_an_empty_row_fits_anything_including_nothing() {
        let laid = lay(0, 20, Vec::<Item<&str>>::new()).expect("room");
        assert!(laid.fits());
        assert!(!laid.needs_affordance());
        assert_eq!(laid.short_by(), 0);
    }

    #[test]
    fn r1791_nothing_moves_that_did_not_have_to() {
        // The row gives up the least it can: 100 of items into 74 with a 4-wide
        // control leaves 70, so exactly one 30 goes and the other stays.
        let items = vec![Item::new(40, "a"), Item::new(30, "b"), Item::new(30, "c")];
        let laid = lay(74, 4, items).expect("room");
        assert_eq!(laid.moved(), ["c"], "one, not both");
        assert_eq!(laid.shown(), ["a", "b"]);
        let width: u32 = [40, 30].iter().sum::<u32>() + 4;
        assert!(width <= 74, "and what is left really does fit: {width}");
    }
}
