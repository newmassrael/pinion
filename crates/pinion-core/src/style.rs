//! Styling primitives (§5.3 v0 schema lock, R20).
//!
//! Houses the typed style sidecars consumed by [`Scene`](crate::Scene)
//! variants per the §5.11 layered shape decision: each variant carries
//! its minimal payload (rect, content, etc.) plus a `*Style` sidecar
//! that names the rendering details. R21 builds these out incrementally
//! — slice 1 introduces [`Color`]; subsequent slices add `BoxStyle`,
//! `TextStyle`, `PathStyle`, `ImageStyle`, plus the `Border` / `Stroke`
//! / `Fit` / `Align` companions per §5.3.
//!
//! `taffy` flexbox/grid integration is explicitly carry-forward
//! (§5.3 R20 caveat) and *not* part of this module.

/// 8-bit-per-channel sRGB color with separate alpha.
///
/// v0 §5.3 lock: `Color { r, g, b, a: u8 }` is the typed replacement
/// for the previous raw `u32` ARGB literals carried in
/// [`BoxNode.fill`](crate::scene::BoxNode). Bit-exact compatibility
/// with the softbuffer-native `0xAARRGGBB` layout is preserved via
/// [`Color::from_argb`] / [`Color::to_argb`].
///
/// Future color-space extensions (HSL / LAB / sRGB-linear) lay on
/// top via `#[non_exhaustive]`-shape methods, not by changing this
/// in-memory representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Construct from raw channels in sRGB.
    #[must_use]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Construct from RGB triplet with fully-opaque alpha.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 0xff)
    }

    /// Decode a softbuffer-style `0xAARRGGBB` ARGB literal.
    ///
    /// The R17 / R18 `BoxNode.fill` field carried this exact layout
    /// (top byte = alpha, then R/G/B). Round-trip with [`Color::to_argb`].
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub const fn from_argb(argb: u32) -> Self {
        let a = ((argb >> 24) & 0xff) as u8;
        let r = ((argb >> 16) & 0xff) as u8;
        let g = ((argb >> 8) & 0xff) as u8;
        let b = (argb & 0xff) as u8;
        Self { r, g, b, a }
    }

    /// Encode back into the softbuffer-style `0xAARRGGBB` literal.
    #[must_use]
    pub const fn to_argb(self) -> u32 {
        ((self.a as u32) << 24)
            | ((self.r as u32) << 16)
            | ((self.g as u32) << 8)
            | (self.b as u32)
    }

    /// Fully-transparent (a=0) ARGB literal `0x0000_0000` decoded.
    /// Same bit layout as the previous default `BoxNode.fill = 0`.
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);

    /// R51.151 §5.28 — colorimetrically-correct linear interpolation
    /// between two [`Color`]s.
    ///
    /// `t = 0.0` returns `self`; `t = 1.0` returns `other`; intermediate
    /// values blend per-channel in **linear-light** space (sRGB EOTF
    /// decoded → blended → re-encoded). Linear-space blending matches
    /// the §5.28 spring solver path
    /// ([`Color::to_linear`](Self::to_linear) →
    /// [`Animatable::lerp`](crate::animation::Animatable::lerp) →
    /// [`Color::from_linear`](Self::from_linear)) so a fade animated
    /// through a spring renders identically to a snapshot-and-lerp.
    ///
    /// ## Inputs and clamping
    ///
    /// - `t` outside `[0.0, 1.0]` is clamped (no extrapolation — fade
    ///   semantics are "between these two visual states", not
    ///   "extrapolate past them"; over-shoots come from the spring
    ///   target re-tune, not from the lerp).
    /// - `t = NaN` is treated as `0.0` (returns `self`) — defensive
    ///   guard mirroring the R51.145
    ///   [`clamp_frame_dt`](crate::frame_pacing::clamp_frame_dt) NaN
    ///   policy so a degraded numerical input does not propagate
    ///   visible artifacts.
    ///
    /// ## Why linear-space (and not sRGB-space)
    ///
    /// Naive per-channel `u8` lerp in sRGB space produces noticeably
    /// darker mid-tones on a gradient (the canonical "muddy gray"
    /// artifact when fading red → green). Linear-space lerp matches
    /// what physical light blending does and what the §5.28 spring
    /// solver outputs internally. The cost is two sRGB
    /// encode/decode passes per call — negligible compared to the
    /// per-frame layout + render cost, and pinion's frame budget cap
    /// (§5.28 R33) is set with this overhead already accounted for.
    #[must_use]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
        let a = self.to_linear();
        let b = other.to_linear();
        Self::from_linear(<crate::animation::AnimVec4 as crate::animation::Animatable>::lerp(
            a, b, t,
        ))
    }

    /// Decode sRGB gamma-encoded channels into linear-light
    /// `[AnimVec4]` space for use with the spring solver (§5.28).
    ///
    /// Alpha is interpolated in linear (premultiplication is a
    /// downstream decision the caller makes after re-encoding). RGB
    /// uses the exact sRGB EOTF (IEC 61966-2-1):
    ///
    /// - `n = c / 255`
    /// - if `n ≤ 0.04045` → `L = n / 12.92`
    /// - else → `L = ((n + 0.055) / 1.055)^2.4`
    ///
    /// Inverse of [`Color::from_linear`].
    ///
    /// [`AnimVec4`]: crate::animation::AnimVec4
    #[must_use]
    pub fn to_linear(self) -> crate::animation::AnimVec4 {
        crate::animation::AnimVec4 {
            x: srgb_decode(self.r),
            y: srgb_decode(self.g),
            z: srgb_decode(self.b),
            w: f32::from(self.a) / 255.0,
        }
    }

    /// Encode a linear-light [`AnimVec4`] back into 8-bit sRGB
    /// channels. Components are clamped to `[0, 1]` before the
    /// inverse-EOTF; out-of-range produces a saturated channel
    /// rather than wrapping. Inverse of [`Color::to_linear`].
    ///
    /// [`AnimVec4`]: crate::animation::AnimVec4
    #[must_use]
    pub fn from_linear(v: crate::animation::AnimVec4) -> Self {
        Self::rgba(
            srgb_encode(v.x),
            srgb_encode(v.y),
            srgb_encode(v.z),
            linear_alpha_encode(v.w),
        )
    }
}

