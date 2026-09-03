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
    /// One item, stating what the row does with it when the room runs out.
    ///
    /// ★★★★★ R1990 — **the policy is an argument and not a default**, which is
    /// the whole of this constructor's design.
    ///
    /// It used to be `new(width, value)` giving [`WhenTight::Move`], with
    /// `kept()` and `first_to_move()` to correct that afterwards. Measured on
    /// the one consumer this module has, that shape produced exactly the defect
    /// it invites: the node lab's toolbar chose per group with
    /// `if group == Run { item.kept() } else { item }`, so a group added later
    /// got `Move` **because nobody said anything**, and its author's written
    /// intent — that it be given up first — was not what the row did.
    /// [`WhenTight::MoveFirst`] existed, was tested, and had no consumer
    /// anywhere in the tree, because the call site offered two of the three.
    ///
    /// A default is an escape hatch that reads like a decision. There is one
    /// way to build an `Item` now and it has room for the policy, so the
    /// compiler asks every author rather than only the one who thought to
    /// reach for a modifier. The two modifiers are gone: they existed to undo
    /// a default that no longer exists.
    pub const fn new(width: u32, when_tight: WhenTight, value: T) -> Self {
        Self {
            width,
            when_tight,
            value,
        }
    }
}

/// A row, decided: what stays, what moved, what it would give up next, and
/// whether it still does not fit.
///
/// # Why one list and a count, and not two lists
///
/// ★★★★★ R1990 — the three questions a caller asks of a decided row are *what
/// is here*, *what went*, and *what goes next*, and the third is the one that
/// makes the other two auditable: a row that can only say what it already gave
/// up leaves its **policy** — the order it gives things up in — as a claim in a
/// comment. Measured on the node lab, that claim had been false for two rounds
/// in the direction that reads worst: a group whose doc said it was *"the first
/// thing a narrow toolbar gives up"* was the **last**, staying on the row while
/// two others went.
///
/// So this holds the items **once**, in the caller's order, plus the concession
/// order over the ones that may move, plus how many of that order were taken.
/// Three properties stop being things to maintain and become things that cannot
/// be written down wrong:
///
/// - [`Self::shown`] and [`Self::moved`] partition the items — one list, one
///   decision per position.
/// - A [`WhenTight::Keep`] item is never in [`Self::concession_order`], which is
///   what makes [`Self::short_by`] mean something.
/// - What moved is a **prefix** of the concession order, because it is a count
///   of it. A row cannot report having given up a later item while keeping an
///   earlier one — the shape has nowhere to say that.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row<T> {
    /// Every item, in the order the caller gave them.
    items: Vec<T>,
    /// Every item that MAY move, by position in `items`, in the order the row
    /// gives them up.
    order: Vec<usize>,
    /// How many of `order` were actually given up — a prefix, always.
    gone: usize,
    affordance: bool,
    short_by: u32,
}

impl<T> Row<T> {
    /// The positions that moved. A prefix of the concession order.
    fn gone(&self) -> &[usize] {
        &self.order[..self.gone]
    }

    /// The items that stay in the row, in the order they were given.
    #[must_use]
    pub fn shown(&self) -> impl DoubleEndedIterator<Item = &T> + Clone {
        let gone = self.gone();
        self.items
            .iter()
            .enumerate()
            .filter(move |(at, _)| !gone.contains(at))
            .map(|(_, item)| item)
    }

    /// How many items stayed in the row.
    #[must_use]
    pub fn shown_len(&self) -> usize {
        self.items.len() - self.gone
    }

    /// The items behind the affordance, in the order they were given.
    ///
    /// ★ The list the floor does not have. A consumer publishes this so a
    /// reader — a person opening the menu, or an agent asking what a screen can
    /// do — is told what moved rather than finding out by not seeing it.
    ///
    /// Row order, not give-up order: this is what a menu draws, and a menu is
    /// read the way the row was. [`Self::concession_order`] is the other one.
    #[must_use]
    pub fn moved(&self) -> impl DoubleEndedIterator<Item = &T> + Clone {
        let gone = self.gone();
        self.items
            .iter()
            .enumerate()
            .filter(move |(at, _)| gone.contains(at))
            .map(|(_, item)| item)
    }

    /// How many items moved behind the affordance.
    #[must_use]
    pub const fn moved_len(&self) -> usize {
        self.gone
    }

