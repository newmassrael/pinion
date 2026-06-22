//! R659 §5.16 §5.45 — backend-agnostic vertical scrollbar peer paint
//! composition.
//!
//! ## Why a substrate
//!
//! Pre-R659, the only consumer of the visible scrollbar peer was
//! [`examples/hello-listbox`](https://github.com/anthropics/pinion/tree/main/examples/hello-listbox)
//! (R55.D.4 first-client landed Round 481). R659 lands the todomvc
//! filter axis and inherits a R658 honest carry: "visible scrollbar
//! peer 미연결" — the todo list scrolls under wheel input but the
//! user has no visible thumb to drag. Wiring a 2nd hand-rolled
//! `build_scrollbar_visual` in todomvc would duplicate ~65 LOC across
//! the two bindings; per
//! [[abstraction-needs-second-consumer]] the **2nd-consumer signal**
//! fires and the helper lifts here.
//!
//! ## API shape
//!
//! [`view_vertical_scrollbar`] is the single entry-point. It takes
//! the shared [`ScrollState`] handle (composition with
//! [`ScrollBarExternal`](pinion_core::widgets::scrollbar::ScrollBarExternal)
//! through the same `Rc`), the active [`Theme`], and a
//! [`VerticalScrollbarStyle`] sidecar carrying the four binding-
//! local sizing constants (viewport height, gutter width, thumb
//! floor, paint-side tag for input-router routing). Returns a single
//! [`Scene::Container`] tagged with the supplied tag so the
//! [`InputRouter`](pinion_runtime::InputRouter) hit-test routes
//! pointer events to the matching
//! [`ScrollBarExternal`](pinion_core::widgets::scrollbar::ScrollBarExternal)
//! registered through
//! [`WidgetCore::create_extra_externals`](pinion_core::WidgetCore::create_extra_externals).
//!
//! ## Why the style is a struct, not a free-fn arg list
//!
//! Four call-site parameters (`viewport_h` / `gutter_w` / `min_thumb`
//! / `tag`) is at the edge of "positional arg list still reads
//! cleanly". A sidecar struct with builders + a sensible default
//! keeps the call-site terse
//! (`VerticalScrollbarStyle::material(VIEWPORT_H, TAG)`)
//! and leaves room for future axes (hover hit-zone width, thumb
//! corner radius, track corner radius) without breaking the call-
//! sites that exist today (additive only, behind `#[non_exhaustive]`).
//!
//! ## Theme role mapping (M3 canonical)
//!
//! | Element | `ColorRole`                  | Why                              |
//! |---------|------------------------------|----------------------------------|
//! | Track   | `SurfaceContainerHighest`    | M3 "inactive container" tier     |
//! | Thumb   | `Outline` (default)          | M3 hairline / divider weight; idle base overridable via [`VerticalScrollbarStyle::with_thumb_role`] (R1046) for an always-visible bar |
//!
//! Mirrors the role mapping `hello-listbox` already landed at Round
//! 577 ([[m3-surface-tier-expansion]]) — the helper inherits the
//! exact same M3 mapping so a palette swap from anywhere in the
//! application repaints both bindings' scrollbars in lock-step
//! without diverging RGB values.
//!
//! ## Edge case: first-paint `max_y == 0`
//!
//! `ScrollState::max` returns `0` on the first paint (the layout pass
//! writes the real max *after* the first view runs). The
//! [`scrollbar_thumb_rect`](pinion_core::widgets::scrollbar::scrollbar_thumb_rect)
//! helper's `content_extent <= viewport_extent` branch then fills
//! the thumb to the full track — the textbook "nothing to scroll"
//! rendering. R55.G.24 [[signal-batch-atomic-multi-axis-update]] /
//! `set_max` Signal-batches, so the next paint re-runs the view fn
//! with the freshly-written max and the thumb snaps to its correct
//! size on frame 2. The helper guards against signed-arithmetic
//! pitfalls via `u32::try_from(max_y.max(0))` so a future
//! `ScrollState::max` representation change cannot corrupt the
//! geometry.

use std::rc::Rc;

use pinion_core::scene::{ContainerNode, Rect, Scene};
use pinion_core::style::{BoxStyle, LayoutStyle, Size};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::scrollbar::{ScrollBarOrientation, ScrollBarState, scrollbar_thumb_rect};

