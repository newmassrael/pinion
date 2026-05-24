//! R657 §5.16 §5.38 — `TextField` paint composition helpers.
//!
//! Lifted from `examples/hello-textfield/src/main.rs` + the
//! `examples/todomvc` 2nd-consumer duplicate per the
//! [[abstraction-needs-second-consumer]] gate. Both bindings now
//! call [`view_field`] for the input visual + [`ime_caret_rect_for`]
//! for the IME platform bridge caret rect, keeping zero per-binding
//! duplication of the ~280 LOC composition body.
//!
//! ## What lives here
//!
//! - [`TextFieldStyle`] — sizing + alpha tuning struct. Default
//!   constructor [`TextFieldStyle::m3_filled`] mirrors the
//!   Material 3 `TextField` filled-variant specs the example
//!   gallery has been using.
//! - [`view_field`] — full `TextField` paint composition. Reads the
//!   reactive [`TextEditState`] / [`CaretBlink`] via Owner-cache
//!   hooks (so the same `(state, frame)` pure input always yields
//!   the same `Scene` *for the same reactive state*), shapes the
//!   text + caret + selection + preedit geometry against the
//!   shared [`LayoutCache`], composes the field container with the
//!   `ColorRole`-resolved fills + caret + selection band +
//!   preedit underline.
//! - [`ime_caret_rect_for`] — companion caret rect derivation for
//!   the platform IME bridge. Lock-step `LayoutCache` key with
//!   [`view_field`] (same `(effective_text, style, max_width)`
//!   tuple → same cache hit, zero extra shape pass).
//! - [`use_text_field_layout_cache`] — Owner-cache hook returning
//!   the shared `Rc<RefCell<LayoutCache>>` both helpers resolve
//!   through.
//! - [`text_field_state_name`] / [`parse_text_field_state`] —
//!   bidirectional SCXML state name <-> enum lookup the
//!   `read_state` + status-line paths both call.
//!
//! ## What does NOT live here
//!
//! - The root view-fn container (title + field + status [+ list
//!   for todomvc]) — binding-specific layout composition stays in
//!   the binding's view fn body. Helpers return only the field
//!   subtree.
//! - The `Self::State` shape + `Self::Event` enum + the SCXML
//!   plumbing (`create_external`, `read_state`, `apply_key`,
//!   `apply_composition`, `apply_middle_click`) — these stay
//!   binding-local because they wire `WidgetCore` trait surfaces.
//! - The clipboard hook (`use_clipboard`) — currently shared by
//!   both bindings but kept binding-local until 3rd consumer fires
//!   per the [[abstraction-needs-second-consumer]] discipline; the
//!   3-line `Owner::cache + ArboardClipboard fallback` pattern is
//!   thin enough to leave duplicated.

use std::cell::RefCell;
use std::rc::Rc;

use pinion_core::external::IntrospectValue;
use pinion_core::reactive::Owner;
use pinion_core::scene::{BoxNode, ContainerNode, Rect};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::TextFieldState;
use pinion_core::{Color, Scene};
use pinion_text::{caret_rect_for_byte_offset, CaretRect, LayoutCache};

/// (R657 §5.16) Owner-cache key for the shared
/// [`LayoutCache`](pinion_text::LayoutCache). The pre-lift bindings
/// each used `"hello_textfield.layout_cache"` (todomvc copied
/// verbatim from hello-textfield) — the lifted helper unifies on a
/// single framework-scoped key so the `LayoutCache` instance is shared
/// across every `TextField` paint in the same Owner sub-tree. The
/// cache is keyed internally by `(text, style, max_width)`, so
/// distinct `TextField` widgets paint distinct cache entries without
/// collision.
const LAYOUT_CACHE_KEY: &str = "pinion_widget_paint.text_field.layout_cache";

