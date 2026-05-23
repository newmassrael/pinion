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
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Hash, serde::Serialize, serde::Deserialize,
)]
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

    /// R615 §5.50 — parse a CSS Color Module Level 4 hex literal.
    ///
    /// Accepts the full canonical CSS spec — leading `#` required,
    /// hex digits case-insensitive, four shapes:
    ///
    /// - `#RGB` — 3 hex digits, each doubled to 8-bit (`#fff` →
    ///   `Color::rgb(0xff, 0xff, 0xff)`). Alpha implicit `0xff`.
    /// - `#RGBA` — 4 hex digits, last is alpha doubled (`#fff8` →
    ///   `Color::rgba(0xff, 0xff, 0xff, 0x88)`).
    /// - `#RRGGBB` — 6 hex digits. Alpha implicit `0xff`.
    /// - `#RRGGBBAA` — 8 hex digits, all four channels explicit.
    ///
    /// Returns [`None`] for any string that does not match one of
    /// the four shapes, contains a non-hex digit, or is missing the
    /// leading `#`.
    ///
    /// # Round-trip
    ///
    /// `Color::from_hex(c.to_hex().as_str()) == Some(c)` holds for
    /// every [`Color`] value — the writer emits the canonical
    /// 6-digit (or 8-digit if non-opaque) form, the reader accepts
    /// it. Round-trip property pinned by
    /// `r615_color_hex_round_trips_for_every_canonical_form`.
    ///
    /// # Why on the substrate
    ///
    /// CSS hex parsing is a framework primitive every theme-aware
    /// consumer needs — RPC wire (R608 `set_theme_palettes`), future
    /// CSS-loader / stylesheet binding, future `ThemeProvider` JSON
    /// import. Pre-R615 the parser lived RPC-side because
    /// [[abstraction-needs-second-consumer]] held it for the second
    /// consumer; R615 lifts on the textbook "framework primitive
    /// for industry-standard format" overrule — CSS Color Module
    /// Level 4 is the canonical spec, the substrate is the canonical
    /// home, and waiting for a second consumer to materialize before
    /// adding a 10-line primitive that the spec explicitly defines
    /// is the wrong trade-off direction.
    #[must_use]
    pub fn from_hex(input: &str) -> Option<Self> {
        let hex = input.strip_prefix('#')?;
        let parse_nibble = |range: std::ops::Range<usize>| -> Option<u8> {
            u8::from_str_radix(hex.get(range)?, 16).ok()
        };
        // The expand_nibble closure doubles a single hex digit into a
        // full byte (e.g. `0xf` → `0xff`) per the CSS Color Module
        // Level 4 shorthand expansion rule. Equivalent to
        // `(digit << 4) | digit`.
        let expand_nibble = |range: std::ops::Range<usize>| -> Option<u8> {
            let d = parse_nibble(range)?;
            Some((d << 4) | d)
        };
        let (red, green, blue, alpha) = match hex.len() {
            3 => (
                expand_nibble(0..1)?,
                expand_nibble(1..2)?,
                expand_nibble(2..3)?,
                0xff_u8,
            ),
            4 => (
                expand_nibble(0..1)?,
                expand_nibble(1..2)?,
                expand_nibble(2..3)?,
                expand_nibble(3..4)?,
            ),
            6 => (
                parse_nibble(0..2)?,
                parse_nibble(2..4)?,
                parse_nibble(4..6)?,
                0xff_u8,
            ),
            8 => (
                parse_nibble(0..2)?,
                parse_nibble(2..4)?,
                parse_nibble(4..6)?,
                parse_nibble(6..8)?,
            ),
            _ => return None,
        };
        Some(Self::rgba(red, green, blue, alpha))
    }

    /// R624 §5.50 — parse a CSS Color Module Level 4 `rgb()` /
    /// `rgba()` functional notation literal.
    ///
    /// Accepts the **legacy comma-separated** form per the CSS spec:
    ///
    /// - `rgb(r, g, b)` — three integer or percentage channels
    ///   (`rgb(255, 0, 0)` or `rgb(100%, 0%, 0%)`); implicit
    ///   opaque alpha.
    /// - `rgba(r, g, b, a)` — three channels plus a `0.0..=1.0`
    ///   alpha (`rgba(255, 0, 0, 0.5)`).
    ///
    /// Whitespace around commas + parens is tolerated. Channel
    /// values:
    ///
    /// - Integer in `0..=255` (`255` / `0` / mid).
    /// - Percentage in `0%..=100%` (`100%` / `0%` / `50%`).
    ///
    /// Mixing integer and percentage channels is rejected (CSS spec
    /// requires consistency within a single call). Alpha is a
    /// floating-point `0.0..=1.0` value (no percent form to keep the
    /// parser simple — CSS Color Level 4 also accepts `%` for alpha
    /// but the bulk of consumers send float).
    ///
    /// # Returns
    ///
    /// `Some(Color)` on a well-formed `rgb(...)` / `rgba(...)`
    /// literal; `None` on any malformed input — including missing
    /// parentheses, mixed unit channels, out-of-range integers, the
    /// modern space-separated syntax (`rgb(255 0 0)`), and any other
    /// CSS form (`hsl()`, `oklch()`, `lab()`).
    ///
    /// # Deferred forms
    ///
    /// - **Modern space-separated syntax** (`rgb(R G B)` /
    ///   `rgb(R G B / A)`): deferred to a future round when a CSS
    ///   modern-form consumer appears. The legacy comma form covers
    ///   the bulk of practical hand-authored CSS today (W3C still
    ///   lists both as Recommended).
    /// - **`hsl()` / `hsla()`**: needs the HSL→sRGB conversion math;
    ///   deferred per [[abstraction-needs-second-consumer]] until a
    ///   stylesheet binding requires the cylindrical form.
    /// - **`oklch()` / `lab()` / `color()`** (Level 4 modern):
    ///   wider color-gamut handling — deferred until pinion paints
    ///   wide-gamut.
    #[must_use]
    pub fn from_rgb_function(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        // Strip prefix `rgb(` or `rgba(`, suffix `)`.
        let (with_alpha, body) = if let Some(b) = trimmed.strip_prefix("rgba(") {
            (true, b)
        } else if let Some(b) = trimmed.strip_prefix("rgb(") {
            (false, b)
        } else {
            return None;
        };
        let body = body.strip_suffix(')')?.trim();
        // Comma-separated parts.
        let parts: Vec<&str> = body.split(',').map(str::trim).collect();
        let expected_len = if with_alpha { 4 } else { 3 };
        if parts.len() != expected_len {
            return None;
        }
        // Detect whether channels are percentage or integer (must be
        // homogeneous within the call). All three must agree.
        let is_percent = parts[0].ends_with('%');
        for &p in &parts[..3] {
            if p.ends_with('%') != is_percent {
                return None;
            }
        }
        let parse_channel = |s: &str| -> Option<u8> {
            if is_percent {
                let n: f32 = s.trim_end_matches('%').trim().parse().ok()?;
                if !(0.0..=100.0).contains(&n) {
                    return None;
                }
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "n is clamped to 0.0..=100.0; final byte is bounded 0..=255"
                )]
                Some((n * 2.55).round() as u8)
            } else {
                let n: i32 = s.parse().ok()?;
                if !(0..=255).contains(&n) {
                    return None;
                }
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "n is bounded 0..=255 above; cast is exact"
                )]
                Some(n as u8)
            }
        };
        let red = parse_channel(parts[0])?;
        let green = parse_channel(parts[1])?;
        let blue = parse_channel(parts[2])?;
        let alpha = if with_alpha {
            let a: f32 = parts[3].parse().ok()?;
            if !(0.0..=1.0).contains(&a) {
                return None;
            }
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a is clamped to 0.0..=1.0; final byte is bounded 0..=255"
            )]
            {
                (a * 255.0).round() as u8
            }
        } else {
            0xff
        };
        Some(Self::rgba(red, green, blue, alpha))
    }

    /// R624 §5.50 — single entry-point that accepts any supported
    /// CSS Color Module Level 4 string form and dispatches to the
    /// appropriate parser.
    ///
    /// Current support matrix:
    ///
    /// - `#rgb` / `#rgba` / `#rrggbb` / `#rrggbbaa` (via
    ///   [`Self::from_hex`])
    /// - `rgb(r, g, b)` / `rgba(r, g, b, a)` legacy comma form (via
    ///   [`Self::from_rgb_function`])
    ///
    /// Deferred per [[abstraction-needs-second-consumer]]: modern
    /// space-separated `rgb(...)` syntax, `hsl()` / `hsla()`,
    /// `oklch()` / `lab()` / `color()`. Each parser will land as a
    /// sibling `from_X_function` next to `from_rgb_function` and
    /// the dispatcher below will pick it up.
    ///
    /// Use this when the input source is arbitrary CSS-string user
    /// content (theme JSON, stylesheet binding) and the caller does
    /// not pre-know which form. When the form is known (e.g. an RPC
    /// wire that only ships hex), call the specific parser directly.
    #[must_use]
    pub fn from_css_string(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        if trimmed.starts_with('#') {
            Self::from_hex(trimmed)
        } else if trimmed.starts_with("rgb(") || trimmed.starts_with("rgba(") {
            Self::from_rgb_function(trimmed)
        } else {
            None
        }
    }

    /// R615 §5.50 — encode as a CSS Color Module Level 4 hex literal.
    ///
    /// Emits the canonical 6-digit form `#rrggbb` when alpha is
    /// fully opaque (`0xff`) and the 8-digit form `#rrggbbaa`
    /// otherwise. Lowercase hex digits per W3C convention.
    ///
    /// Inverse of [`Self::from_hex`] — round-trip pinned by
    /// `r615_color_hex_round_trips_for_every_canonical_form`.
    /// Pre-R615 this logic lived RPC-side as `color_to_hex`; R615
    /// lifts to the substrate alongside [`Self::from_hex`].
    #[must_use]
    pub fn to_hex(self) -> String {
        if self.a == 0xff {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                self.r, self.g, self.b, self.a,
            )
        }
    }

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