/// (R659 §5.45) Sidecar carrying the binding-local sizing constants
/// for [`view_vertical_scrollbar`]. `#[non_exhaustive]` so future
/// rounds can add axes (e.g. `hover_zone_w`, `thumb_corner_radius`)
/// behind builders without breaking the constructor surface.
///
/// Use [`Self::material`] for the M3-canonical 8-px gutter + 24-px
/// thumb floor — the default the R55.D.4 hello-listbox first-client
/// established and the R659 todomvc consumer inherits unchanged.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct VerticalScrollbarStyle {
    /// Track **pixel** height — the cross-axis-perpendicular extent
    /// the track [`Scene::Container`] is laid out at. For the common
    /// pixel-unit consumer this is also the viewport content-extent
    /// (the two coincide when the [`ScrollState`] offset / max are in
    /// pixels), so [`Self::viewport_extent`] defaults to this value.
    ///
    /// (R1032 §5.45) A **row-unit** consumer — a text grid / terminal
    /// whose [`ScrollState`] counts scrollback *rows*, not pixels —
    /// keeps this as the track's real pixel height (e.g. `480`) and
    /// declares the visible *row* count separately through
    /// [`Self::with_viewport_extent`] (e.g. `24`). The track still
    /// renders at the right pixel size while the thumb ratio is
    /// derived from the row counts. See [`Self::content_viewport`].
    pub viewport_h: u32,
    /// (R1032 §5.45) Viewport content-extent in the **same unit as the
    /// paired [`ScrollState`]'s `offset_y` / `max_y`** — the value fed
    /// to [`scrollbar_thumb_rect`] as `viewport_extent`. `None` means
    /// "same as [`Self::viewport_h`]", the pixel-unit case where the
    /// track pixel height and the content viewport coincide.
    ///
    /// Set this (via [`Self::with_viewport_extent`]) only when the
    /// [`ScrollState`] is driven in a non-pixel unit — e.g. a terminal
    /// whose `offset_y` is a scrollback *row* index. Then `viewport_h`
    /// stays the track's pixel height and this carries the visible row
    /// count; the thumb extent / position resolve to the correct
    /// pixels because [`scrollbar_thumb_rect`] only ever consumes
    /// `viewport_extent` / `content_extent` / `offset` as *ratios*
    /// (`track_px · viewport/content`, `travel_px · offset/scroll_max`),
    /// so the content unit cancels.
    pub viewport_extent: Option<u32>,
    /// Track width along the cross axis. Material / web canonical
    /// is 8 px for desktop-class UIs; touch interfaces may bump to
    /// 12+ via a future axis.
    pub gutter_w: u32,
    /// Minimum thumb extent (Material/UIKit floor convention is
    /// 24 px). Setting to `0` reverts to strict ratio-of-content
    /// thumb size, which lets the thumb vanish for very long
    /// content — useful for unit-test math verification but not
    /// recommended for production.
    pub min_thumb: u32,
    /// Paint-side tag attached to the outer track
    /// [`Scene::Container`] so the
    /// [`InputRouter`](pinion_runtime::InputRouter) hit-test routes
    /// pointer events on the track / thumb to the matching
    /// [`ScrollBarExternal`](pinion_core::widgets::scrollbar::ScrollBarExternal)
    /// registered through
    /// [`WidgetCore::create_extra_externals`](pinion_core::WidgetCore::create_extra_externals).
    /// Must match the tag the application registers the external
    /// under, or the drag wire stays inert (paint-only mode).
    pub tag: &'static str,
    /// (R1046 §5.45) Idle thumb **base** [`ColorRole`], default
    /// [`ColorRole::Outline`] (the M3 hairline weight). The hover /
    /// dragging states still lerp *from* this base toward
    /// [`ColorRole::OnSurface`], and Disabled fades toward
    /// [`ColorRole::SurfaceContainerHighest`] — see
    /// [`thumb_fill_for_state`].
    ///
    /// Override (via [`Self::with_thumb_role`]) when the default
    /// `Outline` does not carry enough contrast against the
    /// `SurfaceContainerHighest` track for an *always-visible* bar — on
    /// the light palette `Outline` sits very close to the track tint, so
    /// a gnome-terminal-style permanent scrollbar reads its idle thumb
    /// as invisible. A row-unit terminal consumer passes
    /// [`ColorRole::OnSurfaceMuted`] here to match the keyboard / drag
    /// writers' contrast (light track `rgb(230,224,233)` vs muted thumb
    /// `rgb(96,96,96)`).
    pub thumb_role: ColorRole,
}

impl VerticalScrollbarStyle {
    /// (R659 §5.45) M3-canonical default: 8-px gutter + 24-px thumb
    /// floor, matching the R55.D.4 hello-listbox first-client. The
    /// caller supplies the viewport height (binding-local) + the
    /// paint-side tag (must match the application's
    /// `create_extra_externals` registration).
    #[must_use]
    pub const fn material(viewport_h: u32, tag: &'static str) -> Self {
        Self {
            viewport_h,
            viewport_extent: None,
            gutter_w: 8,
            min_thumb: 24,
            tag,
            thumb_role: ColorRole::Outline,
        }
    }

    /// (R1032 §5.45) Declare a viewport content-extent distinct from
    /// the track pixel height [`Self::viewport_h`]. Use this when the
    /// paired [`ScrollState`] is driven in a non-pixel unit — e.g. a
    /// terminal whose `offset_y` / `max_y` are scrollback *row*
    /// indices: pass the track's real pixel height to
    /// [`Self::material`] and the visible **row** count here. `None`
    /// (the default) keeps the pixel-unit behaviour where the track
    /// height doubles as the content viewport.
    #[must_use]
    pub const fn with_viewport_extent(mut self, viewport_extent: u32) -> Self {
        self.viewport_extent = Some(viewport_extent);
        self
    }

    /// (R1032 §5.45) Effective viewport content-extent fed to
    /// [`scrollbar_thumb_rect`]: the [`Self::viewport_extent`] override
    /// when set, else [`Self::viewport_h`] (the pixel-unit case where
    /// the track height and the content viewport are the same number).
    #[must_use]
    pub const fn content_viewport(&self) -> u32 {
        match self.viewport_extent {
            Some(extent) => extent,
            None => self.viewport_h,
        }
    }

