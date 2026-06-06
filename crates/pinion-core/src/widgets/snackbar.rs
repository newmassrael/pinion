//! R725 §5.28 §5.38 — Snackbar / Toast timed-auto-dismiss driver.
//!
//! A snackbar is a transient, bottom-anchored message that appears in
//! response to an action, optionally carries a single action label
//! (Material 3 `inversePrimary`), and **auto-dismisses after a
//! duration** unless the user dismisses it sooner. This primitive owns
//! only the *visibility + countdown* — the message text, the action
//! button, and the inverse-surface chrome are the binding's
//! composition (mirroring how [`CaretBlink`](super::caret_blink) owns
//! only the blink phase, not the caret geometry).
//!
//! ## Timed dismissal rides the §5.28 animation driver
//!
//! [`SnackbarTimer`] is a [`Tickable`]: each
//! [`Owner::tick_animations`](crate::reactive::Owner::tick_animations)
//! step advances an `elapsed` accumulator, and once it reaches
//! `duration` the `visible` [`Signal`] flips to `false`. That makes the
//! dismissal **deterministically drivable** by the R724 `scene/tick`
//! RPC — an AI client shows the snackbar, ticks past the duration, and
//! observes the auto-dismiss without waiting on wall-clock time. It is
//! the [`CaretBlink`](super::caret_blink) precedent applied to a
//! *one-shot* countdown rather than a repeating phase.
//!
//! ## Material 3 duration
//!
//! [`SnackbarTimer::DEFAULT_DURATION_SECS`] = 4 s — the Material 3
//! "short" snackbar duration (M3 allows up to 10 s for snackbars with
//! an action; the binding passes its own value to [`Self::show`]).

use std::cell::Cell;
use std::rc::Rc;

use crate::animation::Tickable;
use crate::event::Event;
use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, RepaintOwner, ThreadOwnership,
};
use crate::reactive::{Owner, Signal};
use crate::widget_core::ExtraExternal;

/// R725 §5.28 §5.38 — visibility + auto-dismiss countdown for a single
/// snackbar. One instance per snackbar, reached through
/// [`use_snackbar_timer`]. The binding reads [`Self::visible`] in the
/// view-fn to decide whether to paint the overlay; the value
/// auto-subscribes so the paint pass re-runs the instant the timer
/// dismisses.
///
/// Thread-safety: not `Send` / `Sync` (`Rc` + `Cell`), matching every
/// other `pinion-core` reactive primitive. UI thread only.
#[derive(Debug)]
pub struct SnackbarTimer {
    /// `true` = snackbar shown (paint the overlay), `false` = hidden.
    /// Auto-subscribes when read inside a view-fn.
    visible: Signal<bool>,
    /// Seconds elapsed since the current [`Self::show`]. Advanced by
    /// [`Tickable::tick`]; reset to `0.0` on each `show`.
    elapsed: Cell<f32>,
    /// Auto-dismiss horizon in seconds — the snackbar hides once
    /// `elapsed >= duration`. Set per-`show` so different messages can
    /// carry different durations.
    duration: Cell<f32>,
}

impl SnackbarTimer {
    /// Material 3 "short" snackbar duration. M3 permits up to 10 s for
    /// snackbars carrying an action; callers pass their own value to
    /// [`Self::show`].
    pub const DEFAULT_DURATION_SECS: f32 = 4.0;

    /// Construct a hidden timer (no countdown running).
    #[must_use]
    pub fn new() -> Self {
        Self {
            visible: Signal::new(false),
            elapsed: Cell::new(0.0),
            duration: Cell::new(Self::DEFAULT_DURATION_SECS),
        }
    }

    /// Whether the snackbar should paint this frame. Triggers a
    /// [`Signal`] subscription inside a view-fn — the view re-runs when
    /// the timer shows or auto-dismisses.
    #[must_use]
    pub fn visible(&self) -> bool {
        self.visible.get()
    }

    /// Show the snackbar and (re)start the auto-dismiss countdown for
    /// `duration` seconds. Calling `show` while already visible
    /// restarts the countdown (the canonical "new message replaces the
    /// current one" behaviour).
    pub fn show(&self, duration: f32) {
        self.duration.set(duration);
        self.elapsed.set(0.0);
        self.visible.set(true);
    }

