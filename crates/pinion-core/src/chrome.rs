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

#[cfg(test)]
mod tests {
    use super::{
        HostChrome, Part, forget_host_chrome, host_chrome, host_chrome_for, with_host_chrome,
        with_host_chrome_for,
    };

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
}