/// (R657 §5.16 §5.38) Material 3 `TextField` filled-variant tuning.
///
/// Each binding can override individual fields; default = M3
/// filled-text-field spec. The pre-lift bindings carried each value
/// as a `const u32` at module scope; lifting into one struct keeps
/// the M3 contract together and gives future variants (`m3_outlined`,
/// `dense`, `compact`) a place to land without breaking
/// [`view_field`]'s signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextFieldStyle {
    /// Field container width in logical pixels (default 360 = M3
    /// filled `TextField` standard width).
    pub field_w: u32,
    /// Field container height in logical pixels (default 40 = M3
    /// filled `TextField` standard height for the single-line
    /// variant; 56 for the multi-line variant once that axis lands).
    pub field_h: u32,
    /// Inner padding on all four sides in logical pixels (default
    /// 8 = M3 filled `TextField` content-padding).
    pub field_pad: u32,
    /// Corner radius in logical pixels (default 4 = M3 filled
    /// `TextField` shape token).
    pub field_corner: u32,
    /// Body font size in logical pixels (default 18 ≈ M3
    /// `body-medium`).
    pub font_size_px: u32,
    /// Caret rectangle width in logical pixels (default 2 — reads
    /// cleanly on integer-scaled 1.0× displays; Hi-DPI displays
    /// where AA softens single-pixel lines may drop to 1 px).
    pub caret_width: u32,
    /// Selection rect tint alpha (default 0xA0 ≈ 63 % opacity ≈
    /// macOS / Chrome system selection overlay weight).
    pub selection_alpha: u8,
    /// Preedit background tint alpha (default 0x40 ≈ 25 % opacity
    /// — fainter than selection so the IME composition segment
    /// reads as provisional).
    pub preedit_bg_alpha: u8,
    /// Preedit underline thickness in logical pixels (default 1 —
    /// the canonical IME underline shape).
    pub preedit_underline_thickness: u32,
}

impl TextFieldStyle {
    /// (R657 §5.16) Material 3 filled-`TextField` defaults. Mirrors
    /// the constants every pre-lift binding carried.
    #[must_use]
    pub const fn m3_filled() -> Self {
        Self {
            field_w: 360,
            field_h: 40,
            field_pad: 8,
            field_corner: 4,
            font_size_px: 18,
            caret_width: 2,
            selection_alpha: 0xA0,
            preedit_bg_alpha: 0x40,
            preedit_underline_thickness: 1,
        }
    }
}

impl Default for TextFieldStyle {
    fn default() -> Self {
        Self::m3_filled()
    }
}

/// (R657 §5.16) Owner-cache hook returning the shared
/// [`LayoutCache`](pinion_text::LayoutCache) the `TextField` helpers
/// shape against. The cache is keyed internally by
/// `(text, style, max_width)`, so distinct `TextField` widgets on the
/// same Owner sub-tree paint distinct entries without collision —
/// they share the parley `FontContext` initialization cost (the
/// expensive part) but get independent shaped layouts.
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope. Every
/// substrate invocation site (the view fn + `ime_caret_rect_for`)
/// runs inside the framework's `root_owner.run`, so this only fires
/// from raw unit tests that forget the `with_owner` helper.
#[must_use]
pub fn use_text_field_layout_cache() -> Rc<RefCell<LayoutCache>> {
    Owner::current()
        .expect("use_text_field_layout_cache requires an active Owner scope")
        .cache(LAYOUT_CACHE_KEY, || RefCell::new(LayoutCache::new()))
}

/// (R657 §5.16) Material 3 `TextField` text foreground — `OnSurface`
/// when enabled, `OnSurfaceMuted` when disabled. Lock-step between
/// [`view_field`] and [`ime_caret_rect_for`] so the shared
/// `LayoutCache` key matches across both call sites.
fn text_fg_for(theme: &Theme, interaction: TextFieldState) -> Color {
    if matches!(interaction, TextFieldState::Disabled) {
        theme.resolve(ColorRole::OnSurfaceMuted)
    } else {
        theme.resolve(ColorRole::OnSurface)
    }
}

/// (R657 §5.16) Material 3 `TextField` filled-variant container fill.
/// `SurfaceContainerHighest` (idle) lifts one tier to
/// `SurfaceContainerHigh` when focused/editing for an elevated
/// posture without a heavy border, and fades toward `Surface` at
/// 38 % when disabled per the M3 disabled-overlay convention.
fn field_fill_for(theme: &Theme, interaction: TextFieldState) -> Color {
    match interaction {
        TextFieldState::Idle => theme.resolve(ColorRole::SurfaceContainerHighest),
        TextFieldState::Focused | TextFieldState::Editing => {
            theme.resolve(ColorRole::SurfaceContainerHigh)
        }
        TextFieldState::Disabled => theme
            .resolve(ColorRole::SurfaceContainerHighest)
            .lerp(theme.resolve(ColorRole::Surface), 0.38),
    }
}

