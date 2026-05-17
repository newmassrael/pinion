//! R46.3.1 §5.16 `paint_adapter` — Scene → `vello::Scene` framework
//! primitive.
//!
//! Replaces the inline Scene-walker that lived in ai-introspect-demo
//! R46.3 (`build_vello_scene` / `fill_rect` / `stroke_rect` /
//! `pinion_to_peniko` / `root_background`). Promoted here so additional
//! consumers (hello-button R46.5+, future widget catalog) share one
//! Scene → Vello translation path instead of re-implementing it per
//! example — the same lesson R48 codified for input dispatch
//! (application-level workaround → framework primitive).
//!
//! Application-specific tag substitution is exposed as a closure hook
//! ([`to_vello`] generic over `Fn(&BoxNode) -> Option<Color>`) so the
//! framework module stays free of application tags (e.g. the demo's
//! `info_panel` palette indexing). Pass `&|_| None` when no override
//! is required.
//!
//! Border placement reproduces the legacy softbuffer "drawn inside the
//! rect bounds" behaviour by insetting Vello's centered stroke by
//! `width/2`. R46.3.2 carry — when [`pinion_core::style::Border`]
//! grows a `placement: BorderPlacement { Inside, Center, Outside }`
//! field the inset becomes a per-variant choice.
//!
//! Available only under the `vello` feature; non-GUI consumers
//! (headless / TUI / future paint backends) compile without wgpu
//! transitively.

use pinion_core::Scene;
use pinion_core::scene::{BoxNode, Rect};
use pinion_core::style::{Border, Color};
use vello::Scene as VelloScene;
use vello::kurbo::{Affine, Rect as KurboRect, Stroke};
use vello::peniko::{Color as PenikoColor, Fill};

/// Build a Vello scene from a pinion [`Scene`] tree. `fill_hook` is
/// consulted for each [`BoxNode`] visited; a `Some(color)` return
/// overrides the box's native `style.fill`, `None` keeps it. Pass
/// `&|_: &BoxNode| None` when no tag-based substitution is needed.
///
/// Walk semantics (matches the pre-R46.3.1 ai-introspect-demo `paint()`):
///
/// * [`Scene::Container`] — fill `rect` with `style.fill`, recurse
///   into `children`.
/// * [`Scene::Box`] — fill `rect` with `fill_hook(b)` or
///   `b.style.fill`; stroke `b.style.border` when present.
/// * [`Scene::External`] / [`Scene::Effect`] / [`Scene::Text`] /
///   [`Scene::Path`] / [`Scene::Image`] — no-op. Text / Path / Image
///   paint primitives attach in follow-up rounds (§5.X — cosmic-text
///   glyph cache, R31 caveat).
pub fn to_vello<F>(scene: &Scene, fill_hook: &F, out: &mut VelloScene)
where
    F: Fn(&BoxNode) -> Option<Color>,
{
    match scene {
        Scene::Container(c) => {
            fill_rect(out, c.rect, c.style.fill);
            for child in &c.children {
                to_vello(child, fill_hook, out);
            }
        }
        Scene::Box(b) => {
            let fill = fill_hook(b).unwrap_or(b.style.fill);
            fill_rect(out, b.rect, fill);
            if let Some(border) = b.style.border {
                stroke_rect(out, b.rect, border);
            }
        }
        // External / Effect / Text / Path / Image: no-op (matches
        // pre-R46.3.1 paint() behaviour). Text+Path+Image attach in
        // follow-up paint primitive rounds.
        _ => {}
    }
}

/// Background color used as `RenderParams.base_color` — the surface
/// clear that happens *before* any scene draw. Resolves to the root
/// [`Scene::Container`]'s fill so a window resized larger than the
/// canonical scene rect stays visually consistent inside-vs-outside.
/// Any other root variant falls back to black (no canonical "scene
/// background" without a Container).
#[must_use]
pub fn root_background(scene: &Scene) -> PenikoColor {
    match scene {
        Scene::Container(c) => to_peniko(c.style.fill),
        _ => PenikoColor::BLACK,
    }
}

/// Convert a pinion [`Color`] to a peniko `Color`, preserving every
/// channel including alpha. The §5.3 R20 `Color::rgba(r, g, b, a)`
/// shape is the source of truth; the legacy [`Color::from_argb`]
/// decoder reads the high `0xAA__` byte verbatim too, so callers that
/// want explicit opacity must construct via [`Color::rgb`] /
/// [`Color::rgba`] rather than the softbuffer-style `0x00RRGGBB`
/// literal (which decodes to alpha = 0 = fully transparent on Vello).
#[must_use]
pub fn to_peniko(c: Color) -> PenikoColor {
    PenikoColor::from_rgba8(c.r, c.g, c.b, c.a)
}

/// Emit one Vello filled-rectangle path for a pinion (`Rect`, `Color`)
/// pair. Transparent fills are skipped (matches the pre-R46.3.1
/// `paint_filled_rect` early-exit).
fn fill_rect(out: &mut VelloScene, r: Rect, fill: Color) {
    if fill == Color::TRANSPARENT {
        return;
    }
    let rect = KurboRect::new(
        f64::from(r.x),
        f64::from(r.y),
        f64::from(r.x.saturating_add(r.w)),
        f64::from(r.y.saturating_add(r.h)),
    );
    out.fill(Fill::NonZero, Affine::IDENTITY, to_peniko(fill), None, &rect);
}

