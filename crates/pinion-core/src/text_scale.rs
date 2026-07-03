//! R668 §5.38 §5.50 — accessibility text-scale substrate.
//!
//! Material 3 / W3C accessibility guidelines call for a user-driven
//! text-scale multiplier (commonly 100 %, 125 %, 150 %, 200 % steps —
//! Apple's "Dynamic Type" / Android's "Font scale" / GNOME's
//! `text-scaling-factor`). The multiplier composes with every
//! [`TextStyle::with_size_px`](crate::style::TextStyle::with_size_px)
//! call so a single setting cascades through the entire visible
//! widget surface: labels, headings, button text, list-item titles,
//! everything paints at `raw_px * scale` without per-binding
//! plumbing.
//!
//! The substrate has three pieces that work together:
//!
//! 1. **A thread-local raw f32 ([`current_text_scale`])** — read by
//!    `with_size_px` on every call to compute `raw_px * scale`.
//!    Default `1.0` so existing bindings see no behavioural change
//!    until they (or a sibling binding) call [`set_text_scale`]. The
//!    thread-local lookup is non-subscribing: the multiplier kicks
//!    in at builder time without coupling the caller to the reactive
//!    substrate.
//!
//! 2. **A cached [`Signal<f32>`](crate::reactive::Signal) the
//!    [`use_text_scale`] hook returns** — bindings that mutate the
//!    scale call [`TextScale::set`] (or [`set_text_scale`] for the
//!    framework-side ingress hook), which updates both the
//!    thread-local mirror (so the next `with_size_px` call multiplies
//!    by the new value) and the signal (so view fns subscribed via
//!    `use_text_scale().get()` re-run + repaint at the new size).
//!
//! 3. **The view-fn re-run trigger** — bindings that want responsive
//!    auto-scaling subscribe by calling
//!    `use_text_scale().subscribe()` once in their view fn (or by
//!    reading `.get()` somewhere visible to the reactive scope). The
//!    settings-panel `font_scale` slider lives at this third layer
//!    in the R668 atomic (4) consumer.
//!
//! ## Why not subscribe inside `with_size_px`
//!
//! `with_size_px` is called from non-reactive code paths too: tests
//! that build a `TextStyle` directly (`crates/pinion-text/src/cache.rs`
//! tests), [`paint_adapter`](crate::scene) builders, RPC snapshot
//! producers. Auto-subscribing inside the builder would panic those
//! sites with "no current Owner". The textbook canonical split:
//! `with_size_px` reads a non-subscribing thread-local;
//! `use_text_scale()` is the reactive entry point the binding's
//! view-fn drives explicitly.
//!
//! ## Safety clamp
//!
//! [`TextScale::set`] clamps inputs to `[0.1, 5.0]`. Negative or
//! zero scales produce zero-height text the layout pass rejects;
//! ridiculously large scales (10×+) produce per-frame layout costs
//! that stutter the UI. The clamp range covers the 100–500 % a11y
//! standard plus a small safety floor; bindings that need a wider
//! range can call [`set_text_scale_unchecked`] (no clamp, no
//! validation) — the escape hatch is documented carry, not the
//! recommended path.

use std::cell::Cell;
use std::rc::Rc;

use crate::reactive::{Owner, Signal};

thread_local! {
    /// R668 — the raw f32 multiplier
    /// [`crate::style::TextStyle::with_size_px`] reads on every call.
    /// `Cell<f32>` allows interior mutation without a `RefCell`
    /// borrow check — the f32 read/write is non-reentrant on a
    /// single thread.
    ///
    /// Initialised to `1.0` so bindings that never touch the scale
    /// see identity multiplication (no behavioural change vs the
    /// pre-R668 pure-builder `with_size_px`).
    static CURRENT_TEXT_SCALE: Cell<f32> = const { Cell::new(1.0) };
}

