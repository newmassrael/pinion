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
//! `compute_layout` resolves the splitter geometry. So this seam reuses the
//! *shape* of
//! [`ScrollState::measured_viewport`](crate::widgets::scroll::ScrollState::measured_viewport)
//! (R774's `AutoSizer` leg): a post-layout measured-rect readback whose
//! first-paint chicken-and-egg (the reflow Effect must still run on the paint
//! the size is first measured) is resolved by the same scroll-dirty same-frame
//! re-pass — the publish returns a dirty bit the shell ORs into that re-pass.
//! The *mechanism* differs, and the difference is forced by **where the state
//! lives**. R774's harvest (`update_scroll_state_bounds`) runs as a post-layout
//! walk *inside* `compute_layout` because a `ScrollNode` carries its
//! `ScrollState` in the scene tree (`ScrollNode.state`), so the walk is pure
//! scene data. A pane's viewport `Signal` is **not** scene-carried — a
//! `Container` carries only a `tag`, and the tag → signal registry lives in the
//! owner-cache (panes are sparse + tag-addressed; a per-`Container` viewport
//! field would tax the commonest scene node for a rare feature). So the pane
//! harvest lives in the **shell** (`CoreShell::publish_pane_viewports`) — the one
//! layer holding both the laid-out scene *and* the owner — resolving each
//! registered tag's rect via the established scroll-aware
//! [`Scene::rect_for_tag_absolute`](crate::scene::Scene::rect_for_tag_absolute).
//! Folding the harvest into `compute_layout` would couple the layout pass to the
//! reactive owner (a coupling the scene-carried scroll path does not have), so
//! the shell-side harvest is the correct layering, not a deferred optimisation.
//!
//! R1012 is therefore a **sibling** of R1006, not a generalisation: different
//! timing (post-layout vs pre-view) and cardinality (N tag-keyed signals vs one
//! window signal); folding the window seam into the post-layout path would
//! regress its same-paint pre-reflow.
//!
//! ## Convergence invariant
//!
//! The single post-layout publish + one re-pass converges only when a pane's
//! rect is layout-derived and **independent of its own content** — the
//! dock/splitter model, where a pane's width is its splitter share, not its grid
//! glyphs' intrinsic size. A pane sized by its content would have a rect in the
//! re-pass layout differing from the one just published, so its viewport would
//! lag one frame. That is out of scope for this seam (the terminal/canvas panes
//! it serves fill their splitter share).
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
//!    live paint (`compute_paint_scene_internal`); the side-effect-free mirror
//!    (`compute_paint_scene_pure_internal`) and the RPC produce path never reach
//!    it. So an introspection paint cannot fire a pane reflow — there is no
//!    `set`, hence nothing to gate. (Under `dry_run` / `simulate` the inherited
//!    [`is_simulating`](super::is_simulating) gate in
//!    [`Effect`](super::effect::Effect) is the secondary defense, exactly as for
//!    the R1006 window seam.)
//!
//! # Per-window publish (R1021)
//!
//! Unlike the R1006 *window*-size seam — one global signal, published
//! `DEFAULT_WINDOW`-only so a secondary paint cannot clobber the primary — this
//! seam is published for **every painted window**. It can be, because the
//! registry is tag-keyed and window-agnostic: each window publishes the rects of
//! the tags *it* draws, and a tag absent from a window's scene resolves
//! [`Scene::rect_for_tag_absolute`](crate::scene::Scene::rect_for_tag_absolute)
//! `→ None` and is skipped (retains its last size — a foreign window's pane is
//! never clobbered). In the dock model a pane tag is drawn in exactly one window
//! at a time, so there is no ambiguity. This is what lets a torn-off (undock)
//! pane reflow to its secondary window's size — the window's content fills its
//! `(w, h)` via layout's root-fill, so the pane's measured rect is the window
//! rect, with no per-window `use_viewport_size`
//! ([`use_viewport_size`](super::viewport::use_viewport_size) stays
//! primary-gated). sprag R37 undock is the forcing consumer.
//!
//! # Re-entrancy
//!
//! The publish snapshots the `(tag, signal)` pairs via `PaneViewportRegistry::entries`
//! and drops the registry borrow *before* it `set`s any signal, so the
//! synchronous reflow Effect may re-enter [`use_pane_viewport_size`] (re-reading
//! its own pane) without a `RefCell` double-borrow.

use super::provider_slot::ProviderSlot;
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
///
/// **Insert-only.** A tag is registered on first read and never evicted, so the
/// registry is bounded by the number of *distinct pane tags ever seen*. That
/// fits the current consumers (a fixed dock layout's small stable tag set). A
/// per-pane eviction (`release_pane`) for a host that creates and destroys many
/// panes dynamically (dock tear-off at scale) is a deferred additive axis —
/// added when a real such consumer needs it, not speculatively.
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