    /// Dismiss immediately (user tapped the action or the close
    /// affordance). No-op if already hidden (Signal equality-skip).
    pub fn dismiss(&self) {
        self.visible.set(false);
    }

    /// Seconds remaining before auto-dismiss, clamped to `>= 0`; `0.0`
    /// while hidden. Exposed so an AI client / introspection surface
    /// can read the countdown as data (§2 #7).
    #[must_use]
    pub fn remaining(&self) -> f32 {
        if self.visible.get() {
            (self.duration.get() - self.elapsed.get()).max(0.0)
        } else {
            0.0
        }
    }

    /// The current auto-dismiss horizon in seconds (the last
    /// [`Self::show`] duration, or [`Self::DEFAULT_DURATION_SECS`]).
    #[must_use]
    pub fn duration(&self) -> f32 {
        self.duration.get()
    }
}

impl Default for SnackbarTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl Tickable for SnackbarTimer {
    fn tick(&self, dt: f32) {
        if !self.visible.get() {
            return;
        }
        let elapsed = self.elapsed.get() + dt;
        self.elapsed.set(elapsed);
        if elapsed >= self.duration.get() {
            self.visible.set(false);
        }
    }

    fn is_at_rest(&self, _epsilon: f32) -> bool {
        // A hidden snackbar has no further state evolution until the
        // next `show`; a visible one is counting down, so the backend
        // must keep requesting frames until it auto-dismisses.
        !self.visible.get()
    }
}

/// R725 §5.28 §5.38 — resolve (or lazily initialize) a
/// [`SnackbarTimer`] for the current view scope and register it with
/// the owner's animation driver. Mirrors
/// [`use_caret_blink`](super::caret_blink::use_caret_blink): delegates
/// to [`Owner::cache`](crate::reactive::Owner::cache) so the same
/// `Rc<SnackbarTimer>` resolves across view re-runs, and registers the
/// `Tickable` exactly once (gated by
/// [`Owner::cache_contains`](crate::reactive::Owner::cache_contains))
/// so the `tick_animations` walk does not advance it twice per frame.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set (invoked outside a
/// `root_owner.run(...)` wrap) — framework dispatch sites supply the
/// wrap per the callback-root-owner-wrap discipline. Panics if the
/// cache key was previously bound to a different concrete type within
/// the same owner (see [`Owner::cache`]).
#[must_use]
pub fn use_snackbar_timer(key: &'static str) -> Rc<SnackbarTimer> {
    // R727 §5.28 — delegates to the `Owner::register_animation_once` SSOT.
    Owner::current()
        .expect("use_snackbar_timer requires an active Owner scope")
        .register_animation_once(key, SnackbarTimer::new)
}

/// R810 §5.38 §5.12 — the **query-only** RPC introspection face of a
/// [`SnackbarTimer`]. Registered as an extra-external (always present in
/// the scene, shown *or* hidden), it surfaces the live countdown at the
/// introspect paths `visible` / `remaining` / `duration`, so an AI agent
/// driving over the §5.12 RPC plane can ask "is the snackbar up, and how
/// long until it auto-dismisses?" directly — the same first-class query
/// [`ModalIntrospect`](super::modal::ModalIntrospect) gives a modal.
///
/// Before R810 the snackbar's visibility lived only in the
/// [`SnackbarTimer::visible`] [`Signal`] that the view-fn reads to paint
/// the overlay; an AI client had to infer "is it shown?" from
/// scene-tree presence (the exact asymmetry R795 closed for modals).
/// This node closes it without a second source of truth — the flag,
/// countdown, and horizon still live once in [`SnackbarTimer`], and this
/// node merely reads them. Pairs naturally with the R724 `scene/tick`
/// RPC: an agent shows the snackbar, queries `remaining`, ticks past the
/// duration, and observes `visible` flip to `false` — all as data, no
/// wall-clock wait and no pixels (§2 #7).
///
/// ## Query-only by construction
///
/// `visible` / `remaining` / `duration` are all timer-derived — the
/// countdown is advanced by [`Tickable::tick`] and (re)started by
/// [`SnackbarTimer::show`]. A raw `intervene` write would desync the
/// reported state from the running countdown (e.g. forcing `visible`
/// true without arming `elapsed`/`duration`), so every slot is
/// read-only: writes are refused with [`InterveneError::ReadOnly`].
/// Showing and dismissing go through [`SnackbarTimer::show`] /
/// [`SnackbarTimer::dismiss`] (a reducer / action handler), never by
/// rewinding this node.
#[derive(Debug)]
pub struct SnackbarIntrospect {
    timer: Rc<SnackbarTimer>,
}

