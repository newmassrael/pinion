//! R908 §3 §5.53 — own-renderer Scene → PDF document pipeline.
//!
//! pinion paints its **own** UI (inv #1: structured scene, no opaque
//! paint callbacks), so it can also render that scene to a portable
//! *vector* document without ever touching a rasterizer. This module is
//! the leaf peer of the vello paint adapter
//! (`pinion_runtime::paint_adapter`, Scene → GPU fragments) and the
//! accessibility builder (`pinion_a11y`, Scene → `AccessNode` tree): all
//! three walk the same post-layout [`Scene`] tree, each projecting it to
//! a different target. This one's target is PDF page operators.
//!
//! It is the documented-future axis [`pinion_core::print`] reserved:
//! *"a scene → PDF / `PostScript` page-render pipeline is a documented
//! future axis"*. Where `scene/screenshot` still returns
//! `RenderBackendUnavailable` (pixel rendering is gated on the §5.16 RHI
//! that has not shipped), PDF needs **no** rasterizer — the structured
//! scene already carries resolved geometry (`Rect`), fills (`BoxStyle`),
//! and text (`TextNode`), which lower directly to vector drawing
//! operators. That is exactly the inv #1 payoff: a structured scene is
//! renderable to formats a pixel buffer is not.
//!
//! ## Scope (first slice)
//!
//! Handled: filled / bordered / rounded [`BoxNode`] +
//! [`ContainerNode`] backgrounds, [`TextNode`] (the standard-14
//! `Helvetica` family — regular / bold / oblique / bold-oblique — in
//! `DeviceRGB` sRGB), and [`Scene::Scroll`] (a viewport clip plus the
//! `translate(viewport - offset)` the vello adapter applies, so scrolled
//! content lands in the right place and is clipped to its viewport).
//!
//! Deferred (rendered as nothing, a documented carry — additive when a
//! consumer arrives):
//!
//! - [`Scene::Path`] / [`Scene::Image`] — vector paths + image `XObjects`.
//! - `BoxStyle` gradients + drop-shadows.
//! - [`TextNode::runs`] styled-run colours/sizes (the whole node renders
//!   in its base [`TextStyle`]).
//! - [`Scene::External`] / [`Scene::ImmediateModeNode`] — opaque by the
//!   §3 contract; their interior pixels are not in the structured scene,
//!   so a vector renderer honestly draws nothing for them.
//! - Non-ASCII text: bytes outside printable ASCII are octal-escaped so
//!   the document stays valid (and pure ASCII), but they are not
//!   transcoded to the fonts' `WinAnsiEncoding`, so they will not render
//!   under the standard-14 fonts. UTF-8 → `WinAnsi` transcoding and full
//!   font embedding (for non-Latin scripts) are deferred axes.
//! - Multi-page pagination (the whole scene maps onto one page).
//!
//! ## Coordinates
//!
//! Scene space is top-left origin, y-down, integer pixels. PDF user
//! space is bottom-left origin, y-up, in points. The renderer maps one
//! scene pixel to one PDF point (72-per-inch CSS-pixel parity) and flips
//! y: a scene rect at `(x, y, w, h)` becomes a PDF box whose lower-left
//! corner is `(x, page_height - y - h)`. Scaling-to-fit a standard page
//! is a deferred axis; the page defaults to the scene's own bounds
//! (WYSIWYG) but a caller may request an explicit [`PageSize`].
//!
//! ## Determinism
//!
//! `Scene → Vec<u8>` is a pure function — no wall-clock, no GPU, no OS.
//! The same scene always produces byte-identical output (object
//! numbering is fixed, the alpha → `ExtGState` map is keyed in a
//! deterministic order). That is what lets the end-to-end demo assert
//! the document's *structure* exactly (header, `MediaBox`, page tree,
//! content operators, embedded text) with zero flake.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use pinion_core::scene::{Rect, Scene, TextNode};
use pinion_core::style::{BoxStyle, Color, FontStyle, FontWeight, TextStyle};

/// Bézier control-point offset factor for a quarter-circle of radius
/// `r`: the control point sits `KAPPA * r` from the arc endpoint toward
/// the (sharp) corner. The classic 4-segment circle approximation
/// constant (`(4/3)·(√2 − 1) ≈ 0.5523`).
const KAPPA: f64 = 0.552_284_75;

