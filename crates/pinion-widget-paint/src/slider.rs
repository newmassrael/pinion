//! R737 §5.16 §5.38 — Slider introspect reader + R739.1 keyboard scaffold.
//!
//! The backend-agnostic "read the slider's `(SliderState, f32)` from the
//! authoritative state scene" walk, lifted out of the per-binding
//! `read_state` helpers once a 4th consumer appeared (`hello-slider`,
//! `hello-slider-vertical`, `settings-panel`'s `read_font_slider`, and
//! `hello-slider-discrete`). It is the mechanical peer of
//! [`read_text_field_state`](crate::text_field::read_text_field_state):
//! a pure introspect walk with no style opinion, so the R727 / R732
//! 3rd-consumer mandate lifts it to the SSOT home immediately (mechanical
//! duplication, not opinionated paint composition).
//!
//! ## Why it returns `Option` (unlike `read_text_field_state`)
//!
//! The text-field reader bakes a fixed `(Idle, 0)` default for a missing
//! external because caret-0 is the universal empty default. The slider
//! readers, by contrast, each want a *different* fallback value when the
//! external is absent (`hello-slider` → `0.0`, `settings-panel` →
//! `DEFAULT_FONT_SCALE`, `hello-slider-discrete` → the boot tick). That
//! fallback is the part that genuinely diverges per consumer, so this
//! helper owns only the shared walk and returns `None` on a missing /
//! non-introspectable external — each caller applies its own default via
//! `.unwrap_or((SliderState::Idle, <default>))`. Abstracting the walk
//! while leaving the divergent default to the caller is the textbook
//! split (Sandi Metz "duplication is cheaper than the wrong abstraction"
//! applied at the field granularity).

use pinion_core::external::IntrospectValue;
use pinion_core::scene::Scene;
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::slider::SliderState;
use pinion_core::{Color, WidgetStateName};

/// R737 §5.38 — read a slider external's `(SliderState, f32)` from the
/// state scene by tag. Returns `None` when no introspectable external
/// carries `tag` (the caller supplies its own fallback value); on a hit
/// the `state` token routes through the R643
/// [`WidgetStateName::from_name_or_default`] SSOT (unknown / missing →
/// `Idle`) and the `value` reads through
/// [`IntrospectValue::as_f32`](pinion_core::external::IntrospectValue)
/// (f64 → f32 narrowing, `0.0` if the field is absent — unreachable for a
/// real slider but defensively defined).
#[must_use]
pub fn read_slider_state(scene: &Scene, tag: &str) -> Option<(SliderState, f32)> {
    let node = scene.find_external_with_tag(tag)?;
    let intro = node.handle.introspect()?;
    let state = match intro.query("state") {
        Some(IntrospectValue::Text(name)) => SliderState::from_name_or_default(&name),
        _ => SliderState::Idle,
    };
    let value = intro.query("value").and_then(|v| v.as_f32()).unwrap_or(0.0);
    Some((state, value))
}

/// R739.1 §5.38 — the mechanical scaffold every *single-value* slider's
/// `WidgetCore::apply_key` shares: focus-guard on `tag`, downcast the root
/// to its `SliderExternal`, refuse keys while `Disabled` (ARIA), read the
/// current normalised `"value"`, and route the binding's computed next
/// value back through the same `intervene("value", Float)` side door the
/// RPC `scene/intervene` path uses. Returns `true` iff a key was handled
/// and the write succeeded.
///
/// The *policy* — which keys move the thumb and by how much — stays in the
/// binding via `next_value`, a closure over the captured key that returns
/// `Some(new_value)` for a key it handles and `None` otherwise (so the
/// shell falls through to the next handler). This is the keyboard peer of
/// [`read_slider_state`]'s split: the SSOT introspect plumbing lives here,
/// the opinionated key map stays per-widget. Lifted once a 4th identical
/// copy appeared (`hello-slider`, `hello-slider-vertical`,
/// `hello-slider-discrete`, `hello-slider-labeled`) — mechanical wiring,
/// so the R727 / R732 Rule-of-Three applies immediately (it had silently
/// reached four; R739's entry self-grep checked `tick` / reader / labels
/// but missed this axis, the R734.1 "grep some axes, not all" trap).
///
/// The dual-thumb `RangeSliderExternal` deliberately does **not** use this:
/// it routes each arrow to a per-thumb `low` / `high` field selected by the
/// focused tag (and skips the disabled-guard), a genuinely different
/// scaffold that would only be forced to fit by a mode parameter — the
/// Sandi Metz "wrong abstraction" ([[abstraction-needs-second-consumer]]).
pub fn slider_apply_key(
    scene: &mut Scene,
    focused: Option<&str>,
    tag: &str,
    next_value: impl FnOnce(f32) -> Option<f32>,
) -> bool {
    if focused != Some(tag) {
        return false;
    }
    let Scene::External(node) = scene else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    // A disabled slider ignores keyboard input (ARIA).
    if let Some(IntrospectValue::Text(name)) = intro.query("state") {
        if matches!(
            SliderState::from_name_or_default(&name),
            SliderState::Disabled
        ) {
            return false;
        }
    }
    let Some(current) = intro.query("value").and_then(|v| v.as_f32()) else {
        return false;
    };
    let Some(next) = next_value(current) else {
        return false;
    };
    intro
        .intervene("value", IntrospectValue::Float(f64::from(next)))
        .is_ok()
}