/// Emit one Vello stroke for a pinion [`Border`]. Vello strokes are
/// path-centered; the `width/2` inset reproduces the prior "drawn
/// inside the rect bounds" softbuffer convention.
fn stroke_rect(out: &mut VelloScene, r: Rect, border: Border) {
    if border.width == 0 {
        return;
    }
    let w = f64::from(border.width);
    let inset = w / 2.0;
    let rect = KurboRect::new(
        f64::from(r.x) + inset,
        f64::from(r.y) + inset,
        f64::from(r.x.saturating_add(r.w)) - inset,
        f64::from(r.y.saturating_add(r.h)) - inset,
    );
    out.stroke(
        &Stroke::new(w),
        Affine::IDENTITY,
        to_peniko(border.color),
        None,
        &rect,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::{BoxNode, ContainerNode, Rect};
    use pinion_core::style::{BoxStyle, Color};
    use std::cell::Cell;

    #[test]
    fn to_peniko_preserves_all_channels_including_alpha() {
        // R46.3.1 invariant: the conversion is loss-less across all
        // four channels. R46.3 inline had alpha hardcoded to 255; the
        // framework primitive fixes that — pinion::Color::rgba(_,_,_,a)
        // round-trips to peniko via from_rgba8 verbatim.
        let pinion = Color::rgba(0x12, 0x34, 0x56, 0x78);
        let peniko = to_peniko(pinion);
        assert_eq!(peniko, PenikoColor::from_rgba8(0x12, 0x34, 0x56, 0x78));
    }

    #[test]
    fn to_peniko_alpha_zero_is_transparent() {
        // The legacy softbuffer `0x00RRGGBB` literal decodes through
        // Color::from_argb to alpha = 0 = transparent. Callers that
        // expected opacity from `0x00FF_3366` must migrate to
        // Color::rgb(0xFF, 0x33, 0x66); the framework no longer masks
        // the bug by hardcoding 255.
        let from_argb = Color::from_argb(0x00ff_3366);
        let peniko = to_peniko(from_argb);
        assert_eq!(peniko, PenikoColor::from_rgba8(0xff, 0x33, 0x66, 0x00));
    }

    #[test]
    fn root_background_extracts_root_container_fill() {
        let scene = Scene::Container(
            ContainerNode::new(vec![]).with_style(BoxStyle::filled(Color::rgb(0xff, 0, 0))),
        );
        let bg = root_background(&scene);
        assert_eq!(bg, PenikoColor::from_rgba8(0xff, 0, 0, 0xff));
    }

    #[test]
    fn root_background_falls_back_to_black_for_non_container() {
        // Any non-Container root (Box, External, ...) returns BLACK —
        // there's no canonical "scene background" without a Container.
        let scene = Scene::Box(BoxNode::filled(Rect::new(0, 0, 10, 10), Color::rgb(0xff, 0, 0)));
        let bg = root_background(&scene);
        assert_eq!(bg, PenikoColor::BLACK);
    }

    #[test]
    fn to_vello_walks_container_and_box_children() {
        // The walker reaches every BoxNode under a Container. Verify
        // by Cell-counting hook hits (Fn bound; interior mutability
        // for test-side state).
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Box(
                BoxNode::filled(Rect::new(0, 0, 10, 10), Color::rgb(0xff, 0, 0))
                    .with_tag("a"),
            ),
            Scene::Box(
                BoxNode::filled(Rect::new(20, 0, 10, 10), Color::rgb(0, 0xff, 0))
                    .with_tag("b"),
            ),
        ]));
        let mut vello = VelloScene::new();
        let hits = Cell::new(0_u32);
        to_vello(
            &scene,
            &|_b: &BoxNode| {
                hits.set(hits.get() + 1);
                None
            },
            &mut vello,
        );
        assert_eq!(hits.get(), 2, "hook called once per BoxNode");
    }

    #[test]
    fn to_vello_hook_some_overrides_box_native_fill() {
        // When the hook returns Some, that color replaces the box's
        // `style.fill`. We can't read back the emitted Vello commands
        // from outside the crate, but we can verify the hook was
        // consulted with the correct BoxNode (tag-driven dispatch
        // matches ai-introspect-demo's info_panel pattern).
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Box(
                BoxNode::filled(Rect::new(0, 0, 10, 10), Color::rgb(0xff, 0, 0))
                    .with_tag("info_panel"),
            ),
            Scene::Box(
                BoxNode::filled(Rect::new(20, 0, 10, 10), Color::rgb(0, 0xff, 0))
                    .with_tag("save_btn"),
            ),
        ]));
        let mut vello = VelloScene::new();
        let overrides = Cell::new(0_u32);
        let passthroughs = Cell::new(0_u32);
        to_vello(
            &scene,
            &|b: &BoxNode| {
                if b.tag.as_deref() == Some("info_panel") {
                    overrides.set(overrides.get() + 1);
                    Some(Color::rgb(0, 0, 0xff))
                } else {
                    passthroughs.set(passthroughs.get() + 1);
                    None
                }
            },
            &mut vello,
        );
        assert_eq!(overrides.get(), 1);
        assert_eq!(passthroughs.get(), 1);
    }

    #[test]
    fn to_vello_nested_container_recurses() {
        // Two-level Container nesting: outer + inner + leaf box. The
        // walker must visit the leaf box's hook.
        let inner = ContainerNode::new(vec![
            Scene::Box(
                BoxNode::filled(Rect::new(10, 10, 10, 10), Color::rgb(0, 0xff, 0))
                    .with_tag("leaf"),
            ),
        ]);
        let outer = ContainerNode::new(vec![Scene::Container(inner)]);
        let scene = Scene::Container(outer);
        let mut vello = VelloScene::new();
        let saw_leaf = Cell::new(false);
        to_vello(
            &scene,
            &|b: &BoxNode| {
                if b.tag.as_deref() == Some("leaf") {
                    saw_leaf.set(true);
                }
                None
            },
            &mut vello,
        );
        assert!(saw_leaf.get(), "nested leaf BoxNode must be visited");
    }
}