/// sRGB EOTF: 8-bit channel → linear-light `f32` in `[0, 1]`.
fn srgb_decode(c: u8) -> f32 {
    let n = f32::from(c) / 255.0;
    if n <= 0.040_45 {
        n / 12.92
    } else {
        ((n + 0.055) / 1.055).powf(2.4)
    }
}

/// sRGB inverse-EOTF: linear-light `f32` → 8-bit channel with clamp.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn srgb_encode(l: f32) -> u8 {
    let clamped = l.clamp(0.0, 1.0);
    let n = if clamped <= 0.003_130_8 {
        12.92 * clamped
    } else {
        1.055 * clamped.powf(1.0 / 2.4) - 0.055
    };
    (n.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Linear alpha encoder — alpha is interpolated directly in linear,
/// no gamma curve. Clamp + round to `u8`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn linear_alpha_encode(a: f32) -> u8 {
    (a.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod color_linear_tests {
    use super::*;
    use crate::animation::{AnimVec4, Animatable};

    #[test]
    fn srgb_round_trip_endpoints() {
        // 0 and 255 must round-trip exactly.
        let black = Color::rgba(0, 0, 0, 255);
        assert_eq!(Color::from_linear(black.to_linear()), black);
        let white = Color::rgba(255, 255, 255, 255);
        assert_eq!(Color::from_linear(white.to_linear()), white);
    }

    #[test]
    fn srgb_round_trip_midrange_close() {
        // Mid-range channels round-trip within ±1 unit due to
        // EOTF/inverse-EOTF float precision.
        for v in [32, 64, 128, 192, 200].iter().copied() {
            let c = Color::rgba(v, v, v, 255);
            let back = Color::from_linear(c.to_linear());
            assert!(
                back.r.abs_diff(v) <= 1,
                "channel {v} round-tripped to {back:?}",
            );
        }
    }

    #[test]
    fn srgb_dark_linear_region() {
        // Channels below ~0x0D fall in the linear segment (n ≤ 0.04045).
        let dark = srgb_decode(8);
        let manual_linear = (8.0 / 255.0) / 12.92;
        assert!(
            (dark - manual_linear).abs() < 1e-6,
            "dark segment expected {manual_linear}, got {dark}",
        );
    }

    #[test]
    fn alpha_linear_round_trip() {
        let c = Color::rgba(100, 150, 200, 64);
        let linear = c.to_linear();
        assert!((linear.w - 64.0 / 255.0).abs() < 1e-6);
        let back = Color::from_linear(linear);
        assert_eq!(back.a, 64);
    }

    #[test]
    fn lerp_in_linear_space_midpoint() {
        // Mid-grey in linear space is darker in sRGB than the
        // naive (255+0)/2 = 127. Lerping in linear must produce
        // a higher channel value when re-encoded.
        let black = Color::rgba(0, 0, 0, 255);
        let white = Color::rgba(255, 255, 255, 255);
        let mid_linear = AnimVec4::lerp(black.to_linear(), white.to_linear(), 0.5);
        let mid = Color::from_linear(mid_linear);
        // Naive sRGB midpoint is 127; perceptually-linear midpoint is
        // higher (~188 / 0xBC) — the whole point of doing animation
        // in linear space.
        assert!(
            mid.r > 180,
            "expected perceptual midpoint above 180, got {}",
            mid.r,
        );
    }

    #[test]
    fn from_linear_saturates_out_of_range() {
        let over = AnimVec4::new(2.0, -0.5, 1.5, 1.2);
        let c = Color::from_linear(over);
        assert_eq!(c, Color::rgba(255, 0, 255, 255));
    }

    // ─────────────────────────────────────────────────────────────
    // R51.151 §5.28 — Color::lerp tests.
    //
    // Verifies the colorimetrically-correct (linear-space) lerp:
    // endpoint identity, mid-tone perceptual lightness, clamping,
    // NaN guard, and grayscale parity (replaces the bespoke
    // `lerp_grayscale` in hello-button*/hello-button-tui).
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn lerp_at_zero_returns_self() {
        let a = Color::rgb(255, 0, 0);
        let b = Color::rgb(0, 255, 0);
        assert_eq!(a.lerp(b, 0.0), a);
    }

    #[test]
    fn lerp_at_one_returns_other() {
        let a = Color::rgb(255, 0, 0);
        let b = Color::rgb(0, 255, 0);
        assert_eq!(a.lerp(b, 1.0), b);
    }

    #[test]
    fn lerp_grayscale_midpoint_is_perceptually_centered() {
        // White → black at t=0.5 should be perceptually centered.
        // In linear space the midpoint encodes back to ~188 (0xBC)
        // sRGB rather than the naive 127 sRGB midpoint.
        let white = Color::rgb(255, 255, 255);
        let black = Color::rgb(0, 0, 0);
        let mid = white.lerp(black, 0.5);
        assert!(
            mid.r > 180,
            "perceptual mid expected > 180, got {}",
            mid.r,
        );
        assert_eq!(mid.r, mid.g);
        assert_eq!(mid.r, mid.b);
    }

    #[test]
    fn lerp_clamps_negative_t_to_zero() {
        let a = Color::rgb(100, 100, 100);
        let b = Color::rgb(200, 200, 200);
        assert_eq!(a.lerp(b, -1.0), a, "negative t clamps to 0 → self");
    }

    #[test]
    fn lerp_clamps_t_above_one() {
        let a = Color::rgb(100, 100, 100);
        let b = Color::rgb(200, 200, 200);
        assert_eq!(a.lerp(b, 2.0), b, "t > 1 clamps to 1 → other");
    }

    #[test]
    fn lerp_nan_t_returns_self() {
        // R51.151 + R51.145 NaN policy — degraded input must not
        // propagate to visible artifacts. NaN → 0 → self.
        let a = Color::rgb(100, 50, 0);
        let b = Color::rgb(0, 200, 255);
        assert_eq!(a.lerp(b, f32::NAN), a);
    }

    #[test]
    fn lerp_preserves_alpha_endpoint_at_zero_and_one() {
        let a = Color::rgba(100, 0, 0, 255);
        let b = Color::rgba(0, 0, 100, 128);
        assert_eq!(a.lerp(b, 0.0).a, 255);
        assert_eq!(a.lerp(b, 1.0).a, 128);
    }

    #[test]
    fn lerp_replaces_legacy_lerp_grayscale() {
        // Parity smoke test — the hello-button*/hello-button-tui
        // examples used a bespoke `lerp_grayscale(from, to, t)` that
        // did naive per-channel u8 interp. R51.151 redirects them to
        // `Color::lerp` (linear-space). The endpoint values are
        // identical; only mid-tones drift to the perceptually-
        // centered position (verified above).
        let from = Color::rgb(0xff, 0xff, 0xff);
        let to = Color::rgb(0xd0, 0xd0, 0xd0);
        assert_eq!(from.lerp(to, 0.0), from);
        assert_eq!(from.lerp(to, 1.0), to);
        // mid-tone moves above the naive sRGB midpoint (0xe7).
        let mid = from.lerp(to, 0.5);
        assert!(
            mid.r > 0xe7,
            "linear-space mid expected > 0xe7, got {:02x}",
            mid.r,
        );
    }
}

/// Where the border is drawn relative to the [`BoxNode`]'s `rect`.
/// R46.3.2 §5.3 — the legacy softbuffer paint helper drew the border
/// strips *inside* the rect bounds (a 4-strip approximation of CSS
/// `box-sizing: border-box`). The Vello `paint_adapter` reproduces
/// this via centered-stroke + width/2 inset, so the default stays
/// `Inside` for visual continuity. `Center` matches Vello's native
/// stroke semantics; `Outside` matches CSS `box-sizing: content-box`.
///
/// Each render backend (softbuffer, Vello, future thin-RHI) must
/// honour this enum so a tagged scene round-trips identically across
/// targets — placement is a Scene-level concern, not a renderer
/// implementation detail.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum BorderPlacement {
    /// Border drawn entirely inside `rect` bounds (CSS border-box
    /// equivalent). Default — preserves R20-vintage softbuffer
    /// behaviour where `paint_border` stripped 4 inset rects.
    #[default]
    Inside,
    /// Border centred on `rect` edges — Vello's native stroke
    /// geometry. Half the stroke width spills outside `rect`.
    Center,
    /// Border drawn entirely outside `rect` bounds (CSS
    /// content-box equivalent).
    Outside,
}

/// Border for a [`BoxNode`](crate::scene::BoxNode) — §5.3 R20 lock,
/// R46.3.2 added `placement`. `width: u32` is in pixels in the same
/// coordinate space as [`Rect`](crate::scene::Rect).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Border {
    pub color: Color,
    pub width: u32,
    /// Where the border is drawn relative to the box's `rect`. R46.3.2
    /// promoted the implicit "Inside" choice (legacy softbuffer paint
    /// helper baked it into the stroke geometry) into an explicit
    /// scene-level field so different backends paint identically.
    pub placement: BorderPlacement,
}