// ─── R738 §5.38 §5.50 — slider M3 color contract (SSOT) ───────────────
//
// The three color ramps a slider's track + thumb paint reads. Lifted out
// of the per-binding view-fns once a 4th identical consumer appeared
// (`hello-slider`, `hello-slider-vertical`/`settings-panel`, and
// `hello-slider-discrete` already carried byte-identical copies; the
// R738 `hello-range-slider` is the trigger). These are *opinionated*
// paint (specific M3 state-layer lerp weights), so unlike the mechanical
// [`read_slider_state`] reader the R703/R727 rule defers the lift to the
// 3rd *identical* consumer rather than the 2nd — and the R737 carry that
// logged this as a "2-copy deferred" undercounted (it only looked at the
// two sliders R737 touched; `settings-panel` was the silent third). Per
// [[r735.1]] / [[verify-seed-claims-audit-first]], a deferred carry is
// re-audited every round, and the entry self-grep for R738 caught the
// real count. The thumb-fill peer [`checkbox::checkbox_accent_for`] uses
// the same `Accent` + state-layer anchor for its checked fill.

/// R738 §5.38 §5.50 — the *active* (filled) track color. Anchors on
/// [`ColorRole::Accent`] and layers the canonical M3 state-layer overlays
/// (hover 0.08 / dragging 0.12 toward `OnSurface`); `Disabled` fades 0.38
/// toward `Surface` for the washed look. The slider peer of
/// [`checkbox::checkbox_accent_for`](crate::checkbox::checkbox_accent_for).
#[must_use]
pub fn slider_accent_for(theme: &Theme, state: SliderState) -> Color {
    let base = theme.resolve(ColorRole::Accent);
    // Canonical common-case overlay (SliderState::Dragging is the pressed
    // posture; see the `InteractionState` impl) — the shared SSOT.
    crate::state_layer::state_layer(base, state, theme)
}

/// R738 §5.38 §5.50 — the *inactive* (unfilled) track color: M3
/// `surfaceContainerHighest` (the inactive-track tier), fading 0.38
/// toward `Surface` when `Disabled`.
#[must_use]
pub fn slider_track_inactive(theme: &Theme, state: SliderState) -> Color {
    let base = theme.resolve(ColorRole::SurfaceContainerHighest);
    // Divergent: the inactive track carries no hover/pressed overlay (only
    // the disabled fade), so it keeps its own arms but sources the token.
    match state {
        SliderState::Disabled => base.lerp(
            theme.resolve(ColorRole::Surface),
            crate::state_layer::DISABLED,
        ),
        _ => base,
    }
}