/// R51.154 §5.3 — convert a normalized `[0.0, 1.0]` value to a pixel
/// extent on a `[0, total]` axis.
///
/// Common UI math: thumb position on a slider track, progress bar
/// fill, scrubber thumbnail offset, etc. Application code wrote
/// `(value * total as f32) as u32` with three `#[allow(clippy::cast_*)]`
/// lints sprinkled around each call site (hello-slider +
/// hello-slider-vertical); this helper folds the math + lint
/// containment + endpoint-clamp into a single framework primitive.
///
/// ## Behaviour
///
/// - `value ≤ 0.0` (incl. negative + NaN) → `0`.
/// - `value ≥ 1.0` → `total` (no overflow on float drift past 1.0).
/// - Otherwise → `round(value * total)` saturated to `[0, total]`.
///
/// NaN coerces to `0.0` before clamp so a degraded numerical input
/// produces a zero-width fill (textbook silent recovery — matches
/// R51.145 [`clamp_frame_dt`](crate::frame_pacing::clamp_frame_dt)
/// + R51.151 [`Color::lerp`] NaN policy).
///
/// ## Why not a `Size` method
///
/// The conversion is upstream of any [`Size`] / [`LayoutStyle`]
/// construction (callers build the `Size::px(filled_w, ...)` after
/// this returns the pixel count). Threading it through a `Size`
/// builder would tangle the math + layout layers; a free fn keeps
/// the responsibilities split.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
pub fn scale_normalized_to_px(value: f32, total: u32) -> u32 {
    let v = if value.is_nan() { 0.0 } else { value.clamp(0.0, 1.0) };
    let pixels = (v * total as f32).round();
    // Clamp the float result before the as-cast so any drift past
    // `total` (e.g. value = 1.0 + epsilon rounded up) saturates
    // rather than overflowing to 0 via wrap-on-cast for u32-out-of-
    // range f32s.
    let pixels = pixels.clamp(0.0, total as f32) as u32;
    pixels.min(total)
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

    // ─────────────────────────────────────────────────────────────
    // R51.154 §5.3 — scale_normalized_to_px tests.
    //
    // Replaces the bespoke `(value * RANGE as f32) as u32` pattern
    // from hello-slider*/hello-slider-vertical. The tests pin down
    // endpoint behaviour, clamping, NaN guard, drift safety.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn scale_zero_returns_zero() {
        assert_eq!(super::scale_normalized_to_px(0.0, 100), 0);
    }

    #[test]
    fn scale_one_returns_total() {
        assert_eq!(super::scale_normalized_to_px(1.0, 100), 100);
    }

    #[test]
    fn scale_half_returns_half_total_rounded() {
        // 0.5 * 100 = 50; round = 50.
        assert_eq!(super::scale_normalized_to_px(0.5, 100), 50);
        // 0.5 * 101 = 50.5; round → 51.
        assert_eq!(super::scale_normalized_to_px(0.5, 101), 51);
    }

    #[test]
    fn scale_clamps_negative_to_zero() {
        assert_eq!(super::scale_normalized_to_px(-0.3, 100), 0);
        assert_eq!(super::scale_normalized_to_px(f32::NEG_INFINITY, 100), 0);
    }

    #[test]
    fn scale_clamps_above_one_to_total() {
        assert_eq!(super::scale_normalized_to_px(1.5, 100), 100);
        assert_eq!(super::scale_normalized_to_px(f32::INFINITY, 100), 100);
    }

    #[test]
    fn scale_nan_returns_zero() {
        // R51.154 + R51.151 NaN policy mirror.
        assert_eq!(super::scale_normalized_to_px(f32::NAN, 100), 0);
    }

    #[test]
    fn scale_with_zero_total_returns_zero() {
        assert_eq!(super::scale_normalized_to_px(0.5, 0), 0);
        assert_eq!(super::scale_normalized_to_px(1.0, 0), 0);
    }

    #[test]
    fn scale_drift_past_one_saturates_safely() {
        // Float arithmetic can produce 1.0 + 1e-7 from accumulated
        // updates. The clamp guarantees the cast doesn't overflow.
        let drifted = 1.0_f32 + f32::EPSILON;
        assert_eq!(super::scale_normalized_to_px(drifted, 1024), 1024);
    }

    #[test]
    fn scale_large_total_round_trip() {
        // u32 close to its max — the clamp at f32::from(u32) precision
        // boundary keeps us safe.
        assert_eq!(super::scale_normalized_to_px(0.0, u32::MAX), 0);
        // u32::MAX cannot be represented exactly in f32; the rounded
        // result is the nearest representable f32 cast back, which we
        // saturate via `.min(total)`. We accept the small precision
        // gap as documented behaviour for extreme inputs.
        let near_max = super::scale_normalized_to_px(1.0, u32::MAX);
        assert_eq!(near_max, u32::MAX);
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

    /// Builder: override the border colour. Pairs with [`Self::new`] for
    /// chain construction (`Border::new(c, w).with_color(c2)`).
    #[must_use]
    pub const fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Builder: override the pixel width.
    #[must_use]
    pub const fn with_width(mut self, width: u32) -> Self {
        self.width = width;
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

    /// Builder: override the fill colour. Composes with [`Self::default`]
    /// (fully-transparent start) or re-targets an existing instance.
    #[must_use]
    pub const fn with_fill(mut self, fill: Color) -> Self {
        self.fill = fill;
        self
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

    /// Builder: override the stroke colour.
    #[must_use]
    pub const fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Builder: override the pixel width.
    #[must_use]
    pub const fn with_width(mut self, width: u32) -> Self {
        self.width = width;
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

    /// Builder: attach a stroke arm. Composes with [`Self::stroked`] /
    /// [`Self::filled`] / [`Self::default`] so callers chain both arms
    /// (`PathStyle::filled(c).with_stroke(s)`) from any constructor entry.
    #[must_use]
    pub const fn with_stroke(mut self, stroke: Stroke) -> Self {
        self.stroke = Some(stroke);
        self
    }

    /// Builder: attach a fill arm. Mirrors [`Self::with_stroke`] so the
    /// two arms compose independently of the chosen entry constructor.
    #[must_use]
    pub const fn with_fill(mut self, fill: Color) -> Self {
        self.fill = Some(fill);
        self
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
    /// (R55.D.6 §5.45 §5.21) Absolute positioning override. When
    /// `Some((left, top))`, this node is removed from its parent's
    /// flex / block flow and positioned at `(parent.x + left,
    /// parent.y + top)` with its own [`Self::size`]. `None` (the
    /// default) participates in normal flow.
    ///
    /// Mirrors CSS `position: absolute; left/top: <px>` and Slint's
    /// `absolute-position` — the substrate-minimal addition that
    /// closes the R55.D.4 spacer-flex workaround. The
    /// `hello-listbox` scrollbar peer's thumb sits at a precise
    /// `(0, thumb_y_offset)` inside its track container after
    /// R55.D.6; the pre-R55.D.6 spacer Container is retired.
    ///
    /// Coordinates are parent-content-rect-relative (analogous to
    /// CSS `position: absolute` against a `position: relative`
    /// ancestor). The current substrate treats every parent as a
    /// positioning context — a future round adds an opt-out flag if
    /// nested absolute-positioning contexts become necessary.
    pub absolute_position: Option<(u32, u32)>,
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
            // (R55.D.6 §5.45 §5.21) `None` = normal flow, default.
            absolute_position: None,
        }
    }

    /// (R55.D.6 §5.45 §5.21) Builder: pin the node at parent-relative
    /// `(left, top)` outside the parent's flex / block flow. The
    /// node's [`Self::size`] declares the absolute box's dimensions
    /// (use [`Self::with_size`] alongside this builder; `Auto` size
    /// expands to the parent's content rect through taffy's default
    /// resolution for absolute children).
    ///
    /// Mirrors CSS `position: absolute; left/top: <px>` plus
    /// `width/height`. The substrate's first consumer is the
    /// `hello-listbox` scrollbar peer (R55.D.6), where the thumb
    /// container declares `absolute_position(0, thumb_y_offset)` +
    /// `with_size(Size::px(SCROLLBAR_W, thumb_h))` to retire the
    /// R55.D.4 spacer-flex workaround.
    #[must_use]
    pub const fn with_absolute_position(mut self, left: u32, top: u32) -> Self {
        self.absolute_position = Some((left, top));
        self
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

    // ─────────────────────────────────────────────────────────────
    // R615 §5.50 — Color::from_hex + Color::to_hex (CSS Color Module
    // Level 4 spec)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r615_from_hex_accepts_six_digit_lowercase() {
        assert_eq!(Color::from_hex("#fefbff"), Some(Color::rgb(0xfe, 0xfb, 0xff)));
    }

    #[test]
    fn r615_from_hex_accepts_six_digit_uppercase() {
        // CSS Color Module Level 4: hex digits are case-insensitive.
        assert_eq!(Color::from_hex("#FEFBFF"), Some(Color::rgb(0xfe, 0xfb, 0xff)));
    }

    #[test]
    fn r615_from_hex_accepts_eight_digit_with_alpha() {
        assert_eq!(
            Color::from_hex("#10203080"),
            Some(Color::rgba(0x10, 0x20, 0x30, 0x80)),
        );
    }

    #[test]
    fn r615_from_hex_six_digit_implies_opaque_alpha() {
        let c = Color::from_hex("#000000").unwrap();
        assert_eq!(c.a, 0xff);
    }

    #[test]
    fn r615_from_hex_accepts_three_digit_shorthand_per_css_spec() {
        // CSS Color Module Level 4: #RGB expands to #RRGGBB by
        // doubling each digit (`#fff` → `#ffffff`,
        // `#f0a` → `#ff00aa`).
        assert_eq!(Color::from_hex("#fff"), Some(Color::rgb(0xff, 0xff, 0xff)));
        assert_eq!(Color::from_hex("#f0a"), Some(Color::rgb(0xff, 0x00, 0xaa)));
        assert_eq!(Color::from_hex("#000"), Some(Color::rgb(0x00, 0x00, 0x00)));
    }

    #[test]
    fn r615_from_hex_accepts_four_digit_shorthand_with_alpha() {
        // #RGBA — last digit is alpha, also doubled.
        assert_eq!(
            Color::from_hex("#fff8"),
            Some(Color::rgba(0xff, 0xff, 0xff, 0x88)),
        );
    }

    #[test]
    fn r615_from_hex_rejects_missing_hash() {
        assert_eq!(Color::from_hex("fefbff"), None);
    }

    #[test]
    fn r615_from_hex_rejects_invalid_hex_digit() {
        assert_eq!(Color::from_hex("#zzzzzz"), None);
        assert_eq!(Color::from_hex("#12345g"), None);
        assert_eq!(Color::from_hex("#fz0"), None);
    }

    #[test]
    fn r615_from_hex_rejects_wrong_length() {
        // CSS Color Module Level 4 only defines #RGB / #RGBA /
        // #RRGGBB / #RRGGBBAA — every other digit count is invalid.
        assert_eq!(Color::from_hex("#"), None);
        assert_eq!(Color::from_hex("#1"), None);
        assert_eq!(Color::from_hex("#12"), None);
        assert_eq!(Color::from_hex("#12345"), None);
        assert_eq!(Color::from_hex("#1234567"), None);
        assert_eq!(Color::from_hex("#123456789"), None);
    }

    #[test]
    fn r615_to_hex_emits_six_digit_for_opaque() {
        assert_eq!(Color::rgb(0xff, 0xfb, 0xff).to_hex(), "#fffbff");
        assert_eq!(Color::rgb(0x12, 0x12, 0x12).to_hex(), "#121212");
    }

    #[test]
    fn r615_to_hex_emits_eight_digit_for_translucent() {
        assert_eq!(
            Color::rgba(0x10, 0x20, 0x30, 0x80).to_hex(),
            "#10203080",
        );
    }

    #[test]
    fn r615_to_hex_uses_lowercase_hex_digits() {
        // W3C / Material 3 / Web Inspector convention.
        let s = Color::rgb(0xab, 0xcd, 0xef).to_hex();
        assert_eq!(s, "#abcdef");
        assert!(!s.contains('A'));
    }

    #[test]
    fn r615_color_hex_round_trips_for_every_canonical_form() {
        // Property: from_hex(c.to_hex()) == Some(c) for every Color.
        for c in [
            Color::rgb(0x00, 0x00, 0x00),
            Color::rgb(0xff, 0xff, 0xff),
            Color::rgb(0x19, 0x76, 0xd2),  // Material Blue 700
            Color::rgb(0xb3, 0x26, 0x1e),  // Material Error 40
            Color::rgba(0x10, 0x20, 0x30, 0x80),
            Color::rgba(0x00, 0x00, 0x00, 0x01),
            Color::TRANSPARENT,
        ] {
            assert_eq!(
                Color::from_hex(&c.to_hex()),
                Some(c),
                "round-trip failed for {c:?}",
            );
        }
    }

    #[test]
    fn r615_from_hex_shorthand_round_trips_through_expansion() {
        // Shorthand round-trips through expansion: #fff → opaque
        // white → #ffffff (and the same byte triple).
        let c = Color::from_hex("#fff").unwrap();
        assert_eq!(c.to_hex(), "#ffffff");
    }

    // ─────────────────────────────────────────────────────────────
    // R624 §5.50 — Color::from_rgb_function + Color::from_css_string
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r624_from_rgb_function_accepts_legacy_integer_triplet() {
        assert_eq!(
            Color::from_rgb_function("rgb(255, 0, 0)"),
            Some(Color::rgb(0xff, 0x00, 0x00)),
        );
    }

    #[test]
    fn r624_from_rgb_function_accepts_legacy_percentage_triplet() {
        assert_eq!(
            Color::from_rgb_function("rgb(100%, 0%, 0%)"),
            Some(Color::rgb(0xff, 0x00, 0x00)),
        );
    }

    #[test]
    fn r624_from_rgba_function_accepts_legacy_with_float_alpha() {
        let c = Color::from_rgb_function("rgba(255, 0, 0, 0.5)").unwrap();
        assert_eq!((c.r, c.g, c.b), (0xff, 0x00, 0x00));
        // Alpha rounds to 128 (0.5 * 255 ≈ 127.5 → 128).
        assert_eq!(c.a, 128);
    }

    #[test]
    fn r624_from_rgb_function_tolerates_whitespace() {
        assert_eq!(
            Color::from_rgb_function("rgb( 255 , 128 , 64 )"),
            Some(Color::rgb(255, 128, 64)),
        );
    }

    #[test]
    fn r624_from_rgb_function_rejects_modern_space_separated() {
        // Modern syntax `rgb(255 0 0)` is deferred.
        assert_eq!(Color::from_rgb_function("rgb(255 0 0)"), None);
    }

    #[test]
    fn r624_from_rgb_function_rejects_mixed_percent_and_integer() {
        // CSS spec requires channel-unit consistency within one call.
        assert_eq!(Color::from_rgb_function("rgb(100%, 0, 0)"), None);
        assert_eq!(Color::from_rgb_function("rgb(255, 0%, 0)"), None);
    }

    #[test]
    fn r624_from_rgb_function_rejects_out_of_range_integer() {
        assert_eq!(Color::from_rgb_function("rgb(256, 0, 0)"), None);
        assert_eq!(Color::from_rgb_function("rgb(-1, 0, 0)"), None);
    }

    #[test]
    fn r624_from_rgb_function_rejects_out_of_range_percent() {
        assert_eq!(Color::from_rgb_function("rgb(101%, 0%, 0%)"), None);
        assert_eq!(Color::from_rgb_function("rgb(-1%, 0%, 0%)"), None);
    }

    #[test]
    fn r624_from_rgba_function_rejects_out_of_range_alpha() {
        assert_eq!(
            Color::from_rgb_function("rgba(255, 0, 0, 1.5)"),
            None,
        );
        assert_eq!(
            Color::from_rgb_function("rgba(255, 0, 0, -0.1)"),
            None,
        );
    }

    #[test]
    fn r624_from_rgb_function_rejects_wrong_arity() {
        assert_eq!(Color::from_rgb_function("rgb(255, 0)"), None);
        assert_eq!(Color::from_rgb_function("rgb(255, 0, 0, 0)"), None);
        assert_eq!(Color::from_rgb_function("rgba(255, 0, 0)"), None);
    }

    #[test]
    fn r624_from_rgb_function_rejects_missing_parens() {
        assert_eq!(Color::from_rgb_function("rgb 255, 0, 0"), None);
        assert_eq!(Color::from_rgb_function("rgb(255, 0, 0"), None);
        assert_eq!(Color::from_rgb_function("rgb 255, 0, 0)"), None);
    }

    #[test]
    fn r624_from_css_string_dispatches_to_hex() {
        // Per the dispatcher: `#...` lands in from_hex.
        assert_eq!(
            Color::from_css_string("#19 76d2".trim_end()),
            None,
            "embedded whitespace inside hex literal is rejected",
        );
        assert_eq!(
            Color::from_css_string("#1976d2"),
            Some(Color::rgb(0x19, 0x76, 0xd2)),
        );
    }

    #[test]
    fn r624_from_css_string_dispatches_to_rgb_function() {
        assert_eq!(
            Color::from_css_string("rgb(255, 0, 0)"),
            Some(Color::rgb(0xff, 0x00, 0x00)),
        );
        assert_eq!(
            Color::from_css_string("rgba(0, 0, 0, 1)"),
            Some(Color::rgba(0, 0, 0, 0xff)),
        );
    }

    #[test]
    fn r624_from_css_string_tolerates_leading_trailing_whitespace() {
        assert_eq!(
            Color::from_css_string("   #ffffff   "),
            Some(Color::rgb(0xff, 0xff, 0xff)),
        );
        assert_eq!(
            Color::from_css_string("   rgb(255, 0, 0)   "),
            Some(Color::rgb(0xff, 0x00, 0x00)),
        );
    }

    #[test]
    fn r624_from_css_string_rejects_unsupported_forms() {
        // Deferred per docs: hsl / oklch / lab / named-color / etc.
        assert_eq!(Color::from_css_string("hsl(0, 100%, 50%)"), None);
        assert_eq!(Color::from_css_string("oklch(50% 0.5 0)"), None);
        assert_eq!(Color::from_css_string("red"), None);
        assert_eq!(Color::from_css_string(""), None);
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
    fn border_with_color_and_width_builders_chain() {
        let b = Border::new(Color::rgb(0xff, 0, 0), 2)
            .with_color(Color::rgb(0, 0xff, 0))
            .with_width(5)
            .with_placement(BorderPlacement::Outside);
        assert_eq!(b.color, Color::rgb(0, 0xff, 0));
        assert_eq!(b.width, 5);
        assert_eq!(b.placement, BorderPlacement::Outside);
    }

    #[test]
    fn box_style_with_fill_builder_overrides_default_and_filled() {
        // Default starting point: with_fill swaps the transparent fill.
        let s = BoxStyle::default().with_fill(Color::rgb(0x11, 0x22, 0x33));
        assert_eq!(s.fill, Color::rgb(0x11, 0x22, 0x33));
        assert!(s.border.is_none());
        assert_eq!(s.corner_radius, 0);
        // Re-target after filled() + with_corner_radius — last with_fill wins.
        let s = BoxStyle::filled(Color::rgb(1, 2, 3))
            .with_corner_radius(4)
            .with_fill(Color::rgb(9, 9, 9));
        assert_eq!(s.fill, Color::rgb(9, 9, 9));
        assert_eq!(s.corner_radius, 4);
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
    fn stroke_with_color_and_width_builders_chain() {
        let s = Stroke::new(Color::rgb(0, 0, 0), 1)
            .with_color(Color::rgb(0xab, 0xcd, 0xef))
            .with_width(7)
            .with_cap(StrokeCap::Square);
        assert_eq!(s.color, Color::rgb(0xab, 0xcd, 0xef));
        assert_eq!(s.width, 7);
        assert_eq!(s.cap, StrokeCap::Square);
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
    fn path_style_with_stroke_and_fill_compose_symmetrically() {
        let stroke = Stroke::new(Color::rgb(0xff, 0, 0), 3);
        // stroked() entry + with_fill() — both arms set.
        let s = PathStyle::stroked(stroke).with_fill(Color::rgb(0, 0xff, 0));
        assert_eq!(s.stroke, Some(stroke));
        assert_eq!(s.fill, Some(Color::rgb(0, 0xff, 0)));
        // Reverse entry: filled() + with_stroke() — symmetric.
        let s = PathStyle::filled(Color::rgb(0x10, 0x20, 0x30)).with_stroke(stroke);
        assert_eq!(s.stroke, Some(stroke));
        assert_eq!(s.fill, Some(Color::rgb(0x10, 0x20, 0x30)));
        // Default entry + both builders — full chain from scratch.
        let s = PathStyle::default()
            .with_stroke(stroke)
            .with_fill(Color::rgb(7, 7, 7));
        assert_eq!(s.stroke, Some(stroke));
        assert_eq!(s.fill, Some(Color::rgb(7, 7, 7)));
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