/// (R657 §5.16) Selection rect tint — semi-transparent
/// `ColorRole::Accent` overlay so the glyphs under the band stay
/// readable. The selection inherits the active-control hue per the
/// M3 caret-color convention.
fn selection_fill(theme: &Theme, alpha: u8) -> Color {
    let a = theme.resolve(ColorRole::Accent);
    Color::rgba(a.r, a.g, a.b, alpha)
}

/// (R657 §5.16) Preedit background tint — fainter Accent overlay
/// than [`selection_fill`] so the IME composition segment reads as
/// provisional. Companion role for [`preedit_underline`].
fn preedit_bg_fill(theme: &Theme, alpha: u8) -> Color {
    let a = theme.resolve(ColorRole::Accent);
    Color::rgba(a.r, a.g, a.b, alpha)
}

/// (R657 §5.16) Preedit underline color — opaque Accent. Mirrors
/// the M3 / canonical IME convention where the underline matches
/// the active control hue so caret + underline + selection all
/// share `ColorRole::Accent` (a palette swap re-stains the field's
/// interactive affordances coherently).
fn preedit_underline(theme: &Theme) -> Color {
    theme.resolve(ColorRole::Accent)
}

/// (R657 §5.16) Saturating cast from layout-space f32 to paint-space
/// u32. Negative values clamp to 0; out-of-range positives clamp to
/// `u32::MAX`; `NaN` / `Infinity` clamp to 0 (defensive — parley's
/// [`caret_rect_for_byte_offset`] is `finite`-guaranteed by the
/// R56.1.b.2 test battery, but the saturating-cast convention stays
/// the textbook narrowing seam per
/// [[r56-1-b-2-parley-f32-narrowing]]).
fn saturating_f32_to_u32(v: f32) -> u32 {
    #[allow(
        clippy::cast_precision_loss,
        reason = "u32::MAX -> f32 rounds to a single saturating ceiling"
    )]
    let ceiling = u32::MAX as f32;
    if !v.is_finite() || v < 0.0 {
        0
    } else if v >= ceiling {
        u32::MAX
    } else {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "guarded by is_finite / >=0 / < ceiling above"
        )]
        let out = v as u32;
        out
    }
}