/// R1366.6 §5.23 §5.22 — the pane-viewport-registry slot: its key, its default
/// and its inherit verdict as one expression.
///
/// **`Inherited`** by the mechanical predicate — the shell DRIVES this at the
/// root owner: `CoreShell::publish_pane_viewports` PUBLISHES every painted
/// window's pane rects into the ROOT's registry after layout. A child scope
/// resolves that one registry through
/// `Owner::cache_inherited` (crate-private since R1366.10); a per-scope
/// registry would take a torn-off (undock) pane's tags off into a secondary
/// window's own map that `publish_pane_viewports` never reads, so its PTY would
/// silently keep the wrong size — the R1021 / sprag-R37 forcing consumer. Like
/// [`LocalTaskPump`](super::resource::LocalTaskPump) it has no `provide` (the
/// registry is built from nothing, [`PaneViewportRegistry::new`]), so the shell
/// seeds it with [`seed_root`](ProviderSlot::seed_root) — via the pub
/// [`Owner::seed_pane_viewport_registry`](super::owner::Owner::seed_pane_viewport_registry)
/// wrapper, since this static is `pub(crate)`.
/// [`provider_slot_tests!`](crate::provider_slot_tests) EMITS the verdict.
///
/// `pub(crate)`, not `pub`: the registry HANDLE stays crate-private (the pub
/// surface is `use_pane_viewport_size` / `pane_viewport_entries` /
/// `seed_pane_viewport_registry`), so the slot cannot be a `pub static` without
/// leaking the handle. The `Rc<dyn Any>`-storage newtype R1012 wrapped it in is
/// gone: [`Owner::cache`](super::owner::Owner::cache) keys on `(TypeId, key)`, so
/// `PaneViewportRegistry` is already its own type.
pub(crate) static PANE_VIEWPORT_REGISTRY: ProviderSlot<PaneViewportRegistry> =
    ProviderSlot::inherited(
        "__pinion.reactive.pane_viewport_registry",
        PaneViewportRegistry::new,
    );

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
    let owner = super::owner::Owner::current()
        .expect("use_pane_viewport_size requires an active Owner scope");
    PANE_VIEWPORT_REGISTRY
        .resolve(&owner)
        .signal_for(tag.into())
        .get()
}

#[cfg(test)]
mod tests {
    use super::super::effect::Effect;
    use super::super::owner::Owner;
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    // The verdict, EMITTED from the declaration — a child scope resolves the
    // ROOT's registry. `ptr_eq` on the cache's `Rc<V>` (the shared identity),
    // which the behavioural test below could not use when the deleted accessor
    // returned the handle by value.
    crate::provider_slot_tests!(
        r1366_6_pane_viewport_registry_inherits,
        super::PANE_VIEWPORT_REGISTRY,
        PaneViewportRegistry::new
    );

    #[test]
    fn r1366_6_a_child_scope_reads_the_roots_registry_through_the_hook() {
        // R1365.1 §5.22 — the verdict through the BINDING's path (moved here from
        // owner.rs's mod when this slot migrated). Behavioural rather than ptr_eq:
        // what R1021 requires is that a publish into the ROOT's registry is what a
        // pane's child scope reads back through `use_pane_viewport_size`.
        // `Owner::new_child` is what R680 runs a secondary window's view in; a
        // child minting its own registry would read the (0, 0) unknown, so a
        // torn-off pane never reflows (forcing consumer: sprag's R37 undock).
        let root = Owner::new();
        root.seed_pane_viewport_registry();
        PANE_VIEWPORT_REGISTRY
            .resolve(&root)
            .signal_for(Cow::Borrowed("pane.left"))
            .set((640, 480));

        let window_scope = Owner::new_child(&root);
        let seen = window_scope.run(|| use_pane_viewport_size("pane.left"));

        assert_eq!(
            seen,
            (640, 480),
            "a child scope minted its own PaneViewportRegistry and read the (0, 0) \
             unknown — R1021 requires ONE root instance every window publishes",
        );
    }

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
        let reg = PANE_VIEWPORT_REGISTRY.resolve(&owner);
        reg.signal_for(Cow::Borrowed("pane.a")).set((320, 200));
        assert_eq!(owner.run(|| use_pane_viewport_size("pane.a")), (320, 200));
    }

    #[test]
    fn effect_refires_when_pane_signal_set_inside_owner_scope() {
        // R1006 blocker (B), per-pane: set() inside the owner scope lets the
        // synchronous reflow-Effect re-run resolve Owner::current() and observe
        // the new pane size.
        let owner = Owner::new();
        let sig = PANE_VIEWPORT_REGISTRY
            .resolve(&owner)
            .signal_for(Cow::Borrowed("pane.a"));
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
        let reg = PANE_VIEWPORT_REGISTRY.resolve(&owner);
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
    #[should_panic(expected = "use_pane_viewport_size requires an active Owner scope")]
    fn set_outside_owner_scope_panics_blocker_b() {
        // R1012.2 — blocker B negative path (mirror of viewport.rs): a publish
        // that set()s a pane signal with no active owner scope leaves the handle
        // stack empty, so the synchronous reflow re-run's use_pane_viewport_size()
        // -> Owner::current().expect() panics. This is why
        // CoreShell::publish_pane_viewports wraps the set in root_owner.run.
        let owner = Owner::new();
        let sig = PANE_VIEWPORT_REGISTRY
            .resolve(&owner)
            .signal_for(Cow::Borrowed("pane.a"));
        let _eff = owner.run(|| {
            Effect::new(&Owner::current().expect("inside run"), || {
                let _ = use_pane_viewport_size("pane.a");
            })
        });
        sig.set((120, 40)); // no owner.run wrap -> panics during the re-run
    }

    #[test]
    fn same_size_set_is_inert() {
        // Equality-skip: a same-size republish does not re-fire the reflow Effect
        // — the mechanism that floors a steady-state pane at zero extra re-passes.
        let owner = Owner::new();
        let sig = PANE_VIEWPORT_REGISTRY
            .resolve(&owner)
            .signal_for(Cow::Borrowed("pane.a"));
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