    /// Override the gutter width. Useful for touch-tuned interfaces
    /// where 8 px is too thin to hit reliably with a finger
    /// (Material guidance recommends ≥ 12 px on touch surfaces).
    #[must_use]
    pub const fn with_gutter_w(mut self, gutter_w: u32) -> Self {
        self.gutter_w = gutter_w;
        self
    }

    /// Override the thumb-floor. Defaults to 24 px (Material /
    /// `UIKit` grabbable floor). Set to `0` to disable the floor
    /// (thumb scales linearly with viewport/content ratio).
    #[must_use]
    pub const fn with_min_thumb(mut self, min_thumb: u32) -> Self {
        self.min_thumb = min_thumb;
        self
    }

    /// (R1046 §5.45) Override the idle thumb base [`ColorRole`]
    /// (default [`ColorRole::Outline`]). The hover / dragging
    /// state-layer overlays still lerp from this base toward
    /// [`ColorRole::OnSurface`], so a single role swap shifts the whole
    /// always-visible / hover ramp without touching the M3 composition.
    ///
    /// Use [`ColorRole::OnSurfaceMuted`] for a gnome-terminal-style
    /// permanent bar whose idle thumb must stay legible against the
    /// `SurfaceContainerHighest` track on the light palette (where
    /// `Outline` is too low-contrast — the PR-19 motivating case).
    #[must_use]
    pub const fn with_thumb_role(mut self, thumb_role: ColorRole) -> Self {
        self.thumb_role = thumb_role;
        self
    }
}

/// (R660 §5.45) Resolve the Material 3 thumb fill for the given
/// `interaction` state. Composited via [`Color::lerp`] in linear
/// space ([[color-lerp-linear-space]]) so the state-layer overlay
/// renders perceptually-correct mid-tones across both light and
/// dark palettes.
///
/// `base` is the idle role — [`ColorRole::Outline`] by default, or the
/// caller's [`VerticalScrollbarStyle::thumb_role`] override (R1046). The
/// hover / dragging overlays lerp *from* `base`, so swapping the base
/// shifts the whole always-visible ramp without disturbing the M3
/// composition.
///
/// | State    | Composition                                  | Why                              |
/// |----------|----------------------------------------------|----------------------------------|
/// | Idle     | `base`                                       | M3 hairline / divider weight     |
/// | Hover    | `base` lerp(`OnSurface`, 0.08)               | M3 hover state-layer opacity     |
/// | Dragging | `base` lerp(`OnSurface`, 0.16)               | M3 dragged state-layer opacity   |
/// | Disabled | `base` lerp(`SurfaceContainerHighest`, 0.62) | M3 disabled = 38% visible (1 − 0.62 fade toward track) |
///
/// [`Color::lerp`]: pinion_core::style::Color::lerp
fn thumb_fill_for_state(
    theme: &Theme,
    interaction: ScrollBarState,
    base_role: ColorRole,
) -> pinion_core::style::Color {
    let base = theme.resolve(base_role);
    match interaction {
        ScrollBarState::Idle => base,
        // Hover is the shared M3 8% token; Dragging (0.16) is the
        // scrollbar-specific dragged opacity, not a shared state-layer token.
        ScrollBarState::Hover => base.lerp(
            theme.resolve(ColorRole::OnSurface),
            crate::state_layer::HOVER,
        ),
        ScrollBarState::Dragging => base.lerp(theme.resolve(ColorRole::OnSurface), 0.16),
        ScrollBarState::Disabled => {
            base.lerp(theme.resolve(ColorRole::SurfaceContainerHighest), 0.62)
        }
    }
}