impl Border {
    /// Construct a border from a color and pixel width. Placement
    /// defaults to [`BorderPlacement::Inside`] (R46.3.2 — legacy
    /// softbuffer-compatible).
    #[must_use]
    pub const fn new(color: Color, width: u32) -> Self {
        Self {
            color,
            width,
            placement: BorderPlacement::Inside,
        }
    }

    /// Builder: override the placement policy. R46.3.2 §5.3.
    #[must_use]
    pub const fn with_placement(mut self, placement: BorderPlacement) -> Self {
        self.placement = placement;
        self
    }
}

/// Sidecar style for [`BoxNode`](crate::scene::BoxNode) per the §5.11
/// "layered" decision (§5.3 R20 lock).
///
/// `Default` produces a fully-transparent box with no border —
/// drop-in compatible with the previous `BoxNode { fill: 0, .. }`
/// shape.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct BoxStyle {
    pub fill: Color,
    pub border: Option<Border>,
    pub corner_radius: u32,
}

impl BoxStyle {
    /// Solid-fill `BoxStyle` with no border and no rounding.
    #[must_use]
    pub const fn filled(fill: Color) -> Self {
        Self {
            fill,
            border: None,
            corner_radius: 0,
        }
    }

    /// Builder: attach a border.
    #[must_use]
    pub const fn with_border(mut self, border: Border) -> Self {
        self.border = Some(border);
        self
    }

    /// Builder: set the corner radius in pixels.
    #[must_use]
    pub const fn with_corner_radius(mut self, radius: u32) -> Self {
        self.corner_radius = radius;
        self
    }
}

/// CSS / OpenType font-weight axis value (§5.36 R47.5 Figma-fidelity).
///
/// Newtype over `u16` for CSS-style integer values in `[1, 1000]`. The
/// 11 named constants (`THIN`..`EXTRA_BLACK`) cover the common variable
/// font instances; other values are accepted for variable axis tuning.
/// fontique's `FontWeight` is `f32` for `wght` axis fidelity — pinion
/// keeps `u16` so the value participates in `Hash` / `Eq` (cache key
/// stability), then `pinion-text` widens to `f32` at the parley wire
/// in R47.6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontWeight(pub u16);

impl FontWeight {
    /// Weight 100 — Thin / Hairline.
    pub const THIN: Self = Self(100);
    /// Weight 200 — Extra Light / Ultra Light.
    pub const EXTRA_LIGHT: Self = Self(200);
    /// Weight 300 — Light.
    pub const LIGHT: Self = Self(300);
    /// Weight 350 — Semi Light.
    pub const SEMI_LIGHT: Self = Self(350);
    /// Weight 400 — Regular / Normal (default).
    pub const NORMAL: Self = Self(400);
    /// Weight 500 — Medium.
    pub const MEDIUM: Self = Self(500);
    /// Weight 600 — Semi Bold / Demi Bold.
    pub const SEMI_BOLD: Self = Self(600);
    /// Weight 700 — Bold.
    pub const BOLD: Self = Self(700);
    /// Weight 800 — Extra Bold / Ultra Bold.
    pub const EXTRA_BOLD: Self = Self(800);
    /// Weight 900 — Black / Heavy.
    pub const BLACK: Self = Self(900);
    /// Weight 950 — Extra Black / Ultra Black.
    pub const EXTRA_BLACK: Self = Self(950);
}

impl Default for FontWeight {
    fn default() -> Self {
        Self::NORMAL
    }
}

