//! R1012 §5.23 §5.22 — `use_pane_viewport_size`: the per-pane measured
//! pixel-rect reactive read seam.
//!
//! # Why this exists
//!
//! [`use_viewport_size`](super::viewport::use_viewport_size) (R1006) hands a
//! binding the *window* layout viewport `(w, h)`. A multi-pane shell — a dock
//! layout whose split panes each host their own grid-filling producer (the
//! sprag multi-pane terminal: each pane a PTY whose `(cols, rows) = pane_rect /
//! cell`) — needs the same value *per pane*, because a splitter drag resizes
//! the panes independently while the window size is unchanged.
//!
//! # The post-layout asymmetry (vs R1006)
//!
//! The window seam publishes its value **before** `view` runs: the OS hands the
//! shell the window `inner_size` up front, so a reflow
//! [`Effect`](super::effect::Effect) reading
//! [`use_viewport_size`](super::viewport::use_viewport_size) reacts and the same
//! paint's `view` reads the post-reflow producer state.
//!
//! A *pane* rect has no such pre-view source: it is only known **after**
//! `compute_layout` resolves the splitter geometry. So this seam is the
//! [`ScrollState::measured_viewport`](crate::widgets::scroll::ScrollState::measured_viewport)
//! `AutoSizer` leg (R774): the shell publishes each pane's measured rect *after*
//! layout, and the first-paint chicken-and-egg (the reflow Effect must still run
//! on the paint the size is first measured) is resolved by the same scroll-dirty
//! same-frame re-pass — the publish returns a dirty bit the shell ORs into that
//! re-pass. R1012 is therefore a **sibling** of R1006, not a generalisation:
//! different timing (post-layout vs pre-view) and cardinality (N tag-keyed
//! signals vs one window signal); folding the window seam into the post-layout
//! path would regress its same-paint pre-reflow.
//!
//! # Contract (inherited from R1006 — load-bearing)
//!
//! 1. **The shell MUST `set` each pane signal inside its `root_owner` scope**
//!    (R1006 blocker B). A [`Signal::set`](super::signal::Signal::set)
//!    synchronously re-runs the subscribed reflow Effect, whose body resolves
//!    [`Owner::current`](super::owner::Owner::current) (the owner-handle stack,
//!    NOT the subscriber stack the re-run pushes). Setting outside the scope
//!    leaves that stack empty and the first publish panics in
//!    [`use_pane_viewport_size`]'s `expect`.
//! 2. **`(0, 0)` means "pane unmeasured"** — the lazy default before the pane is
//!    first laid out. A reflow Effect runs eagerly once at registration with
//!    this value, so it MUST skip on `(0, 0)` to avoid a spurious `1 x 1` reflow.
//! 3. **Introspection paint does not publish.** The publish runs only in the
//!    live, primary-window paint (`compute_paint_scene_internal`); the
//!    side-effect-free mirror (`compute_paint_scene_pure_internal`) and the RPC
//!    produce path never reach it. So an introspection paint cannot fire a pane
//!    reflow — there is no `set`, hence nothing to gate.
//!
//! # Re-entrancy
//!
//! The publish snapshots the `(tag, signal)` pairs via `PaneViewportRegistry::entries`
//! and drops the registry borrow *before* it `set`s any signal, so the
//! synchronous reflow Effect may re-enter [`use_pane_viewport_size`] (re-reading
//! its own pane) without a `RefCell` double-borrow.

use super::signal::Signal;
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