/// (R659 §5.45 — R660 §5.16 hover/drag) Backend-agnostic vertical
/// scrollbar peer composition.
///
/// Reads `(offset_y, max_y)` off the shared
/// [`ScrollState`](pinion_core::widgets::scroll::ScrollState),
/// derives the thumb rectangle via
/// [`scrollbar_thumb_rect`](pinion_core::widgets::scrollbar::scrollbar_thumb_rect)
/// (R55.D.1 closed-form helper), and composites the result as a
/// single absolute-positioned thumb [`Scene::Container`] inside an
/// outer track [`Scene::Container`] filled with
/// [`ColorRole::SurfaceContainerHighest`]. The outer container
/// carries [`VerticalScrollbarStyle::tag`] so the input router
/// resolves pointer events to the matching
/// [`ScrollBarExternal`](pinion_core::widgets::scrollbar::ScrollBarExternal).
///
/// ## R660 §5.16 — interaction state argument
///
/// `interaction` is the live [`ScrollBarState`] read off the shared
/// [`ScrollBarInteractionSignal`](pinion_core::widgets::scrollbar::ScrollBarInteractionSignal)
/// the application gets back from
/// [`use_scrollbar_interaction`](pinion_core::widgets::scrollbar::use_scrollbar_interaction).
/// Drives the Material 3 thumb state-layer overlay (see
/// [`thumb_fill_for_state`] for the role table). Callers wire the
/// paired `ScrollBarExternal::attach_interaction(handle.clone())` so
/// the framework writes hover / drag transitions back into the same
/// signal — auto-subscription on the read side repaints the next
/// frame.
///
/// ## Unit: pixel vs row content
///
/// (R1032 §5.45) By default the track pixel height
/// ([`VerticalScrollbarStyle::viewport_h`]) doubles as the viewport
/// content-extent — correct when the [`ScrollState`] offset / max are
/// in pixels. A **row-unit** consumer (terminal / text grid whose
/// `offset_y` is a scrollback row index) declares the visible row
/// count via [`VerticalScrollbarStyle::with_viewport_extent`] while
/// keeping `viewport_h` as the real track pixel height; the thumb
/// extent / position still resolve to the correct pixels because
/// [`scrollbar_thumb_rect`] consumes the content scalars only as
/// ratios. The drag wire below is unaffected — `pointer_move` writes
/// `ScrollState::scroll_to` in whatever unit the state already holds.
///
/// ## Drag-able wire
///
/// The visible peer is **paint-only**. To make the thumb drag-able
/// the binding must register a
/// [`ScrollBarExternal::new().attach_state(scroll_state.clone())`](pinion_core::widgets::scrollbar::ScrollBarExternal)
/// through [`WidgetCore::create_extra_externals`](pinion_core::WidgetCore::create_extra_externals)
/// tagged with the same [`VerticalScrollbarStyle::tag`]. The shared
/// [`ScrollState`] handle threads the
/// [`InputRouter`](pinion_runtime::InputRouter)'s
/// `ScrollBarExternal::pointer_move` → `ScrollState::scroll_to` →
/// next-paint cycle.
///
/// ## Substrate inheritance
///
/// Identical M3 role mapping the R55.D.4 hello-listbox first-client
/// landed at Round 577; identical thumb geometry the R55.D.1 closed-
/// form helper produces; identical absolute-position substrate the
/// R55.D.6 [[absolute-position-via-layoutstyle]] memory pinned. The
/// only R660 delta vs R659 is the conditional thumb fill — the track
/// fill, geometry, and layout stay bit-identical when
/// `interaction == ScrollBarState::Idle`.
///
/// # Panics
///
/// Never panics: the [`u32::try_from`] guards saturate negative
/// `max_y` to `0` (matches the "nothing to scroll" branch), and the
/// [`saturating_add`](u32::saturating_add) prevents content-extent
/// overflow on pathological 4 GB scroll content (`u32::MAX` viewport
/// + max).
#[must_use]
pub fn view_vertical_scrollbar(
    scroll_state: &Rc<ScrollState>,
    theme: &Theme,
    style: &VerticalScrollbarStyle,
    interaction: ScrollBarState,
) -> Scene {
    let (_, offset_y) = scroll_state.offset();
    let (_, max_y) = scroll_state.max();
    // (R1032 §5.45) Track is laid out at its real pixel height; the
    // thumb ratio is derived from the content-unit viewport extent,
    // which equals the pixel height for pixel-unit consumers and the
    // visible *row* count for a row-unit consumer (terminal / text
    // grid). [`scrollbar_thumb_rect`] consumes `viewport_extent` /
    // `content_extent` / `offset` only as ratios, so the content unit
    // cancels against the pixel `track_rect` height.
    let track_rect = Rect::new(0, 0, style.gutter_w, style.viewport_h);
    let viewport_extent = style.content_viewport();
    // [`ScrollState::max`] returns `content_extent − viewport_extent`,
    // so `content_extent = viewport_extent + max` (in the ScrollState's
    // own unit). First-frame `max_y == 0` → thumb fills the track
    // ("nothing to scroll"); R55.G.24
    // [[signal-batch-atomic-multi-axis-update]] writes the real max on
    // the next paint, the view fn re-runs, the thumb snaps to size.
    let max_y_nonneg = u32::try_from(max_y.max(0)).unwrap_or(0);
    let content_extent = viewport_extent.saturating_add(max_y_nonneg);
    let scroll_offset = u32::try_from(offset_y.max(0)).unwrap_or(0);
    let geom = scrollbar_thumb_rect(
        ScrollBarOrientation::Vertical,
        track_rect,
        viewport_extent,
        content_extent,
        scroll_offset,
        style.min_thumb,
    );

    // `thumb.y - track.y` is the offset of the thumb's top edge from
    // the track origin. Fed into [`LayoutStyle::with_absolute_position`]
    // (R55.D.6 [[absolute-position-via-layoutstyle]]) so the thumb
    // lands at `(0, thumb_y_offset)` inside the track without a flex
    // helper.
    let thumb_y_offset = geom.thumb.y.saturating_sub(geom.track.y);
    let thumb_h = geom.thumb.h;

    let thumb = Scene::Container(
        ContainerNode::new(vec![])
            .with_style(
                BoxStyle::filled(thumb_fill_for_state(theme, interaction, style.thumb_role))
                    .with_corner_radius(2),
            )
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(style.gutter_w, thumb_h))
                    .with_absolute_position(0, thumb_y_offset),
            ),
    );

    Scene::Container(
        ContainerNode::new(vec![thumb])
            .with_tag(style.tag)
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerHighest),
            ))
            .with_layout(LayoutStyle::new().with_size(Size::px(style.gutter_w, style.viewport_h))),
    )
}

#[cfg(test)]
mod tests {
    //! R659 §5.45 — scrollbar lift unit tests. Pin the four invariants
    //! the hello-listbox + todomvc consumers share through the lifted
    //! helper:
    //!
    //! 1. **Paint shape**: outer track Container carries `tag` + 1
    //!    child (thumb), thumb has `absolute_position` set.
    //! 2. **First-frame max == 0**: thumb fills the full track
    //!    (`absolute_position == (0, 0)` + `size.h == viewport_h`).
    //! 3. **Mid-scroll max != 0**: thumb partial extent + offset
    //!    matches the R55.D.1 closed-form derivation.
    //! 4. **Theme role mapping**: track fill = `SurfaceContainerHighest`;
    //!    thumb fill = `Outline`. Tested against both Light + Dark
    //!    palettes so a future palette tweak doesn't regress the role
    //!    pointer.

