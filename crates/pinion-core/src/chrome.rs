//! ★★★★★ R1725 §5.16 §5.38 §2 #7 — **what the place you were put in already
//! provides.**
//!
//! # What was missing, measured on the running application
//!
//! R1724 made a screen mountable, and the first mount drew this: at the
//! Catalog destination the shell's navigation occupies x=0..52 and the mounted
//! screen paints **its own** at x=52..106 — two navigations side by side, for
//! one application, and the accessibility tree published both of them
//! (`role=navigation`, named *Destinations* and *sections*). Mounting did not
//! create that; it made it visible.
//!
//! The guest is not at fault and neither is the host. A screen that has ever
//! run standalone **needs** its own navigation, and that need stops being true
//! the moment it is placed inside an application that has one — which is a fact
//! about *the place*, not about the screen. Nothing carried that fact.
//!
//! # The shape
//!
//! Exactly [`with_surface_extent`](crate::external::with_surface_extent)'s:
//! the placer states what it provides, for the duration of building the guest's
//! scene, and the guest reads it. A screen that is not placed reads
//! [`HostChrome::NONE`] and behaves as it always did, so the standalone path is
//! byte-identical.
//!
//! ```
//! use pinion_core::chrome::{HostChrome, Part, host_chrome, with_host_chrome};
//!
//! // Standalone: nothing is provided, so the screen draws its own.
//! assert!(!host_chrome().provides(Part::Navigation));
//!
//! // Placed inside a shell that has a navigation rail:
//! with_host_chrome(HostChrome::NONE.with(Part::Navigation), || {
//!     assert!(host_chrome().provides(Part::Navigation));
//!     assert!(!host_chrome().provides(Part::ApplicationBar));
//! });
//! ```
//!
//! ## Why a scope and not a field on the screen
//!
//! A screen does not know where it is, and it must not have to be told twice —
//! once when it is constructed and again when it moves. The same binding is a
//! window in one process and a page in another **in the same build**, and the
//! answer differs per frame boundary rather than per instance.
//!
//! ⚠ **R1825 — this paragraph used to end "a scope is the only form in which
//! the fact cannot be stale", and a measurement refuted it.** A scope makes the
//! fact *absent* everywhere else, and absent is read as [`HostChrome::NONE`],
//! which is not "no answer" but the specific answer *you are standalone*. The
//! framework calls a mounted guest's pointer, wheel and drag hooks from outside
//! every scope, so a screen that laid its panes out for a host drawing the
//! application bar hit-tested them for a window where it draws its own. On the
//! analysis tool: **41 of the node lab's regions addressed a DIFFERENT region
//! at their own centre when mounted, and 0 did standalone at the same size.**
//!
//! ⚠ The number without a denominator on purpose, and R1825's own closing audit
//! is why: this said *41 of 182* in four places, and 182 is the count of
//! regions painted in a frame taken AFTER a scroll, while the reading cited as
//! authoritative was taken before one and painted 179. The astray count is 41
//! in both and the standalone comparison is 0 in both — those are the facts
//! about the defect. A denominator that moves with what happens to be scrolled
//! into view is a fact about the frame.
//!
//! [`with_host_chrome_for`] / [`host_chrome_for`] add the
//! recorded fallback [`layout_size`](crate::external::layout_size) has had all
//! along, and a screen should read the `_for` spelling.
//!
//! ## Against the reference toolkit
//!
//! Measured by building a probe against 6.11.1 and running it. A complete
//! application window — menu bar, tool bar, status bar — was placed inside
//! another application's page container:
//!
//! | question | there | here |
//! |---|---|---|
//! | the guest's own menu bar, once placed | **still drawn**, 23 px of it | omitted, because the guest asked |
//! | its tool bar | **still drawn** | the guest's own to keep or omit |
//! | its status bar | **still drawn** | likewise |
//! | menu bars in the accessibility tree | **2** | one navigation at the destination |
//! | tool bars in the tree | **2** | — |
//! | status bars in the tree | **2** | — |
//! | can the guest ask what its place provides | **nothing to ask.** A child reads geometry, palette, font, locale, layout direction and style from its parent; none of them names a bar | [`host_chrome`] |
//! | the nearest available signal | `window()` answers the **host's** window, so a guest can learn *that* it is embedded and nothing about what that place has | a set, so the guest omits what is provided and keeps what is not |
//!
//! The last two rows are the axis. There, "am I embedded" is a boolean a guest
//! can infer, and every guest that acts on it must then *assume* what its host
//! provides — which is why the ordinary outcome is the first three rows: two of
//! every bar, and a reader told the application has two navigations.

use std::cell::RefCell;

use crate::scene::Rect;