/// (R657 §5.16 §5.38) Build the `TextField` paint Container — the
/// rectangular input visual with text + caret + selection band +
/// preedit underline overlays.
///
/// # Inputs
///
/// - `tag` — paint Container tag (matches the `WidgetCore::tag()`
///   the binding declares; the `InputRouter` hit-tests against this).
/// - `interaction` — current SCXML state (drives fg + fill).
/// - `caret_byte` — caret position from the cached state snapshot.
/// - `theme` — already-resolved palette (caller decides between
///   `theme()` and `theme_animated()` depending on whether the
///   R57.X.theme-fade cross-fade applies).
/// - `style` — sizing + alpha tuning (default = M3 filled).
/// - `aria_label` — accessibility name pinned to the container via
///   `with_aria_label` for the a11y enrich pass.
///
/// # Returns
///
/// A [`Scene::Container`] tagged `tag` with the M3 filled-`TextField`
/// visual. Binding view fns wrap this in their root container with
/// any surrounding chrome (title + status + list etc.).
#[must_use]
#[allow(clippy::too_many_lines, reason = "view-fn shape — sequential composition pass")]
pub fn view_field(
    tag: &'static str,
    interaction: TextFieldState,
    caret_byte: u32,
    theme: &Theme,
    style: &TextFieldStyle,
    aria_label: &str,
) -> Scene {
    let text_state = use_text_edit_state(tag);
    let blink = use_caret_blink(tag);

    // R56.2.f §5.38 §5.22 — preedit splice: the composed view of
    // "committed buffer + spliced preedit" the user sees during IME
    // composition. When no composition is active, effective_text ==
    // committed text and the range is None. Mirrors W3C
    // compositionupdate canonical caret-at-preedit-end semantics.
    let (effective_text, visual_caret_byte, preedit_byte_range) =
        text_state.splice_preedit(caret_byte as usize);

    let text_style = TextStyle::new()
        .with_size_px(style.font_size_px)
        .with_fg(text_fg_for(theme, interaction));

    // Shape once via the shared LayoutCache, derive caret +
    // selection + preedit pixel rects from the same Layout.
    let layout_cache = use_text_field_layout_cache();
    let selection_range = text_state.selection_range();
    let (caret_pixel_rect, selection_pixel, preedit_pixel) = {
        let mut cache = layout_cache.borrow_mut();
        let layout = cache.layout(effective_text.as_str(), &text_style, None);
        #[allow(
            clippy::cast_precision_loss,
            reason = "caret_width fits f32 losslessly (small u32)"
        )]
        let cw = style.caret_width as f32;
        let rect = caret_rect_for_byte_offset(layout, visual_caret_byte, cw);
        let height_floor = saturating_f32_to_u32(rect.height).max(style.font_size_px);
        let caret = (
            saturating_f32_to_u32(rect.x),
            saturating_f32_to_u32(rect.y),
            height_floor,
        );
        let selection = selection_range.map(|(start, end)| {
            let start_rect = caret_rect_for_byte_offset(layout, start, cw);
            let end_rect = caret_rect_for_byte_offset(layout, end, cw);
            let start_x = saturating_f32_to_u32(start_rect.x);
            let end_x = saturating_f32_to_u32(end_rect.x);
            let sel_y = saturating_f32_to_u32(start_rect.y);
            let sel_h = saturating_f32_to_u32(start_rect.height).max(style.font_size_px);
            (start_x, sel_y, end_x.saturating_sub(start_x), sel_h)
        });
        let preedit_p = preedit_byte_range.map(|(start, end)| {
            let start_rect = caret_rect_for_byte_offset(layout, start, cw);
            let end_rect = caret_rect_for_byte_offset(layout, end, cw);
            let start_x = saturating_f32_to_u32(start_rect.x);
            let end_x = saturating_f32_to_u32(end_rect.x);
            let pre_y = saturating_f32_to_u32(start_rect.y);
            let pre_h = saturating_f32_to_u32(start_rect.height).max(style.font_size_px);
            (start_x, pre_y, end_x.saturating_sub(start_x), pre_h)
        });
        (caret, selection, preedit_p)
    };
    let (caret_layout_x, caret_layout_y, caret_box_height) = caret_pixel_rect;

    let field_fill = field_fill_for(theme, interaction);

    // Text node — natural-flow child of the field container. Empty
    // text renders as a zero-width run so the caret still appears
    // at x=0 inside the padded field. During composition the
    // rendered text is the composed `effective_text`, not the raw
    // text_state.text() buffer.
    let text_node = Scene::Text(pinion_core::scene::TextNode::styled(
        effective_text.clone(),
        Rect::default(),
        text_style,
    ));

    // Caret painted only when focused/editing AND blink phase is
    // visible. R56.1.h ties blink's enabled gate to SCXML state, so
    // blink is always paused outside focused/editing posture.
    let caret_painted = matches!(
        interaction,
        TextFieldState::Focused | TextFieldState::Editing,
    ) && blink.visible();

    let mut field_children: Vec<Scene> = Vec::with_capacity(4);
    let pad = style.field_pad;

    // R56.1.f.3 §5.22 — selection rect paints BEFORE text_node so
    // glyphs render on top. Vello composites children in vector
    // order (later children paint atop earlier).
    if let Some((sel_x, sel_y, sel_w, sel_h)) = selection_pixel {
        if sel_w > 0 {
            let sel_left = pad.saturating_add(sel_x);
            let sel_top = pad.saturating_add(sel_y);
            let selection_box = Scene::Box(
                BoxNode::new(
                    Rect::default(),
                    BoxStyle::filled(selection_fill(theme, style.selection_alpha)),
                )
                .with_layout(
                    LayoutStyle::new()
                        .with_size(Size::px(sel_w, sel_h))
                        .with_absolute_position(sel_left, sel_top),
                ),
            );
            field_children.push(selection_box);
        }
    }

    // R56.1.g.3 §5.22 — preedit background tint paints BEFORE text
    // (same layering rule as selection band).
    if let Some((pre_x, pre_y, pre_w, pre_h)) = preedit_pixel {
        if pre_w > 0 {
            let pre_left = pad.saturating_add(pre_x);
            let pre_top = pad.saturating_add(pre_y);
            let preedit_bg = Scene::Box(
                BoxNode::new(
                    Rect::default(),
                    BoxStyle::filled(preedit_bg_fill(theme, style.preedit_bg_alpha)),
                )
                .with_layout(
                    LayoutStyle::new()
                        .with_size(Size::px(pre_w, pre_h))
                        .with_absolute_position(pre_left, pre_top),
                ),
            );
            field_children.push(preedit_bg);
        }
    }

    field_children.push(text_node);

    // R56.1.g.3 §5.22 — preedit underline paints AFTER the text
    // node so the line sits over the descender region.
    if let Some((pre_x, pre_y, pre_w, pre_h)) = preedit_pixel {
        if pre_w > 0 {
            let pre_left = pad.saturating_add(pre_x);
            let underline_top = pad
                .saturating_add(pre_y)
                .saturating_add(pre_h)
                .saturating_sub(style.preedit_underline_thickness);
            let underline = Scene::Box(
                BoxNode::new(Rect::default(), BoxStyle::filled(preedit_underline(theme)))
                    .with_layout(
                        LayoutStyle::new()
                            .with_size(Size::px(pre_w, style.preedit_underline_thickness))
                            .with_absolute_position(pre_left, underline_top),
                    ),
            );
            field_children.push(underline);
        }
    }

    if caret_painted {
        let caret_left = pad.saturating_add(caret_layout_x);
        let caret_top = pad.saturating_add(caret_layout_y);
        let caret_box = Scene::Box(
            BoxNode::new(
                Rect::default(),
                BoxStyle::filled(theme.resolve(ColorRole::Accent)),
            )
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(style.caret_width, caret_box_height))
                    .with_absolute_position(caret_left, caret_top),
            ),
        );
        field_children.push(caret_box);
    }

    Scene::Container(
        ContainerNode::new(field_children)
            .with_tag(tag.to_owned())
            // R51.69 §5.40 — explicit accessible-name (WAI-ARIA
            // `aria-label`) pinned at the field container so the
            // scene-walk name derivation in
            // `enrich_names_from_scene` populates the AccessNode's
            // `name` without a duplicate literal in `access_node`.
            .with_aria_label(aria_label.to_owned())
            .with_style(BoxStyle::filled(field_fill).with_corner_radius(style.field_corner))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(style.field_w, style.field_h))
                    .with_padding(Rect::new(
                        style.field_pad,
                        style.field_pad,
                        style.field_pad,
                        style.field_pad,
                    )),
            ),
    )
}