    use super::{VerticalScrollbarStyle, thumb_fill_for_state, view_vertical_scrollbar};
    use pinion_core::reactive::Owner;
    use pinion_core::scene::Scene;
    use pinion_core::style::Size;
    use pinion_core::theme::{ColorRole, Theme};
    use pinion_core::widgets::scroll::use_scroll_state;
    use pinion_core::widgets::scrollbar::ScrollBarState;
    use std::rc::Rc;

    const TEST_TAG: &str = "test_scrollbar";
    const TEST_VIEWPORT_H: u32 = 200;

    fn run<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    #[test]
    fn r659_material_default_sets_8px_gutter_24px_floor() {
        let s = VerticalScrollbarStyle::material(TEST_VIEWPORT_H, TEST_TAG);
        assert_eq!(s.viewport_h, TEST_VIEWPORT_H);
        assert_eq!(s.gutter_w, 8);
        assert_eq!(s.min_thumb, 24);
        assert_eq!(s.tag, TEST_TAG);
    }

    #[test]
    fn r659_with_gutter_w_overrides() {
        let s = VerticalScrollbarStyle::material(TEST_VIEWPORT_H, TEST_TAG).with_gutter_w(12);
        assert_eq!(s.gutter_w, 12);
    }

    #[test]
    fn r659_with_min_thumb_overrides() {
        let s = VerticalScrollbarStyle::material(TEST_VIEWPORT_H, TEST_TAG).with_min_thumb(48);
        assert_eq!(s.min_thumb, 48);
    }

    #[test]
    fn r659_first_frame_thumb_fills_track_when_max_is_zero() {
        run(|| {
            let scroll_state = use_scroll_state("r659_scrollbar_test_a");
            // Fresh state: offset=0 max=0 → "nothing to scroll" branch
            // → thumb fills the track.
            let theme = Theme::light();
            let style = VerticalScrollbarStyle::material(TEST_VIEWPORT_H, TEST_TAG);
            let scene =
                view_vertical_scrollbar(&scroll_state, &theme, &style, ScrollBarState::Idle);

            let Scene::Container(track) = scene else {
                panic!("top-level must be a track Container");
            };
            assert_eq!(track.tag.as_deref(), Some(TEST_TAG));
            assert_eq!(track.children.len(), 1, "track has only the thumb child");
            assert_eq!(
                track.layout.size,
                Size::px(style.gutter_w, style.viewport_h)
            );

            let Scene::Container(thumb) = &track.children[0] else {
                panic!("thumb must be a Container");
            };
            let (left, top) = thumb
                .layout
                .absolute_position
                .expect("thumb declares absolute_position (R55.D.6)");
            assert_eq!(left, 0, "thumb hugs left edge of track");
            assert_eq!(top, 0, "max=0 → thumb at track origin");
            assert_eq!(
                thumb.layout.size.height,
                Size::px(style.gutter_w, style.viewport_h).height,
                "max=0 → thumb extends full track height",
            );
        });
    }

    #[test]
    fn r659_mid_scroll_thumb_partial_extent() {
        run(|| {
            let scroll_state = use_scroll_state("r659_scrollbar_test_b");
            // Declare a max bound + offset so the thumb shrinks +
            // shifts. content = viewport + max = 200 + 800 = 1000;
            // thumb_extent = floor(200 * 200 / 1000) = 40, clamped to
            // min_thumb = 40 (already >= 24). Offset 200 of 800 →
            // thumb_pos = (200 - 40) * 200 / 800 = 40.
            scroll_state.set_max(0, 800);
            scroll_state.scroll_to(0, 200);

            let theme = Theme::light();
            let style = VerticalScrollbarStyle::material(TEST_VIEWPORT_H, TEST_TAG);
            let scene =
                view_vertical_scrollbar(&scroll_state, &theme, &style, ScrollBarState::Idle);

            let Scene::Container(track) = scene else {
                panic!("top-level Container expected");
            };
            let Scene::Container(thumb) = &track.children[0] else {
                panic!("thumb Container expected");
            };
            let (_, thumb_top) = thumb
                .layout
                .absolute_position
                .expect("thumb absolute_position set");
            // R659 — closed-form: thumb_extent=40, scroll_max=800,
            // offset=200 → thumb_top = (200-40) * 200 / 800 = 40
            assert_eq!(thumb_top, 40, "thumb shifted down to mid-scroll position");
        });
    }

    #[test]
    fn r659_track_fill_uses_surface_container_highest_role() {
        run(|| {
            let scroll_state = use_scroll_state("r659_scrollbar_test_c");
            let style = VerticalScrollbarStyle::material(TEST_VIEWPORT_H, TEST_TAG);
            for theme in [Theme::light(), Theme::dark()] {
                let scene =
                    view_vertical_scrollbar(&scroll_state, &theme, &style, ScrollBarState::Idle);
                let Scene::Container(track) = &scene else {
                    panic!("track Container");
                };
                assert_eq!(
                    track.style.fill, theme.surface_container_highest,
                    "track fill = SurfaceContainerHighest (M3 canonical, both palettes)",
                );
            }
        });
    }