/// One part of the application frame a host can provide to the screens it
/// shows.
///
/// A closed set on purpose: each arm is a thing a *guest* must be able to
/// decide about itself, so an arm nobody can act on would be a declaration
/// with no consumer. Adding one is a decision about what a screen may omit.
///
/// # Why it is not called `Chrome`
///
/// [`style::Chrome`](crate::style::Chrome) already is, and it means something
/// else one level down: *a band this box keeps of its own edge*, for a caption
/// or a header or a toolbar. This is about the application **around** a page.
/// Two concepts under one word is the defect this tree keeps finding written
/// the other way round — one concept spelled two ways — and it costs the same
/// either way, so the word stays where it was and the new type takes a name
/// that is only ever read module-qualified: `chrome::Part::Navigation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Part {
    /// The navigation that moves between an application's destinations — the
    /// rail, the tab bar, the sidebar.
    ///
    /// Provided by a host that has a destination roster, which is every host
    /// that can show more than one screen.
    Navigation,
    /// The application bar: who the application is, what it is globally doing,
    /// and the global search.
    ///
    /// Distinct from [`Part::Navigation`] because a screen may legitimately
    /// keep a bar of its **own** content — a graph's name and run state are the
    /// screen's, not the application's — while having no business restating the
    /// application's identity.
    ApplicationBar,
}

impl Part {
    /// Every arm, so a census over this vocabulary cannot go stale by being
    /// hand-written.
    pub const ALL: &'static [Part] = &[Part::Navigation, Part::ApplicationBar];

    /// The name this arm carries on the wire and in a report.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Part::Navigation => "navigation",
            Part::ApplicationBar => "application_bar",
        }
    }

    const fn bit(self) -> u32 {
        match self {
            Part::Navigation => 1 << 0,
            Part::ApplicationBar => 1 << 1,
        }
    }
}

/// What the place a screen was put in already provides.
///
/// A set rather than a boolean: a host that has a navigation rail but no
/// application bar is an ordinary arrangement, and a guest deciding what to
/// omit needs the difference. `Copy` and const-constructible so a host can
/// declare it beside its other constants.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostChrome(u32);

impl HostChrome {
    /// Nothing is provided — a screen filling its own window, which is what
    /// [`host_chrome`] answers outside any scope.
    pub const NONE: Self = Self(0);

    /// This set, plus `part`.
    #[must_use]
    pub const fn with(self, part: Part) -> Self {
        Self(self.0 | part.bit())
    }

    /// Whether the place provides `part`, and therefore whether a guest should
    /// leave its own out.
    #[must_use]
    pub const fn provides(self, part: Part) -> bool {
        self.0 & part.bit() != 0
    }

    /// Whether the place provides nothing at all.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// What is provided, in [`Part::ALL`] order — for a report, and for the
    /// wire.
    #[must_use]
    pub fn names(self) -> Vec<&'static str> {
        Part::ALL
            .iter()
            .filter(|c| self.provides(**c))
            .map(|c| c.name())
            .collect()
    }
}