/// (R657 §5.16 §5.38) Build the IME platform-bridge caret rect.
///
/// Used by the binding's [`WidgetView::ime_caret_rect`] impl to
/// publish the caret position so the platform IME candidate window
/// (ibus-hangul, fcitx5-hangul, macOS Hangul, Microsoft IME)
/// positions next to the caret rather than at the default screen
/// corner.
///
/// Coordinate composition:
///
/// 1. **Field rect in window coords** — supplied by the caller from
///    `pinion_runtime::rect_for_tag(scene, tag)`. (The helper does
///    not import pinion-runtime — the caller handles the scene walk
///    so pinion-widget-paint stays free of pinion-runtime deps.)
/// 2. **Text origin within the field** — `style.field_pad` on both
///    axes.
/// 3. **Caret rect within the text layout** — same
///    `caret_rect_for_byte_offset` call [`view_field`] runs (cache
///    hit on the same shared `LayoutCache`, no re-shape) using the
///    visual caret byte (preedit-end during composition).
///
/// Sum (1) + (2) + (3) → window-coord caret rect; the shell hands
/// it to `Window::set_ime_cursor_area`.
///
/// The returned width carries `caret_local.width.max(1.0)` so an IME
/// that uses the popup anchor width never sees a zero-width rect;
/// height carries the `style.font_size_px` floor so the candidate
/// popup never collapses to a sliver when the layout's reported
/// `height` is short (matches the floor [`view_field`] applies for
/// the visible caret box).
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope (same
/// shape as [`view_field`] — only test paths can trigger).
#[must_use]
#[allow(
    clippy::similar_names,
    reason = "field_origin_x_f / field_origin_y_f mirror the field_rect.x / field_rect.y source"
)]
pub fn ime_caret_rect_for(
    tag: &'static str,
    interaction: TextFieldState,
    caret_byte: u32,
    field_rect: Rect,
    theme: &Theme,
    style: &TextFieldStyle,
) -> CaretRect {
    let text_state = use_text_edit_state(tag);
    let (effective_text, visual_caret_byte, _preedit_byte_range) =
        text_state.splice_preedit(caret_byte as usize);

    // Mirror view_field's text style so the (text, style, max_width)
    // LayoutCache key matches and the lookup is a same-frame hit.
    let text_style = TextStyle::new()
        .with_size_px(style.font_size_px)
        .with_fg(text_fg_for(theme, interaction));

    let layout_cache = use_text_field_layout_cache();
    let caret_local = {
        let mut cache = layout_cache.borrow_mut();
        let layout = cache.layout(effective_text.as_str(), &text_style, None);
        #[allow(
            clippy::cast_precision_loss,
            reason = "caret_width fits f32 losslessly (small u32)"
        )]
        let cw = style.caret_width as f32;
        caret_rect_for_byte_offset(layout, visual_caret_byte, cw)
    };

    #[allow(
        clippy::cast_precision_loss,
        reason = "field_rect.{x,y} are u32 viewport coords; window sizes never approach 2^24 logical px"
    )]
    let field_origin_x_f = field_rect.x as f32;
    #[allow(
        clippy::cast_precision_loss,
        reason = "field_rect.{x,y} are u32 viewport coords; window sizes never approach 2^24 logical px"
    )]
    let field_origin_y_f = field_rect.y as f32;
    #[allow(
        clippy::cast_precision_loss,
        reason = "field_pad + font_size_px are small u32 constants"
    )]
    let pad_f = style.field_pad as f32;
    #[allow(
        clippy::cast_precision_loss,
        reason = "font_size_px is a small u32 constant"
    )]
    let font_size_f = style.font_size_px as f32;
    CaretRect::new(
        field_origin_x_f + pad_f + caret_local.x,
        field_origin_y_f + pad_f + caret_local.y,
        caret_local.width.max(1.0),
        caret_local.height.max(font_size_f),
    )
}