/// Approximate cap-height / ascent as a fraction of the font size, used
/// to place a text node's baseline below the top of its layout rect.
/// The structured scene does not carry per-font metrics here (font
/// embedding / metric extraction is a deferred axis), so the baseline is
/// approximated; the demo asserts text *presence* and *page-bounds*, not
/// pixel-exact placement.
const ASCENT_RATIO: f64 = 0.8;

/// Page dimensions in PDF points (1 pt = 1/72 inch). The renderer maps
/// one scene pixel to one point, so a page can simply mirror the scene's
/// pixel bounds (the WYSIWYG default) or be an explicit standard size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageSize {
    /// Page width in points.
    pub width_pt: u32,
    /// Page height in points.
    pub height_pt: u32,
}

impl PageSize {
    /// US Letter, portrait: 8.5 × 11 in = 612 × 792 pt.
    pub const LETTER: Self = Self { width_pt: 612, height_pt: 792 };
    /// ISO A4, portrait: 210 × 297 mm ≈ 595 × 842 pt.
    pub const A4: Self = Self { width_pt: 595, height_pt: 842 };

    /// Construct an explicit page size.
    #[must_use]
    pub const fn new(width_pt: u32, height_pt: u32) -> Self {
        Self { width_pt, height_pt }
    }

    /// The landscape form (width and height swapped).
    #[must_use]
    pub const fn landscape(self) -> Self {
        Self { width_pt: self.height_pt, height_pt: self.width_pt }
    }

    /// A page sized to the scene's own pixel bounds — the WYSIWYG
    /// default. Falls back to [`Self::LETTER`] for a zero-area scene
    /// (a never-painted root) so the page is always non-degenerate.
    #[must_use]
    pub fn from_scene_bounds(scene: &Scene) -> Self {
        let r = scene.rect();
        if r.w == 0 || r.h == 0 {
            Self::LETTER
        } else {
            Self { width_pt: r.w, height_pt: r.h }
        }
    }
}

/// A rendered PDF document plus the structural facts a caller (or an AI
/// agent over RPC) needs without re-parsing the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPdf {
    /// The complete PDF, fully ASCII by construction (binary bytes in
    /// text are octal-escaped), so it round-trips losslessly through a
    /// JSON string on the wire.
    pub document: String,
    /// Pages in the document. One in this slice (pagination deferred).
    pub page_count: u32,
    /// Page width in points (`MediaBox` width).
    pub page_width_pt: u32,
    /// Page height in points (`MediaBox` height).
    pub page_height_pt: u32,
    /// Indirect objects in the document, excluding the free object 0
    /// (catalog + pages + page + content + 4 fonts + N alpha
    /// `ExtGState`s). The `xref` table lists `object_count + 1` slots.
    pub object_count: u32,
}

impl RenderedPdf {
    /// The document as bytes (it is ASCII, so this is a lossless view).
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.document.as_bytes()
    }

    /// Document length in bytes.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.document.len()
    }
}

/// Render `scene` (a post-layout paint scene) onto a single PDF page of
/// `page` size. The scene is placed at 1:1 scene-pixel-to-point, y
/// flipped, top-left aligned with the page; content beyond the page box
/// is clipped by viewers to the `MediaBox`.
#[must_use]
pub fn render_scene(scene: &Scene, page: PageSize) -> RenderedPdf {
    let mut content = ContentBuilder::new(f64::from(page.height_pt));
    content.walk(scene, 0.0, 0.0);
    assemble(&content, page)
}

/// Accumulates the page content stream while walking the scene, plus the
/// distinct sub-opaque alphas encountered (each becomes one `ExtGState`
/// referenced via a `/GSn gs` operator).
struct ContentBuilder {
    ops: String,
    page_h: f64,
    /// alpha (`< 255`) → 0-based `ExtGState` index. `BTreeMap` keeps
    /// emission deterministic; the index is assigned on first encounter.
    alpha_gs: BTreeMap<u8, u32>,
}

impl ContentBuilder {
    fn new(page_h: f64) -> Self {
        Self { ops: String::new(), page_h, alpha_gs: BTreeMap::new() }
    }