thread_local! {
    /// The stack of places. A stack because a placed screen may itself place
    /// one, and the innermost is the place.
    ///
    /// ★★ **Obligation 3b, measured and deferred with the count.** This is the
    /// *second* scoped-fact stack in this crate — a `RefCell<Vec<_>>` whose
    /// innermost entry wins and whose pop is a `Drop` guard so a panicking view
    /// unwinds it. The other is
    /// [`external::with_surface_extent`](crate::external::with_surface_extent),
    /// R1724's, and both say the same kind of thing: *a fact about the place
    /// you were put, true only while the placer is building the frame it
    /// computed*. They are not identical — the extent is keyed by surface tag
    /// because several surfaces share a scope, and chrome is not because only
    /// one screen is being built at a time — so this is the opinionated case
    /// the lift rule defers to a **third** identical site rather than the
    /// mechanical case it lifts at once. Whoever writes the third one lifts all
    /// three; the count is written here so that decision is not re-derived.
    ///
    /// ★ R1825 — **the two are now the same shape**, which strengthens that
    /// deferred lift rather than weakening it: each is a scope for the build
    /// plus a per-surface record for the calls the framework makes afterwards.
    /// Re-measured this round, they are still the only two scoped *place* facts
    /// in this crate. ⚠ And the pairing is the lesson a third site must inherit:
    /// **a scoped fact needs a fallback that is the last known truth, because
    /// its default is read as an answer.** The extent had one from the start
    /// and chrome did not, and the difference cost a mounted screen 41 regions
    /// that addressed something other than themselves.
    static PROVIDED: RefCell<Vec<HostChrome>> = const { RefCell::new(Vec::new()) };

    /// ★★★★★ R1825 — **the placed screen's declaration, for the calls that do
    /// not happen inside a build.**
    ///
    /// The stack above is true exactly while the host is building the guest's
    /// scene. The module header used to argue that this is the only form in
    /// which the fact cannot be stale. **Measured, that argument was wrong in a
    /// way that costs a screen its gestures**: the framework calls a mounted
    /// guest's [`External`](crate::external::External) hooks — `target_at`, the
    /// press path, `wheel` — from OUTSIDE every scope, and outside a scope
    /// [`host_chrome`] answers [`HostChrome::NONE`], which is not "no answer"
    /// but the specific answer *you are standalone*. So a guest laid its panes
    /// out for a host that draws the application bar and hit-tested them for a
    /// window where it draws its own, and every rectangle below the bar was one
    /// bar's height out of step. Measured on the analysis tool at R1825: 41 of
    /// the node lab's regions addressed a DIFFERENT region at their own centre,
    /// and 0 did so standalone at the same size. (No denominator: see this
    /// module's header for why one would be a fact about the frame.)
    ///
    /// ⚠ Absence read as a default is the general shape, and this crate already
    /// had the answer to it one module over:
    /// [`layout_size`](crate::external::layout_size) falls back to the surface's
    /// last RECORDED size rather than to a design constant, so a call from
    /// outside a build gets the last known truth. Chrome had no such fallback.
    /// This is it.
    ///
    /// Keyed by surface tag, and holding at most the screen that is placed:
    /// [`forget_host_chrome`] is what a host calls when it stops placing one, so
    /// a record cannot outlive the placement it describes.
    static RECORDED: RefCell<Vec<(String, HostChrome)>> = const { RefCell::new(Vec::new()) };

    /// ★★★★★ R2135 — **the other half of the declaration: who read it.**
    ///
    /// `PROVIDED` and `RECORDED` are both what a host SAYS. Nothing here held
    /// what a guest DID about it, so the axis was one-way and a reader census
    /// over it was impossible to write — which is why the only accounting that
    /// existed was a hand-written test per (guest, part) in the host's own
    /// file, and why 2 of 12 such pairs had one.
    ///
    /// A `Vec` rather than a set, and cumulative rather than scoped, for two
    /// reasons that are the same reason: what it holds is a fact about the
    /// guest's CODE — *this screen decides about this part* — not about a
    /// placement, so it must outlive a placement the way the code does, and it
    /// is bounded by (surfaces × [`Part::ALL`]) rather than by frames.
    static ASKED: RefCell<Vec<(String, Part)>> = const { RefCell::new(Vec::new()) };
}

/// State what this host provides, for the duration of `body`.
///
/// The innermost scope wins, so a screen that mounts a screen answers about
/// **its** guest's place rather than about its own.
pub fn with_host_chrome<R>(chrome: HostChrome, body: impl FnOnce() -> R) -> R {
    PROVIDED.with(|stack| stack.borrow_mut().push(chrome));
    // A guard rather than a line after `body()`: a view that panics would
    // otherwise leave the declaration standing, and the next frame's guest
    // would omit chrome nobody is providing — which is worse than drawing two,
    // because the missing one cannot be reached at all.
    let _pop = PopOnDrop;
    body()
}

struct PopOnDrop;

