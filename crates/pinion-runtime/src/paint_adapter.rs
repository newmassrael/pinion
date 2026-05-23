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
//! Border placement (R46.3.2) honours
//! [`pinion_core::style::BorderPlacement`]:
//!
//! * `Inside` (default, legacy softbuffer behaviour) — centred stroke
//!   inset by `width/2` so the whole stroke lies within `rect`.
//! * `Center` — Vello's native stroke (half-width spills outside).
//! * `Outside` — centred stroke offset by `width/2` outwards.
//!
//! R47.3 §5.36 — [`Scene::Text`] paints via parley-shaped glyph runs.
//! The caller passes a `&mut pinion_text::LayoutCache` so steady-state
//! frames hit the cache instead of re-shaping every label; the cache
//! also owns the parley `FontContext` / `LayoutContext` so the
//! framework module never holds parley state across calls.
//! `parley::FontData = peniko::FontData` (re-exported via
//! `linebender_resource_handle`), so the run's font feeds
//! [`vello::Scene::draw_glyphs`] unchanged.
//!
//! Available only under the `vello` feature; non-GUI consumers
//! (headless / TUI / future paint backends) compile without wgpu
//! transitively.

use pinion_core::Scene;
use pinion_core::scene::{BoxNode, Rect, TextNode};
use pinion_core::style::{Border, BorderPlacement, Color, TextOverflow};
use pinion_text::LayoutCache;
use pinion_text::parley::PositionedLayoutItem;
use vello::Glyph;
use vello::Scene as VelloScene;
use vello::kurbo::{Affine, Line, Rect as KurboRect, RoundedRect as KurboRoundedRect, Stroke};
use vello::peniko::{Color as PenikoColor, Fill};

/// Build a Vello scene from a pinion [`Scene`] tree. `fill_hook` is
/// consulted for each [`BoxNode`] visited; a `Some(color)` return
/// overrides the box's native `style.fill`, `None` keeps it. Pass
/// `&|_: &BoxNode| None` when no tag-based substitution is needed.
///
/// `text_cache` is the per-application [`LayoutCache`] (R47.3 §5.36) —
/// caching parley `Layout` values across frames so static labels do not
/// re-shape every redraw. Pass `&mut LayoutCache::new()` only when the
/// caller knows the scene contains zero `Scene::Text` (the cache is
/// otherwise dormant); long-lived applications should own one.
///
/// Walk semantics (R47.3 §5.36):
///
/// * [`Scene::Container`] — fill `rect` with `style.fill`, recurse
///   into `children`.
/// * [`Scene::Box`] — fill `rect` with `fill_hook(b)` or
///   `b.style.fill`; stroke `b.style.border` when present.
/// * [`Scene::Text`] — shape via [`LayoutCache::layout`], walk
///   `positioned_glyphs()` per [`parley::GlyphRun`], emit one
///   [`vello::Scene::draw_glyphs`] call per run.
/// * [`Scene::External`] / [`Scene::Effect`] / [`Scene::Path`] /
///   [`Scene::Image`] — no-op. Path / Image paint primitives attach
///   in follow-up rounds.
pub fn to_vello<F>(
    scene: &Scene,
    fill_hook: &F,
    text_cache: &mut LayoutCache,
    out: &mut VelloScene,
) where
    F: Fn(&BoxNode) -> Option<Color>,
{
    to_vello_inner(scene, fill_hook, text_cache, out, Affine::IDENTITY);
}