    #[test]
    fn r659_thumb_fill_uses_outline_role() {
        run(|| {
            let scroll_state = use_scroll_state("r659_scrollbar_test_d");
            let style = VerticalScrollbarStyle::material(TEST_VIEWPORT_H, TEST_TAG);
            for theme in [Theme::light(), Theme::dark()] {
                let scene =
                    view_vertical_scrollbar(&scroll_state, &theme, &style, ScrollBarState::Idle);
                let Scene::Container(track) = &scene else {
                    panic!("track");
                };
                let Scene::Container(thumb) = &track.children[0] else {
                    panic!("thumb");
                };
                assert_eq!(
                    thumb.style.fill, theme.outline,
                    "thumb fill = Outline (M3 canonical hairline / divider weight)",
                );
            }
        });
    }

    #[test]
    fn r659_custom_gutter_propagates_to_track_size_and_thumb_width() {
        run(|| {
            let scroll_state = use_scroll_state("r659_scrollbar_test_e");
            let style =
                VerticalScrollbarStyle::material(TEST_VIEWPORT_H, TEST_TAG).with_gutter_w(12);
            let scene = view_vertical_scrollbar(
                &scroll_state,
                &Theme::light(),
                &style,
                ScrollBarState::Idle,
            );
            let Scene::Container(track) = &scene else {
                panic!("track");
            };
            assert_eq!(track.layout.size, Size::px(12, TEST_VIEWPORT_H));
            let Scene::Container(thumb) = &track.children[0] else {
                panic!("thumb");
            };
            assert_eq!(
                thumb.layout.size.width,
                Size::px(12, 0).width,
                "thumb width matches custom gutter",
            );
        });
    }

    #[test]
    fn r659_helper_accepts_rc_scroll_state_by_reference() {
        // Substrate signature contract: `&Rc<ScrollState>` so callers
        // can pass a cached state without cloning the Rc just to feed
        // the helper. Compiles iff signature stays this way.
        run(|| {
            let scroll_state: Rc<pinion_core::widgets::scroll::ScrollState> =
                use_scroll_state("r659_scrollbar_test_f");
            let style = VerticalScrollbarStyle::material(TEST_VIEWPORT_H, TEST_TAG);
            let _ = view_vertical_scrollbar(
                &scroll_state,
                &Theme::light(),
                &style,
                ScrollBarState::Idle,
            );
            // After the call, `scroll_state` is still owned by the
            // caller — no move / consume.
            let _again = view_vertical_scrollbar(
                &scroll_state,
                &Theme::dark(),
                &style,
                ScrollBarState::Idle,
            );
        });
    }

    // R1032 §5.45 — track pixel height vs content viewport extent split.
    // A row-unit consumer (terminal / text grid whose ScrollState counts
    // scrollback rows) keeps `viewport_h` as the real track pixel height
    // and declares the visible ROW count through `with_viewport_extent`.
    // The thumb ratio then resolves to the correct pixels off the row
    // content scalars, decoupled from the px track height.

    #[test]
    fn r1032_viewport_extent_defaults_to_track_height_else_override() {
        // material() alone: content viewport == track pixel height — the
        // pixel-unit case the pre-R1032 consumers rely on (unchanged).
        let px = VerticalScrollbarStyle::material(480, TEST_TAG);
        assert_eq!(px.viewport_extent, None);
        assert_eq!(px.content_viewport(), 480);
        // Row-unit override: track stays 480 px, content viewport = 24 rows.
        let rows = VerticalScrollbarStyle::material(480, TEST_TAG).with_viewport_extent(24);
        assert_eq!(rows.viewport_extent, Some(24));
        assert_eq!(
            rows.viewport_h, 480,
            "track pixel height untouched by the content-extent override",
        );
        assert_eq!(rows.content_viewport(), 24);
    }

    #[test]
    fn r1032_row_unit_thumb_sized_by_row_content_not_track_pixels() {
        run(|| {
            // Terminal-style row-unit ScrollState: offset_y / max_y count
            // scrollback ROWS while the track renders at 480 px. visible =
            // 24 rows, total = 120 rows → max_y = 96 rows, offset = 48 rows.
            let scroll = use_scroll_state("r1032_row_unit");
            scroll.set_max(0, 96); // rows
            scroll.scroll_to(0, 48); // rows
            let style = VerticalScrollbarStyle::material(480, TEST_TAG).with_viewport_extent(24);
            let scene =
                view_vertical_scrollbar(&scroll, &Theme::light(), &style, ScrollBarState::Idle);

            let Scene::Container(track) = scene else {
                panic!("track Container");
            };
            // Track laid out at its real pixel height, NOT the 24-row count.
            assert_eq!(
                track.layout.size,
                Size::px(8, 480),
                "track keeps pixel height"
            );

            let Scene::Container(thumb) = &track.children[0] else {
                panic!("thumb Container");
            };
            let (_, thumb_top) = thumb
                .layout
                .absolute_position
                .expect("thumb absolute_position set");
            // Closed-form with ROW ratios over a 480 px track:
            // content = 24 + 96 = 120; thumb_extent = 480 * 24/120 = 96 px;
            // scroll_max = 96; travel = 480 - 96 = 384; pos = 384 * 48/96 = 192.
            assert_eq!(
                thumb_top, 192,
                "thumb positioned by row ratio over the pixel track"
            );
            assert_eq!(
                thumb.layout.size.height,
                Size::px(8, 96).height,
                "thumb extent derived from the row content ratio",
            );
            // Contrast guard: had the helper kept treating viewport_h = 480
            // as the row viewport, content would be 480 + 96 = 576 → thumb
            // extent 400 px, position 40 px. Pinning 192 / 96 proves the
            // content-extent path consumes the row scalars, not the px height.
        });
    }