/// A registered pane's `(tag, measured-size signal)` pair — the unit
/// `PaneViewportRegistry::entries` /
/// [`Owner::pane_viewport_entries`](super::owner::Owner::pane_viewport_entries)
/// yield for the shell's post-layout publish.
pub type PaneViewportEntry = (Cow<'static, str>, Signal<(u32, u32)>);

/// The pane registry's tag → measured-size map.
type PaneViewportMap = BTreeMap<Cow<'static, str>, Signal<(u32, u32)>>;

/// R1012 §5.23 §5.22 — the per-owner registry of pane-tag → measured-size
/// [`Signal`]. A cheaply-clonable `Rc` handle: the consumer's
/// [`use_pane_viewport_size`] read and the shell's publish both resolve the same
/// root-owner registry, so they share one signal per pane tag.
#[derive(Clone)]
pub(crate) struct PaneViewportRegistry {
    inner: Rc<RefCell<PaneViewportMap>>,
}

impl PaneViewportRegistry {
    pub(crate) fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(BTreeMap::new())),
        }
    }

    /// Get-or-insert the pane `tag`'s [`Signal`], lazily defaulting to
    /// `(0, 0)` ("pane unmeasured"). Idempotent: a later read of the same tag
    /// returns the same underlying cell, so the shell's publish is observed.
    pub(crate) fn signal_for(&self, tag: Cow<'static, str>) -> Signal<(u32, u32)> {
        self.inner
            .borrow_mut()
            .entry(tag)
            .or_insert_with(|| Signal::new((0_u32, 0_u32)))
            .clone()
    }

    /// Snapshot every registered `(tag, signal)` pair into an owned `Vec`,
    /// dropping the registry borrow. The shell's publish iterates this snapshot
    /// so a synchronous reflow Effect (fired by the publish's `set`) can
    /// re-enter [`Self::signal_for`] via [`use_pane_viewport_size`] without a
    /// `RefCell` double-borrow.
    pub(crate) fn entries(&self) -> Vec<PaneViewportEntry> {
        self.inner
            .borrow()
            .iter()
            .map(|(tag, sig)| (tag.clone(), sig.clone()))
            .collect()
    }
}

/// Owner-cache newtype: [`Owner::cache`](super::owner::Owner::cache) stores
/// `Rc<dyn Any>`, so the registry handle rides inside this holder (mirrors
/// [`ViewportSizeHolder`](super::viewport::ViewportSizeHolder)).
pub(crate) struct PaneViewportRegistryHolder(pub(crate) PaneViewportRegistry);

/// Private owner-cache key for the single per-owner pane-viewport registry slot
/// (mirrors the [`VIEWPORT_SIZE_KEY`](super::viewport::VIEWPORT_SIZE_KEY)
/// private-key convention).
pub(crate) const PANE_VIEWPORT_REGISTRY_KEY: &str = "__pinion.reactive.pane_viewport_registry";

/// R1012 §5.23 §5.22 — read the measured pixel size `(width, height)` of the
/// pane tagged `tag` via the active owner scope's registry, subscribing the
/// caller. Registers the tag on first read (lazy `(0, 0)` until the shell's
/// post-layout publish writes the laid-out rect).
///
/// Returns `(0, 0)` ("pane unmeasured") before the first layout pass measures
/// the pane (or in headless / RPC / unit tests where no shell publishes).
/// Called inside a reflow [`Effect`](super::effect::Effect); the tracked `get`
/// re-fires that Effect when the pane is resized (e.g. a splitter drag).
///
/// # Panics
///
/// Panics when called outside an [`Owner::run`](super::owner::Owner::run) scope
/// — the same strict shape as
/// [`use_viewport_size`](super::viewport::use_viewport_size), for the same
/// reason: it is read in a reflow Effect that always runs in a scope, and strict
/// makes contract (1) above (the shell must `set` inside `root_owner`) fail loud
/// rather than silently reflow to `1 x 1`.
#[must_use]
pub fn use_pane_viewport_size(tag: impl Into<Cow<'static, str>>) -> (u32, u32) {
    super::owner::Owner::current()
        .expect("use_pane_viewport_size requires an active Owner scope")
        .pane_viewport_signal(tag.into())
        .get()
}

#[cfg(test)]
mod tests {
    use super::super::effect::Effect;
    use super::super::owner::Owner;
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn unregistered_pane_is_zero_unknown() {
        let owner = Owner::new();
        // First read of a tag lazily registers it at the (0, 0) sentinel.
        assert_eq!(owner.run(|| use_pane_viewport_size("pane.a")), (0, 0));
    }

    #[test]
    fn signal_for_same_tag_is_the_shared_handle() {
        let reg = PaneViewportRegistry::new();
        let a = reg.signal_for(Cow::Borrowed("pane.a"));
        let a2 = reg.signal_for(Cow::Borrowed("pane.a"));
        // Same underlying cell: a write through one is observed through the other.
        a.set((640, 480));
        assert_eq!(a2.get(), (640, 480));
    }