impl SnackbarIntrospect {
    /// Wrap a shared [`SnackbarTimer`] as its query-only introspection
    /// node. The `Rc` is cloned, so the view-fn, the animation driver,
    /// and this node all report the same live countdown.
    #[must_use]
    pub fn new(timer: Rc<SnackbarTimer>) -> Self {
        Self { timer }
    }
}

impl External for SnackbarIntrospect {
    /// RPC-only: the node carries no pixels (the binding paints the
    /// snackbar overlay itself), so the visual backends skip it while
    /// §5.12 `query` still routes through it.
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn handles_event(&self, _event: &Event) -> bool {
        false
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for SnackbarIntrospect {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("visible", "bool"),
            ("remaining", "float"),
            ("duration", "float"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "visible" => Some(IntrospectValue::Bool(self.timer.visible())),
            "remaining" => Some(IntrospectValue::Float(f64::from(self.timer.remaining()))),
            "duration" => Some(IntrospectValue::Float(f64::from(self.timer.duration()))),
            _ => None,
        }
    }

    /// Every slot is read-only — see the type docs. The countdown is
    /// timer-driven, so showing / dismissing route through
    /// [`SnackbarTimer::show`] / [`SnackbarTimer::dismiss`], not a
    /// rewind here.
    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "visible" | "remaining" | "duration" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }
}