    // R660 §5.16 §5.45 — thumb state-layer overlay tint pin tests.
    // The Material 3 state-layer composition lifts the bare `Outline`
    // tint toward `OnSurface` (Hover / Dragging) or `SurfaceContainerHighest`
    // (Disabled). Pin RGB inequality (≠ Idle) for Hover / Dragging and
    // distinctness between Hover and Dragging so a future Color::lerp
    // regression cannot collapse them into one tint.

    #[test]
    fn r660_idle_thumb_fill_is_outline() {
        for theme in [Theme::light(), Theme::dark()] {
            let fill = thumb_fill_for_state(&theme, ScrollBarState::Idle, ColorRole::Outline);
            assert_eq!(
                fill,
                theme.resolve(ColorRole::Outline),
                "Idle = bare Outline (R55.D.4 carry behavior, both palettes)",
            );
        }
    }

    #[test]
    fn r660_hover_thumb_lifts_toward_on_surface() {
        for theme in [Theme::light(), Theme::dark()] {
            let idle = thumb_fill_for_state(&theme, ScrollBarState::Idle, ColorRole::Outline);
            let hover = thumb_fill_for_state(&theme, ScrollBarState::Hover, ColorRole::Outline);
            assert_ne!(hover, idle, "Hover tint must differ from Idle");
            // The hover overlay is lerped *toward* OnSurface at 0.08
            // alpha — verify the result is strictly between Idle and
            // OnSurface (linear-space midpoint shift).
            assert_ne!(
                hover,
                theme.resolve(ColorRole::OnSurface),
                "Hover != pure OnSurface (overlay alpha = 0.08, not 1.0)",
            );
        }
    }

    #[test]
    fn r660_dragging_thumb_lifts_more_than_hover() {
        for theme in [Theme::light(), Theme::dark()] {
            let hover = thumb_fill_for_state(&theme, ScrollBarState::Hover, ColorRole::Outline);
            let dragging =
                thumb_fill_for_state(&theme, ScrollBarState::Dragging, ColorRole::Outline);
            assert_ne!(
                dragging, hover,
                "Dragging tint must be visually distinct from Hover \
                 (M3: 0.16 vs 0.08 state-layer alpha)",
            );
        }
    }

    #[test]
    fn r660_disabled_thumb_fades_toward_track() {
        // Disabled fades the thumb toward the track tint
        // (SurfaceContainerHighest) at 0.62 alpha — M3 disabled token
        // = 38% visible (1 − 0.62 fade). Verify the result differs
        // from Idle AND moves toward the track.
        for theme in [Theme::light(), Theme::dark()] {
            let idle = thumb_fill_for_state(&theme, ScrollBarState::Idle, ColorRole::Outline);
            let disabled =
                thumb_fill_for_state(&theme, ScrollBarState::Disabled, ColorRole::Outline);
            assert_ne!(disabled, idle, "Disabled tint must differ from Idle");
        }
    }

    #[test]
    fn r660_view_threads_interaction_into_thumb_fill() {
        // End-to-end pin: view fn output thumb.style.fill matches
        // thumb_fill_for_state for the given state. Catches a future
        // refactor that bypasses the helper.
        run(|| {
            let scroll_state = use_scroll_state("r660_scrollbar_state_view_test");
            let style = VerticalScrollbarStyle::material(TEST_VIEWPORT_H, TEST_TAG);
            for state in [
                ScrollBarState::Idle,
                ScrollBarState::Hover,
                ScrollBarState::Dragging,
                ScrollBarState::Disabled,
            ] {
                let scene = view_vertical_scrollbar(&scroll_state, &Theme::light(), &style, state);
                let Scene::Container(track) = &scene else {
                    panic!("track");
                };
                let Scene::Container(thumb) = &track.children[0] else {
                    panic!("thumb");
                };
                assert_eq!(
                    thumb.style.fill,
                    thumb_fill_for_state(&Theme::light(), state, ColorRole::Outline),
                    "thumb.style.fill matches helper for state={state:?}",
                );
            }
        });
    }

    // R1046 §5.45 — VerticalScrollbarStyle::with_thumb_role: idle thumb
    // base-role override (default Outline). The always-visible row-unit
    // terminal consumer (PR-19) overrides to OnSurfaceMuted so the idle
    // thumb stays legible against the SurfaceContainerHighest track on
    // the light palette, where Outline is too low-contrast.

    #[test]
    fn r1046_material_default_thumb_role_is_outline() {
        let style = VerticalScrollbarStyle::material(480, TEST_TAG);
        assert_eq!(
            style.thumb_role,
            ColorRole::Outline,
            "default base role is the M3 Outline hairline (R660 carry)",
        );
        assert_eq!(
            style.with_thumb_role(ColorRole::OnSurfaceMuted).thumb_role,
            ColorRole::OnSurfaceMuted,
            "with_thumb_role swaps the base role",
        );
    }