    #[test]
    fn distinct_tags_are_independent_signals() {
        let reg = PaneViewportRegistry::new();
        let a = reg.signal_for(Cow::Borrowed("pane.a"));
        let b = reg.signal_for(Cow::Borrowed("pane.b"));
        a.set((100, 40));
        b.set((80, 24));
        assert_eq!(a.get(), (100, 40));
        assert_eq!(b.get(), (80, 24));
        // Two tags registered, snapshot returns both (BTreeMap → tag order).
        let tags: Vec<_> = reg.entries().into_iter().map(|(t, _)| t).collect();
        assert_eq!(tags, vec![Cow::Borrowed("pane.a"), Cow::Borrowed("pane.b")]);
    }

    #[test]
    fn use_pane_viewport_size_resolves_inside_owner_run() {
        let owner = Owner::new();
        // Seed via the registry directly (the shell's publish does this through
        // the owner-cache registry), then read through the hook.
        let reg = owner.pane_viewport_registry();
        reg.signal_for(Cow::Borrowed("pane.a")).set((320, 200));
        assert_eq!(owner.run(|| use_pane_viewport_size("pane.a")), (320, 200));
    }

    #[test]
    fn effect_refires_when_pane_signal_set_inside_owner_scope() {
        // R1006 blocker (B), per-pane: set() inside the owner scope lets the
        // synchronous reflow-Effect re-run resolve Owner::current() and observe
        // the new pane size.
        let owner = Owner::new();
        let sig = owner.pane_viewport_registry().signal_for(Cow::Borrowed("pane.a"));
        let seen = Rc::new(RefCell::new(Vec::new()));
        let seen_c = Rc::clone(&seen);
        let _eff = owner.run(|| {
            Effect::new(&Owner::current().expect("inside run"), move || {
                seen_c.borrow_mut().push(use_pane_viewport_size("pane.a"));
            })
        });
        assert_eq!(seen.borrow().as_slice(), &[(0, 0)]); // eager run, unmeasured
        owner.run(|| sig.set((120, 40)));
        assert_eq!(seen.borrow().as_slice(), &[(0, 0), (120, 40)]);
    }

    #[test]
    fn reflow_effect_may_reenter_use_hook_without_double_borrow() {
        // The publish snapshots entries() before set()ing, so a reflow Effect
        // that re-reads its own pane during the synchronous re-run does not
        // double-borrow the registry RefCell. Mirror that ordering here.
        let owner = Owner::new();
        let reg = owner.pane_viewport_registry();
        let last = Rc::new(RefCell::new((0_u32, 0_u32)));
        let last_c = Rc::clone(&last);
        let _eff = owner.run(|| {
            Effect::new(&Owner::current().expect("inside run"), move || {
                // Re-entrant read of the same pane through the hook (borrows the
                // registry) while a set() is mid-flight below.
                *last_c.borrow_mut() = use_pane_viewport_size("pane.a");
            })
        });
        let panes = reg.entries(); // snapshot, borrow dropped
        owner.run(|| {
            for (_, sig) in &panes {
                sig.set((200, 60)); // re-fires the Effect, which re-borrows reg
            }
        });
        assert_eq!(*last.borrow(), (200, 60));
    }

    #[test]
    fn same_size_set_is_inert() {
        // Equality-skip: a same-size republish does not re-fire the reflow Effect
        // — the mechanism that floors a steady-state pane at zero extra re-passes.
        let owner = Owner::new();
        let sig = owner.pane_viewport_registry().signal_for(Cow::Borrowed("pane.a"));
        let count = Rc::new(std::cell::Cell::new(0_u32));
        let count_c = Rc::clone(&count);
        let _eff = owner.run(|| {
            Effect::new(&Owner::current().expect("inside run"), move || {
                let _ = use_pane_viewport_size("pane.a");
                count_c.set(count_c.get() + 1);
            })
        });
        assert_eq!(count.get(), 1); // eager
        owner.run(|| sig.set((80, 24)));
        assert_eq!(count.get(), 2);
        owner.run(|| sig.set((80, 24))); // same -> no notify
        assert_eq!(count.get(), 2, "equality-skip: no re-fire");
    }
}
