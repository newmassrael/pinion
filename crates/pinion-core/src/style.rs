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
}

/// Border for a [`BoxNode`](crate::scene::BoxNode) — §5.3 R20 lock.
/// `width: u32` is in pixels in the same coordinate space as
/// [`Rect`](crate::scene::Rect).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Border {
    pub color: Color,
    pub width: u32,
}

impl Border {
    /// Construct a border from a color and pixel width.
    #[must_use]
    pub const fn new(color: Color, width: u32) -> Self {
        Self { color, width }
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

/// Sidecar style for [`TextNode`](crate::scene::TextNode) per §5.3 R20.
///
/// `font_family = None` means "use the system default" — cosmic-text /
/// fontdb resolves an installed sans-serif fallback. `font_size_px`
/// is in CSS-style pixel units (cosmic-text converts to font-units
/// internally). `fg_color` defaults to opaque black so that a freshly
/// constructed [`TextNode`](crate::scene::TextNode) is visible without
/// requiring style configuration.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextStyle {
    pub font_family: Option<std::borrow::Cow<'static, str>>,
    pub font_size_px: u32,
    pub fg_color: Color,
}

impl TextStyle {
    /// v0 default: system font, 16px, opaque black.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            font_family: None,
            font_size_px: 16,
            fg_color: Color::rgb(0, 0, 0),
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
