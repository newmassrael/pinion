//! R759 §5.38 — `Badge` widget: a descriptive (non-interactive) count /
//! dot status overlay anchored to another element (an icon, an avatar, a
//! tab).
//!
//! Like [`ProgressBarExternal`](super::progress_bar) and
//! [`TooltipExternal`](super::tooltip), a badge owns **no interaction
//! statechart**: it has no pointer states, no keyboard model, and emits
//! no §5.20 intents. It is a plain value holder ([R718] established the
//! descriptive-widget pattern: `operable` and `interactive-state` are
//! orthogonal axes — a badge is neither, so a hand-written [`External`]
//! without an SCXML machine is the textbook form, not a statechart).
//!
//! The observable axes are:
//!
//! * [`count`](Self::count) — the raw number the badge reports (e.g. the
//!   unread-message count). Always the *uncapped* value, so an AI client
//!   reads the true magnitude even when the visible label is capped.
//! * [`max`](Self::max) — the overflow threshold. When `count > max` the
//!   visible [`label`](Self::label) reads `"{max}+"` (the Material 3
//!   large-badge overflow form), while `count` keeps the real number.
//! * [`dot`](Self::dot) — the *small badge* variant: a bare dot with no
//!   number (M3's "there is something new" affordance). When set, the
//!   label is empty regardless of `count`.
//!
//! Two derived axes are exposed read-only so the paint binding, the a11y
//! binding, and an AI client all read **one** capped string / one
//! visibility verdict (no drift between what is painted and what is
//! announced):
//!
//! * [`label`](Self::label) — the visible string (`""` for a dot, the
//!   number, or `"{max}+"` on overflow);
//! * [`visible`](Self::visible) — whether the badge renders at all (a
//!   count badge with `count == 0` is hidden, matching M3 / the web
//!   platform; a dot badge is shown whenever set).
//!
//! Every mutable axis is **writable** through the §5.15 introspect
//! channel (`intervene`), the same side door the RPC `scene/intervene`
//! route and a host application's notification updater both use — so the
//! AI client and the host converge on one observable state (the
//! [`ProgressBarExternal`] / [`SliderExternal`](super::slider) contract).
//!
//! a11y is left to the binding (no a11y state lives on the holder): the
//! count is announced by augmenting the anchor's accessible description
//! — a [`AriaRole::Status`](pinion_a11y::AriaRole::Status) live region
//! the anchor points at via `aria-describedby`. That reuses existing a11y
//! fields (no new primitive), the WAI-ARIA-canonical way to expose a
//! badge to AT (a visually-hidden, polite live region rather than a bare
//! decorative number).

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, RepaintOwner, ThreadOwnership,
};

/// R759 §5.38 — count / dot status-overlay value holder.
///
/// A plain `Copy` value (no interaction state to carry — a badge is not
/// operable), distinct from the SCXML-backed operable widgets and mirror
/// of [`ProgressBarExternal`](super::progress_bar::ProgressBarExternal).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadgeExternal {
    /// The raw, uncapped count. The visible label caps this at
    /// [`Self::max`]; `count` itself is never capped, so AT / an AI
    /// client read the true magnitude.
    count: u32,
    /// Overflow threshold. `count > max` ⇒ the label reads `"{max}+"`.
    /// Clamped to at least `1` by [`Self::set_max`] (a `0` threshold
    /// would render the degenerate `"0+"`).
    max: u32,
    /// Small-badge variant: a bare dot with no number. Orthogonal to
    /// `count` — when set, the label is empty whatever `count` holds.
    dot: bool,
}

impl BadgeExternal {
    /// Material 3 large-badge default overflow cap. Counts above this
    /// render as `"99+"` — the common notification-badge ceiling.
    pub const DEFAULT_MAX: u32 = 99;

    /// Construct an empty count badge (`count = 0`, hidden until a count
    /// arrives, default [`Self::DEFAULT_MAX`] overflow cap).
    #[must_use]
    pub fn new() -> Self {
        Self {
            count: 0,
            max: Self::DEFAULT_MAX,
            dot: false,
        }
    }

    /// Construct a count badge showing `count` (overflow cap left at
    /// [`Self::DEFAULT_MAX`]).
    #[must_use]
    pub fn with_count(count: u32) -> Self {
        let mut b = Self::new();
        b.set_count(count);
        b
    }

    /// Construct a *dot* badge (the small "something new" variant — no
    /// number).
    #[must_use]
    pub fn dot_badge() -> Self {
        let mut b = Self::new();
        b.set_dot(true);
        b
    }