    /// Depth-first walk mirroring `pinion_runtime::paint_adapter`: the
    /// outer tree is absolute (translate `(0, 0)`); `Scene::Scroll`
    /// composes `translate(viewport - offset)` and a viewport clip the
    /// same way the vello adapter does, so content lands and clips
    /// identically.
    fn walk(&mut self, scene: &Scene, tx: f64, ty: f64) {
        match scene {
            Scene::Box(n) => self.paint_box(n.rect, &n.style, tx, ty),
            Scene::Container(n) => {
                self.paint_box(n.rect, &n.style, tx, ty);
                for child in &n.children {
                    self.walk(child, tx, ty);
                }
            }
            Scene::Text(n) => self.paint_text(n, tx, ty),
            Scene::Scroll(n) => {
                // Viewport clip in the current frame, then content under
                // the scroll translate (R55.E.1 paint-adapter parity).
                self.ops.push_str("q\n");
                self.clip_rect(n.viewport, tx, ty);
                let cx = tx + f64::from(n.viewport.x) - f64::from(n.offset_x);
                let cy = ty + f64::from(n.viewport.y) - f64::from(n.offset_y);
                self.walk(&n.content, cx, cy);
                self.ops.push_str("Q\n");
            }
            // Deferred / opaque primitives render nothing (module doc):
            // Path + Image (vector paths / image XObjects, a deferred
            // axis), Effect (no geometry), External + ImmediateModeNode
            // (opaque by the §3 contract — no interior in the structured
            // scene), and — `Scene` being `#[non_exhaustive]` — any
            // future primitive until a vector projection exists for it.
            _ => {}
        }
    }

    /// Paint a box: optional alpha-gated solid fill, then optional
    /// border stroke. A fully-transparent fill (`a == 0`) is skipped.
    fn paint_box(&mut self, rect: Rect, style: &BoxStyle, tx: f64, ty: f64) {
        let (x0, y0, w, h) = self.device_box(rect, tx, ty);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let radius = f64::from(style.corner_radius).min(w / 2.0).min(h / 2.0);

        if style.fill.a != 0 {
            self.set_alpha(style.fill.a);
            self.set_fill_color(style.fill);
            self.rect_path(x0, y0, w, h, radius);
            self.ops.push_str("f\n");
        }

        if let Some(border) = &style.border {
            if border.color.a != 0 && border.width > 0 {
                self.set_alpha(border.color.a);
                self.set_stroke_color(border.color);
                let _ = writeln!(self.ops, "{} w", fmt_num(f64::from(border.width)));
                self.rect_path(x0, y0, w, h, radius);
                self.ops.push_str("S\n");
            }
        }
    }

    /// Paint a text node's `content` in its base style (styled `runs`
    /// are a deferred axis). Skips empty / transparent text.
    fn paint_text(&mut self, node: &TextNode, tx: f64, ty: f64) {
        if node.content.is_empty() || node.style.fg_color.a == 0 {
            return;
        }
        let style: &TextStyle = &node.style;
        let size = f64::from(style.font_size_px.max(1));
        // Baseline below the top of the text rect, then y-flipped.
        let top = f64::from(node.rect.y) + ty;
        let baseline = top + size * ASCENT_RATIO;
        let pdf_y = self.page_h - baseline;
        let x = f64::from(node.rect.x) + tx;
        let font = font_resource(style.font_weight, style.font_style);

        self.set_alpha(style.fg_color.a);
        self.ops.push_str("BT\n");
        let _ = writeln!(self.ops, "/{font} {} Tf", fmt_num(size));
        self.write_fill_color(style.fg_color);
        let _ = writeln!(self.ops, "{} {} Td", fmt_num(x), fmt_num(pdf_y));
        let _ = writeln!(self.ops, "({}) Tj", escape_pdf_string(&node.content));
        self.ops.push_str("ET\n");
    }

    /// Set the current clip to `rect` (in the current translate frame).
    fn clip_rect(&mut self, rect: Rect, tx: f64, ty: f64) {
        let (x0, y0, w, h) = self.device_box(rect, tx, ty);
        let _ = writeln!(
            self.ops,
            "{} {} {} {} re W n",
            fmt_num(x0),
            fmt_num(y0),
            fmt_num(w),
            fmt_num(h),
        );
    }

    /// Translate + y-flip a scene rect into PDF user space, returning the
    /// lower-left corner `(x0, y0)` plus width / height.
    fn device_box(&self, rect: Rect, tx: f64, ty: f64) -> (f64, f64, f64, f64) {
        let x0 = f64::from(rect.x) + tx;
        let scene_top = f64::from(rect.y) + ty;
        let w = f64::from(rect.w);
        let h = f64::from(rect.h);
        let y0 = self.page_h - scene_top - h;
        (x0, y0, w, h)
    }

