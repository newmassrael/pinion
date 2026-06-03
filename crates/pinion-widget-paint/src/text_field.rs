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
//! - [`read_text_field_state`] — extracts `(TextFieldState, caret)`
//!   from the scene-root External's introspect surface, routing the
//!   state-name lookup through the `WidgetStateName` SSOT (R698 §5.16).
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
use pinion_core::scene::{BoxNode, ContainerNode, Rect, ScrollNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::TextFieldState;
use pinion_core::{Color, CompositionEvent, Modifiers, Scene, WidgetStateName};
use pinion_text::{
    byte_offset_for_line_boundary, byte_offset_for_line_move, byte_offset_for_point,
    caret_rect_for_byte_offset, selection_rects_for_range, CaretRect, LayoutCache,
};

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
    /// R764 §5.22 — multi-line (textarea) mode. `false` (default) is
    /// the single-line filled `TextField`: the content is
    /// vertically *centred* in the 40 px field box (text origin ≈
    /// `field_pad`). `true` is the textarea: the content is
    /// top-aligned so the text block's origin is exactly
    /// `(field_pad, field_pad)`, matching the absolute-positioned
    /// caret / per-line selection bands (which anchor at
    /// `field_pad + layout_y`). The text already lays out across
    /// multiple visual lines whenever it carries `\n` (parley breaks
    /// on explicit newlines regardless of this flag) — the flag only
    /// governs vertical alignment so a tall box top-anchors instead
    /// of centring the whole block.
    pub multi_line: bool,
    /// R765 §5.22 §5.36 — soft-wrap the content at the field's inner
    /// width (`field_w - 2 * field_pad`). `false` (default for the
    /// single-line filled field) leaves the layout unbounded
    /// (`max_width = None`): the text lays out on one visual line per
    /// `\n` and an over-long line runs unwrapped past the box edge —
    /// the pre-R765 behaviour, byte-identical. `true` (default for
    /// [`Self::m3_multiline`]) bounds the parley layout so a line that
    /// exceeds the inner width breaks onto additional *visual* lines
    /// (the canonical textarea `wrap="soft"` model). The flag feeds the
    /// [`field_shaping`] SSOT, so paint, the caret rect, the pointer
    /// hit-test, and vertical caret navigation all shape against the
    /// *same* wrapped [`Layout`](pinion_text::Layout) — wrapped-line
    /// caret/selection/hit-test geometry stays consistent for free
    /// because parley resolves cursor moves and point hits over visual
    /// lines.
    pub soft_wrap: bool,
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
            multi_line: false,
            soft_wrap: false,
        }
    }

    /// R764 §5.22 — Material 3 multi-line (textarea) defaults: the
    /// filled-field tokens with `multi_line = true` (top-aligned
    /// content) and a `rows`-tall box. The height is
    /// `rows * line_height + 2 * field_pad` where `line_height` is
    /// derived from `font_size_px` with the M3 ~1.4 body line-height
    /// ratio, so the box is `rows` visual lines tall. R765 enables
    /// [`soft_wrap`](Self::soft_wrap) here, so an over-long line breaks
    /// onto more visual lines than there are `\n`s; when the wrapped
    /// content is taller than the `rows`-line box it is clipped to the
    /// box and scrolled vertically to keep the caret visible
    /// (R765 scroll-to-caret — see [`scroll_into_view`]). `rows`
    /// is clamped to `>= 1`.
    #[must_use]
    pub const fn m3_multiline(rows: u32) -> Self {
        let base = Self::m3_filled();
        // M3 body line-height ≈ 1.4 × font size (24 px for the 18 px
        // body-medium token); integer-rounded to keep the box a whole
        // number of logical pixels.
        let line_height = base.font_size_px * 7 / 5;
        let rows = if rows == 0 { 1 } else { rows };
        Self {
            field_h: rows * line_height + 2 * base.field_pad,
            multi_line: true,
            // A textarea soft-wraps at the inner width by default
            // (HTML `<textarea wrap="soft">`); an explicit no-wrap
            // (horizontal-scroll) variant would set this false once a
            // consumer needs it.
            soft_wrap: true,
            ..base
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

/// R762 §5.36 §5.38 — the spliced caret/preedit view + resolved text
/// style for a field's content, produced by [`field_shaping`].
struct FieldShaping {
    /// Committed buffer with the IME preedit spliced in at the caret
    /// (== committed text when no composition is active).
    effective_text: String,
    /// Caret byte within `effective_text` (preedit-end while composing).
    visual_caret_byte: usize,
    /// Byte range of the spliced preedit within `effective_text`.
    preedit_byte_range: Option<(usize, usize)>,
    /// Resolved text style — the style component of the `LayoutCache` key.
    text_style: TextStyle,
    /// R765 — the wrap width component of the `LayoutCache` key:
    /// `Some(field_w - 2 * field_pad)` when `style.soft_wrap`, else
    /// `None` (unbounded). Every consumer (paint / caret / hit-test /
    /// vertical move) passes this same value to `cache.layout` so they
    /// shape one identical wrapped `Layout`.
    max_width: Option<u32>,
}

/// R762 §5.36 §5.38 — **single source** for a text field's `LayoutCache`
/// key + spliced caret/preedit projection. The three places that shape a
/// field's content — paint ([`view_field`]), the forward caret rect
/// ([`ime_caret_rect_for`]), and the reverse pixel→byte hit-test
/// ([`byte_for_field_point`]) — MUST derive the *identical*
/// `(effective_text, text_style)` so they all hit one shared
/// [`Layout`](pinion_text::Layout). Any divergence misaligns the caret /
/// hit-test geometry with the painted glyphs (decode-must-match-encode,
/// R743.1) or silently re-shapes on a cache miss. This helper is that one
/// derivation; callers borrow [`use_text_field_layout_cache`] and call
/// `.layout(shaping.effective_text.as_str(), &shaping.text_style, None)`
/// themselves so each `Layout` borrow stays local to its use.
fn field_shaping(
    tag: &'static str,
    caret_byte: usize,
    interaction: TextFieldState,
    theme: &Theme,
    style: &TextFieldStyle,
) -> FieldShaping {
    let (effective_text, visual_caret_byte, preedit_byte_range) =
        use_text_edit_state(tag).splice_preedit(caret_byte);
    let text_style = TextStyle::new()
        .with_size_px(style.font_size_px)
        .with_fg(text_fg_for(theme, interaction));
    // R765 — soft-wrap bound: the inner content width (box minus both
    // pads). `field_w` is the field's explicit logical width, so the
    // wrap width is known at view-fn time (no layout-pass round-trip).
    // `saturating_sub` guards a degenerate `field_pad * 2 > field_w`
    // style from underflowing to a huge wrap width.
    let max_width = style
        .soft_wrap
        .then(|| style.field_w.saturating_sub(2 * style.field_pad));
    FieldShaping {
        effective_text,
        visual_caret_byte,
        preedit_byte_range,
        text_style,
        max_width,
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
        // Divergent: TextFieldState has no hover/pressed posture, only the
        // disabled fade — keep the custom arm but source the shared token.
        TextFieldState::Disabled => theme
            .resolve(ColorRole::SurfaceContainerHighest)
            .lerp(theme.resolve(ColorRole::Surface), crate::state_layer::DISABLED),
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

/// R765 §5.22 §5.45 — canonical **scroll-into-view**: the minimal new
/// vertical scroll offset that keeps the caret visible given the
/// *previous* offset. The caret window is `[prev, prev + viewport_h]`
/// (content-space px). The offset only moves when the caret leaves that
/// window — scroll up to `caret_top` if the caret is above it, down to
/// `caret_bottom - viewport_h` if below — and otherwise stays put. So
/// arrowing or clicking *within* the visible region never scrolls the
/// document (unlike a pin-to-bottom rule, which glues the caret to the
/// bottom edge and scrolls on every move). Result is clamped to
/// `[0, max_scroll]`.
///
/// This is a pure fixpoint (after scrolling the caret to an edge it sits
/// inside the window, so a second evaluation is a no-op), which is why
/// the paint view fn can run it against the [`ScrollState`] offset and
/// converge in at most one extra render. The *previous* offset is the
/// stored [`ScrollState`] Signal — the single source the hit-test and
/// IME helpers read back, so they agree with the painted scroll without
/// recomputation.
fn scroll_into_view(
    prev: u32,
    caret_top: u32,
    caret_bottom: u32,
    viewport_h: u32,
    max_scroll: u32,
) -> u32 {
    let new = if caret_top < prev {
        caret_top
    } else if caret_bottom > prev.saturating_add(viewport_h) {
        caret_bottom.saturating_sub(viewport_h)
    } else {
        prev
    };
    new.min(max_scroll)
}

/// R765 §5.45 — the stored vertical scroll offset for a multi-line
/// field, read back by the pointer hit-test and IME caret helpers so
/// they project through the *same* offset the paint applied. Single-line
/// fields never scroll (always 0), so they never touch the
/// [`ScrollState`] cache slot. The paint view fn is the sole writer
/// (via [`scroll_into_view`]); this is a pure read.
fn field_scroll_offset(tag: &'static str, style: &TextFieldStyle) -> u32 {
    if !style.multi_line {
        return 0;
    }
    #[allow(
        clippy::cast_sign_loss,
        reason = "ScrollState::offset_y is clamped to [0, max] so it is non-negative"
    )]
    let off = use_scroll_state(tag).offset_y().max(0) as u32;
    off
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

    // R762 — the (effective_text, text_style) cache-key derivation is
    // the `field_shaping` SSOT shared with `ime_caret_rect_for` +
    // `byte_for_field_point` so paint / caret / hit-test shape one
    // identical Layout. `effective_text` splices the IME preedit in at
    // the caret (== committed text when idle); `preedit_byte_range`
    // drives the composition underline.
    let FieldShaping {
        effective_text,
        visual_caret_byte,
        preedit_byte_range,
        text_style,
        max_width,
    } = field_shaping(tag, caret_byte as usize, interaction, theme, style);

    // Shape once via the shared LayoutCache, derive caret +
    // selection + preedit pixel rects from the same Layout.
    let layout_cache = use_text_field_layout_cache();
    let selection_range = text_state.selection_range();
    let (caret_pixel_rect, selection_pixel, preedit_pixel, content_h) = {
        let mut cache = layout_cache.borrow_mut();
        let layout = cache.layout(effective_text.as_str(), &text_style, max_width);
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
        // R764 §5.22 — per-line selection bands. `selection_rects_for_range`
        // (parley `Selection::geometry`) yields one rect per visual line,
        // so a multi-line selection paints a partial first line, full
        // middle lines, and partial last line. A single-line selection
        // collapses to one rect — bit-identical to the pre-R764 single
        // band (the same start_x..end_x on one line).
        let selection: Vec<(u32, u32, u32, u32)> = selection_range
            .map(|(start, end)| {
                selection_rects_for_range(layout, start, end)
                    .into_iter()
                    .map(|r| {
                        (
                            saturating_f32_to_u32(r.x),
                            saturating_f32_to_u32(r.y),
                            saturating_f32_to_u32(r.width),
                            saturating_f32_to_u32(r.height).max(style.font_size_px),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let preedit_p = preedit_byte_range.map(|(start, end)| {
            let start_rect = caret_rect_for_byte_offset(layout, start, cw);
            let end_rect = caret_rect_for_byte_offset(layout, end, cw);
            let start_x = saturating_f32_to_u32(start_rect.x);
            let end_x = saturating_f32_to_u32(end_rect.x);
            let pre_y = saturating_f32_to_u32(start_rect.y);
            let pre_h = saturating_f32_to_u32(start_rect.height).max(style.font_size_px);
            (start_x, pre_y, end_x.saturating_sub(start_x), pre_h)
        });
        (caret, selection, preedit_p, saturating_f32_to_u32(layout.height()))
    };
    let (caret_layout_x, caret_layout_y, caret_box_height) = caret_pixel_rect;

    // R765 §5.45 — scroll-into-view: keep the caret visible when wrapped
    // content overflows the box. The offset is STORED in the field's
    // `ScrollState` (the SSOT the hit-test + IME helpers read back), and
    // only moves when the caret leaves the visible window — the
    // canonical editor behaviour, not pin-to-bottom. Multi-line only;
    // single-line never scrolls (offset stays 0, no ScrollState touched).
    let scroll_y = if style.multi_line {
        let viewport_h = style.field_h.saturating_sub(2 * style.field_pad);
        let max_scroll = content_h.saturating_sub(viewport_h);
        let caret_bottom = caret_layout_y.saturating_add(caret_box_height);
        let scroll = use_scroll_state(tag);
        #[allow(
            clippy::cast_possible_wrap,
            reason = "max_scroll <= content_h; UI content height never approaches i32::MAX"
        )]
        let max_scroll_i = max_scroll as i32;
        scroll.set_max(0, max_scroll_i);
        #[allow(
            clippy::cast_sign_loss,
            reason = "offset_y is clamped to [0, max] so it is non-negative"
        )]
        let prev = scroll.offset_y().max(0) as u32;
        let new = scroll_into_view(prev, caret_layout_y, caret_bottom, viewport_h, max_scroll);
        if new != prev {
            #[allow(
                clippy::cast_possible_wrap,
                reason = "new <= max_scroll <= content_h; never approaches i32::MAX"
            )]
            let new_i = new as i32;
            scroll.scroll_to(0, new_i);
        }
        new
    } else {
        0
    };

    let field_fill = field_fill_for(theme, interaction);

    // Text node — natural-flow child of the field container. Empty
    // text renders as a zero-width run so the caret still appears
    // at x=0 inside the padded field. During composition the
    // rendered text is the composed `effective_text`, not the raw
    // text_state.text() buffer.
    //
    // R765 — soft-wrap must constrain the PAINTED text too, not just
    // the geometry layout. The painted `Scene::Text` is laid out
    // independently by taffy's measure callback, which only wraps when
    // it is handed a `Definite` width. Without an explicit width the
    // flex text child is probed as `MaxContent` (unbounded) and runs
    // off the right edge on one line, while the caret/selection
    // geometry (shaped at `max_width`) believes the text wrapped —
    // caret drops down, glyphs run right. Pinning the text node's
    // width to the same `max_width` makes taffy wrap the painted glyphs
    // at the identical break points, so paint and geometry agree.
    let mut text_node = pinion_core::scene::TextNode::styled(
        effective_text.clone(),
        Rect::default(),
        text_style,
    );
    if let Some(wrap_w) = max_width {
        text_node = text_node.with_layout(
            LayoutStyle::new().with_size(Size::auto().with_width(SizeValue::Px(wrap_w))),
        );
    }
    let text_node = Scene::Text(text_node);

    // Caret painted only when focused/editing AND blink phase is
    // visible. R56.1.h ties blink's enabled gate to SCXML state, so
    // blink is always paused outside focused/editing posture.
    let caret_painted = matches!(
        interaction,
        TextFieldState::Focused | TextFieldState::Editing,
    ) && blink.visible();

    let mut field_children: Vec<Scene> = Vec::with_capacity(4);
    let pad = style.field_pad;
    // R765 — single-line anchors its absolute overlays at `pad` within
    // the field's border box (the field's own padding positions the
    // flow text node at `pad`). Multi-line instead nests the content in
    // a `Scene::Scroll` whose viewport already sits at `pad` (the field
    // padding positions the Scroll), so the overlays inside that scroll
    // content anchor at 0 — the `pad` lives in the Scroll's placement,
    // not in each band's offset.
    let inner_pad = if style.multi_line { 0 } else { pad };

    // R56.1.f.3 §5.22 — selection rect paints BEFORE text_node so
    // glyphs render on top. Vello composites children in vector
    // order (later children paint atop earlier).
    for &(sel_x, sel_y, sel_w, sel_h) in &selection_pixel {
        if sel_w > 0 {
            let sel_left = inner_pad.saturating_add(sel_x);
            let sel_top = inner_pad.saturating_add(sel_y);
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
            let pre_left = inner_pad.saturating_add(pre_x);
            let pre_top = inner_pad.saturating_add(pre_y);
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
            let pre_left = inner_pad.saturating_add(pre_x);
            let underline_top = inner_pad
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
        let caret_left = inner_pad.saturating_add(caret_layout_x);
        let caret_top = inner_pad.saturating_add(caret_layout_y);
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

    // R765 §5.22 §5.45 — multi-line: clip the (possibly overflowing)
    // wrapped content to the inner box and scroll it vertically so the
    // caret stays visible. The `Scene::Scroll` viewport sits inside the
    // field's padding (the field padding positions it at `pad`); the
    // content nested in it carries the band/caret overlays anchored at
    // `inner_pad` (= 0). `scroll_y` (pure fn of caret + layout) shifts
    // the content up. Single-line keeps the flat child list — no Scroll,
    // byte-identical to pre-R765.
    let root_children: Vec<Scene> = if style.multi_line {
        let inner_w = style.field_w.saturating_sub(2 * style.field_pad);
        let inner_h = style.field_h.saturating_sub(2 * style.field_pad);
        let content = Scene::Container(
            ContainerNode::new(field_children).with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Start),
            ),
        );
        #[allow(
            clippy::cast_possible_wrap,
            reason = "scroll_y <= content_h; UI content height never approaches i32::MAX"
        )]
        let offset_y = scroll_y as i32;
        vec![Scene::Scroll(
            ScrollNode::new(Rect::new(0, 0, inner_w, inner_h), content)
                .with_offset(0, offset_y),
        )]
    } else {
        field_children
    };

    Scene::Container(
        ContainerNode::new(root_children)
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
                    // R764 §5.22 — single-line centres the content in the
                    // 40 px box; multi-line top-aligns so the text block's
                    // origin is (pad, pad), matching the absolute caret /
                    // per-line selection bands anchored at pad + layout_y.
                    .with_align_items(if style.multi_line {
                        AlignItems::Start
                    } else {
                        AlignItems::Center
                    })
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
    // R762 — shared `field_shaping` SSOT: identical (text, style) key as
    // `view_field` so this forward caret rect addresses the painted Layout.
    let FieldShaping {
        effective_text,
        visual_caret_byte,
        text_style,
        max_width,
        ..
    } = field_shaping(tag, caret_byte as usize, interaction, theme, style);

    let layout_cache = use_text_field_layout_cache();
    let caret_local = {
        let mut cache = layout_cache.borrow_mut();
        let layout = cache.layout(effective_text.as_str(), &text_style, max_width);
        #[allow(
            clippy::cast_precision_loss,
            reason = "caret_width fits f32 losslessly (small u32)"
        )]
        let cw = style.caret_width as f32;
        caret_rect_for_byte_offset(layout, visual_caret_byte, cw)
    };
    // R765 — subtract the same stored scroll offset the paint applied so
    // the platform IME caret rect tracks the on-screen caret when the
    // multi-line field has scrolled.
    #[allow(
        clippy::cast_precision_loss,
        reason = "scroll_y <= content_h; never approaches 2^24 logical px"
    )]
    let scroll_y_f = field_scroll_offset(tag, style) as f32;

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
        field_origin_y_f + pad_f + caret_local.y - scroll_y_f,
        caret_local.width.max(1.0),
        caret_local.height.max(font_size_f),
    )
}