    /// The raw, uncapped count.
    #[must_use]
    pub fn count(&self) -> u32 {
        self.count
    }

    /// The overflow threshold (always `>= 1`).
    #[must_use]
    pub fn max(&self) -> u32 {
        self.max
    }

    /// Whether this is the dot (small) variant.
    #[must_use]
    pub fn dot(&self) -> bool {
        self.dot
    }

    /// Set the raw count.
    pub fn set_count(&mut self, count: u32) {
        self.count = count;
    }

    /// Set the overflow threshold, clamped to at least `1` (a `0`
    /// threshold renders the degenerate `"0+"`).
    pub fn set_max(&mut self, max: u32) {
        self.max = max.max(1);
    }

    /// Set the dot variant.
    pub fn set_dot(&mut self, dot: bool) {
        self.dot = dot;
    }

    /// The visible label string: `""` for a dot badge, the number, or
    /// `"{max}+"` when `count` overflows the cap. Single source of truth
    /// for both the painted text and any textual a11y echo.
    #[must_use]
    pub fn label(&self) -> String {
        if self.dot {
            String::new()
        } else if self.count > self.max {
            format!("{}+", self.max)
        } else {
            self.count.to_string()
        }
    }

    /// Whether the badge renders at all. A count badge with `count == 0`
    /// is hidden (M3 / web-platform behaviour); a dot badge is shown
    /// whenever set.
    #[must_use]
    pub fn visible(&self) -> bool {
        self.dot || self.count > 0
    }
}

impl Default for BadgeExternal {
    fn default() -> Self {
        Self::new()
    }
}

impl External for BadgeExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    /// A badge emits no §5.20 intents — its count is observed and driven
    /// through the introspect channel, never broadcast as a command /
    /// selection / value intent.
    fn drain_intents(&mut self, _sink: &mut dyn FnMut(crate::intent::Intent)) {}

    /// The count never changes on its own (no internal clock); every
    /// mutation arrives through `intervene`, which the framework already
    /// follows with a repaint. So the badge is never self-dirty.
    fn is_dirty(&self) -> bool {
        false
    }
}