    /// Emit a rect path: a sharp `re` when `radius <= 0`, otherwise four
    /// edges joined by Bézier-approximated quarter-circle corners. Leaves
    /// the path current for a following `f` (fill) or `S` (stroke).
    fn rect_path(&mut self, x0: f64, y0: f64, w: f64, h: f64, radius: f64) {
        if radius <= 0.0 {
            let _ = writeln!(
                self.ops,
                "{} {} {} {} re",
                fmt_num(x0),
                fmt_num(y0),
                fmt_num(w),
                fmt_num(h),
            );
            return;
        }
        let x1 = x0 + w;
        let y1 = y0 + h;
        let rad = radius;
        // Control-point offset from each arc endpoint toward the corner.
        let kr = KAPPA * rad;
        let nf = fmt_num;
        // Start after the bottom-left corner, go counter-clockwise.
        let _ = writeln!(self.ops, "{} {} m", nf(x0 + rad), nf(y0));
        let _ = writeln!(self.ops, "{} {} l", nf(x1 - rad), nf(y0));
        let _ = writeln!(
            self.ops,
            "{} {} {} {} {} {} c",
            nf(x1 - rad + kr), nf(y0), nf(x1), nf(y0 + rad - kr), nf(x1), nf(y0 + rad),
        );
        let _ = writeln!(self.ops, "{} {} l", nf(x1), nf(y1 - rad));
        let _ = writeln!(
            self.ops,
            "{} {} {} {} {} {} c",
            nf(x1), nf(y1 - rad + kr), nf(x1 - rad + kr), nf(y1), nf(x1 - rad), nf(y1),
        );
        let _ = writeln!(self.ops, "{} {} l", nf(x0 + rad), nf(y1));
        let _ = writeln!(
            self.ops,
            "{} {} {} {} {} {} c",
            nf(x0 + rad - kr), nf(y1), nf(x0), nf(y1 - rad + kr), nf(x0), nf(y1 - rad),
        );
        let _ = writeln!(self.ops, "{} {} l", nf(x0), nf(y0 + rad));
        let _ = writeln!(
            self.ops,
            "{} {} {} {} {} {} c",
            nf(x0), nf(y0 + rad - kr), nf(x0 + rad - kr), nf(y0), nf(x0 + rad), nf(y0),
        );
        self.ops.push_str("h\n");
    }

    /// Register `alpha` (when `< 255`) and emit a `/GSn gs` operator so
    /// the next fill / stroke / text uses that constant alpha. Opaque
    /// (`255`) needs no `ExtGState` — the PDF default is `ca = CA = 1`.
    fn set_alpha(&mut self, alpha: u8) {
        if alpha == 0xff {
            return;
        }
        let next = u32::try_from(self.alpha_gs.len()).unwrap_or(u32::MAX);
        let idx = *self.alpha_gs.entry(alpha).or_insert(next);
        let _ = writeln!(self.ops, "/GS{idx} gs");
    }

    fn set_fill_color(&mut self, c: Color) {
        let (r, g, b) = rgb_unit(c);
        let _ = writeln!(self.ops, "{} {} {} rg", fmt_num(r), fmt_num(g), fmt_num(b));
    }

    /// Fill colour set inside a `BT`/`ET` text object (same `rg`
    /// operator; text painting uses the fill colour).
    fn write_fill_color(&mut self, c: Color) {
        self.set_fill_color(c);
    }

    fn set_stroke_color(&mut self, c: Color) {
        let (r, g, b) = rgb_unit(c);
        let _ = writeln!(self.ops, "{} {} {} RG", fmt_num(r), fmt_num(g), fmt_num(b));
    }
}

/// The four standard-14 `Helvetica` resource names, by `(bold, italic)`.
/// `font_resource` picks one; `assemble` always declares all four (a
/// fixed object layout keeps rendering deterministic).
const FONT_REGULAR: &str = "F1";
const FONT_BOLD: &str = "F2";
const FONT_ITALIC: &str = "F3";
const FONT_BOLD_ITALIC: &str = "F4";