/// (R657 §5.16) SCXML state name → enum. Defensive default
/// (`Idle`) on any unexpected token guards against a future SCXML
/// rename leaking a silent crash. Lifted from the pre-R657 binding
/// duplicate per the [[abstraction-needs-second-consumer]] gate.
#[must_use]
pub fn parse_text_field_state(name: &str) -> TextFieldState {
    match name {
        "Focused" => TextFieldState::Focused,
        "Editing" => TextFieldState::Editing,
        "Disabled" => TextFieldState::Disabled,
        _ => TextFieldState::Idle,
    }
}

/// (R657 §5.16) Inverse of [`parse_text_field_state`] — used by the
/// status-line + transition-log path. Could be auto-derived through
/// `WidgetStateName` (R643) but kept hand-written to stay
/// vendor/sce-rename-resistant per [[sce-priority-over-pinion]].
#[must_use]
pub const fn text_field_state_name(state: TextFieldState) -> &'static str {
    match state {
        TextFieldState::Idle => "Idle",
        TextFieldState::Focused => "Focused",
        TextFieldState::Editing => "Editing",
        TextFieldState::Disabled => "Disabled",
    }
}

/// (R657 §5.16) Convenience helper extracting `(TextFieldState,
/// u32)` from the scene-root External's introspect surface. Lifted
/// from the duplicate `read_state` body both bindings carried. The
/// `tag` argument matches the paint-side container tag (the
/// router's hit-test target), so the same identifier resolves the
/// External via `find_external_with_tag`.
///
/// Defensive defaults: returns `(Idle, 0)` when the External is
/// missing or its introspect is opted out — the shell's normal
/// init path will populate the live state on the next paint cycle.
#[must_use]
pub fn read_text_field_state(scene: &Scene, tag: &str) -> (TextFieldState, u32) {
    let Some(node) = scene.find_external_with_tag(tag) else {
        return (TextFieldState::Idle, 0);
    };
    let Some(intro) = node.handle.introspect() else {
        return (TextFieldState::Idle, 0);
    };
    let interaction = match intro.query("state") {
        Some(IntrospectValue::Text(name)) => parse_text_field_state(&name),
        _ => TextFieldState::Idle,
    };
    let caret = match intro.query("caret") {
        Some(IntrospectValue::Int(n)) => u32::try_from(n.max(0)).unwrap_or(u32::MAX),
        _ => 0,
    };
    (interaction, caret)
}