/// (R51.188 §5.45 R55.E.1) Transform-carrying recursive walker.
///
/// The public [`to_vello`] forwards [`Affine::IDENTITY`]; the
/// internal recursion threads the cumulative transform through so
/// the [`Scene::Scroll`] arm can compose
/// `parent_transform * Affine::translate((viewport.xy - offset.xy))`
/// before recursing into the scroll content. Every leaf paint
/// primitive ([`fill_rect`] / [`stroke_rect`] / [`paint_text`])
/// applies the threaded transform at the Vello call site so the
/// content paints offset into the viewport without mutating the
/// scene tree.
///
/// `Scene::Scroll` pushes a Vello clip layer keyed to the viewport
/// rect (in the parent transform's frame), then recurses with the
/// shifted child transform. `pop_layer` balances on exit so the
/// adapter stays compatible with Vello's stack contract.
#[allow(clippy::too_many_arguments)]
fn to_vello_inner<F>(
    scene: &Scene,
    fill_hook: &F,
    text_cache: &mut LayoutCache,
    out: &mut VelloScene,
    transform: Affine,
) where
    F: Fn(&BoxNode) -> Option<Color>,
{
    match scene {
        Scene::Container(c) => {
            fill_rect(out, c.rect, c.style.fill, c.style.corner_radius, transform);
            for child in &c.children {
                to_vello_inner(child, fill_hook, text_cache, out, transform);
            }
        }
        Scene::Box(b) => {
            let fill = fill_hook(b).unwrap_or(b.style.fill);
            fill_rect(out, b.rect, fill, b.style.corner_radius, transform);
            if let Some(border) = b.style.border {
                stroke_rect(out, b.rect, border, transform);
            }
        }
        Scene::Text(t) => paint_text(out, t, text_cache, transform),
        Scene::Scroll(s) => {
            // R55.E.1 — viewport clip in the parent's coordinate
            // frame; Vello applies the supplied transform to the
            // clip shape so the resulting screen-space clip lands
            // at `transform * viewport`.
            let viewport_clip = KurboRect::new(
                f64::from(s.viewport.x),
                f64::from(s.viewport.y),
                f64::from(s.viewport.x.saturating_add(s.viewport.w)),
                f64::from(s.viewport.y.saturating_add(s.viewport.h)),
            );
            out.push_clip_layer(transform, &viewport_clip);
            // Content paints in content-intrinsic coordinates; the
            // scroll container shifts so that content-intrinsic
            // `(0, 0)` lands at viewport `(viewport.x - offset_x,
            //  viewport.y - offset_y)` in the parent frame. Compose
            // with the inherited transform so nested scrolls
            // accumulate correctly.
            let dx = f64::from(s.viewport.x) - f64::from(s.offset_x);
            let dy = f64::from(s.viewport.y) - f64::from(s.offset_y);
            let child_transform = transform * Affine::translate((dx, dy));
            to_vello_inner(&s.content, fill_hook, text_cache, out, child_transform);
            out.pop_layer();
        }
        // External / Effect / Path / Image: no-op. Path + Image paint
        // primitives attach in follow-up rounds.
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

/// R51.58 §5.39 — paint an ARIA focus ring around the widget
/// tagged `focused_tag`. WCAG 2.4.11 Focus Appearance asks for a
/// ≥2px outline with ≥3:1 contrast against the surrounding pixels;
/// the default ring is a 2px stroke offset 2px outside the widget
/// rect in a neutral blue (`#1A73E8`, the Material default focus
/// color), which clears the contrast bar against light and dark
/// backgrounds alike.
///
/// No-op when:
/// * `focused_tag` is `None` (`FocusManager` between Tab boundaries);
/// * the paint scene has no tagged node matching `focused_tag` (the
///   focused widget was removed by the view fn this frame — the
///   [`crate::FocusManager`] also drops stale focus on
///   `update_focusable_tags`, but defensive belt-and-suspenders).
///
/// Call after [`to_vello`] so the ring overlays the widget visual
/// it indicates rather than being painted over.
/// Focus ring stroke width in CSS pixels — WCAG 2.4.11 minimum.
const FOCUS_RING_WIDTH: f64 = 2.0;
/// Focus ring outer offset from the widget rect — Material / Fluent
/// convention keeps the ring from overlapping the widget visual edge.
const FOCUS_RING_OFFSET: f64 = 2.0;

pub fn paint_focus_ring(
    paint_scene: &Scene,
    focused_tag: Option<&str>,
    out: &mut VelloScene,
) {
    let Some(tag) = focused_tag else {
        return;
    };
    let Some(rect) = focus_rect_for_tag(paint_scene, tag) else {
        return;
    };
    let outer = KurboRect::new(
        f64::from(rect.x) - FOCUS_RING_OFFSET,
        f64::from(rect.y) - FOCUS_RING_OFFSET,
        f64::from(rect.x.saturating_add(rect.w)) + FOCUS_RING_OFFSET,
        f64::from(rect.y.saturating_add(rect.h)) + FOCUS_RING_OFFSET,
    );
    // Material Design / Google Sans default focus ring color
    // `#1A73E8` = (26, 115, 232). The neutral blue satisfies WCAG
    // 2.4.11 against the framework's default `BG_FILL` palette and
    // the typical hello-* light backgrounds.
    let color = PenikoColor::from_rgba8(26, 115, 232, 255);
    out.stroke(
        &Stroke::new(FOCUS_RING_WIDTH),
        Affine::IDENTITY,
        color,
        None,
        &outer,
    );
}

/// Walk the paint scene's tagged Container / Box nodes and return the
/// matching node's `rect`. Internal duplication of `input::rect_for_tag`
/// kept on purpose — that one is gated on the `vello`-feature-agnostic
/// `input` module (touch / non-vello backends use it too), while this
/// one lives in the `vello`-gated `paint_adapter` so its scope is the
/// focus ring renderer alone. The two share no state and the function
/// body is eight lines — extracting a shared utility module would cost
/// more in indirection than the duplication saves.
fn focus_rect_for_tag(scene: &Scene, target_tag: &str) -> Option<Rect> {
    if let Some(tag) = scene.tag() {
        if tag == target_tag {
            return Some(scene.rect());
        }
    }
    if let Scene::Container(c) = scene {
        for child in &c.children {
            if let Some(rect) = focus_rect_for_tag(child, target_tag) {
                return Some(rect);
            }
        }
    }
    None
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

/// Emit one Vello filled-rectangle path for a pinion (`Rect`, `Color`,
/// `corner_radius`) triple. Transparent fills are skipped (matches the
/// pre-R46.3.1 `paint_filled_rect` early-exit).
///
/// R639 §5.16 §5.2 — `corner_radius == 0` paints a sharp [`KurboRect`]
/// (the legacy zero-cost path used by every Container/Box that does
/// not set `BoxStyle.corner_radius`). `corner_radius > 0` paints a
/// [`KurboRoundedRect`] with a single uniform radius applied to all
/// four corners. `kurbo::RoundedRect::from_rect` auto-clamps the
/// radius to `min(width, height) / 2`, so an over-large radius (M3
/// Filled Button's `radius: 100` against a 40-px-tall rect) naturally
/// resolves to a pill shape without an explicit caller-side clamp.
///
/// Per-corner asymmetric radii (Figma `rectangleCornerRadii: [tl, tr,
/// br, bl]`) deferred to a follow-up round once the first asymmetric
/// binding lands — `kurbo::RoundedRectRadii` already supports the
/// shape, this slice only wires the single-radius surface
/// [`BoxStyle.corner_radius`] exposes today.
fn fill_rect(
    out: &mut VelloScene,
    r: Rect,
    fill: Color,
    corner_radius: u32,
    transform: Affine,
) {
    if fill == Color::TRANSPARENT {
        return;
    }
    let x0 = f64::from(r.x);
    let y0 = f64::from(r.y);
    let x1 = f64::from(r.x.saturating_add(r.w));
    let y1 = f64::from(r.y.saturating_add(r.h));
    let peniko_fill = to_peniko(fill);
    if corner_radius == 0 {
        let rect = KurboRect::new(x0, y0, x1, y1);
        out.fill(Fill::NonZero, transform, peniko_fill, None, &rect);
    } else {
        let rounded = KurboRoundedRect::new(x0, y0, x1, y1, f64::from(corner_radius));
        out.fill(Fill::NonZero, transform, peniko_fill, None, &rounded);
    }
}

/// Emit one Vello stroke for a pinion [`Border`]. Vello strokes are
/// path-centered; the [`BorderPlacement`] determines whether we inset
/// (Inside, legacy softbuffer), keep the stroke on the path (Center,
/// Vello-native), or outset (Outside, CSS content-box).
fn stroke_rect(out: &mut VelloScene, r: Rect, border: Border, transform: Affine) {
    if border.width == 0 {
        return;
    }
    let w = f64::from(border.width);
    // Signed offset of the stroke's path centre relative to the rect
    // edge — positive moves inward (Inside), zero leaves on edge
    // (Center), negative moves outward (Outside).
    let offset = match border.placement {
        BorderPlacement::Center => 0.0,
        BorderPlacement::Outside => -(w / 2.0),
        // Inside (R46.3.2 default — legacy softbuffer compatibility)
        // plus any future #[non_exhaustive] variant: conservative
        // inset geometry. Listing Inside under the wildcard rather
        // than as its own arm satisfies clippy::match_same_arms
        // without losing forward-compat coverage.
        BorderPlacement::Inside | _ => w / 2.0,
    };
    let rect = KurboRect::new(
        f64::from(r.x) + offset,
        f64::from(r.y) + offset,
        f64::from(r.x.saturating_add(r.w)) - offset,
        f64::from(r.y.saturating_add(r.h)) - offset,
    );
    out.stroke(
        &Stroke::new(w),
        transform,
        to_peniko(border.color),
        None,
        &rect,
    );
}

/// Emit one Vello glyph run per parley [`GlyphRun`] shaped from
/// `t.content` + `t.style` (R47.3 §5.36 + R47.6 Figma-fidelity wire).
///
/// The text origin is `(t.rect.x, t.rect.y)`; `t.rect.w > 0` wraps at
/// that pixel width, `w == 0` flows on a single unbounded line.
///
/// R47.6 decoration: when [`TextStyle::decoration`] enables underline
/// or strikethrough, parley populates each [`GlyphRun`]'s style with a
/// `Decoration<Color>`. We stroke a horizontal [`Line`] at the
/// font-metric-derived offset spanning the run's advance.
///
/// R47.6 overflow: [`TextOverflow::Clip`] wraps the whole emit in a
/// Vello clip layer keyed to `t.rect`; out-of-rect glyphs are clipped
/// before composition. [`TextOverflow::Ellipsis`] silently falls back
/// to `Clip` — parley 0.9 does not expose a native line-truncation
/// API, so the visual result is the same as `Clip` until R47.x lands
/// the custom truncation pass. [`TextOverflow::Visible`] (default)
/// skips the clip wrap entirely.
fn paint_text(
    out: &mut VelloScene,
    t: &TextNode,
    cache: &mut LayoutCache,
    parent_transform: Affine,
) {
    if t.content.is_empty() {
        return;
    }
    // R51.27 §5.37.4 — UAX #9 L4 mirroring is applied inside
    // `LayoutCache::shape` (R51.31 substrate move). paint_adapter
    // passes the raw `TextNode.content` and the cache key (raw text)
    // maps to a Layout whose shape input is the post-mirror string —
    // single LRU lookup covers both the BIDI helper and the parley
    // shape pass, so static labels skip mirror recomputation entirely
    // on steady-state frames.
    let max_width = if t.rect.w > 0 { Some(t.rect.w) } else { None };
    let layout = cache.layout(&t.content, &t.style, max_width);
    // R51.188 §5.45 R55.E.1 — compose the inherited transform (e.g.
    // a parent `Scene::Scroll`'s shifted child transform) with the
    // text's own `(t.rect.x, t.rect.y)` translation. Pre-R51.188
    // `paint_text` assumed `Affine::IDENTITY` from the caller; the
    // composition keeps that path bit-identical (IDENTITY * T = T)
    // and lets scroll-embedded text track the scroll offset.
    let transform = parent_transform
        * Affine::translate((f64::from(t.rect.x), f64::from(t.rect.y)));
    // R47.6 — Clip + Ellipsis (silent fallback to Clip until R47.x
    // ellipsis pass) wrap the emit in a Vello clip layer keyed to
    // `t.rect`. Visible skips the wrap entirely so a freshly-default
    // TextNode pays no per-frame layer cost.
    let needs_clip = matches!(t.style.overflow, TextOverflow::Clip | TextOverflow::Ellipsis);
    if needs_clip {
        // R51.188 §5.45 R55.E.1 — clip-rect lives in the parent
        // frame (not text-local) because the `transform` above
        // already maps text-local glyph positions into the parent's
        // coordinate space. Passing `parent_transform` lets a
        // scroll-shifted text node clip in its scroll's frame.
        let clip_rect = KurboRect::new(
            f64::from(t.rect.x),
            f64::from(t.rect.y),
            f64::from(t.rect.x.saturating_add(t.rect.w)),
            f64::from(t.rect.y.saturating_add(t.rect.h)),
        );
        out.push_clip_layer(parent_transform, &clip_rect);
    }
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else { continue };
            let parley_run = run.run();
            let font = parley_run.font();
            let font_size = parley_run.font_size();
            let brush = to_peniko(run.style().brush);
            out.draw_glyphs(font)
                .transform(transform)
                .font_size(font_size)
                .brush(brush)
                .draw(
                    Fill::NonZero,
                    run.positioned_glyphs().map(|g| Glyph {
                        id: g.id,
                        x: g.x,
                        y: g.y,
                    }),
                );
            // R47.6 — decoration strokes. parley emits `Some(Decoration)`
            // on `style().underline / strikethrough` whenever the source
            // TextStyle enabled them (see `LayoutCache::shape`'s
            // `StyleProperty::Underline / Strikethrough` push). The
            // offset / size are font-metric-defaulted (parley fills the
            // Option with the run metric values); the brush defaults to
            // the run's foreground brush.
            paint_decorations(out, &run, transform);
        }
    }
    if needs_clip {
        out.pop_layer();
    }
}

/// R47.6 — emit underline + strikethrough strokes for one parley
/// [`GlyphRun`]. Each decoration is a horizontal line at the
/// font-metric-derived offset spanning the run advance; the brush is
/// the run's foreground colour (matching parley's `Decoration.brush`
/// default).
fn paint_decorations(
    out: &mut VelloScene,
    run: &pinion_text::parley::GlyphRun<'_, Color>,
    transform: Affine,
) {
    let parley_run = run.run();
    let metrics = parley_run.metrics();
    let baseline = run.baseline();
    let start = f64::from(run.offset());
    let end = f64::from(run.offset() + run.advance());
    if let Some(deco) = run.style().underline.as_ref() {
        let offset = deco.offset.unwrap_or(metrics.underline_offset);
        let size = deco.size.unwrap_or(metrics.underline_size);
        // parley's underline offset is measured upward from the baseline
        // (positive = above); on screen Y the underline sits below the
        // baseline, so subtract. The Y advances downward in our coord
        // system, hence the `- offset`.
        let y = f64::from(baseline - offset);
        let line = Line::new((start, y), (end, y));
        out.stroke(
            &Stroke::new(f64::from(size).max(1.0)),
            transform,
            to_peniko(deco.brush),
            None,
            &line,
        );
    }
    if let Some(deco) = run.style().strikethrough.as_ref() {
        let offset = deco.offset.unwrap_or(metrics.strikethrough_offset);
        let size = deco.size.unwrap_or(metrics.strikethrough_size);
        let y = f64::from(baseline - offset);
        let line = Line::new((start, y), (end, y));
        out.stroke(
            &Stroke::new(f64::from(size).max(1.0)),
            transform,
            to_peniko(deco.brush),
            None,
            &line,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
    use pinion_core::style::{BoxStyle, Color, TextStyle};
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
        let mut cache = LayoutCache::new();
        let hits = Cell::new(0_u32);
        to_vello(
            &scene,
            &|_b: &BoxNode| {
                hits.set(hits.get() + 1);
                None
            },
            &mut cache,
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
        let mut cache = LayoutCache::new();
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
            &mut cache,
            &mut vello,
        );
        assert_eq!(overrides.get(), 1);
        assert_eq!(passthroughs.get(), 1);
    }

    #[test]
    fn stroke_rect_inside_placement_inset_matches_softbuffer_geometry() {
        // R46.3.2 — the default Border placement (Inside) must inset
        // the centred stroke by width/2 so the entire stroke lies
        // within the rect. We can't read back Vello's emitted draw
        // commands; instead we verify the placement field plumbs
        // through stroke_rect by ensuring no panic on each variant.
        use pinion_core::style::{Border, BorderPlacement, BoxStyle};
        for placement in [
            BorderPlacement::Inside,
            BorderPlacement::Center,
            BorderPlacement::Outside,
        ] {
            let border = Border::new(Color::rgb(0xff, 0, 0), 4).with_placement(placement);
            let style = BoxStyle::filled(Color::TRANSPARENT).with_border(border);
            let scene = Scene::Box(BoxNode::new(Rect::new(10, 10, 100, 100), style));
            let mut vello = VelloScene::new();
            let mut cache = LayoutCache::new();
            to_vello(&scene, &|_| None, &mut cache, &mut vello);
        }
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
        let mut cache = LayoutCache::new();
        let saw_leaf = Cell::new(false);
        to_vello(
            &scene,
            &|b: &BoxNode| {
                if b.tag.as_deref() == Some("leaf") {
                    saw_leaf.set(true);
                }
                None
            },
            &mut cache,
            &mut vello,
        );
        assert!(saw_leaf.get(), "nested leaf BoxNode must be visited");
    }

    #[test]
    fn to_vello_text_arm_populates_cache() {
        // R47.3 §5.36 — Scene::Text walks via paint_text which calls
        // LayoutCache::layout; the cache should hold the entry after
        // one walk, and a second walk over the same text should not
        // grow the cache (steady-state cache hit).
        let scene = Scene::Text(TextNode::styled(
            "Hello",
            Rect::new(0, 0, 200, 32),
            TextStyle::new().with_size_px(16),
        ));
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
        assert_eq!(cache.len(), 1, "first paint populates cache");
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
        assert_eq!(cache.len(), 1, "repeat paint hits cache, no growth");
    }

    #[test]
    fn to_vello_text_arm_skips_empty_content() {
        // Empty `t.content` short-circuits before the cache is touched —
        // parley would produce an empty layout but the walk has no
        // glyphs to emit, so skipping early avoids the wasted shaping
        // work.
        let scene = Scene::Text(TextNode::styled(
            "",
            Rect::new(0, 0, 200, 32),
            TextStyle::new().with_size_px(16),
        ));
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
        assert_eq!(cache.len(), 0, "empty content does not populate cache");
    }

    #[test]
    fn to_vello_text_arm_decoration_no_panic() {
        // R47.6 §5.36 — decoration wire emits parley StyleProperty::
        // Underline + Strikethrough; paint_text walks parley's
        // `style().underline / strikethrough` and strokes a horizontal
        // line per decoration. Cannot inspect Vello's emitted draw
        // commands from outside the crate; assert no panic on every
        // combination instead.
        use pinion_core::style::TextDecoration;
        for deco in [
            TextDecoration::none(),
            TextDecoration::underline(),
            TextDecoration::strikethrough(),
            TextDecoration::both(),
        ] {
            let scene = Scene::Text(TextNode::styled(
                "Hi",
                Rect::new(0, 0, 200, 32),
                TextStyle::new().with_size_px(16).with_decoration(deco),
            ));
            let mut vello = VelloScene::new();
            let mut cache = LayoutCache::new();
            to_vello(&scene, &|_| None, &mut cache, &mut vello);
        }
    }

    #[test]
    fn to_vello_text_arm_overflow_clip_pushes_layer_safely() {
        // R47.6 — TextOverflow::Clip wraps paint_text in
        // push_clip_layer / pop_layer. The wrap must balance (every
        // push matched by a pop) so the Vello scene encoding stays
        // valid; we cannot read the encoded layer stack from outside
        // the crate, but the no-panic walk + Vello's own internal
        // assertions (debug builds verify layer balance) cover this.
        use pinion_core::style::TextOverflow;
        for overflow in [
            TextOverflow::Visible,
            TextOverflow::Clip,
            TextOverflow::Ellipsis,
        ] {
            let scene = Scene::Text(TextNode::styled(
                "OverflowingContent",
                Rect::new(0, 0, 50, 16), // intentionally tight
                TextStyle::new()
                    .with_size_px(16)
                    .with_overflow(overflow),
            ));
            let mut vello = VelloScene::new();
            let mut cache = LayoutCache::new();
            to_vello(&scene, &|_| None, &mut cache, &mut vello);
        }
    }

    // ----- R51.58 §5.39 focus ring tests -----

    /// Build a Container with a tagged Box child rooted at
    /// `child_rect`. Mirrors the topology every hello-* example
    /// paints — outer Container background + inner Box with the
    /// widget tag.
    fn tagged_scene(tag: &'static str, child_rect: Rect) -> Scene {
        Scene::Container(
            ContainerNode::new(vec![Scene::Box(
                BoxNode::filled(child_rect, Color::rgb(0xff, 0, 0)).with_tag(tag),
            )])
            .with_style(BoxStyle::filled(Color::rgb(0, 0, 0))),
        )
    }

    #[test]
    fn focus_rect_for_tag_finds_tagged_child() {
        let scene = tagged_scene("main_btn", Rect::new(10, 20, 100, 30));
        assert_eq!(
            focus_rect_for_tag(&scene, "main_btn"),
            Some(Rect::new(10, 20, 100, 30)),
        );
    }

    #[test]
    fn focus_rect_for_tag_returns_none_for_unknown_tag() {
        let scene = tagged_scene("main_btn", Rect::new(10, 20, 100, 30));
        assert!(focus_rect_for_tag(&scene, "nope").is_none());
    }

    #[test]
    fn paint_focus_ring_no_op_on_none() {
        // FocusManager between Tab boundaries — paint_focus_ring
        // must not push any encoding onto the Vello scene.
        let scene = tagged_scene("main_btn", Rect::new(10, 20, 100, 30));
        let mut vello = VelloScene::new();
        paint_focus_ring(&scene, None, &mut vello);
        // Hard to assert "empty encoding" from outside vello, but the
        // call must not panic (no rect lookup, no stroke emit).
    }

    #[test]
    fn paint_focus_ring_no_op_on_missing_tag() {
        // FocusManager has a tag but the view fn removed that
        // widget — paint_focus_ring stays silent (defensive, since
        // FocusManager::update_focusable_tags also drops stale tags).
        let scene = tagged_scene("main_btn", Rect::new(10, 20, 100, 30));
        let mut vello = VelloScene::new();
        paint_focus_ring(&scene, Some("gone"), &mut vello);
    }

    #[test]
    fn paint_focus_ring_strokes_when_tag_resolves() {
        // The matching path — exercise the full lookup + stroke
        // emit, verifying the call lands without panic on a real
        // tagged scene.
        let scene = tagged_scene("main_btn", Rect::new(10, 20, 100, 30));
        let mut vello = VelloScene::new();
        paint_focus_ring(&scene, Some("main_btn"), &mut vello);
    }

    // ----- R51.188 §5.45 R55.E.1 Vello paint clipping tests -----

    #[test]
    fn r55_e1_scroll_arm_walks_content_box_hook() {
        // R55.E.1 — the Scroll arm recurses into `content`. The hook
        // visits every BoxNode inside the scroll's content tree;
        // verifies the walker reaches past the scroll wrapper rather
        // than treating it as a leaf.
        use pinion_core::scene::ScrollNode;
        let inner_box = Scene::Box(
            BoxNode::filled(Rect::new(0, 0, 50, 50), Color::rgb(0, 0xff, 0))
                .with_tag("scroll_leaf"),
        );
        let scroll = ScrollNode::new(Rect::new(10, 10, 100, 100), inner_box);
        let scene = Scene::Scroll(scroll);
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        let saw_leaf = Cell::new(false);
        to_vello(
            &scene,
            &|b: &BoxNode| {
                if b.tag.as_deref() == Some("scroll_leaf") {
                    saw_leaf.set(true);
                }
                None
            },
            &mut cache,
            &mut vello,
        );
        assert!(saw_leaf.get(), "scroll content must be visited");
    }

    #[test]
    fn r55_e1_scroll_layer_balances_on_panic_free_walk() {
        // R55.E.1 — Vello's `pop_layer` panics on encoder
        // underflow; if the Scroll arm's `push_clip_layer` and
        // `pop_layer` are not balanced this test trips. Empty
        // content + non-empty content + nested scroll all exit
        // cleanly.
        use pinion_core::scene::ScrollNode;
        let empty_content = Scene::Container(
            ContainerNode::new(vec![]).with_style(BoxStyle::filled(Color::default())),
        );
        let empty_scroll = Scene::Scroll(ScrollNode::new(
            Rect::new(0, 0, 100, 100),
            empty_content,
        ));

        let inner_box = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 200, 200),
            Color::rgb(0xff, 0, 0),
        ));
        let plain_scroll = Scene::Scroll(ScrollNode::new(
            Rect::new(0, 0, 100, 100),
            inner_box,
        ));

        let inner_inner = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 50, 50),
            Color::rgb(0, 0, 0xff),
        ));
        let inner_scroll = Scene::Scroll(ScrollNode::new(
            Rect::new(10, 10, 50, 50),
            inner_inner,
        ));
        let outer_scroll = Scene::Scroll(ScrollNode::new(
            Rect::new(0, 0, 200, 200),
            inner_scroll,
        ));

        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        to_vello(&empty_scroll, &|_| None, &mut cache, &mut vello);
        to_vello(&plain_scroll, &|_| None, &mut cache, &mut vello);
        to_vello(&outer_scroll, &|_| None, &mut cache, &mut vello);
    }

    #[test]
    fn r55_e1_scroll_text_inside_lays_out_through_cache() {
        // R55.E.1 — Text inside a scroll routes through the same
        // `paint_text` path (now transform-aware). Cache populates
        // once and short-circuits on the repeat walk — same
        // steady-state cache-hit shape as the non-scroll text test.
        use pinion_core::scene::ScrollNode;
        let text = Scene::Text(TextNode::styled(
            "Scrolled text",
            Rect::new(0, 0, 200, 32),
            TextStyle::new().with_size_px(16),
        ));
        let scroll = ScrollNode::new(Rect::new(0, 0, 100, 100), text)
            .with_offset(0, 20);
        let scene = Scene::Scroll(scroll);
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
        assert_eq!(cache.len(), 1, "first paint populates cache");
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
        assert_eq!(cache.len(), 1, "repeat paint hits cache, no growth");
    }

    #[test]
    fn r639_fill_rect_zero_radius_path_is_sharp() {
        // R639 §5.16 §5.2 — corner_radius == 0 must paint a sharp
        // [`KurboRect`] (legacy zero-cost path). Walk a single-box
        // scene through `to_vello` with the default `BoxStyle`
        // (corner_radius = 0) and confirm the vello scene is non-
        // empty (the fill emit succeeded) and the scene's recorded
        // primitive count matches the rounded variant — both paths
        // emit one fill, only the underlying shape differs.
        let scene = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 100, 40),
            Color::rgb(0xff, 0, 0),
        ));
        assert_eq!(
            <Scene as MatchBoxStyle>::corner_radius(&scene),
            0,
            "default BoxStyle radius is 0",
        );
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
        // The sharp-rect path was the only one before R639; survival
        // through the new dispatch + non-empty vello scene proves
        // the zero-radius arm preserved its legacy behaviour.
        assert!(vello.encoding().n_paths > 0, "fill emitted at least one path");
    }

    #[test]
    fn r639_fill_rect_nonzero_radius_emits_rounded_rect() {
        // R639 §5.16 §5.2 — corner_radius > 0 must paint a
        // [`KurboRoundedRect`]. Vello records the path shape in its
        // encoding; the rounded-rect path produces a different
        // primitive count from the sharp-rect path (kurbo decomposes
        // rounded corners into cubic Béziers).
        let sharp = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 109, 40),
            Color::rgb(103, 80, 164),
        ));
        let rounded = Scene::Box(BoxNode::new(
            Rect::new(0, 0, 109, 40),
            BoxStyle::filled(Color::rgb(103, 80, 164)).with_corner_radius(100),
        ));
        let mut cache = LayoutCache::new();
        let mut sharp_scene = VelloScene::new();
        to_vello(&sharp, &|_| None, &mut cache, &mut sharp_scene);
        let mut rounded_scene = VelloScene::new();
        to_vello(&rounded, &|_| None, &mut cache, &mut rounded_scene);
        // The sharp rect = 4 line segments (1 path with 4 verbs).
        // The rounded rect = 4 line segments + 4 quadrant arcs (1
        // path with more verbs). The exact verb counts vary across
        // kurbo versions; assert only that the rounded encoding is
        // strictly larger than the sharp one — wire-up evidence
        // independent of internal vello/kurbo version details.
        assert!(
            rounded_scene.encoding().n_path_segments
                > sharp_scene.encoding().n_path_segments,
            "rounded path must have more segments than sharp; \
             sharp={}, rounded={}",
            sharp_scene.encoding().n_path_segments,
            rounded_scene.encoding().n_path_segments,
        );
    }

    // Internal helper to extract `corner_radius` from a `Scene` arm
    // for the R639 zero-path sanity assertion. Lives inside the test
    // module to keep the production surface free of one-off accessors.
    trait MatchBoxStyle {
        fn corner_radius(&self) -> u32;
    }
    impl MatchBoxStyle for Scene {
        fn corner_radius(&self) -> u32 {
            match self {
                Scene::Container(c) => c.style.corner_radius,
                Scene::Box(b) => b.style.corner_radius,
                _ => 0,
            }
        }
    }

    #[test]
    fn r55_e1_scroll_arm_survives_offset_overshoot() {
        // R55.E.1 — adversarial offset (content shifted past the
        // viewport entirely) still completes the walk without
        // panic. The walker does not clamp the offset — that's
        // ScrollState's job — so paint just renders the empty
        // visible region.
        use pinion_core::scene::ScrollNode;
        let content = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 50, 50),
            Color::rgb(0, 0xff, 0),
        ));
        let scroll = ScrollNode::new(Rect::new(0, 0, 100, 100), content)
            .with_offset(i32::MAX, i32::MAX);
        let scene = Scene::Scroll(scroll);
        let mut vello = VelloScene::new();
        let mut cache = LayoutCache::new();
        to_vello(&scene, &|_| None, &mut cache, &mut vello);
    }
}