    #[test]
    fn r1046_idle_fill_follows_the_override_base() {
        for theme in [Theme::light(), Theme::dark()] {
            let idle =
                thumb_fill_for_state(&theme, ScrollBarState::Idle, ColorRole::OnSurfaceMuted);
            assert_eq!(
                idle,
                theme.resolve(ColorRole::OnSurfaceMuted),
                "idle thumb paints the override base verbatim",
            );
            assert_ne!(
                idle,
                thumb_fill_for_state(&theme, ScrollBarState::Idle, ColorRole::Outline),
                "the override base differs from the default Outline idle",
            );
        }
    }

    #[test]
    fn r1046_override_base_still_lerps_for_hover_and_drag() {
        // The hover / dragging overlays must ride the *override* base,
        // not the default Outline — one role swap shifts the whole ramp.
        for theme in [Theme::light(), Theme::dark()] {
            let muted_idle =
                thumb_fill_for_state(&theme, ScrollBarState::Idle, ColorRole::OnSurfaceMuted);
            let muted_hover =
                thumb_fill_for_state(&theme, ScrollBarState::Hover, ColorRole::OnSurfaceMuted);
            let muted_drag =
                thumb_fill_for_state(&theme, ScrollBarState::Dragging, ColorRole::OnSurfaceMuted);
            assert_ne!(muted_hover, muted_idle, "hover lifts off the override base");
            assert_ne!(muted_drag, muted_hover, "drag lifts further than hover");
            assert_ne!(
                muted_hover,
                thumb_fill_for_state(&theme, ScrollBarState::Hover, ColorRole::Outline),
                "hover composition rides the override base, not Outline",
            );
        }
    }

    #[test]
    fn r1046_override_idle_out_contrasts_outline_on_light() {
        // The PR-19 visibility guarantee: on the light palette the
        // OnSurfaceMuted idle thumb stands clear of the
        // SurfaceContainerHighest track, where the default Outline thumb
        // nearly vanishes. Pin per-channel L1 distance to the track:
        // muted >> outline.
        let theme = Theme::light();
        let track = theme.resolve(ColorRole::SurfaceContainerHighest);
        let outline_idle = thumb_fill_for_state(&theme, ScrollBarState::Idle, ColorRole::Outline);
        let muted_idle =
            thumb_fill_for_state(&theme, ScrollBarState::Idle, ColorRole::OnSurfaceMuted);
        let dist = |a: pinion_core::style::Color, b: pinion_core::style::Color| {
            u32::from(a.r).abs_diff(u32::from(b.r))
                + u32::from(a.g).abs_diff(u32::from(b.g))
                + u32::from(a.b).abs_diff(u32::from(b.b))
        };
        let muted_dist = dist(muted_idle, track);
        let outline_dist = dist(outline_idle, track);
        assert!(
            muted_dist > outline_dist,
            "OnSurfaceMuted idle thumb must out-contrast the Outline default \
             against the track (muted={muted_dist} vs outline={outline_dist})",
        );
    }

    #[test]
    fn r1046_view_threads_thumb_role_into_idle_fill() {
        // End-to-end: with_thumb_role on the style reaches the painted
        // thumb fill through view_vertical_scrollbar.
        run(|| {
            let scroll_state = use_scroll_state("r1046_scrollbar_thumb_role_view");
            let style = VerticalScrollbarStyle::material(TEST_VIEWPORT_H, TEST_TAG)
                .with_thumb_role(ColorRole::OnSurfaceMuted);
            let scene = view_vertical_scrollbar(
                &scroll_state,
                &Theme::light(),
                &style,
                ScrollBarState::Idle,
            );
            let Scene::Container(track) = &scene else {
                panic!("track");
            };
            let Scene::Container(thumb) = &track.children[0] else {
                panic!("thumb");
            };
            assert_eq!(
                thumb.style.fill,
                Theme::light().resolve(ColorRole::OnSurfaceMuted),
                "view fn threads style.thumb_role into the idle thumb fill",
            );
        });
    }

    // R660 §5.45 — use_scrollbar_interaction Owner::cache singleton
    // contract. Same `tag` returns the same Rc handle; different
    // tags yield different handles. Mirrors the use_text_edit_state
    // / use_caret_blink convention.

    #[test]
    fn r660_use_scrollbar_interaction_returns_shared_handle_for_same_tag() {
        use pinion_core::widgets::scrollbar::use_scrollbar_interaction;
        run(|| {
            let a = use_scrollbar_interaction("r660_scrollbar_iact_share");
            let b = use_scrollbar_interaction("r660_scrollbar_iact_share");
            assert!(
                Rc::ptr_eq(&a, &b),
                "Same tag → same cached handle (Owner::cache singleton)",
            );
        });
    }

    #[test]
    fn r660_scrollbar_interaction_signal_roundtrip() {
        use pinion_core::widgets::scrollbar::ScrollBarInteractionSignal;
        let sig = ScrollBarInteractionSignal::new();
        assert_eq!(sig.get(), ScrollBarState::Idle, "fresh = Idle");
        sig.set(ScrollBarState::Hover);
        assert_eq!(sig.get(), ScrollBarState::Hover);
        sig.set(ScrollBarState::Dragging);
        assert_eq!(sig.get(), ScrollBarState::Dragging);
        sig.set(ScrollBarState::Disabled);
        assert_eq!(sig.get(), ScrollBarState::Disabled);
        sig.set(ScrollBarState::Idle);
        assert_eq!(sig.get(), ScrollBarState::Idle);
    }
}