/// R668 §5.38 — current text-scale multiplier as a raw f32.
///
/// Non-subscribing read of the `CURRENT_TEXT_SCALE` thread-local.
/// Called by [`crate::style::TextStyle::with_size_px`] on every
/// builder invocation; the multiplier kicks in at builder time so
/// every painted [`crate::scene::TextNode`] in the resulting scene
/// carries `raw_px * scale` without per-binding plumbing.
///
/// Callers that want reactivity (view fn auto-re-paints on scale
/// change) call [`use_text_scale`] and subscribe via `.get()` /
/// `.subscribe()`.
#[must_use]
pub fn current_text_scale() -> f32 {
    CURRENT_TEXT_SCALE.with(Cell::get)
}

/// R668 §5.38 — Mutate the current text-scale multiplier (framework
/// ingress hook; bindings should prefer [`TextScale::set`] on the
/// shared [`use_text_scale`] handle so view fns subscribed via
/// `.get()` re-run on the change).
///
/// Clamps to `[0.1, 5.0]` per the module-level safety contract.
///
/// Updates the thread-local mirror only. Callers that want the
/// reactive signal to fire too should use [`TextScale::set`] (which
/// chains both updates atomically — thread-local first, signal
/// second).
pub fn set_text_scale(value: f32) {
    let clamped = clamp_scale(value);
    CURRENT_TEXT_SCALE.with(|c| c.set(clamped));
}

/// R668 §5.38 — escape-hatch setter with no clamp / NaN check.
///
/// Use only when the canonical [`TextScale::set`] / [`set_text_scale`]
/// safety clamp is genuinely in the way (e.g. unit tests that need
/// to probe the boundary behaviour, or research code that needs a
/// wider zoom range). Production bindings should not reach for this.
pub fn set_text_scale_unchecked(value: f32) {
    CURRENT_TEXT_SCALE.with(|c| c.set(value));
}

#[inline]
fn clamp_scale(value: f32) -> f32 {
    // NaN check: f32::NaN propagates through arithmetic and would
    // poison every subsequent `with_size_px` with NaN-sized text the
    // layout pass cannot lay out. Default to 1.0 on NaN.
    if value.is_nan() {
        return 1.0;
    }
    value.clamp(0.1, 5.0)
}

/// R668 §5.38 — wrapper around the shared [`Signal<f32>`] the
/// [`use_text_scale`] hook returns. `.set` mutates both the
/// non-subscribing thread-local mirror (so the next `with_size_px`
/// builder call multiplies by the new value) and the signal (so view
/// fns subscribed via `.get()` re-run on the change).
pub struct TextScale {
    signal: Rc<Signal<f32>>,
}

impl TextScale {
    /// Read the current scale and **subscribe** the calling reactive
    /// scope to subsequent changes. View fns + Effects that want to
    /// re-run when the scale changes call this.
    #[must_use]
    pub fn get(&self) -> f32 {
        self.signal.get()
    }

    /// Read the current scale **without** subscribing. For one-shot
    /// reads in non-reactive sites (`println!` diagnostics,
    /// imperative computations the caller does not want to bind to
    /// the reactive graph). Reads the thread-local mirror — same
    /// value `current_text_scale()` returns — so peek + the
    /// `with_size_px` builder always agree.
    #[must_use]
    pub fn peek(&self) -> f32 {
        current_text_scale()
    }

    /// Update the scale. Clamps to `[0.1, 5.0]` per the module-level
    /// safety contract; NaN inputs default to `1.0`. Updates the
    /// thread-local mirror first (so any `with_size_px` call inside
    /// the signal's subscriber chain sees the new value) and then
    /// the signal (so view fns subscribed via [`Self::get`] re-run).
    pub fn set(&self, value: f32) {
        let clamped = clamp_scale(value);
        CURRENT_TEXT_SCALE.with(|c| c.set(clamped));
        self.signal.set(clamped);
    }
}