/// CSS / OpenType font-style axis value (§5.36 R47.5 Figma-fidelity).
///
/// Mirrors fontique's `FontStyle` with one simplification: the oblique
/// angle (when supplied) is `Option<i16>` degrees rather than `f32`, so
/// the enum stays `Hash + Eq` — required by `LayoutCache::LayoutKey`.
/// `None` inside `Oblique` means "let the font default" (parley reads
/// `slnt` axis); `Some(deg)` pins a custom slant. R47.6 widens to
/// `f32` at the parley wire.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontStyle {
    /// Upright / "roman" form. Default.
    #[default]
    Normal,
    /// Slanted style with its own glyph forms (semi-cursive history).
    Italic,
    /// Algorithmically-slanted upright glyphs; degrees CCW from vertical.
    Oblique(Option<i16>),
}

/// Line-height policy (§5.36 R47.5 Figma-fidelity).
///
/// `Normal` defers to the font's preferred line height (parley
/// `MetricsRelative(1.0)` equivalent). `Px` pins absolute pixels;
/// `MultiplierX100` is a CSS-style unitless number × 100 fixed point
/// (e.g. `MultiplierX100(150)` = 1.5× font size). Fixed point keeps
/// the enum `Hash + Eq`; R47.6 widens to `f32` at the parley wire.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LineHeight {
    /// Use the font's preferred line height (= parley `MetricsRelative(1.0)`).
    #[default]
    Normal,
    /// Absolute line height in CSS pixels.
    Px(u32),
    /// Multiplier of font size, in 1/100 units (e.g. `150` = `1.5×`).
    MultiplierX100(u16),
}

/// Inline text alignment along the writing-mode main axis (§5.36 R47.5).
///
/// `Start` / `End` resolve to left / right in LTR text (and reverse in
/// RTL). `Center` centres each line. `Justify` distributes inter-word
/// space to fill the line — meaningful only with multi-line layout.
/// Maps to `parley::Alignment` at the R47.6 wire (`paint_text` honour
/// + `LayoutCache::shape` alignment argument).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextAlign {
    /// Writing-mode start (default — left in LTR, right in RTL).
    #[default]
    Start,
    /// Centre each line.
    Center,
    /// Writing-mode end (right in LTR, left in RTL).
    End,
    /// Distribute inter-word space to fill the line (multi-line only).
    Justify,
}

/// Inline text decoration (§5.36 R47.5 Figma-fidelity).
///
/// Both `underline` and `strikethrough` may be `true` simultaneously
/// (Figma allows this combination). R47.6 wires each into parley as
/// `StyleProperty::Underline(bool)` + `StyleProperty::Strikethrough(bool)`;
/// offset / brush per-decoration tuning is R47.x carry.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TextDecoration {
    pub underline: bool,
    pub strikethrough: bool,
}

impl TextDecoration {
    /// All-off (default — no decoration). `const`-fn for zero-cost
    /// composition in const contexts.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            underline: false,
            strikethrough: false,
        }
    }

    /// Both `underline` and `strikethrough` enabled — Figma allows
    /// this combination.
    #[must_use]
    pub const fn both() -> Self {
        Self {
            underline: true,
            strikethrough: true,
        }
    }

    /// Set only `underline = true`.
    #[must_use]
    pub const fn underline() -> Self {
        Self {
            underline: true,
            strikethrough: false,
        }
    }

    /// Set only `strikethrough = true`.
    #[must_use]
    pub const fn strikethrough() -> Self {
        Self {
            underline: false,
            strikethrough: true,
        }
    }

    /// Builder: toggle the underline flag.
    #[must_use]
    pub const fn with_underline(mut self, on: bool) -> Self {
        self.underline = on;
        self
    }

    /// Builder: toggle the strikethrough flag.
    #[must_use]
    pub const fn with_strikethrough(mut self, on: bool) -> Self {
        self.strikethrough = on;
        self
    }
}

/// Behaviour when text content exceeds the layout box (§5.36 R47.5).
///
/// `Visible` (default) — glyphs render beyond the rect. `Clip` — paint
/// adapter scissors against the box edge. `Ellipsis` — parley
/// truncates the last line and appends "…". R47.6 wires `Clip` /
/// `Ellipsis` at the `paint_text` + parley line-break interaction.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextOverflow {
    /// Paint glyphs beyond `rect` edge (default — legacy R47.3 behaviour).
    #[default]
    Visible,
    /// Scissor paint to `rect` edge.
    Clip,
    /// Truncate to fit + append `…` on the last line.
    Ellipsis,
}

/// Sidecar style for [`TextNode`](crate::scene::TextNode) per §5.3 R20.
///
/// R47.5 §5.36 Figma-fidelity expansion: `font_weight`, `font_style`,
/// `line_height`, `letter_spacing`, `text_align`, `decoration`,
/// `overflow` join `font_family` / `font_size_px` / `fg_color` in the
/// schema. All new fields are `Hash + Eq` (integer-based) so the
/// `LayoutCache::LayoutKey` continues to deduplicate stable inputs;
/// any field change (including weight / line-height / alignment)
/// produces a fresh cache entry on the next shape pass.
///
/// `pinion-core` carries the schema only — no parley dependency. The
/// `pinion-text` crate wires each field into the corresponding
/// `parley::StyleProperty` / `parley::Alignment` at R47.6 (`paint_text`
/// + `LayoutCache::shape`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextStyle {
    pub font_family: Option<std::borrow::Cow<'static, str>>,
    pub font_size_px: u32,
    pub fg_color: Color,
    /// CSS `font-weight` (R47.5). Default = [`FontWeight::NORMAL`] (400).
    pub font_weight: FontWeight,
    /// CSS `font-style` (R47.5). Default = [`FontStyle::Normal`].
    pub font_style: FontStyle,
    /// CSS `line-height` (R47.5). Default = [`LineHeight::Normal`].
    pub line_height: LineHeight,
    /// CSS `letter-spacing` in px (signed) (R47.5). Default = `0`.
    pub letter_spacing: i32,
    /// CSS `text-align` (R47.5). Default = [`TextAlign::Start`].
    pub text_align: TextAlign,
    /// CSS `text-decoration` (R47.5). Default = both `false`.
    pub decoration: TextDecoration,
    /// CSS `text-overflow` (R47.5). Default = [`TextOverflow::Visible`].
    pub overflow: TextOverflow,
}