/// R762 §5.36 §5.38 — **reverse** of [`ime_caret_rect_for`]: hit-test a
/// window-coord pointer point to the UTF-8 byte offset under it, for
/// click-to-position-caret. Subtracts the field origin + padding to
/// reach text-local layout-space, reshapes the *same* `(text, style,
/// width)` [`view_field`] used (same-frame `LayoutCache` hit, no extra
/// shape), and feeds [`byte_offset_for_point`].
///
/// `point_x` / `point_y` are window-local logical pixels (the
/// shell-side cursor position on press). The returned offset is a char
/// boundary (parley resolves to a cluster edge), safe to pass straight
/// into [`TextEditState::set_caret`](pinion_core::widgets::text_edit::TextEditState::set_caret).
/// A point left of / before the text clamps to 0, past the end clamps
/// to `text.len()` (parley `Cursor::from_point` is total).
///
/// Mirrors the IME forward helper exactly so the screen caret a click
/// lands on and the byte it resolves to stay one SSOT — the same
/// `splice_preedit` + style + cache the visible caret is built from.
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope (same shape as
/// [`ime_caret_rect_for`] / [`view_field`] — only test paths trigger).
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "field_rect.{x,y} + field_pad are small u32 viewport coords; never approach 2^24 logical px"
)]
pub fn byte_for_field_point(
    tag: &'static str,
    interaction: TextFieldState,
    point_x: f32,
    point_y: f32,
    field_rect: Rect,
    theme: &Theme,
    style: &TextFieldStyle,
) -> usize {
    // R762 — shared `field_shaping` SSOT: identical (text, style) key as
    // `view_field` so the hit-test resolves against the painted Layout
    // (the glyph a click lands on == the byte returned).
    let caret = use_text_edit_state(tag).caret();
    let FieldShaping {
        effective_text,
        text_style,
        max_width,
        ..
    } = field_shaping(tag, caret, interaction, theme, style);

    // window-coord point → text-local layout-space: subtract the field
    // origin and the text padding the forward helper added.
    let local_x = point_x - field_rect.x as f32 - style.field_pad as f32;
    let local_y = point_y - field_rect.y as f32 - style.field_pad as f32;

    // R765 — the content is scrolled up by `scroll_y` on screen, so a
    // window point maps to a content-space point `scroll_y` lower. Add it
    // back (the same stored offset paint applied via `ScrollState`)
    // before resolving the byte, so a click on a scrolled line lands on
    // the glyph under it.
    #[allow(
        clippy::cast_precision_loss,
        reason = "scroll_y <= content_h; never approaches 2^24 logical px"
    )]
    let scroll_y_f = field_scroll_offset(tag, style) as f32;
    let layout_cache = use_text_field_layout_cache();
    let mut cache = layout_cache.borrow_mut();
    let layout = cache.layout(effective_text.as_str(), &text_style, max_width);
    byte_offset_for_point(layout, local_x, local_y + scroll_y_f)
}