#[cfg(test)]
mod tests {
    //! R657 §5.16 §5.38 — first-consumer regression battery for the
    //! lifted `TextField` paint helpers. Pinned at the substrate level
    //! so a contract drift surfaces here before reaching either
    //! consumer binding.
    use super::*;
    use pinion_core::reactive::Owner;
    use pinion_core::theme::Theme;

    fn with_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    #[test]
    fn m3_filled_default_matches_pre_lift_constants() {
        // Lock in the M3 filled-variant defaults the pre-R657
        // bindings carried as individual `const u32`s.
        let s = TextFieldStyle::m3_filled();
        assert_eq!(s.field_w, 360);
        assert_eq!(s.field_h, 40);
        assert_eq!(s.field_pad, 8);
        assert_eq!(s.field_corner, 4);
        assert_eq!(s.font_size_px, 18);
        assert_eq!(s.caret_width, 2);
        assert_eq!(s.selection_alpha, 0xA0);
        assert_eq!(s.preedit_bg_alpha, 0x40);
        assert_eq!(s.preedit_underline_thickness, 1);
    }

    #[test]
    fn default_impl_matches_m3_filled() {
        assert_eq!(TextFieldStyle::default(), TextFieldStyle::m3_filled());
    }

    #[test]
    fn state_name_round_trips() {
        for s in [
            TextFieldState::Idle,
            TextFieldState::Focused,
            TextFieldState::Editing,
            TextFieldState::Disabled,
        ] {
            assert_eq!(parse_text_field_state(text_field_state_name(s)), s);
        }
    }

    #[test]
    fn parse_unknown_token_defaults_to_idle() {
        assert_eq!(parse_text_field_state("__bogus__"), TextFieldState::Idle);
        assert_eq!(parse_text_field_state(""), TextFieldState::Idle);
    }

    #[test]
    fn view_field_carries_tag() {
        with_owner(|| {
            let theme = Theme::light();
            let scene = view_field(
                "tf_test",
                TextFieldState::Idle,
                0,
                &theme,
                &TextFieldStyle::default(),
                "Test input",
            );
            assert!(scene.contains_tag("tf_test"));
        });
    }

    #[test]
    fn view_field_idle_omits_caret_box() {
        with_owner(|| {
            let theme = Theme::light();
            let scene = view_field(
                "tf_test",
                TextFieldState::Idle,
                0,
                &theme,
                &TextFieldStyle::default(),
                "Test input",
            );
            // Idle posture: blink paused, caret_painted false. Field
            // contains the text node only (no selection / no preedit
            // / no caret box).
            match scene {
                Scene::Container(c) => {
                    // Only the text node (1 child) at default fixture
                    // state.
                    assert_eq!(
                        c.children.len(),
                        1,
                        "Idle field has only the text node",
                    );
                }
                _ => panic!("view_field must return a Container"),
            }
        });
    }

    #[test]
    fn use_text_field_layout_cache_dedups_across_calls() {
        with_owner(|| {
            let a = use_text_field_layout_cache();
            let b = use_text_field_layout_cache();
            assert!(
                Rc::ptr_eq(&a, &b),
                "Owner::cache dedup — both calls return the same Rc",
            );
        });
    }
}