impl TextStyle {
    /// v0 default: system font, 16px, opaque black, Figma-fidelity
    /// fields all at their CSS defaults (Normal weight, Normal style,
    /// Normal line height, 0 letter-spacing, Start align, no
    /// decoration, Visible overflow).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            font_family: None,
            font_size_px: 16,
            fg_color: Color::rgb(0, 0, 0),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            line_height: LineHeight::Normal,
            letter_spacing: 0,
            text_align: TextAlign::Start,
            decoration: TextDecoration::none(),
            overflow: TextOverflow::Visible,
        }
    }

    /// Builder: override the font size in CSS pixels.
    #[must_use]
    pub const fn with_size_px(mut self, size: u32) -> Self {
        self.font_size_px = size;
        self
    }

    /// Builder: override the foreground color.
    #[must_use]
    pub const fn with_fg(mut self, color: Color) -> Self {
        self.fg_color = color;
        self
    }

    /// Builder: pin a font family (static or owned string).
    #[must_use]
    pub fn with_font_family(
        mut self,
        family: impl Into<std::borrow::Cow<'static, str>>,
    ) -> Self {
        self.font_family = Some(family.into());
        self
    }

    /// Builder: override the font weight (R47.5).
    #[must_use]
    pub const fn with_weight(mut self, weight: FontWeight) -> Self {
        self.font_weight = weight;
        self
    }

    /// Builder: override the font style (R47.5).
    #[must_use]
    pub const fn with_style(mut self, style: FontStyle) -> Self {
        self.font_style = style;
        self
    }

    /// Builder: override the line height (R47.5).
    #[must_use]
    pub const fn with_line_height(mut self, line_height: LineHeight) -> Self {
        self.line_height = line_height;
        self
    }

    /// Builder: override the letter-spacing (px, signed) (R47.5).
    #[must_use]
    pub const fn with_letter_spacing(mut self, px: i32) -> Self {
        self.letter_spacing = px;
        self
    }

    /// Builder: override the text alignment (R47.5).
    #[must_use]
    pub const fn with_align(mut self, align: TextAlign) -> Self {
        self.text_align = align;
        self
    }

    /// Builder: override the text decoration (R47.5).
    #[must_use]
    pub const fn with_decoration(mut self, decoration: TextDecoration) -> Self {
        self.decoration = decoration;
        self
    }

    /// Builder: override the overflow policy (R47.5).
    #[must_use]
    pub const fn with_overflow(mut self, overflow: TextOverflow) -> Self {
        self.overflow = overflow;
        self
    }
}

impl Default for TextStyle {
    fn default() -> Self {
        Self::new()
    }
}

/// Stroke line-cap style per §5.3 R20. v0 covers the three canonical
/// shapes; dash patterns and miter-join behaviour are carry-forward.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum StrokeCap {
    #[default]
    Butt,
    Round,
    Square,
}

/// Stroke description for [`PathNode`](crate::scene::PathNode). Width
/// is in pixels matching the [`Rect`](crate::scene::Rect) coordinate
/// space.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Stroke {
    pub color: Color,
    pub width: u32,
    pub cap: StrokeCap,
}

impl Stroke {
    /// Default stroke: given colour, given width, [`StrokeCap::Butt`].
    #[must_use]
    pub const fn new(color: Color, width: u32) -> Self {
        Self {
            color,
            width,
            cap: StrokeCap::Butt,
        }
    }

    /// Builder: override the cap style.
    #[must_use]
    pub const fn with_cap(mut self, cap: StrokeCap) -> Self {
        self.cap = cap;
        self
    }
}

/// Sidecar style for [`PathNode`](crate::scene::PathNode) per §5.3 R20.
/// Either the `stroke` or `fill` arm can be `None`; rasterizers must
/// gracefully ignore an empty style (no-op).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct PathStyle {
    pub stroke: Option<Stroke>,
    pub fill: Option<Color>,
}

impl PathStyle {
    /// Stroke-only style: `Stroke` present, no fill.
    #[must_use]
    pub const fn stroked(stroke: Stroke) -> Self {
        Self {
            stroke: Some(stroke),
            fill: None,
        }
    }

    /// Fill-only style: solid fill colour, no stroke.
    #[must_use]
    pub const fn filled(fill: Color) -> Self {
        Self {
            stroke: None,
            fill: Some(fill),
        }
    }
}

/// Image fit policy per §5.3 R20. Names follow the common CSS
/// `object-fit` vocabulary so authors with web background pick them
/// up without translation.
///
///   * `Fill` — stretch to exactly fill `rect`, ignoring aspect ratio.
///   * `Contain` — letter-box: largest centered image that fits inside
///     `rect`, preserving aspect.
///   * `Cover` — fill `rect` entirely, cropping overflow, preserving
///     aspect.
///   * `Tile` — repeat the image at its native size across `rect`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Fit {
    #[default]
    Fill,
    Contain,
    Cover,
    Tile,
}

/// Sidecar style for [`ImageNode`](crate::scene::ImageNode) per §5.3
/// R20. `tint` is an optional multiply-blend overlay (e.g. icon
/// recoloring); `None` paints the source as-is.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct ImageStyle {
    pub fit: Fit,
    pub tint: Option<Color>,
}

impl ImageStyle {
    /// Builder: override the fit policy.
    #[must_use]
    pub const fn with_fit(mut self, fit: Fit) -> Self {
        self.fit = fit;
        self
    }

    /// Builder: attach a tint overlay.
    #[must_use]
    pub const fn with_tint(mut self, tint: Color) -> Self {
        self.tint = Some(tint);
        self
    }
}

/// Nine-position alignment grid per §5.3 R20 Modifier expansion.
/// `TopLeft` is the default to match the top-left-origin coordinate
/// space pinion uses for [`Rect`](crate::scene::Rect).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Align {
    #[default]
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

// ---------------------------------------------------------------------------
// §5.21 R23/R24 layout system: LayoutStyle + flex enums.
// pinion-core stays free of any layout-engine dependency; pinion-runtime
// translates these types to taffy::Style at compute_layout time.
// ---------------------------------------------------------------------------

/// Top-level display mode per §5.21 R23.
/// `Block` (default) opts out of flex — node occupies its parent-given
/// rect as-is. `Flex` activates the §5.21 flex layout pass over the
/// children.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Display {
    #[default]
    Block,
    Flex,
}

/// Main-axis direction for flex children per §5.21.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum FlexDirection {
    #[default]
    Row,
    Column,
}

/// Main-axis distribution per §5.21.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum JustifyContent {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
}

/// Cross-axis alignment per §5.21.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum AlignItems {
    #[default]
    Stretch,
    Start,
    Center,
    End,
}