/// R764 §5.36 §5.22 / R766 — vertical caret navigation for a multi-line
/// field: resolve the byte offset after moving the caret `delta` visual
/// lines (`ArrowUp` = `-1`, `ArrowDown` = `+1`) from the field's current
/// caret, holding the persistent **goal column**. Reshapes the *same*
/// `(text, style)` [`view_field`] paints (same-frame `LayoutCache` hit)
/// and feeds [`byte_offset_for_line_move`].
///
/// Returns `(new_byte, goal_x)`. The binding writes `new_byte` to the
/// caret (`set_caret` / `set_selection`) and then **re-arms** the goal
/// with `goal_x` (the caret write itself cleared it), so the next move
/// in the run reuses the same target column — the caret returns to the
/// original column after crossing a short line instead of drifting. The
/// current goal is read from
/// [`TextEditState::goal_column`](pinion_core::widgets::text_edit::TextEditState::goal_column)
/// here, so the binding need not thread it back in.
///
/// Mirrors [`byte_for_field_point`] (the pointer hit-test) for the
/// keyboard vertical axis: both resolve a new caret byte against the
/// painted Layout via the `field_shaping` SSOT. The returned offset is a
/// char boundary, safe for `TextEditState::set_caret` / `set_selection`.
/// A multi-line binding calls this on ArrowUp/Down (which need the
/// layout geometry the `apply_key` path lacks).
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope (same shape as
/// [`byte_for_field_point`] — only test paths trigger).
#[must_use]
pub fn byte_for_field_vertical_move(
    tag: &'static str,
    interaction: TextFieldState,
    delta: isize,
    theme: &Theme,
    style: &TextFieldStyle,
) -> (usize, f32) {
    let edit = use_text_edit_state(tag);
    let caret = edit.caret();
    let goal = edit.goal_column();
    let FieldShaping {
        effective_text,
        text_style,
        visual_caret_byte,
        max_width,
        ..
    } = field_shaping(tag, caret, interaction, theme, style);
    let layout_cache = use_text_field_layout_cache();
    let mut cache = layout_cache.borrow_mut();
    let layout = cache.layout(effective_text.as_str(), &text_style, max_width);
    byte_offset_for_line_move(layout, visual_caret_byte, delta, goal)
}

/// R766 §5.36 §5.22 — visual line boundary (`Home` / `End`) for a
/// multi-line field: resolve the byte offset of the start (`end =
/// false`) or end (`end = true`) of the wrapped visual line the caret
/// sits on. Reshapes the *same* `(text, style)` [`view_field`] paints
/// and feeds [`byte_offset_for_line_boundary`].
///
/// "Visual" is the canonical multi-line `Home` / `End`: on a
/// soft-wrapped paragraph the caret jumps to the start / end of the
/// displayed row, not the whole hard-break-delimited line. The single-
/// line field keeps the geometry-free buffer-absolute
/// [`move_home`](pinion_core::widgets::text_edit::TextEditState::move_home)
/// / `move_end` (one visual line ⇒ identical result). Selection
/// extension (`Shift+Home` / `Shift+End`) is the binding's, exactly like
/// the vertical-move shift path: it feeds the returned offset to
/// `set_selection` against the retained anchor.
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope (same shape as
/// [`byte_for_field_point`] — only test paths trigger).
#[must_use]
pub fn byte_for_field_line_boundary(
    tag: &'static str,
    interaction: TextFieldState,
    end: bool,
    theme: &Theme,
    style: &TextFieldStyle,
) -> usize {
    let caret = use_text_edit_state(tag).caret();
    let FieldShaping {
        effective_text,
        text_style,
        visual_caret_byte,
        max_width,
        ..
    } = field_shaping(tag, caret, interaction, theme, style);
    let layout_cache = use_text_field_layout_cache();
    let mut cache = layout_cache.borrow_mut();
    let layout = cache.layout(effective_text.as_str(), &text_style, max_width);
    byte_offset_for_line_boundary(layout, visual_caret_byte, end)
}

/// R764.1 §5.38 §5.22 — forward a W3C `KeyboardEvent.key` to the
/// `TextField`-class External tagged `tag`, the SSOT every `TextField`
/// binding's `WidgetCore::apply_key` routes a recognised key through.
/// Pre-R764.1 this `find_external_with_tag_mut` then `introspect_mut`
/// then `invoke("key", …)` then match-`Bool` block was hand-rolled in 5
/// sites across 4 bindings (hello-textfield, hello-textarea, todomvc
/// `TF_TAG` and `EDIT_TF_TAG`, hello-combobox-editable `INPUT_TAG`).
///
/// Empty `modifiers` sends the bare [`IntrospectValue::Text`] wire shape
/// (the R56.1.d single-keystroke path); any held modifier sends the
/// R56.1.f.0 Json shape carrying the four W3C bits so `Shift+Arrow` /
/// `Ctrl+A` reach the substrate's modifier-aware selection arms.
///
/// Returns the External's recognition result (the W3C `defaultPrevented`
/// semantic the binding propagates from `apply_key`): `true` only on
/// `Ok(Bool(true))`, `false` on an unrecognised key or any non-`Bool`
/// shape (a substrate misconfiguration defers to the shell fallback
/// chain rather than silently swallowing the key). The binding keeps its
/// own `focused == Some(<my tag>)` roving-tabindex guard before calling.
#[must_use]
pub fn forward_key_to_field(
    scene: &mut Scene,
    tag: &str,
    key: &str,
    modifiers: Modifiers,
) -> bool {
    let Some(node) = scene.find_external_with_tag_mut(tag) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    let args = if modifiers == Modifiers::empty() {
        IntrospectValue::Text(key.to_owned())
    } else {
        IntrospectValue::Json(serde_json::json!({
            "key": key,
            "shift": modifiers.shift_key(),
            "ctrl": modifiers.control_key(),
            "alt": modifiers.alt_key(),
            "meta": modifiers.meta_key(),
        }))
    };
    matches!(intro.invoke("key", args), Ok(IntrospectValue::Bool(true)))
}

/// R764.1 §5.38 §5.13 — forward a platform [`CompositionEvent`] to the
/// `TextField`-class External tagged `tag`, the SSOT every `TextField`
/// binding's `WidgetView::apply_composition` routes through. Reformats
/// the typed enum into the R56.1.g.2 `invoke("composition", Json{action,
/// data?})` wire shape so the platform IME path (winit `WindowEvent::Ime`)
/// and the AI-client `scene/invoke` path land on one substrate funnel.
/// Pre-R764.1 this block was hand-rolled in 3 bindings (hello-textfield,
/// hello-textarea, todomvc).
///
/// Returns `true` when the invoke channel accepts the event; a future
/// `#[non_exhaustive]` `CompositionEvent` variant returns `false`
/// (defers to the shell fallback). The binding keeps its own focus
/// guard before calling.
#[must_use]
pub fn forward_composition_to_field(
    scene: &mut Scene,
    tag: &str,
    event: &CompositionEvent,
) -> bool {
    let Some(node) = scene.find_external_with_tag_mut(tag) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    let args = match event {
        CompositionEvent::Start => serde_json::json!({ "action": "start" }),
        CompositionEvent::Update(text) => {
            serde_json::json!({ "action": "update", "data": text })
        }
        CompositionEvent::Commit(text) => serde_json::json!({ "action": "end", "data": text }),
        CompositionEvent::Cancel => serde_json::json!({ "action": "cancel" }),
        // `CompositionEvent` is `#[non_exhaustive]`; a future variant
        // (delete_surrounding etc.) defers to the shell fallback.
        _ => return false,
    };
    intro.invoke("composition", IntrospectValue::Json(args)).is_ok()
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
    // R698 §5.16 — route through the `WidgetStateName` SSOT primitive
    // (R643) instead of a paint-side duplicate match table; unknown /
    // missing tokens still collapse to `Idle` via `from_name_or_default`.
    let interaction = match intro.query("state") {
        Some(IntrospectValue::Text(name)) => {
            TextFieldState::from_name_or_default(&name)
        }
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
        // R698 §5.16 — the round-trip now exercises the `WidgetStateName`
        // SSOT primitive (R643) that `read_text_field_state` routes through.
        for s in [
            TextFieldState::Idle,
            TextFieldState::Focused,
            TextFieldState::Editing,
            TextFieldState::Disabled,
        ] {
            assert_eq!(TextFieldState::from_name_or_default(s.as_name()), s);
        }
    }

    #[test]
    fn parse_unknown_token_defaults_to_idle() {
        assert_eq!(
            TextFieldState::from_name_or_default("__bogus__"),
            TextFieldState::Idle
        );
        assert_eq!(
            TextFieldState::from_name_or_default(""),
            TextFieldState::Idle
        );
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

    #[test]
    fn m3_filled_does_not_soft_wrap() {
        // R765 — the single-line filled field stays unbounded so its
        // LayoutCache key is byte-identical to the pre-R765 `None`.
        assert!(!TextFieldStyle::m3_filled().soft_wrap);
    }

    #[test]
    fn m3_multiline_soft_wraps() {
        // R765 — a textarea soft-wraps at the inner width by default.
        assert!(TextFieldStyle::m3_multiline(3).soft_wrap);
    }

    #[test]
    fn field_shaping_single_line_is_unbounded() {
        // R765 — soft_wrap = false threads `None` to every consumer's
        // `cache.layout` (the pre-R765 unbounded layout).
        with_owner(|| {
            let theme = Theme::light();
            let shaping = field_shaping(
                "tf_test",
                0,
                TextFieldState::Idle,
                &theme,
                &TextFieldStyle::m3_filled(),
            );
            assert_eq!(shaping.max_width, None);
        });
    }

    #[test]
    fn field_shaping_multiline_bounds_inner_width() {
        // R765 — the wrap width is the box minus both pads, known at
        // view-fn time from the explicit `field_w` (no layout pass).
        with_owner(|| {
            let theme = Theme::light();
            let style = TextFieldStyle::m3_multiline(3);
            let shaping = field_shaping(
                "ta_test",
                0,
                TextFieldState::Idle,
                &theme,
                &style,
            );
            assert_eq!(
                shaping.max_width,
                Some(style.field_w - 2 * style.field_pad),
            );
        });
    }

    #[test]
    fn scroll_into_view_only_scrolls_at_edges() {
        // R765 §5.45 — the canonical scroll-into-view contract. Window =
        // [prev, prev + viewport_h]; the offset moves only when the caret
        // leaves it, never pins the caret to an edge while inside.
        let vp = 125;
        // Caret fully inside the window -> offset unchanged (the fix for
        // the pin-to-bottom defect: arrowing within view does not scroll).
        assert_eq!(
            scroll_into_view(50, 60, 86, vp, 300),
            50,
            "caret inside the window leaves the offset untouched",
        );
        // Caret below the window -> scroll down just enough to reveal it.
        assert_eq!(
            scroll_into_view(50, 200, 226, vp, 300),
            226 - vp,
            "caret below scrolls to caret_bottom - viewport_h",
        );
        // Caret above the window -> scroll up to the caret top.
        assert_eq!(
            scroll_into_view(50, 10, 36, vp, 300),
            10,
            "caret above scrolls up to caret_top",
        );
        // Result is clamped to the content's max scroll.
        assert_eq!(
            scroll_into_view(0, 500, 526, vp, 100),
            100,
            "offset clamps to max_scroll",
        );
    }

    #[test]
    fn soft_wrap_breaks_long_line_into_visual_lines() {
        // R765 — the structural payoff: a long line with no `\n`
        // exceeds the 344 px inner width and parley breaks it onto
        // additional *visual* lines. The same content unbounded
        // (`None`) stays one visual line, proving the wrap — not a `\n`
        // — drives the break.
        with_owner(|| {
            let theme = Theme::light();
            let tag = "ta_wrap";
            let long = "the quick brown fox jumps over the lazy dog repeatedly today";
            use_text_edit_state(tag).insert(long);
            let style = TextFieldStyle::m3_multiline(3);
            let shaping = field_shaping(
                tag,
                long.len(),
                TextFieldState::Editing,
                &theme,
                &style,
            );
            let cache = use_text_field_layout_cache();
            let mut cache = cache.borrow_mut();
            let wrapped_lines = {
                let wrapped = cache.layout(
                    shaping.effective_text.as_str(),
                    &shaping.text_style,
                    shaping.max_width,
                );
                wrapped.lines().count()
            };
            assert!(
                wrapped_lines > 1,
                "soft-wrap breaks the long no-newline line: got {wrapped_lines}",
            );
            let flat_lines = {
                let flat = cache.layout(
                    shaping.effective_text.as_str(),
                    &shaping.text_style,
                    None,
                );
                flat.lines().count()
            };
            assert_eq!(
                flat_lines, 1,
                "unbounded layout keeps the no-newline line on one visual line",
            );
        });
    }
}