/// R738 §5.38 §5.50 — the thumb fill: M3 `OnAccent` (the paired-contrast
/// role for controls on accent fills). `Dragging` tints 0.2 toward
/// `Accent` so the moment of capture is visible; `Disabled` washes 0.38
/// toward `Surface`.
#[must_use]
pub fn slider_thumb_fill(theme: &Theme, state: SliderState) -> Color {
    let on_accent = theme.resolve(ColorRole::OnAccent);
    match state {
        SliderState::Idle | SliderState::Hover => on_accent,
        SliderState::Dragging => on_accent.lerp(theme.resolve(ColorRole::Accent), 0.2),
        SliderState::Disabled => on_accent.lerp(
            theme.resolve(ColorRole::Surface),
            crate::state_layer::DISABLED,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::{ContainerNode, ExternalNode};
    use pinion_core::widgets::slider::{SliderEvent, SliderExternal};

    fn slider_scene(tag: &'static str, value: f32) -> Scene {
        let mut ext = SliderExternal::new();
        ext.set_value(value);
        Scene::External(ExternalNode::new(Box::new(ext)).with_tag(tag))
    }

    #[test]
    fn reads_state_and_value_from_root_external() {
        let scene = slider_scene("vol", 0.6);
        let (state, value) = read_slider_state(&scene, "vol").expect("external present");
        assert_eq!(state, SliderState::Idle);
        assert!((value - 0.6).abs() < 1e-5);
    }

    #[test]
    fn finds_slider_among_multiple_externals() {
        let scene = Scene::Container(ContainerNode::new(vec![
            slider_scene("a", 0.2),
            slider_scene("b", 0.8),
        ]));
        let (_, value) = read_slider_state(&scene, "b").expect("tag b present");
        assert!(
            (value - 0.8).abs() < 1e-5,
            "reads the addressed external, not the first"
        );
    }

    #[test]
    fn returns_none_for_missing_external() {
        let scene = slider_scene("vol", 0.5);
        assert_eq!(
            read_slider_state(&scene, "nope"),
            None,
            "missing tag → None (caller defaults)"
        );
    }

    // ── R739.1 slider_apply_key scaffold (the SSOT shared by every
    //    single-value slider binding's apply_key) ──────────────────────

    fn value_of(scene: &Scene, tag: &str) -> f32 {
        read_slider_state(scene, tag).expect("external present").1
    }

    #[test]
    fn apply_key_ignores_unfocused_and_unhandled_keys() {
        let mut scene = slider_scene("vol", 0.5);
        // Wrong focus → the scaffold refuses before reading the value.
        let handled = slider_apply_key(&mut scene, Some("other"), "vol", |c| Some(c + 0.1));
        assert!(!handled, "key for a different tag is not handled");
        assert!(
            (value_of(&scene, "vol") - 0.5).abs() < 1e-5,
            "value unchanged"
        );
        // Focused but the policy closure declines (None) → not handled.
        let handled = slider_apply_key(&mut scene, Some("vol"), "vol", |_| None);
        assert!(!handled, "a key the policy declines falls through");
        assert!(
            (value_of(&scene, "vol") - 0.5).abs() < 1e-5,
            "value unchanged"
        );
    }

    #[test]
    fn apply_key_routes_policy_value_through_intervene() {
        let mut scene = slider_scene("vol", 0.5);
        let handled = slider_apply_key(&mut scene, Some("vol"), "vol", |c| {
            Some((c + 0.2).clamp(0.0, 1.0))
        });
        assert!(
            handled,
            "focused + policy value → written through intervene"
        );
        assert!(
            (value_of(&scene, "vol") - 0.7).abs() < 1e-5,
            "0.5 + 0.2 = 0.7"
        );
    }

    #[test]
    fn apply_key_refuses_while_disabled() {
        // A disabled slider ignores keyboard input (ARIA) — the scaffold's
        // disabled-guard short-circuits before the policy closure runs.
        let mut ext = SliderExternal::new();
        ext.set_value(0.5);
        ext.send(SliderEvent::Disable);
        let mut scene = Scene::External(ExternalNode::new(Box::new(ext)).with_tag("vol"));
        let mut ran = false;
        let handled = slider_apply_key(&mut scene, Some("vol"), "vol", |c| {
            ran = true;
            Some(c + 0.2)
        });
        assert!(!handled, "disabled slider refuses the key");
        assert!(!ran, "policy closure never runs while disabled");
        assert!(
            (value_of(&scene, "vol") - 0.5).abs() < 1e-5,
            "value unchanged while disabled"
        );
    }

    // ── R738 color contract (pre-lift parity with the inline bindings) ──

    #[test]
    fn slider_accent_idle_resolves_to_theme_accent() {
        let theme = Theme::light();
        assert_eq!(slider_accent_for(&theme, SliderState::Idle), theme.accent);
    }

    #[test]
    fn slider_accent_dragging_lerps_toward_on_surface() {
        let theme = Theme::light();
        let expected = theme
            .resolve(ColorRole::Accent)
            .lerp(theme.resolve(ColorRole::OnSurface), 0.12);
        assert_eq!(slider_accent_for(&theme, SliderState::Dragging), expected);
    }

    #[test]
    fn slider_track_inactive_idle_is_surface_container_highest() {
        let theme = Theme::light();
        assert_eq!(
            slider_track_inactive(&theme, SliderState::Idle),
            theme.resolve(ColorRole::SurfaceContainerHighest)
        );
        // Disabled fades toward Surface (observationally distinct).
        assert_ne!(
            slider_track_inactive(&theme, SliderState::Disabled),
            theme.resolve(ColorRole::SurfaceContainerHighest)
        );
    }

    #[test]
    fn slider_thumb_fill_idle_is_on_accent_dragging_tints() {
        let theme = Theme::light();
        assert_eq!(
            slider_thumb_fill(&theme, SliderState::Idle),
            theme.resolve(ColorRole::OnAccent)
        );
        let expected_drag = theme
            .resolve(ColorRole::OnAccent)
            .lerp(theme.resolve(ColorRole::Accent), 0.2);
        assert_eq!(
            slider_thumb_fill(&theme, SliderState::Dragging),
            expected_drag
        );
    }
}