/// Length value for [`Size`] / `flex_basis` / etc. per §5.21.
///
/// `Auto` defers to taffy's intrinsic sizing (e.g. text measures its
/// own rasterized width); `Px(n)` pins a pixel size; `Percent(n)`
/// expresses a fraction of the parent container (0–100).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SizeValue {
    #[default]
    Auto,
    Px(u32),
    Percent(u8),
}

/// Width / height pair per §5.21.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub width: SizeValue,
    pub height: SizeValue,
}

impl Size {
    /// Fixed pixel width and height.
    #[must_use]
    pub const fn px(width: u32, height: u32) -> Self {
        Self {
            width: SizeValue::Px(width),
            height: SizeValue::Px(height),
        }
    }
}

/// Layout sidecar — companion to [`BoxStyle`] / [`TextStyle`] / etc.
///
/// Carries the flex + sizing information the §5.21 R23 layout pass
/// (`pinion-runtime::layout::compute_layout`) translates into taffy
/// style. Every Scene primitive (including the opaque `External`)
/// carries one; default is `Display::Block` which means "use the
/// rect I was given" — backward-compatible with R17 manual placement.
///
/// `padding` / `margin` reuse [`Rect`](crate::scene::Rect) as a
/// 4-inset (x=left, y=top, w=right, h=bottom) per the R20 §5.3
/// Modifier shape. Slice 4 of R24 absorbed them out of the standalone
/// Modifier struct directly into `LayoutStyle` so there is exactly one
/// sidecar driving the taffy pass.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LayoutStyle {
    pub display: Display,
    pub flex_direction: FlexDirection,
    pub justify_content: JustifyContent,
    pub align_items: AlignItems,
    pub gap: u32,
    pub size: Size,
    pub flex_grow: f32,
    pub padding: crate::scene::Rect,
    pub margin: crate::scene::Rect,
}