impl ExternalIntrospect for BadgeExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("count", "int"),
            ("max", "int"),
            ("dot", "bool"),
            // Derived, read-only — the capped display string + the
            // visibility verdict, exposed so an AI client reads exactly
            // what the binding paints / announces.
            ("label", "string"),
            ("visible", "bool"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "count" => Some(IntrospectValue::Int(i64::from(self.count))),
            "max" => Some(IntrospectValue::Int(i64::from(self.max))),
            "dot" => Some(IntrospectValue::Bool(self.dot)),
            "label" => Some(IntrospectValue::Text(self.label())),
            "visible" => Some(IntrospectValue::Bool(self.visible())),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // The raw count. A negative wire value clamps to `0`, an
            // over-`u32` value saturates — a malformed payload can never
            // poison the count.
            "count" => match value {
                IntrospectValue::Int(n) => {
                    self.set_count(clamp_to_u32(n));
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // The overflow threshold (clamped to `>= 1` inside set_max).
            "max" => match value {
                IntrospectValue::Int(n) => {
                    self.set_max(clamp_to_u32(n));
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // Toggle the dot variant.
            "dot" => match value {
                IntrospectValue::Bool(b) => {
                    self.set_dot(b);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // `label` / `visible` are derived from `count` / `max` /
            // `dot`; they reject writes the same way a progress bar's
            // fixed `min` / `max` bounds do.
            "label" | "visible" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }
}

/// Clamp a signed wire integer into `0..=u32::MAX`. Negatives become `0`,
/// over-`u32` values saturate — the `clamp` keeps the result inside the
/// `u32` range so the `try_from` is infallible.
fn clamp_to_u32(n: i64) -> u32 {
    u32::try_from(n.clamp(0, i64::from(u32::MAX))).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_empty_and_hidden() {
        let b = BadgeExternal::new();
        assert_eq!(b.count(), 0);
        assert_eq!(b.max(), BadgeExternal::DEFAULT_MAX);
        assert!(!b.dot());
        assert!(!b.visible(), "an empty count badge is hidden");
        assert_eq!(b.label(), "0");
    }

    #[test]
    fn with_count_shows_the_number() {
        let b = BadgeExternal::with_count(7);
        assert_eq!(b.count(), 7);
        assert_eq!(b.label(), "7");
        assert!(b.visible());
    }

    #[test]
    fn count_overflows_to_max_plus() {
        let mut b = BadgeExternal::with_count(150);
        // Default cap is 99 -> "99+".
        assert_eq!(b.label(), "99+");
        // Exactly at the cap shows the number (no "+").
        b.set_count(99);
        assert_eq!(b.label(), "99");
        // One past the cap overflows.
        b.set_count(100);
        assert_eq!(b.label(), "99+");
    }

    #[test]
    fn custom_max_changes_overflow_point() {
        let mut b = BadgeExternal::with_count(12);
        b.set_max(9);
        assert_eq!(b.label(), "9+");
        assert_eq!(b.count(), 12, "the raw count is never capped");
    }

    #[test]
    fn max_clamps_to_at_least_one() {
        let mut b = BadgeExternal::with_count(5);
        b.set_max(0);
        assert_eq!(b.max(), 1, "a zero threshold clamps to 1");
        assert_eq!(b.label(), "1+");
    }

    #[test]
    fn dot_variant_has_empty_label_and_is_visible() {
        let b = BadgeExternal::dot_badge();
        assert!(b.dot());
        assert_eq!(b.label(), "", "a dot badge shows no number");
        assert!(b.visible(), "a dot badge is shown whenever set");
    }

    #[test]
    fn dot_suppresses_the_number_even_with_a_count() {
        let mut b = BadgeExternal::with_count(5);
        b.set_dot(true);
        assert_eq!(b.label(), "", "dot wins over the count for the label");
        assert!(b.visible());
    }

    #[test]
    fn query_reports_every_axis() {
        let b = BadgeExternal::with_count(150);
        assert_eq!(b.query("count"), Some(IntrospectValue::Int(150)));
        assert_eq!(b.query("max"), Some(IntrospectValue::Int(99)));
        assert_eq!(b.query("dot"), Some(IntrospectValue::Bool(false)));
        assert_eq!(b.query("label"), Some(IntrospectValue::Text("99+".to_string())));
        assert_eq!(b.query("visible"), Some(IntrospectValue::Bool(true)));
        assert_eq!(b.query("nope"), None);
    }

    #[test]
    fn intervene_count_clamps_negative_to_zero() {
        let mut b = BadgeExternal::with_count(5);
        b.intervene("count", IntrospectValue::Int(-3)).expect("int accepted");
        assert_eq!(b.count(), 0);
        assert!(!b.visible(), "count 0 hides a count badge");
    }

    #[test]
    fn intervene_count_saturates_over_u32() {
        let mut b = BadgeExternal::new();
        b.intervene("count", IntrospectValue::Int(i64::from(u32::MAX) + 10))
            .expect("int accepted");
        assert_eq!(b.count(), u32::MAX);
    }

    #[test]
    fn intervene_dot_toggles() {
        let mut b = BadgeExternal::with_count(3);
        b.intervene("dot", IntrospectValue::Bool(true)).expect("bool accepted");
        assert!(b.dot());
        assert_eq!(b.query("label"), Some(IntrospectValue::Text(String::new())));
    }

    #[test]
    fn intervene_max_clamps_and_drives_overflow() {
        let mut b = BadgeExternal::with_count(50);
        b.intervene("max", IntrospectValue::Int(9)).expect("int accepted");
        assert_eq!(b.query("label"), Some(IntrospectValue::Text("9+".to_string())));
    }

    #[test]
    fn intervene_wrong_type_is_type_mismatch() {
        let mut b = BadgeExternal::new();
        assert_eq!(
            b.intervene("count", IntrospectValue::Bool(true)),
            Err(InterveneError::TypeMismatch),
        );
        assert_eq!(
            b.intervene("dot", IntrospectValue::Int(1)),
            Err(InterveneError::TypeMismatch),
        );
    }

    #[test]
    fn intervene_derived_axes_are_read_only() {
        let mut b = BadgeExternal::new();
        assert_eq!(
            b.intervene("label", IntrospectValue::Text("x".to_string())),
            Err(InterveneError::ReadOnly),
        );
        assert_eq!(
            b.intervene("visible", IntrospectValue::Bool(false)),
            Err(InterveneError::ReadOnly),
        );
    }

    #[test]
    fn intervene_unknown_path_rejected() {
        let mut b = BadgeExternal::new();
        assert_eq!(
            b.intervene("color", IntrospectValue::Text("red".to_string())),
            Err(InterveneError::UnknownPath),
        );
    }
}