/// R810 §5.38 §5.12 — register a [`SnackbarTimer`]'s query-only
/// introspection face as an [`ExtraExternal`] under `tag`. Mirrors
/// [`modal_introspection_extra`](super::modal::modal_introspection_extra):
/// a snackbar binding wires
/// `snackbar_introspection_extra(tag, use_snackbar_timer(key))` in its
/// [`create_extra_externals`](crate::WidgetCore::create_extra_externals),
/// resolving the same `Rc<SnackbarTimer>` its view-fn reads, so the node
/// reports the live countdown.
#[must_use]
pub fn snackbar_introspection_extra(tag: &'static str, timer: Rc<SnackbarTimer>) -> ExtraExternal {
    ExtraExternal::new(tag, Box::new(SnackbarIntrospect::new(timer)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::Owner;

    #[test]
    fn hidden_by_default_and_at_rest() {
        let t = SnackbarTimer::new();
        assert!(!t.visible());
        assert!(t.is_at_rest(0.0));
        assert!(t.remaining().abs() < f32::EPSILON);
    }

    #[test]
    fn show_makes_visible_and_starts_countdown() {
        let t = SnackbarTimer::new();
        t.show(4.0);
        assert!(t.visible());
        assert!(!t.is_at_rest(0.0), "a visible timer is counting down");
        assert!((t.remaining() - 4.0).abs() < f32::EPSILON);
    }

    #[test]
    fn tick_auto_dismisses_after_duration() {
        let t = SnackbarTimer::new();
        t.show(4.0);
        t.tick(1.0);
        assert!(t.visible(), "still visible mid-countdown");
        assert!((t.remaining() - 3.0).abs() < 1e-6);
        t.tick(3.0); // total 4.0 == duration
        assert!(!t.visible(), "auto-dismissed at duration");
        assert!(t.remaining().abs() < f32::EPSILON);
    }

    #[test]
    fn tick_while_hidden_is_noop() {
        let t = SnackbarTimer::new();
        t.tick(10.0);
        assert!(!t.visible());
    }

    #[test]
    fn dismiss_hides_immediately() {
        let t = SnackbarTimer::new();
        t.show(4.0);
        t.dismiss();
        assert!(!t.visible());
        assert!(t.is_at_rest(0.0));
    }

    #[test]
    fn show_restarts_countdown() {
        let t = SnackbarTimer::new();
        t.show(4.0);
        t.tick(3.5);
        t.show(4.0); // restart
        assert!(t.visible());
        assert!((t.remaining() - 4.0).abs() < f32::EPSILON);
    }

    #[test]
    fn use_hook_registers_animation_once_per_key() {
        let owner = Owner::new();
        owner.run(|| {
            let _a = use_snackbar_timer("snack");
            let _b = use_snackbar_timer("snack");
            let _c = use_snackbar_timer("snack");
        });
        assert_eq!(owner.registered_animation_count(), 1);
    }

    #[test]
    fn r810_introspect_reports_hidden_then_visible_countdown() {
        let timer = Rc::new(SnackbarTimer::new());
        let node = SnackbarIntrospect::new(timer.clone());
        // Hidden: visible=false, remaining=0, duration still the default.
        assert_eq!(node.query("visible"), Some(IntrospectValue::Bool(false)));
        assert_eq!(node.query("remaining"), Some(IntrospectValue::Float(0.0)));
        assert_eq!(
            node.query("duration"),
            Some(IntrospectValue::Float(f64::from(SnackbarTimer::DEFAULT_DURATION_SECS))),
        );
        // Shown: visible flips and the countdown starts at the full duration.
        timer.show(4.0);
        assert_eq!(node.query("visible"), Some(IntrospectValue::Bool(true)));
        assert_eq!(node.query("remaining"), Some(IntrospectValue::Float(4.0)));
        assert_eq!(node.query("duration"), Some(IntrospectValue::Float(4.0)));
    }

    #[test]
    fn r810_introspect_remaining_tracks_ticks_and_auto_dismiss() {
        let timer = Rc::new(SnackbarTimer::new());
        let node = SnackbarIntrospect::new(timer.clone());
        timer.show(4.0);
        timer.tick(1.0);
        assert_eq!(
            node.query("remaining"),
            Some(IntrospectValue::Float(3.0)),
            "remaining counts down with the shared timer (no second source)",
        );
        assert_eq!(node.query("visible"), Some(IntrospectValue::Bool(true)));
        timer.tick(3.0); // total 4.0 == duration → auto-dismiss
        assert_eq!(node.query("visible"), Some(IntrospectValue::Bool(false)));
        assert_eq!(node.query("remaining"), Some(IntrospectValue::Float(0.0)));
    }

    #[test]
    fn r810_introspect_all_slots_are_read_only() {
        let mut node = SnackbarIntrospect::new(Rc::new(SnackbarTimer::new()));
        for slot in ["visible", "remaining", "duration"] {
            assert_eq!(
                node.intervene(slot, IntrospectValue::Bool(true)),
                Err(InterveneError::ReadOnly),
                "{slot} is timer-driven: a raw write would desync the countdown",
            );
        }
        assert_eq!(
            node.intervene("nope", IntrospectValue::Null),
            Err(InterveneError::UnknownPath),
            "an unknown slot is UnknownPath, distinct from the read-only slots",
        );
    }

    #[test]
    fn r810_introspect_unknown_query_path_is_none() {
        let node = SnackbarIntrospect::new(Rc::new(SnackbarTimer::new()));
        assert_eq!(node.query("elapsed"), None, "only visible/remaining/duration are query slots");
    }

    #[test]
    fn r810_introspect_schema_declares_the_three_slots() {
        let node = SnackbarIntrospect::new(Rc::new(SnackbarTimer::new()));
        assert_eq!(
            node.schema().fields,
            &[("visible", "bool"), ("remaining", "float"), ("duration", "float")],
        );
    }

    #[test]
    fn r810_introspection_extra_carries_tag_and_shared_timer() {
        let timer = Rc::new(SnackbarTimer::new());
        let extra = snackbar_introspection_extra("snackbar_state", timer.clone());
        assert_eq!(extra.tag, "snackbar_state", "the helper registers under the given tag");
        let introspect = extra
            .handle
            .introspect()
            .expect("the snackbar node exposes a query-only introspection face");
        assert_eq!(introspect.query("visible"), Some(IntrospectValue::Bool(false)));
        timer.show(4.0);
        assert_eq!(
            introspect.query("visible"),
            Some(IntrospectValue::Bool(true)),
            "showing the shared timer flips the registered node (one SSOT)",
        );
    }
}