impl LayoutStyle {
    /// Identity layout: `Block`, auto sizing, no gap, zero insets.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            display: Display::Block,
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::Start,
            align_items: AlignItems::Stretch,
            gap: 0,
            size: Size {
                width: SizeValue::Auto,
                height: SizeValue::Auto,
            },
            flex_grow: 0.0,
            padding: crate::scene::Rect::new(0, 0, 0, 0),
            margin: crate::scene::Rect::new(0, 0, 0, 0),
        }
    }

    /// Builder: padding insets (x=left, y=top, w=right, h=bottom).
    #[must_use]
    pub const fn with_padding(mut self, insets: crate::scene::Rect) -> Self {
        self.padding = insets;
        self
    }

    /// Builder: margin insets (x=left, y=top, w=right, h=bottom).
    #[must_use]
    pub const fn with_margin(mut self, insets: crate::scene::Rect) -> Self {
        self.margin = insets;
        self
    }

    /// Builder: switch this node into flex mode (children are
    /// arranged along [`FlexDirection`]).
    #[must_use]
    pub const fn flex(mut self, direction: FlexDirection) -> Self {
        self.display = Display::Flex;
        self.flex_direction = direction;
        self
    }

    /// Builder: main-axis distribution.
    #[must_use]
    pub const fn with_justify(mut self, justify: JustifyContent) -> Self {
        self.justify_content = justify;
        self
    }

    /// Builder: cross-axis alignment.
    #[must_use]
    pub const fn with_align_items(mut self, align: AlignItems) -> Self {
        self.align_items = align;
        self
    }

    /// Builder: gap between children (pixels).
    #[must_use]
    pub const fn with_gap(mut self, gap: u32) -> Self {
        self.gap = gap;
        self
    }

    /// Builder: pin the node's size (overrides `Auto`).
    #[must_use]
    pub const fn with_size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Builder: flex-grow factor (`0.0` = don't expand, `1.0` =
    /// take remaining main-axis space).
    #[must_use]
    pub const fn with_flex_grow(mut self, grow: f32) -> Self {
        self.flex_grow = grow;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_constructor_preserves_channels() {
        let c = Color::rgba(0x12, 0x34, 0x56, 0x78);
        assert_eq!(c.r, 0x12);
        assert_eq!(c.g, 0x34);
        assert_eq!(c.b, 0x56);
        assert_eq!(c.a, 0x78);
    }

    #[test]
    fn rgb_helper_sets_opaque_alpha() {
        let c = Color::rgb(0xff, 0x00, 0x00);
        assert_eq!(c.a, 0xff);
    }

    #[test]
    fn from_argb_decodes_softbuffer_layout() {
        // 0xAARRGGBB → {r, g, b, a}
        let c = Color::from_argb(0x8012_3456);
        assert_eq!(c.a, 0x80);
        assert_eq!(c.r, 0x12);
        assert_eq!(c.g, 0x34);
        assert_eq!(c.b, 0x56);
    }

    #[test]
    fn to_argb_round_trips_with_from_argb() {
        let argb = 0xff_ab_cd_ef;
        let c = Color::from_argb(argb);
        assert_eq!(c.to_argb(), argb);
    }

    #[test]
    fn round_trip_a_full_sweep() {
        // Bit-exact compat across the previously-used hello-button
        // palette ensures no visual regression when call sites swap
        // raw u32 fills for typed Color.
        for argb in [0x0020_3040_u32, 0x00ff_ffff, 0x00d0_d0d0, 0x0050_5050, 0x00b0_2020] {
            let c = Color::from_argb(argb);
            assert_eq!(c.to_argb(), argb);
        }
    }

    #[test]
    fn transparent_round_trips_through_argb_zero() {
        assert_eq!(Color::TRANSPARENT.to_argb(), 0);
        assert_eq!(Color::from_argb(0), Color::TRANSPARENT);
    }

    #[test]
    fn default_is_transparent() {
        assert_eq!(Color::default(), Color::TRANSPARENT);
    }

    #[test]
    fn box_style_default_is_transparent_fill_no_border() {
        let s = BoxStyle::default();
        assert_eq!(s.fill, Color::TRANSPARENT);
        assert!(s.border.is_none());
        assert_eq!(s.corner_radius, 0);
    }

    #[test]
    fn box_style_filled_helper_sets_fill_only() {
        let s = BoxStyle::filled(Color::rgb(0x10, 0x20, 0x30));
        assert_eq!(s.fill, Color::rgb(0x10, 0x20, 0x30));
        assert!(s.border.is_none());
        assert_eq!(s.corner_radius, 0);
    }

    #[test]
    fn box_style_with_border_builder_attaches() {
        let s = BoxStyle::filled(Color::TRANSPARENT)
            .with_border(Border::new(Color::rgb(0xff, 0, 0), 2));
        let border = s.border.expect("border was attached");
        assert_eq!(border.color, Color::rgb(0xff, 0, 0));
        assert_eq!(border.width, 2);
    }

    #[test]
    fn border_default_placement_is_inside() {
        // R46.3.2 — `Border::new` defaults to the legacy softbuffer
        // "drawn inside the rect bounds" placement so existing code
        // (R20 vintage call sites, pinion-overlay highlights, etc.)
        // keeps its visual identity after the field addition.
        let b = Border::new(Color::rgb(0xff, 0, 0), 2);
        assert_eq!(b.placement, BorderPlacement::Inside);
    }

    #[test]
    fn border_with_placement_builder_overrides_default() {
        let b = Border::new(Color::rgb(0xff, 0, 0), 2)
            .with_placement(BorderPlacement::Outside);
        assert_eq!(b.placement, BorderPlacement::Outside);
    }

    #[test]
    fn border_placement_three_variants_distinct() {
        assert_ne!(BorderPlacement::Inside, BorderPlacement::Center);
        assert_ne!(BorderPlacement::Center, BorderPlacement::Outside);
        assert_ne!(BorderPlacement::Inside, BorderPlacement::Outside);
        // Default = Inside matches Default::default()
        assert_eq!(BorderPlacement::default(), BorderPlacement::Inside);
    }

    #[test]
    fn box_style_with_corner_radius_builder() {
        let s = BoxStyle::filled(Color::TRANSPARENT).with_corner_radius(8);
        assert_eq!(s.corner_radius, 8);
    }

    #[test]
    fn text_style_default_is_system_font_16px_black() {
        let s = TextStyle::default();
        assert!(s.font_family.is_none());
        assert_eq!(s.font_size_px, 16);
        assert_eq!(s.fg_color, Color::rgb(0, 0, 0));
        // R47.5 Figma-fidelity defaults — every new field at its CSS
        // default so that a freshly-constructed TextStyle behaves
        // identically to the pre-R47.5 shape.
        assert_eq!(s.font_weight, FontWeight::NORMAL);
        assert_eq!(s.font_style, FontStyle::Normal);
        assert_eq!(s.line_height, LineHeight::Normal);
        assert_eq!(s.letter_spacing, 0);
        assert_eq!(s.text_align, TextAlign::Start);
        assert_eq!(s.decoration, TextDecoration::none());
        assert_eq!(s.overflow, TextOverflow::Visible);
    }

    #[test]
    fn text_style_with_size_builder_overrides_default() {
        let s = TextStyle::new().with_size_px(24);
        assert_eq!(s.font_size_px, 24);
    }

    #[test]
    fn text_style_with_fg_builder_overrides_default() {
        let s = TextStyle::new().with_fg(Color::rgb(0xff, 0xff, 0xff));
        assert_eq!(s.fg_color, Color::rgb(0xff, 0xff, 0xff));
    }

    #[test]
    fn text_style_with_font_family_accepts_static_str() {
        let s = TextStyle::new().with_font_family("Inter");
        assert_eq!(s.font_family.as_deref(), Some("Inter"));
    }

    #[test]
    fn font_weight_named_constants_match_css_integers() {
        // The 11 named constants map to the CSS Fonts Level 4 integer
        // weights. Variable axis fidelity (350 / 950) is preserved.
        assert_eq!(FontWeight::THIN.0, 100);
        assert_eq!(FontWeight::EXTRA_LIGHT.0, 200);
        assert_eq!(FontWeight::LIGHT.0, 300);
        assert_eq!(FontWeight::SEMI_LIGHT.0, 350);
        assert_eq!(FontWeight::NORMAL.0, 400);
        assert_eq!(FontWeight::MEDIUM.0, 500);
        assert_eq!(FontWeight::SEMI_BOLD.0, 600);
        assert_eq!(FontWeight::BOLD.0, 700);
        assert_eq!(FontWeight::EXTRA_BOLD.0, 800);
        assert_eq!(FontWeight::BLACK.0, 900);
        assert_eq!(FontWeight::EXTRA_BLACK.0, 950);
        assert_eq!(FontWeight::default(), FontWeight::NORMAL);
    }

    #[test]
    fn text_style_with_weight_builder_overrides_default() {
        let s = TextStyle::new().with_weight(FontWeight::BOLD);
        assert_eq!(s.font_weight, FontWeight::BOLD);
    }

    #[test]
    fn text_style_with_style_builder_accepts_italic_and_oblique() {
        let s = TextStyle::new().with_style(FontStyle::Italic);
        assert_eq!(s.font_style, FontStyle::Italic);
        let o = TextStyle::new().with_style(FontStyle::Oblique(Some(12)));
        assert_eq!(o.font_style, FontStyle::Oblique(Some(12)));
        let auto = TextStyle::new().with_style(FontStyle::Oblique(None));
        assert_eq!(auto.font_style, FontStyle::Oblique(None));
    }

    #[test]
    fn text_style_with_line_height_variants_distinguish_for_hash() {
        // Each LineHeight variant is a distinct cache key (R47.5 →
        // R47.6 cache hit/miss boundary). Hash + Eq must separate them.
        use std::collections::HashMap;
        let mut m: HashMap<TextStyle, &'static str> = HashMap::new();
        m.insert(TextStyle::new().with_line_height(LineHeight::Normal), "normal");
        m.insert(TextStyle::new().with_line_height(LineHeight::Px(20)), "px20");
        m.insert(
            TextStyle::new().with_line_height(LineHeight::MultiplierX100(150)),
            "x1.5",
        );
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn text_style_with_letter_spacing_accepts_signed_values() {
        let s = TextStyle::new().with_letter_spacing(-2);
        assert_eq!(s.letter_spacing, -2);
        let s = TextStyle::new().with_letter_spacing(4);
        assert_eq!(s.letter_spacing, 4);
    }

    #[test]
    fn text_style_with_align_builder_overrides_default() {
        for a in [TextAlign::Start, TextAlign::Center, TextAlign::End, TextAlign::Justify] {
            let s = TextStyle::new().with_align(a);
            assert_eq!(s.text_align, a);
        }
    }

    #[test]
    fn text_decoration_combinations_distinguish_for_hash() {
        use std::collections::HashSet;
        let mut s = HashSet::new();
        s.insert(TextDecoration::none());
        s.insert(TextDecoration::underline());
        s.insert(TextDecoration::strikethrough());
        s.insert(TextDecoration::both());
        assert_eq!(s.len(), 4, "all 4 decoration combinations hash distinctly");
        // Builder composition matches the named constructors.
        let composed = TextDecoration::none().with_underline(true).with_strikethrough(true);
        assert_eq!(composed, TextDecoration::both());
    }

    #[test]
    fn text_style_with_overflow_builder_overrides_default() {
        for o in [TextOverflow::Visible, TextOverflow::Clip, TextOverflow::Ellipsis] {
            let s = TextStyle::new().with_overflow(o);
            assert_eq!(s.overflow, o);
        }
    }

    #[test]
    fn text_style_variant_styles_produce_distinct_hashes() {
        // R47.5 — different Figma-fidelity field values must produce
        // distinct cache keys so LayoutCache shapes them independently.
        // R47.6 wires each into parley; the cache-key distinction is
        // the prereq.
        use std::collections::HashSet;
        let mut s = HashSet::new();
        s.insert(TextStyle::new());
        s.insert(TextStyle::new().with_weight(FontWeight::BOLD));
        s.insert(TextStyle::new().with_style(FontStyle::Italic));
        s.insert(TextStyle::new().with_line_height(LineHeight::MultiplierX100(120)));
        s.insert(TextStyle::new().with_letter_spacing(2));
        s.insert(TextStyle::new().with_align(TextAlign::Center));
        s.insert(TextStyle::new().with_decoration(TextDecoration::underline()));
        s.insert(TextStyle::new().with_overflow(TextOverflow::Ellipsis));
        assert_eq!(
            s.len(),
            8,
            "every R47.5 axis variant must hash distinctly from default"
        );
    }

    #[test]
    fn stroke_default_is_butt_cap() {
        let s = Stroke::new(Color::rgb(0, 0, 0), 2);
        assert_eq!(s.cap, StrokeCap::Butt);
        assert_eq!(s.width, 2);
    }

    #[test]
    fn stroke_with_cap_builder() {
        let s = Stroke::new(Color::rgb(0, 0, 0), 1).with_cap(StrokeCap::Round);
        assert_eq!(s.cap, StrokeCap::Round);
    }

    #[test]
    fn path_style_stroked_helper() {
        let s = PathStyle::stroked(Stroke::new(Color::rgb(0xff, 0, 0), 3));
        assert!(s.stroke.is_some());
        assert!(s.fill.is_none());
    }

    #[test]
    fn path_style_filled_helper() {
        let s = PathStyle::filled(Color::rgb(0, 0xff, 0));
        assert_eq!(s.fill, Some(Color::rgb(0, 0xff, 0)));
        assert!(s.stroke.is_none());
    }

    #[test]
    fn path_style_default_is_empty() {
        let s = PathStyle::default();
        assert!(s.stroke.is_none());
        assert!(s.fill.is_none());
    }

    #[test]
    fn image_style_default_is_fill_no_tint() {
        let s = ImageStyle::default();
        assert_eq!(s.fit, Fit::Fill);
        assert!(s.tint.is_none());
    }

    #[test]
    fn image_style_with_fit_builder() {
        let s = ImageStyle::default().with_fit(Fit::Contain);
        assert_eq!(s.fit, Fit::Contain);
    }

    #[test]
    fn image_style_with_tint_builder() {
        let s = ImageStyle::default().with_tint(Color::rgb(0xff, 0, 0));
        assert_eq!(s.tint, Some(Color::rgb(0xff, 0, 0)));
    }

    #[test]
    fn align_default_is_top_left() {
        assert_eq!(Align::default(), Align::TopLeft);
    }

    #[test]
    fn layout_style_default_is_block_auto() {
        let l = LayoutStyle::default();
        assert_eq!(l.display, Display::Block);
        assert_eq!(l.flex_direction, FlexDirection::Row);
        assert_eq!(l.justify_content, JustifyContent::Start);
        assert_eq!(l.align_items, AlignItems::Stretch);
        assert_eq!(l.gap, 0);
        assert_eq!(l.size.width, SizeValue::Auto);
        assert_eq!(l.size.height, SizeValue::Auto);
        assert!((l.flex_grow - 0.0).abs() < f32::EPSILON);
        assert_eq!(l.padding, crate::scene::Rect::new(0, 0, 0, 0));
        assert_eq!(l.margin, crate::scene::Rect::new(0, 0, 0, 0));
    }

    #[test]
    fn layout_style_padding_margin_builders() {
        use crate::scene::Rect;
        let l = LayoutStyle::new()
            .with_padding(Rect::new(4, 8, 4, 8))
            .with_margin(Rect::new(2, 2, 2, 2));
        assert_eq!(l.padding, Rect::new(4, 8, 4, 8));
        assert_eq!(l.margin, Rect::new(2, 2, 2, 2));
    }

    #[test]
    fn layout_style_flex_builder_switches_display() {
        let l = LayoutStyle::new().flex(FlexDirection::Column);
        assert_eq!(l.display, Display::Flex);
        assert_eq!(l.flex_direction, FlexDirection::Column);
    }

    #[test]
    fn layout_style_chained_builders() {
        let l = LayoutStyle::new()
            .flex(FlexDirection::Row)
            .with_justify(JustifyContent::Center)
            .with_align_items(AlignItems::Center)
            .with_gap(8)
            .with_size(Size::px(320, 200))
            .with_flex_grow(1.0);
        assert_eq!(l.display, Display::Flex);
        assert_eq!(l.justify_content, JustifyContent::Center);
        assert_eq!(l.align_items, AlignItems::Center);
        assert_eq!(l.gap, 8);
        assert_eq!(l.size, Size::px(320, 200));
        assert!((l.flex_grow - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn size_px_helper_sets_both_dims() {
        let s = Size::px(160, 80);
        assert_eq!(s.width, SizeValue::Px(160));
        assert_eq!(s.height, SizeValue::Px(80));
    }

    #[test]
    fn align_nine_variants_distinct() {
        let all = [
            Align::TopLeft,
            Align::TopCenter,
            Align::TopRight,
            Align::CenterLeft,
            Align::Center,
            Align::CenterRight,
            Align::BottomLeft,
            Align::BottomCenter,
            Align::BottomRight,
        ];
        // Sanity: nine distinct values.
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }
}