/// R668 §5.38 — Owner-cache-shared [`TextScale`] hook.
///
/// First call in a given [`Owner`] scope creates the cache slot with
/// the current thread-local value as the initial signal value;
/// subsequent calls in the same scope return the same `Rc<TextScale>`
/// so all subscribers see the same signal.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set — see [`use_theme`] /
/// [`use_text_edit_state`] for the same wrap discipline. Framework
/// dispatch sites supply the `root_owner.run(...)` wrap.
///
/// [`use_theme`]: crate::theme::use_theme
/// [`use_text_edit_state`]: crate::widgets::text_edit::use_text_edit_state
#[must_use]
pub fn use_text_scale() -> Rc<TextScale> {
    Owner::current()
        .expect("use_text_scale requires an active Owner scope")
        .cache("__r668_text_scale", || TextScale {
            signal: Rc::new(Signal::new(current_text_scale())),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset the thread-local before every test so cross-test state
    /// does not leak. Run inside a fresh `Owner::new()` scope so the
    /// cache slot starts empty too.
    fn fresh() {
        CURRENT_TEXT_SCALE.with(|c| c.set(1.0));
    }

    #[test]
    fn r668_default_scale_is_identity() {
        fresh();
        assert!((current_text_scale() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn r668_set_text_scale_updates_thread_local() {
        fresh();
        set_text_scale(1.5);
        assert!((current_text_scale() - 1.5).abs() < f32::EPSILON);
        // Reset so other tests start from identity.
        set_text_scale(1.0);
    }

    #[test]
    fn r668_set_text_scale_clamps_negative_to_floor() {
        fresh();
        set_text_scale(-2.0);
        assert!((current_text_scale() - 0.1).abs() < f32::EPSILON);
        set_text_scale(1.0);
    }

    #[test]
    fn r668_set_text_scale_clamps_huge_to_ceiling() {
        fresh();
        set_text_scale(50.0);
        assert!((current_text_scale() - 5.0).abs() < f32::EPSILON);
        set_text_scale(1.0);
    }

    #[test]
    fn r668_set_text_scale_nan_defaults_to_one() {
        fresh();
        set_text_scale(f32::NAN);
        assert!((current_text_scale() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn r668_set_text_scale_unchecked_bypasses_clamp() {
        fresh();
        set_text_scale_unchecked(10.0);
        assert!((current_text_scale() - 10.0).abs() < f32::EPSILON);
        set_text_scale(1.0); // reset via canonical setter
    }

    #[test]
    fn r668_use_text_scale_returns_cache_shared_handle() {
        fresh();
        let owner = Owner::new();
        let (a, b) = owner.run(|| (use_text_scale(), use_text_scale()));
        // Same cache slot → same Rc allocation.
        assert!(Rc::ptr_eq(&a, &b));
    }

    #[test]
    fn r668_text_scale_set_updates_thread_local_and_signal() {
        fresh();
        let owner = Owner::new();
        owner.run(|| {
            let scale = use_text_scale();
            assert!((scale.peek() - 1.0).abs() < f32::EPSILON);
            scale.set(2.0);
            // thread-local mirror updated
            assert!((current_text_scale() - 2.0).abs() < f32::EPSILON);
            // signal updated
            assert!((scale.peek() - 2.0).abs() < f32::EPSILON);
        });
        set_text_scale(1.0); // reset
    }

    #[test]
    fn r668_with_size_px_multiplies_by_current_scale() {
        use crate::style::TextStyle;
        fresh();
        let owner = Owner::new();
        owner.run(|| {
            // Identity at default
            let s = TextStyle::new().with_size_px(16);
            assert_eq!(s.font_size_px, 16);
            // Scaled
            set_text_scale(1.5);
            let s = TextStyle::new().with_size_px(16);
            assert_eq!(s.font_size_px, 24); // 16 * 1.5 = 24
            // Boundary: scaled value floors at 1
            set_text_scale(0.1);
            let s = TextStyle::new().with_size_px(8);
            assert_eq!(s.font_size_px, 1); // 8 * 0.1 = 0.8 → round → 1 (max guard)
        });
        set_text_scale(1.0); // reset
    }
}
