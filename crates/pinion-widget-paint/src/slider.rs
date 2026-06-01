//! R737 §5.16 §5.38 — Slider introspect reader.
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
use pinion_core::widgets::slider::SliderState;
use pinion_core::WidgetStateName;

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

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::{ContainerNode, ExternalNode};
    use pinion_core::widgets::slider::SliderExternal;

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
        assert!((value - 0.8).abs() < 1e-5, "reads the addressed external, not the first");
    }

    #[test]
    fn returns_none_for_missing_external() {
        let scene = slider_scene("vol", 0.5);
        assert_eq!(read_slider_state(&scene, "nope"), None, "missing tag → None (caller defaults)");
    }
}