/// Map a text style's weight + style to one of the standard-14
/// `Helvetica` resource names. Bold is `weight >= 700` (CSS bold);
/// italic is `Italic` or any `Oblique`.
fn font_resource(weight: FontWeight, style: FontStyle) -> &'static str {
    let bold = weight.0 >= FontWeight::BOLD.0;
    let italic = matches!(style, FontStyle::Italic | FontStyle::Oblique(_));
    match (bold, italic) {
        (false, false) => FONT_REGULAR,
        (true, false) => FONT_BOLD,
        (false, true) => FONT_ITALIC,
        (true, true) => FONT_BOLD_ITALIC,
    }
}

/// The base font that `F1`..`F4` resolve to, in order.
const FONT_BASE_NAMES: [&str; 4] =
    ["Helvetica", "Helvetica-Bold", "Helvetica-Oblique", "Helvetica-BoldOblique"];

/// sRGB channels normalized to PDF's `0.0..=1.0` `DeviceRGB` range. No
/// linearization — `DeviceRGB` values are device (sRGB-ish) intensities,
/// unlike the vello paint path which decodes to linear space for
/// blending ([[color-lerp-linear-space]]). So the native sRGB `u8`
/// divided by 255 is exactly right here.
fn rgb_unit(c: Color) -> (f64, f64, f64) {
    (
        f64::from(c.r) / 255.0,
        f64::from(c.g) / 255.0,
        f64::from(c.b) / 255.0,
    )
}

/// Format a PDF real number: an integer when it has no fractional part,
/// else up to three decimals with trailing zeros trimmed. Deterministic
/// (no locale, no exponent form — PDF forbids `1e3`).
fn fmt_num(v: f64) -> String {
    // Guard the degenerate inputs PDF cannot express, then prefer the
    // compact integer form.
    if !v.is_finite() {
        return "0".to_owned();
    }
    if (v - v.trunc()).abs() < 1e-9 && v.abs() < 1e15 {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "guarded |v| < 1e15 and integral; fits i64"
        )]
        return (v.trunc() as i64).to_string();
    }
    let s = format!("{v:.3}");
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_owned()
}

/// Escape a string for a PDF literal `(...)` string, keeping the output
/// pure ASCII: `(`, `)`, `\` are backslash-escaped; printable ASCII
/// passes through; every other byte (control chars, the UTF-8 bytes of
/// non-Latin text) becomes a `\ooo` octal escape. This keeps the
/// document valid and pure ASCII, but the escaped bytes are *not*
/// transcoded to the fonts' `WinAnsiEncoding`, so non-ASCII text will
/// not render under the standard-14 fonts (UTF-8 → `WinAnsi` transcoding
/// + font embedding are deferred axes).
fn escape_pdf_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'(' => out.push_str("\\("),
            b')' => out.push_str("\\)"),
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7e => out.push(b as char),
            _ => {
                let _ = write!(out, "\\{b:03o}");
            }
        }
    }
    out
}

/// Assemble the indirect objects, `xref` table, and trailer around the
/// built content stream. Object layout is fixed:
///
/// 1 Catalog · 2 Pages · 3 Page · 4 Contents · 5–8 Fonts (`F1`–`F4`) ·
/// 9.. one `ExtGState` per distinct sub-opaque alpha.
fn assemble(content: &ContentBuilder, page: PageSize) -> RenderedPdf {
    // `ExtGState`s in index order: object N is emitted in this order
    // (object number = 9 + index), so Resources references and emitted
    // objects agree even when alphas were *encountered* out of sorted
    // order.
    let gs_order = alpha_gs_in_index_order(content);
    let object_count = 8 + gs_order.len(); // excludes the free object 0.

    // Page Resources: all four fonts plus any alpha ExtGStates.
    let mut resources = String::from("<< /Font << /F1 5 0 R /F2 6 0 R /F3 7 0 R /F4 8 0 R >>");
    if !gs_order.is_empty() {
        resources.push_str(" /ExtGState <<");
        for (idx, _alpha) in &gs_order {
            let _ = write!(resources, " /GS{idx} {} 0 R", 9 + idx);
        }
        resources.push_str(" >>");
    }
    resources.push_str(" >>");

    let mut out = String::new();
    out.push_str("%PDF-1.7\n");

    let mut offsets: Vec<usize> = Vec::with_capacity(object_count + 1);

    let push_obj = |out: &mut String, offsets: &mut Vec<usize>, body: &str| {
        offsets.push(out.len());
        let n = offsets.len(); // 1-based object number.
        let _ = write!(out, "{n} 0 obj\n{body}\nendobj\n");
    };

    push_obj(&mut out, &mut offsets, "<< /Type /Catalog /Pages 2 0 R >>");
    push_obj(&mut out, &mut offsets, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    push_obj(
        &mut out,
        &mut offsets,
        &format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents 4 0 R /Resources {resources} >>",
            page.width_pt, page.height_pt,
        ),
    );
    push_obj(
        &mut out,
        &mut offsets,
        &format!(
            "<< /Length {} >>\nstream\n{}endstream",
            content.ops.len(),
            content.ops,
        ),
    );
    for base in FONT_BASE_NAMES {
        push_obj(
            &mut out,
            &mut offsets,
            &format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /{base} /Encoding /WinAnsiEncoding >>"
            ),
        );
    }
    for (_idx, alpha) in &gs_order {
        let ca = fmt_num(f64::from(*alpha) / 255.0);
        push_obj(
            &mut out,
            &mut offsets,
            &format!("<< /Type /ExtGState /ca {ca} /CA {ca} >>"),
        );
    }

    // xref + trailer.
    let xref_offset = out.len();
    let size = offsets.len() + 1; // + the free object 0.
    let _ = write!(out, "xref\n0 {size}\n");
    out.push_str("0000000000 65535 f\r\n");
    for off in &offsets {
        let _ = write!(out, "{off:010} 00000 n\r\n");
    }
    let _ = write!(
        out,
        "trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
    );

    RenderedPdf {
        document: out,
        page_count: 1,
        page_width_pt: page.width_pt,
        page_height_pt: page.height_pt,
        object_count: u32::try_from(object_count).unwrap_or(u32::MAX),
    }
}