    /// **Every item that may move, in the order the row gives them up** —
    /// whether or not it has come to that yet.
    ///
    /// ★★★★★ R1990 — the row's policy, as a value rather than as a sentence.
    /// [`Self::moved`] answers *what went*, which is a fact about this width;
    /// this answers *what would go, in what order*, which is a fact about the
    /// row itself and therefore the thing a gate can hold still while the
    /// window moves. A [`WhenTight::Keep`] item is absent from it, so "may
    /// move" is readable too.
    ///
    /// The floor has neither: its toolbar gives up from the end with no way to
    /// say otherwise, and no member reports the order it would use.
    #[must_use]
    pub fn concession_order(&self) -> impl ExactSizeIterator<Item = &T> + Clone {
        self.order.iter().map(|&at| &self.items[at])
    }

    /// What the row gives up next if it loses more room, or `None` when
    /// everything that may move already has.
    ///
    /// The first item of [`Self::concession_order`] that is still shown — which
    /// is `order[gone]`, because what moved is a prefix.
    #[must_use]
    pub fn next_to_move(&self) -> Option<&T> {
        self.order.get(self.gone).map(|&at| &self.items[at])
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
    // Which to give up, in the order they are given up: the eager ones in row
    // order, then the ordinary ones from the end. `Keep` is never in this list,
    // which is what makes `short_by` mean something.
    //
    // ★ R1990 — computed for EVERY row, including one that fits. It is the
    // row's policy, not a consequence of this width, and a row that fits still
    // has to be able to say what it would give up first — see
    // [`Row::concession_order`].
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

    let total: u32 = items.iter().map(|item| item.width).sum();
    if total <= available {
        return Ok(Row {
            items: items.into_iter().map(|item| item.value).collect(),
            order,
            gone: 0,
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

    let room = available - affordance;
    let mut width = total;
    let mut gone = 0usize;
    for &at in &order {
        if width <= room {
            break;
        }
        width -= items[at].width;
        gone += 1;
    }

    Ok(Row {
        items: items.into_iter().map(|item| item.value).collect(),
        order,
        gone,
        affordance: true,
        short_by: width.saturating_sub(room),
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
            .map(|(w, name)| Item::new(*w, WhenTight::Move, name))
            .collect()
    }

    /// The three readers answer iterators, so a test comparing against a
    /// literal names what it is comparing.
    fn words<'a>(it: impl Iterator<Item = &'a &'static str>) -> Vec<&'static str> {
        it.copied().collect()
    }

    #[test]
    fn r1791_a_row_that_fits_moves_nothing_and_draws_no_control() {
        let laid = lay(100, 20, row(&[10, 20, 30])).expect("room");
        assert_eq!(words(laid.shown()), ["a", "b", "c"]);
        assert_eq!(laid.moved_len(), 0);
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
        assert_eq!(words(laid.shown()), ["a", "b"]);
        assert_eq!(words(laid.moved()), ["c"]);
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
            words(laid.shown()),
            ["a", "b", "c"],
            "the reader's eye is at the start"
        );
        assert_eq!(words(laid.moved()), ["d"]);
    }

    #[test]
    fn r1791_an_eager_item_goes_first_wherever_it_sits() {
        let items = vec![
            Item::new(10, WhenTight::Move, "a"),
            Item::new(10, WhenTight::MoveFirst, "b"),
            Item::new(10, WhenTight::Move, "c"),
        ];
        let laid = lay(25, 5, items).expect("room");
        assert_eq!(
            words(laid.moved()),
            ["b"],
            "★ before `c`, which is later in the row"
        );
        assert_eq!(words(laid.shown()), ["a", "c"]);
    }

    #[test]
    fn r1791_a_kept_item_never_moves_and_the_shortfall_is_reported() {
        let items = vec![
            Item::new(40, WhenTight::Keep, "a"),
            Item::new(40, WhenTight::Keep, "b"),
            Item::new(10, WhenTight::Move, "c"),
        ];
        let laid = lay(50, 5, items).expect("room");
        assert_eq!(words(laid.shown()), ["a", "b"], "neither may move");
        assert_eq!(words(laid.moved()), ["c"], "and everything movable did");
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
            assert!(
                !laid.shown().any(|kept| kept == name),
                "{name} is in both lists"
            );
        }
        let mut all: Vec<&str> = words(laid.shown().chain(laid.moved()));
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
        assert_eq!(laid.shown_len(), 0, "no room for a single item beside it");
        assert_eq!(words(laid.moved()), ["a", "b", "c"]);
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
        let items = vec![
            Item::new(40, WhenTight::Move, "a"),
            Item::new(30, WhenTight::Move, "b"),
            Item::new(30, WhenTight::Move, "c"),
        ];
        let laid = lay(74, 4, items).expect("room");
        assert_eq!(words(laid.moved()), ["c"], "one, not both");
        assert_eq!(words(laid.shown()), ["a", "b"]);
        let width: u32 = [40, 30].iter().sum::<u32>() + 4;
        assert!(width <= 74, "and what is left really does fit: {width}");
    }

    // ── R1990 — the row states the order it gives things up in ──────────

    #[test]
    fn r1990_a_row_that_fits_still_says_what_it_would_give_up_first() {
        // ★★★★★ The question the two-list shape could not be asked. Nothing has
        // moved, so `moved` is empty and says nothing about policy — and the
        // policy is exactly what a reader needs to check a claim of the form
        // "this one goes first", which is where the node lab's doc was wrong.
        let laid = lay(100, 20, row(&[10, 20, 30])).expect("room");
        assert!(laid.fits(), "the premise: it fits");
        assert_eq!(laid.moved_len(), 0);
        assert_eq!(
            words(laid.concession_order()),
            ["c", "b", "a"],
            "from the end, and it is knowable before it happens"
        );
        assert_eq!(laid.next_to_move(), Some(&"c"));
    }

    #[test]
    fn r1990_an_eager_item_is_first_in_the_order_wherever_it_sits() {
        let items = vec![
            Item::new(10, WhenTight::Move, "a"),
            Item::new(10, WhenTight::MoveFirst, "b"),
            Item::new(10, WhenTight::Move, "c"),
        ];
        // Wide enough that NOTHING has moved: the order is still the order.
        let laid = lay(1000, 5, items).expect("room");
        assert!(laid.fits());
        assert_eq!(words(laid.concession_order()), ["b", "c", "a"]);
        assert_eq!(
            laid.next_to_move(),
            Some(&"b"),
            "★ the claim `first_to_move` makes, readable at a width where it \
             has had no effect yet"
        );
    }

    #[test]
    fn r1990_a_kept_item_is_not_in_the_order_at_all() {
        // "May move" is readable, not only "did move". A row whose every item
        // is `Keep` gives up nothing and says so by having nothing to give.
        let items = vec![
            Item::new(40, WhenTight::Keep, "a"),
            Item::new(40, WhenTight::Keep, "b"),
            Item::new(10, WhenTight::Move, "c"),
        ];
        let laid = lay(50, 5, items).expect("room");
        assert_eq!(
            words(laid.concession_order()),
            ["c"],
            "the only movable one"
        );
        assert_eq!(
            laid.next_to_move(),
            None,
            "it already went, and neither `Keep` item can follow it"
        );
        assert_eq!(laid.short_by(), 35, "which is why the row is short");

        let all_kept = vec![
            Item::new(40, WhenTight::Keep, "a"),
            Item::new(40, WhenTight::Keep, "b"),
        ];
        let laid = lay(10, 5, all_kept).expect("room");
        assert_eq!(laid.concession_order().len(), 0);
        assert_eq!(laid.next_to_move(), None);
    }

    #[test]
    fn r1990_what_moved_is_a_prefix_of_the_order_at_every_width() {
        // ★★★★★ The invariant the shape makes unrepresentable, performed over
        // every width the row passes through. `moved` is a COUNT of `order`, so
        // there is nowhere to record "gave up the third but kept the second" —
        // this asserts the reader agrees with that, and that `next_to_move` is
        // the boundary between the two halves.
        for available in 0..=120u32 {
            let items = vec![
                Item::new(10, WhenTight::Move, "a"),
                Item::new(20, WhenTight::MoveFirst, "b"),
                Item::new(30, WhenTight::Move, "c"),
                Item::new(40, WhenTight::Keep, "d"),
            ];
            let Ok(laid) = lay(available, 5, items) else {
                continue;
            };
            let order = words(laid.concession_order());
            let moved = words(laid.moved());
            let (taken, left) = order.split_at(laid.moved_len());

            let mut taken_sorted = taken.to_vec();
            let mut moved_sorted = moved.clone();
            taken_sorted.sort_unstable();
            moved_sorted.sort_unstable();
            assert_eq!(
                taken_sorted,
                moved_sorted,
                "at {available}: what moved is the first {} of {order:?}",
                laid.moved_len()
            );
            assert_eq!(
                laid.next_to_move().copied(),
                left.first().copied(),
                "at {available}: next is the first not yet taken"
            );
            // And the partition, from the other side.
            assert_eq!(laid.shown_len() + laid.moved_len(), 4, "at {available}");
            for gone in &moved {
                assert!(
                    !words(laid.shown()).contains(gone),
                    "at {available}: {gone} is in both lists"
                );
            }
            assert!(
                !moved.contains(&"d"),
                "at {available}: a kept item never moves"
            );
        }
    }
}