impl Drop for PopOnDrop {
    fn drop(&mut self) {
        PROVIDED.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

/// State what this host provides to the surface `tag`, for the duration of
/// `body` **and** for the calls the framework makes on that surface afterwards.
///
/// R1825. Identical to [`with_host_chrome`] inside `body`, and additionally
/// records the declaration against `tag` so [`host_chrome_for`] can answer from
/// outside a build. The defect that asked for it is in this module's header: a
/// mounted screen's pointer hooks run outside every scope, and a screen that
/// answers "standalone" there lays out and hit-tests two different screens.
///
/// A host that calls this owes [`forget_host_chrome`] when it stops placing the
/// surface, so a record cannot outlive the placement.
pub fn with_host_chrome_for<R>(tag: &str, chrome: HostChrome, body: impl FnOnce() -> R) -> R {
    RECORDED.with(|rec| {
        let mut rec = rec.borrow_mut();
        match rec.iter_mut().find(|(t, _)| t == tag) {
            Some(slot) => slot.1 = chrome,
            None => rec.push((tag.to_string(), chrome)),
        }
    });
    with_host_chrome(chrome, body)
}

/// Drop the record [`with_host_chrome_for`] kept for `tag` — what a host calls
/// when it stops placing that surface.
///
/// Idempotent, and cheap enough to call every frame: a host that is not placing
/// anything can clear unconditionally rather than tracking whether it was.
pub fn forget_host_chrome(tag: &str) {
    RECORDED.with(|rec| rec.borrow_mut().retain(|(t, _)| t != tag));
}

/// What the innermost enclosing [`with_host_chrome`] provides.
///
/// [`HostChrome::NONE`] outside any scope, which is a screen running in its own
/// window — so a binding that never asks, and a binding asked while standalone,
/// both behave exactly as they did before this existed.
///
/// ⚠ **A screen that is MOUNTED should read [`host_chrome_for`] instead.** This
/// answers `NONE` outside a build, and for a placed screen that is a wrong
/// answer rather than a missing one — see this module's header.
#[must_use]
pub fn host_chrome() -> HostChrome {
    PROVIDED.with(|stack| stack.borrow().last().copied().unwrap_or(HostChrome::NONE))
}

/// What the place the surface `tag` was put in provides — from the enclosing
/// scope when there is one, and otherwise from what its host last declared.
///
/// R1825, and this is what a **screen** should ask. The scope answers while the
/// host is building the guest's scene; the record answers for every call the
/// framework makes on the guest afterwards, which is where the pointer, the
/// wheel and the drag hooks all live. Both spellings agree inside a build, so a
/// screen that reads this reads one fact rather than two.
///
/// [`HostChrome::NONE`] when neither has anything to say, which is a screen
/// running in its own window.
#[must_use]
pub fn host_chrome_for(tag: &str) -> HostChrome {
    if let Some(scoped) = PROVIDED.with(|stack| stack.borrow().last().copied()) {
        return scoped;
    }
    RECORDED.with(|rec| {
        rec.borrow()
            .iter()
            .find(|(t, _)| t == tag)
            .map_or(HostChrome::NONE, |(_, chrome)| *chrome)
    })
}

/// ★★★★★ R2135 — **ask what this place provides, and be on the record for
/// having asked.**
///
/// The spelling a mounted screen should use. Identical in answer to
/// `host_chrome_for(tag).provides(part)`, and additionally records that the
/// surface `tag` decided about `part`, which is what [`unasked`] reads.
///
/// # Why the asking has to be recorded
///
/// Before this, a host declared a set and each guest independently wrote its
/// own `!…provides(…)` and derived its own number from it. Nothing related the
/// offer to the decision, so the offer was a broadcast with no reader census —
/// and both failures that follow from that are ones this module has already
/// paid for:
///
/// * [`Part::ApplicationBar`] sat in the vocabulary declared by nobody and read
///   by nobody for **97 rounds** (R1725 → R1822). A dead arm is not
///   distinguishable from a live one when nothing counts readers.
/// * Measured at R2135 on the analysis tool: the shell mounts **six** screens
///   and declares both parts to all six. **One** of them asks.
///   `hello-packet-view` paints its own application bar — the interface and the
///   packet rate, which is what the host's bar carries — under a host that is
///   providing one, and every gate is green because no gate had the pair as a
///   population.
///
/// ⚠ **And the census that was prescribed for this would have been green on
/// arrival.** The prescription was *a census over [`Part::ALL`] answering
/// whether both arms are consumed*. Measured, both arms are consumed — by the
/// one guest that asks — so that assertion passes while five of six guests
/// ignore the declaration entirely. **The arm is not the unit. The
/// (surface, arm) pair is.**
///
/// # What it cannot see, and why that is the right way round
///
/// It cannot see a guest that never ran. So the population does not come from
/// here: it comes from the HOST — every screen it mounted, crossed with every
/// part it declared — and a screen missing from this record is *reported*. A
/// census that failed to reach a destination therefore fails loudly, rather
/// than passing for having asked about nothing.
#[must_use]
pub fn ask(tag: &str, part: Part) -> bool {
    ASKED.with(|asked| {
        let mut asked = asked.borrow_mut();
        if !asked.iter().any(|(t, p)| t == tag && *p == part) {
            asked.push((tag.to_string(), part));
        }
    });
    host_chrome_for(tag).provides(part)
}

/// Whether surface `tag` has ever [`ask`]ed about `part`.
#[must_use]
pub fn has_asked(tag: &str, part: Part) -> bool {
    ASKED.with(|asked| asked.borrow().iter().any(|(t, p)| t == tag && *p == part))
}

/// Every `(surface, part)` pair that has been [`ask`]ed, ordered by surface and
/// then by [`Part::ALL`] so a report does not move with the order things ran in.
#[must_use]
pub fn asked() -> Vec<(String, Part)> {
    let mut all = ASKED.with(|asked| asked.borrow().clone());
    all.sort();
    all
}

/// Of what `provided` offers, the parts surface `tag` has never [`ask`]ed about.
///
/// The shape a host's census wants: hand it what it is providing to one screen
/// and it answers what that screen has not decided about. Ordered by
/// [`Part::ALL`], so the report does not move with the caller.
///
/// Empty for a `tag` that has asked about everything offered — and empty, too,
/// for a host that offers nothing, which is the honest answer: a guest owes no
/// decision about a part nobody is providing.
#[must_use]
pub fn unasked(tag: &str, provided: HostChrome) -> Vec<Part> {
    Part::ALL
        .iter()
        .copied()
        .filter(|part| provided.provides(*part) && !has_asked(tag, *part))
        .collect()
}

/// Forget that `tag` ever asked — for a test that needs the record to start
/// empty, and for nothing else.
///
/// ⚠ **Not the twin of [`forget_host_chrome`], and deliberately not called
/// where that is.** What a host declares is a fact about a *placement* and ends
/// with it. Whether a screen's code decides about a part is a fact about *the
/// screen*, and it does not stop being true because the journey moved. Clearing
/// it alongside the declaration would make the census answer depend on which
/// destination the reader happens to be standing at — which is the class of
/// defect this module opened with.
pub fn forget_asked(tag: &str) {
    ASKED.with(|asked| asked.borrow_mut().retain(|(t, _)| t != tag));
}

/// ★★★★★ R1861 — **where to put a floating overlay so it covers nothing.**
///
/// `seat` moved the shortest distance that clears **every** band in `bands`
/// while staying wholly inside `within`, or `None` when no such place exists.
///
/// # The defect this comes from
///
/// A host's status toast is positioned against the window — the reference's own
/// `bottom: 22px`, centred — and a guest lays its own content out against the
/// region it was given. Neither knows about the other, and measured on the
/// analysis tool at its shipping size the toast covered **the top 6 pixels of
/// the node lab's gesture hint** and **the whole of two reassembly lane
/// readouts** on the capture viewer. A person reported the first of those as
/// *"I cannot read it"*, and every gate was green: containment asks whether a
/// mark is inside its own box, and an overlay is its own top-level box.
///
/// # Why moving the overlay and not the content
///
/// The guest's layout is its specification's; the overlay is the thing that
/// floats. Tooltips and popups in every mature toolkit flip or slide to stay on
/// screen for the same reason — what is displaced is what was placed *over*.
/// What is new here is the thing avoided: those policies avoid the screen's
/// EDGE, and this avoids the CONTENT, because a reader loses a sentence the
/// same way whether it left the window or was painted on.
///
/// # The rule
///
/// The four ways out of each band are tried and the shortest that clears them
/// **all** wins, with a fixed order — above, below, left, right — so a tie is
/// resolved the same way on every frame. An overlay that jumped between two
/// equally good places as a sentence changed length would be worse than one that
/// covered something.
///
/// # ⚠ `bands` is a list because the first draft took one, and one was half
///
/// It took the band the *guest* declared, and the first pixel measurement of the
/// repair found the overlay still on top of a sentence — **the host's own**. The
/// reader had named runs from two different strips in one sentence and this had
/// only asked about one of them. A seat has to clear everything under it, and
/// "everything" is not a fact one declaration can carry.
#[must_use]
pub fn clear_of(seat: Rect, bands: &[Rect], within: Rect) -> Option<Rect> {
    let meets = |a: Rect, b: Rect| {
        !(a.x >= b.x + b.w || b.x >= a.x + a.w || a.y >= b.y + b.h || b.y >= a.y + a.h)
    };
    let inside = |r: Rect| {
        r.x >= within.x
            && r.y >= within.y
            && r.x + r.w <= within.x + within.w
            && r.y + r.h <= within.y + within.h
    };
    let clears = |r: Rect| bands.iter().all(|band| !meets(r, *band));
    if clears(seat) {
        // Already clear. Reported as `Some` rather than as a distinct arm: a
        // caller wants the seat to use, and "it did not have to move" is a fact
        // about the frame rather than a different kind of answer.
        return Some(seat);
    }
    bands
        .iter()
        .flat_map(|band| {
            [
                Rect::new(seat.x, band.y.saturating_sub(seat.h), seat.w, seat.h),
                Rect::new(seat.x, band.y + band.h, seat.w, seat.h),
                Rect::new(band.x.saturating_sub(seat.w), seat.y, seat.w, seat.h),
                Rect::new(band.x + band.w, seat.y, seat.w, seat.h),
            ]
        })
        .filter(|candidate| inside(*candidate) && clears(*candidate))
        .map(|candidate| {
            let moved = candidate.x.abs_diff(seat.x) + candidate.y.abs_diff(seat.y);
            (moved, candidate)
        })
        // `min_by_key` keeps the FIRST of equal keys, which is what makes the
        // order above the tie-break rather than an accident of iteration.
        .min_by_key(|(moved, _)| *moved)
        .map(|(_, candidate)| candidate)
}

#[cfg(test)]
mod tests {
    use super::{
        HostChrome, Part, Rect, ask, asked, clear_of, forget_asked, forget_host_chrome, has_asked,
        host_chrome, host_chrome_for, unasked, with_host_chrome, with_host_chrome_for,
    };

    /// The window the analysis tool ships in, and the region it hands a guest.
    const WITHIN: Rect = Rect::new(52, 52, 1388, 848);
    /// Where the host puts its toast before anything is avoided — the
    /// reference's own placement, centred and 22 above the foot.
    const SEAT: Rect = Rect::new(630, 844, 180, 34);

    /// The host's own help strip, measured on the assembled tool.
    const HELP: Rect = Rect::new(662, 853, 400, 14);
    /// The node lab's gesture hint, measured on the assembled tool.
    const HINT: Rect = Rect::new(294, 866, 560, 24);
    /// The capture viewer's reassembly strip, which spans the whole region.
    const STRIP: Rect = Rect::new(52, 804, 1388, 96);

    #[test]
    fn r1861_a_seat_that_covers_nothing_does_not_move() {
        let elsewhere = Rect::new(294, 200, 560, 24);
        assert_eq!(clear_of(SEAT, &[elsewhere], WITHIN), Some(SEAT));
    }

    #[test]
    fn r1861_a_seat_over_a_band_rises_just_clear_of_it() {
        let moved = clear_of(SEAT, &[HINT], WITHIN).expect("there is room above the hint");
        assert_eq!(moved, Rect::new(630, 832, 180, 34));
        assert!(
            moved.y + moved.h <= HINT.y,
            "the seat still reaches into the band it was moved off"
        );
    }

    /// ★ The strip spans the whole region, so the two sideways ways out do not
    /// fit and the answer has to be the one that does — which is what makes this
    /// more than a repeat of the case above.
    #[test]
    fn r1861_a_band_spanning_the_region_leaves_only_one_way_out() {
        let moved = clear_of(SEAT, &[STRIP], WITHIN).expect("there is room above the strip");
        assert_eq!(moved, Rect::new(630, 770, 180, 34));
    }

    /// ★★★★★ The case the one-band draft got wrong: clearing the guest's strip
    /// put the seat straight onto the HOST's, and only asking about both finds
    /// the place that clears each.
    #[test]
    fn r1861_a_seat_clears_every_band_and_not_just_the_first() {
        let one = clear_of(SEAT, &[HINT], WITHIN).expect("a place clear of the hint");
        assert!(
            one.y < HELP.y + HELP.h && HELP.y < one.y + one.h,
            "the one-band answer must land on the host's own strip, or this \
             test is not about the defect it was written for"
        );
        let both = clear_of(SEAT, &[HELP, HINT], WITHIN).expect("a place clear of both");
        for band in [HELP, HINT] {
            let meets = both.y < band.y + band.h
                && band.y < both.y + both.h
                && both.x < band.x + band.w
                && band.x < both.x + both.w;
            assert!(!meets, "the seat still meets {band:?}");
        }
        assert_eq!(both, Rect::new(630, 819, 180, 34));
    }

    #[test]
    fn r1861_a_band_with_no_room_around_it_refuses() {
        let everything = Rect::new(52, 52, 1388, 848);
        assert_eq!(clear_of(SEAT, &[everything], WITHIN), None);
    }

    /// ★ Ties resolve the same way every frame, or an overlay would jump
    /// between two equally good places as its sentence changed length.
    #[test]
    fn r1861_an_equal_choice_is_resolved_the_same_way_every_time() {
        let within = Rect::new(0, 0, 400, 400);
        let seat = Rect::new(150, 180, 100, 40);
        let band = Rect::new(100, 180, 200, 40);
        let up = clear_of(seat, &[band], within).expect("both ways fit");
        assert_eq!(up, Rect::new(150, 140, 100, 40), "above wins a tie");
        assert_eq!(
            clear_of(seat, &[band], within),
            Some(up),
            "and it keeps winning"
        );
    }

    /// ★★★★★ R1825 — **the declaration survives the build it was made in**,
    /// which is the whole repair.
    ///
    /// The framework calls a mounted guest's pointer hooks from outside every
    /// scope. Before this, such a call read `NONE` — the answer that means
    /// *standalone* — so a placed screen laid out for one arrangement and
    /// hit-tested for another.
    #[test]
    fn r1825_a_placed_surface_answers_after_the_build_scope_has_closed() {
        forget_host_chrome("guest");
        let placed = HostChrome::NONE.with(Part::ApplicationBar);

        with_host_chrome_for("guest", placed, || {
            assert_eq!(
                host_chrome_for("guest"),
                placed,
                "inside, the scope answers"
            );
            assert_eq!(host_chrome(), placed, "and the old spelling agrees");
        });

        assert_eq!(
            host_chrome(),
            HostChrome::NONE,
            "the scope really did close -- otherwise this proves nothing"
        );
        assert_eq!(
            host_chrome_for("guest"),
            placed,
            "★ and the placed surface still reads its place, which is the call \
             the framework makes when it asks what is under a pointer"
        );
        forget_host_chrome("guest");
    }

    #[test]
    fn r1825_a_surface_nobody_placed_reads_nothing_from_the_record() {
        forget_host_chrome("guest");
        with_host_chrome_for("guest", HostChrome::NONE.with(Part::Navigation), || {});
        assert_eq!(
            host_chrome_for("other"),
            HostChrome::NONE,
            "the record is keyed by surface: one guest's place is not another's"
        );
        forget_host_chrome("guest");
    }

    #[test]
    fn r1825_forgetting_is_what_stops_a_record_outliving_its_placement() {
        forget_host_chrome("guest");
        with_host_chrome_for("guest", HostChrome::NONE.with(Part::Navigation), || {});
        assert!(host_chrome_for("guest").provides(Part::Navigation));
        forget_host_chrome("guest");
        assert_eq!(
            host_chrome_for("guest"),
            HostChrome::NONE,
            "★ a screen that stops being placed must stop reading a place, or it \
             would omit chrome nobody is drawing -- the failure the scope's own \
             Drop guard exists to prevent, one level up"
        );
        forget_host_chrome("guest");
    }

    #[test]
    fn r1825_an_inner_scope_still_wins_over_an_outer_record() {
        forget_host_chrome("guest");
        with_host_chrome_for("guest", HostChrome::NONE.with(Part::Navigation), || {
            with_host_chrome(HostChrome::NONE.with(Part::ApplicationBar), || {
                assert_eq!(
                    host_chrome_for("guest"),
                    HostChrome::NONE.with(Part::ApplicationBar),
                    "a screen that places a screen answers about ITS guest's \
                     place, and the record must not shadow that"
                );
            });
        });
        forget_host_chrome("guest");
    }

    #[test]
    fn r1725_a_screen_that_is_not_placed_is_told_nothing() {
        assert_eq!(host_chrome(), HostChrome::NONE);
        assert!(host_chrome().is_empty());
        for chrome in Part::ALL {
            assert!(
                !host_chrome().provides(*chrome),
                "{chrome:?} must not be reported provided outside any scope, or \
                 a standalone screen would omit chrome nothing is drawing"
            );
        }
    }

    #[test]
    fn r1725_the_set_distinguishes_its_members() {
        let nav = HostChrome::NONE.with(Part::Navigation);
        assert!(nav.provides(Part::Navigation));
        assert!(
            !nav.provides(Part::ApplicationBar),
            "a host with a rail and no application bar is an ordinary \
             arrangement, and a guest deciding what to omit needs the difference"
        );
        let both = nav.with(Part::ApplicationBar);
        assert!(both.provides(Part::Navigation) && both.provides(Part::ApplicationBar));
        assert_eq!(both.names(), vec!["navigation", "application_bar"]);
    }

    #[test]
    fn r1725_the_declaration_ends_with_the_scope() {
        with_host_chrome(HostChrome::NONE.with(Part::Navigation), || {
            assert!(host_chrome().provides(Part::Navigation));
        });
        assert_eq!(
            host_chrome(),
            HostChrome::NONE,
            "a stale declaration would make a guest omit a rail nobody draws"
        );
    }

    /// ★ The innermost place is the place: a screen that mounts a screen
    /// answers about its guest's surroundings, not its own.
    #[test]
    fn r1725_the_innermost_place_is_the_one_you_are_in() {
        with_host_chrome(HostChrome::NONE.with(Part::Navigation), || {
            with_host_chrome(HostChrome::NONE.with(Part::ApplicationBar), || {
                assert!(host_chrome().provides(Part::ApplicationBar));
                assert!(!host_chrome().provides(Part::Navigation));
            });
            assert!(
                host_chrome().provides(Part::Navigation),
                "and the outer place is restored when the inner one ends"
            );
        });
    }

    /// ★★★★★ A panicking guest must not leave the declaration standing. The
    /// direction matters: a stale "the host provides navigation" makes the next
    /// screen omit a rail nothing is drawing, and a destination nobody can
    /// reach is worse than a destination drawn twice.
    #[test]
    fn r1725_a_panicking_guest_leaves_no_declaration_standing() {
        let caught = std::panic::catch_unwind(|| {
            with_host_chrome(HostChrome::NONE.with(Part::Navigation), || {
                panic!("the guest's view failed");
            })
        });
        assert!(caught.is_err(), "the panic is the fixture");
        assert_eq!(host_chrome(), HostChrome::NONE);
    }

    /// The vocabulary and its names are one list, so a report cannot name an
    /// arm the set cannot hold.
    #[test]
    fn r1725_every_arm_is_named_and_distinct() {
        let mut names: Vec<&str> = Part::ALL.iter().map(|c| c.name()).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "two arms share a name");
        for chrome in Part::ALL {
            assert_eq!(
                HostChrome::NONE.with(*chrome).names(),
                vec![chrome.name()],
                "a set holding only {chrome:?} must report only its name"
            );
        }
    }

    // ── R2135: the reader half of the declaration ────────────────────────────
    //
    // ⚠ Each `#[test]` runs on its own thread and the record is thread-local,
    // so these do not need to isolate from one another. They isolate from
    // THEMSELVES — `forget_asked` at the top — because a test that passed only
    // because an earlier line in the same test had already asked would be
    // asserting the opposite of what it says.

    /// ★★★★★ The pair, not the arm. This is the measurement that refuted the
    /// prescribed census: *both arms are consumed* is true while a guest that
    /// never asks sits beside the one that does.
    #[test]
    fn r2135_an_arm_with_a_reader_says_nothing_about_a_guest_that_never_asked() {
        let both = HostChrome::NONE
            .with(Part::Navigation)
            .with(Part::ApplicationBar);
        forget_asked("asks");
        forget_asked("silent");

        with_host_chrome_for("asks", both, || {
            assert!(ask("asks", Part::Navigation));
            assert!(ask("asks", Part::ApplicationBar));
        });

        // The arm-level question — "does every arm have a reader" — is now YES
        // for both arms, and it is exactly as green as it would be with a
        // hundred silent guests.
        for part in Part::ALL {
            assert!(
                asked().iter().any(|(_, p)| p == part),
                "{part:?} has no reader at all, so this fixture is not the one \
                 the test is about"
            );
        }

        // The pair-level question answers the thing that matters.
        assert_eq!(unasked("asks", both), Vec::<Part>::new());
        assert_eq!(
            unasked("silent", both),
            vec![Part::Navigation, Part::ApplicationBar]
        );
    }

    /// A guest owes a decision only about what it is offered: a host providing
    /// nothing is owed nothing, which is what keeps a standalone screen out of
    /// every census.
    #[test]
    fn r2135_nothing_is_owed_about_a_part_nobody_provides() {
        forget_asked("standalone");
        assert_eq!(unasked("standalone", HostChrome::NONE), Vec::<Part>::new());
        assert_eq!(
            unasked("standalone", HostChrome::NONE.with(Part::Navigation)),
            vec![Part::Navigation],
            "offered one part and having decided about neither, exactly the \
             offered one is owed"
        );
    }

    /// ★★ Asking is recorded whatever the answer is. A guest told *no* has
    /// decided just as much as one told *yes* — the record is about the
    /// decision, not about the omission — so a standalone screen that asks is
    /// not reported as having ignored anything.
    #[test]
    fn r2135_a_guest_told_no_has_still_asked() {
        forget_asked("told_no");
        assert!(
            !ask("told_no", Part::Navigation),
            "nothing is being provided"
        );
        assert!(has_asked("told_no", Part::Navigation));
        assert_eq!(
            unasked("told_no", HostChrome::NONE.with(Part::Navigation)),
            Vec::<Part>::new()
        );
    }

    /// ★★★★★ The record outlives the placement, and that is the whole reason it
    /// is not cleared beside it: whether this screen's code decides about a
    /// part does not stop being true because the journey moved off it.
    ///
    /// Without this, a host's census would answer differently depending on
    /// which destination a reader happened to be standing at — and the answer
    /// it would give is *this guest ignores the declaration*, about a guest
    /// that does not.
    #[test]
    fn r2135_a_record_of_asking_survives_the_placement_it_was_made_under() {
        let nav = HostChrome::NONE.with(Part::Navigation);
        forget_asked("moved_off");

        with_host_chrome_for("moved_off", nav, || {
            assert!(ask("moved_off", Part::Navigation));
        });
        forget_host_chrome("moved_off");

        assert_eq!(
            host_chrome_for("moved_off"),
            HostChrome::NONE,
            "the placement is gone, which is the fixture"
        );
        assert!(
            has_asked("moved_off", Part::Navigation),
            "★★★★★ the screen's decision went with the placement, so a census \
             run at another destination would report a guest that asks as one \
             that never did"
        );
    }

    /// Asking twice is asking once: the record is a set of pairs, so a screen
    /// whose paint, tree, keyboard and hit test all ask — which is the arrangement
    /// `draws_own_rail` exists to enforce — does not weigh five times.
    #[test]
    fn r2135_the_record_holds_one_entry_per_pair() {
        forget_asked("repeat");
        for _ in 0..5 {
            let _ = ask("repeat", Part::Navigation);
        }
        assert_eq!(
            asked()
                .iter()
                .filter(|(t, p)| t == "repeat" && *p == Part::Navigation)
                .count(),
            1
        );
    }

    /// The answer is the same as the spelling it replaces, for every arm and
    /// both ways round — so migrating a guest onto `ask` cannot change what it
    /// draws, only what is known about it.
    #[test]
    fn r2135_asking_answers_exactly_what_provides_does() {
        forget_asked("same");
        for declared in [
            HostChrome::NONE,
            HostChrome::NONE.with(Part::Navigation),
            HostChrome::NONE.with(Part::ApplicationBar),
            HostChrome::NONE
                .with(Part::Navigation)
                .with(Part::ApplicationBar),
        ] {
            with_host_chrome_for("same", declared, || {
                for part in Part::ALL {
                    assert_eq!(
                        ask("same", *part),
                        host_chrome_for("same").provides(*part),
                        "{part:?} under {declared:?}"
                    );
                }
            });
        }
    }
}