/// The `(index, alpha)` pairs in ascending *index* order — the order
/// `ExtGState` objects are emitted in (object number = `9 + index`). The
/// index was assigned on first encounter, which need not be ascending by
/// alpha value, so both Resources references and object emission must
/// walk this same index order to agree.
fn alpha_gs_in_index_order(content: &ContentBuilder) -> Vec<(u32, u8)> {
    let mut pairs: Vec<(u32, u8)> =
        content.alpha_gs.iter().map(|(alpha, idx)| (*idx, *alpha)).collect();
    pairs.sort_unstable_by_key(|(idx, _)| *idx);
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::{BoxNode, ContainerNode};
    use pinion_core::style::Border;

    fn text_node(content: &str, x: u32, y: u32, size: u32, color: Color) -> Scene {
        Scene::Text(TextNode::styled(
            content.to_owned(),
            Rect::new(x, y, 200, size),
            TextStyle::new().with_size_px(size).with_fg(color),
        ))
    }

    #[test]
    fn r908_minimal_document_has_valid_skeleton() {
        let scene = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 100, 100),
            Color::rgb(0xff, 0xff, 0xff),
        ));
        let pdf = render_scene(&scene, PageSize::new(100, 100));
        let doc = &pdf.document;
        assert!(doc.starts_with("%PDF-1.7"), "header present");
        assert!(doc.trim_end().ends_with("%%EOF"), "trailer present");
        assert!(doc.contains("/Type /Catalog"));
        assert!(doc.contains("/Type /Pages"));
        assert!(doc.contains("/Type /Page"));
        assert!(doc.contains("/MediaBox [0 0 100 100]"));
        assert!(doc.contains("/Count 1"));
        assert!(doc.contains("xref"));
        assert!(doc.contains("startxref"));
        assert_eq!(pdf.page_count, 1);
        assert_eq!(pdf.page_width_pt, 100);
        assert_eq!(pdf.page_height_pt, 100);
        // catalog + pages + page + contents + 4 fonts, no alpha.
        assert_eq!(pdf.object_count, 8);
    }

    #[test]
    fn r908_document_is_pure_ascii() {
        let scene = text_node("Héllo — wörld", 10, 10, 14, Color::rgb(0, 0, 0));
        let pdf = render_scene(&scene, PageSize::LETTER);
        assert!(
            pdf.document.is_ascii(),
            "non-Latin text must octal-escape into a pure-ASCII document",
        );
    }

    #[test]
    fn r908_filled_rect_emits_color_and_fill_op() {
        let scene = Scene::Box(BoxNode::filled(
            Rect::new(10, 20, 30, 40),
            Color::rgb(0xff, 0x00, 0x00),
        ));
        let pdf = render_scene(&scene, PageSize::new(200, 200));
        // red fill: 1 0 0 rg.
        assert!(pdf.document.contains("1 0 0 rg"), "red fill color set");
        // sharp rect with y-flip: page_h(200) - y(20) - h(40) = 140.
        assert!(pdf.document.contains("10 140 30 40 re"), "y-flipped rect path");
        assert!(pdf.document.contains("re\nf\n"), "fill operator follows the path");
    }

    #[test]
    fn r908_transparent_fill_is_skipped() {
        let scene = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 50, 50),
            Color::rgba(0xff, 0, 0, 0),
        ));
        let pdf = render_scene(&scene, PageSize::new(50, 50));
        assert!(!pdf.document.contains(" re\n") && !pdf.document.contains(" f\n"));
        assert_eq!(pdf.object_count, 8, "no ExtGState for a skipped fill");
    }

    #[test]
    fn r908_sub_opaque_fill_registers_one_extgstate() {
        let scene = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 50, 50),
            Color::rgba(0, 0, 0, 0x80),
        ));
        let pdf = render_scene(&scene, PageSize::new(50, 50));
        assert!(pdf.document.contains("/GS0 gs"), "alpha gs referenced");
        assert!(pdf.document.contains("/Type /ExtGState"));
        assert!(pdf.document.contains("/ca 0.502"), "128/255 ~= 0.502");
        assert_eq!(pdf.object_count, 9, "one extra object for the alpha");
        assert!(pdf.document.contains("/ExtGState << /GS0 9 0 R >>"));
    }

    #[test]
    fn r908_two_alphas_keep_gs_index_and_object_number_aligned() {
        // Encounter a higher alpha first, then a lower one, so the
        // encounter order (GS index) is NOT ascending by alpha value.
        // GS0 (encountered first, a=200) must resolve to its own object,
        // GS1 (a=100) to the next — a regression pin for the index-order
        // vs emission-order alignment.
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Box(BoxNode::filled(Rect::new(0, 0, 10, 10), Color::rgba(0, 0, 0, 200))),
            Scene::Box(BoxNode::filled(Rect::new(0, 0, 10, 10), Color::rgba(0, 0, 0, 100))),
        ]));
        let pdf = render_scene(&scene, PageSize::new(20, 20));
        let doc = &pdf.document;
        // GS0 -> object 9, GS1 -> object 10.
        assert!(doc.contains("/ExtGState << /GS0 9 0 R /GS1 10 0 R >>"));
        // Object 9 carries a=200 (200/255 ~= 0.784), object 10 a=100
        // (100/255 ~= 0.392). Their order in the body must match.
        let obj9 = doc.find("9 0 obj").expect("object 9");
        let obj10 = doc.find("10 0 obj").expect("object 10");
        assert!(obj9 < obj10, "object 9 precedes object 10 in the body");
        let body9 = &doc[obj9..obj10];
        assert!(body9.contains("/ca 0.784"), "GS0's object carries a=200");
        assert_eq!(pdf.object_count, 10, "8 base + 2 alpha objects");
    }

    #[test]
    fn r908_text_emits_font_size_and_string() {
        let scene = text_node("Invoice", 12, 12, 18, Color::rgb(0, 0, 0));
        let pdf = render_scene(&scene, PageSize::new(300, 300));
        assert!(pdf.document.contains("BT\n"), "text object opens");
        assert!(pdf.document.contains("/F1 18 Tf"), "regular Helvetica at 18pt");
        assert!(pdf.document.contains("(Invoice) Tj"), "text shown");
        assert!(pdf.document.contains("ET\n"), "text object closes");
    }

    #[test]
    fn r908_bold_italic_select_the_right_standard_font() {
        use pinion_core::style::FontStyle;
        let bold = TextStyle::new().with_size_px(12).with_weight(FontWeight::BOLD);
        let scene = Scene::Text(TextNode::styled("B".to_owned(), Rect::new(0, 0, 50, 12), bold));
        assert!(render_scene(&scene, PageSize::LETTER).document.contains("/F2 12 Tf"));

        let italic = TextStyle::new().with_size_px(12).with_style(FontStyle::Italic);
        let scene = Scene::Text(TextNode::styled("I".to_owned(), Rect::new(0, 0, 50, 12), italic));
        assert!(render_scene(&scene, PageSize::LETTER).document.contains("/F3 12 Tf"));
    }

    #[test]
    fn r908_rounded_rect_uses_beziers_not_re() {
        let style = BoxStyle::filled(Color::rgb(0, 0, 0)).with_corner_radius(8);
        let scene = Scene::Box(BoxNode::new(Rect::new(0, 0, 60, 60), style));
        let pdf = render_scene(&scene, PageSize::new(60, 60));
        assert!(pdf.document.contains(" c\n"), "bezier corner operator present");
        assert!(pdf.document.contains(" m\n"), "path moveto present");
        assert!(pdf.document.contains("c\nh\n"), "path closed after final curve");
        assert!(!pdf.document.contains(" re\n"), "rounded path uses curves, not re");
    }

    #[test]
    fn r908_border_strokes_with_width_and_color() {
        let style = BoxStyle::filled(Color::rgb(0xff, 0xff, 0xff))
            .with_border(Border::new(Color::rgb(0, 0, 0), 2));
        let scene = Scene::Box(BoxNode::new(Rect::new(0, 0, 40, 40), style));
        let pdf = render_scene(&scene, PageSize::new(40, 40));
        assert!(pdf.document.contains("0 0 0 RG"), "stroke color set");
        assert!(pdf.document.contains("2 w"), "line width set");
        assert!(pdf.document.contains("re\nS\n"), "stroke operator follows the path");
    }

    #[test]
    fn r908_container_renders_children() {
        let child = text_node("Child", 5, 5, 12, Color::rgb(0, 0, 0));
        let scene = Scene::Container(
            ContainerNode::new(vec![child])
                .with_style(BoxStyle::filled(Color::rgb(0xee, 0xee, 0xee))),
        );
        let pdf = render_scene(&scene, PageSize::new(120, 120));
        assert!(pdf.document.contains("(Child) Tj"), "child text rendered");
    }

    #[test]
    fn r908_render_is_deterministic() {
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Box(BoxNode::filled(Rect::new(0, 0, 50, 50), Color::rgba(1, 2, 3, 0x80))),
            text_node("Deterministic", 4, 4, 11, Color::rgb(9, 8, 7)),
        ]));
        let a = render_scene(&scene, PageSize::A4);
        let b = render_scene(&scene, PageSize::A4);
        assert_eq!(a, b, "Scene -> PDF is a pure function");
    }

    #[test]
    fn r908_page_size_helpers() {
        assert_eq!(PageSize::LETTER, PageSize::new(612, 792));
        assert_eq!(PageSize::A4, PageSize::new(595, 842));
        assert_eq!(PageSize::LETTER.landscape(), PageSize::new(792, 612));
        let scene = Scene::Box(BoxNode::filled(Rect::new(0, 0, 320, 240), Color::rgb(0, 0, 0)));
        assert_eq!(PageSize::from_scene_bounds(&scene), PageSize::new(320, 240));
        let empty = Scene::Box(BoxNode::filled(Rect::new(0, 0, 0, 0), Color::rgb(0, 0, 0)));
        assert_eq!(PageSize::from_scene_bounds(&empty), PageSize::LETTER);
    }

    #[test]
    fn r908_xref_offsets_point_at_obj_headers() {
        let scene = Scene::Box(BoxNode::filled(Rect::new(0, 0, 10, 10), Color::rgb(0, 0, 0)));
        let pdf = render_scene(&scene, PageSize::new(10, 10));
        let doc = &pdf.document;
        // Each xref 'n' entry's 10-digit offset must land on "<n> 0 obj".
        let xref_start = doc.find("xref\n").expect("xref table present");
        let table = &doc[xref_start..];
        // The first in-use entry (object 1) must point at "1 0 obj".
        let obj1_off = doc.find("1 0 obj").expect("object 1 present");
        let needle = format!("{obj1_off:010} 00000 n");
        assert!(table.contains(&needle), "xref offset for obj 1 is byte-correct");
    }

    #[test]
    fn r908_scroll_clips_and_translates_content() {
        use pinion_core::scene::ScrollNode;
        let inner = text_node("Scrolled", 0, 0, 12, Color::rgb(0, 0, 0));
        let scroll = Scene::Scroll(
            ScrollNode::new(Rect::new(10, 10, 80, 80), inner).with_offset(0, 20),
        );
        let pdf = render_scene(&scroll, PageSize::new(200, 200));
        assert!(pdf.document.contains("W n"), "viewport clip emitted");
        assert!(pdf.document.contains("q\n") && pdf.document.contains("Q\n"), "save/restore");
        assert!(pdf.document.contains("(Scrolled) Tj"), "clipped content rendered");
    }
}
