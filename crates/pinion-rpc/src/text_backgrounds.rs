//! `scene/text_backgrounds` — where each declared text background was actually
//! PAINTED, and whether the text on it is readable (R1546 §5.12 §5.36 §2 #7).
//!
//! A [`TextStyle::bg_color`] is a *declaration*: "these bytes have a background".
//! `scene/snapshot` publishes that, because the declaration is scene data. What
//! it cannot publish is the consequence — the rectangle the painter filled,
//! which exists only after shaping, depends on the resolved font and the wrap
//! width, and is the only thing that answers "is my highlight where I think it
//! is". This method publishes that consequence, from the same derivation the
//! painter replays, so the answer and the pixels cannot disagree.
//!
//! # Against Qt 6.11
//!
//! Qt has the declaration — `QTextCharFormat::setBackground(QBrush)`, read back
//! with `background()`. It has no peer for either half of what is below.
//!
//! - **Where it landed.** The rect Qt fills for a format range is computed
//!   inside `QTextLayout::draw`, per glyph run, and is never stored or
//!   returned; `QTextCharFormat` knows the brush and nothing about geometry.
//!   To find it a Qt application re-derives it: `QTextLine::cursorToX` at each
//!   end, the line's own `rect()` for the vertical extent, and its own handling
//!   of a range that spans lines or reverses under bidi. That re-derivation is
//!   a second implementation of the painter's, free to disagree with it — which
//!   is exactly the class of bug an introspection surface exists to remove.
//! - **Whether it reads.** Qt will paint any brush behind any pen and say
//!   nothing. `contrast` is the WCAG 2.x ratio of the run's foreground against
//!   the background actually painted under it, so "no highlight in this
//!   application drops below 4.5:1" is a property a test can assert in one
//!   call. Nothing in Qt computes it; the application is on its own.
//!
//! # Wire shape
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "bands": [
//!       { "tag": "note", "start": 4, "end": 9,
//!         "x": 36, "y": 20, "width": 41, "height": 19,
//!         "color": { "r": 255, "g": 241, "b": 118, "a": 255 },
//!         "fg_color": { "r": 0, "g": 0, "b": 0, "a": 255 },
//!         "contrast": 17.42 }
//!     ]
//!   }
//! }
//! ```
//!
//! Request — no parameters; the method reads the last painted scene, so the
//! bands it reports are the bands of the frame on screen.
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/text_backgrounds", "id": 1 }
//! ```
//!
//! A binding that has not painted, or one whose text declares no background,
//! answers with an empty list. That is the true answer in both cases and is not
//! an error: "nothing is highlighted" is a legitimate state of a UI.
//!
//! # Coordinates
//!
//! `x` / `y` are window-absolute, with any enclosing `Scene::Scroll` offsets
//! folded in exactly as `Scene::rect_for_tag_absolute` folds them — one
//! convention for "where is this on screen", shared with `scene/bbox` and the
//! focus-ring overlay rather than invented here.
//!
//! A band's vertical extent is the shaped **line box**, whose natural top can
//! sit a pixel or so ABOVE the rect the layout engine gave the text node —
//! parley reports it that way and the glyph ink lives there too, so a band is
//! not guaranteed to be contained in its node's rect. A consumer checking "is
//! this highlight on that line" should test registration (the band's top is
//! within one line height of the node's) rather than containment.
//!
//! # `contrast` may be absent
//!
//! `contrast` is `null` when the background is not opaque, and `contrast_note`
//! then says why. WCAG is defined on rendered colours, and a translucent
//! background's rendered colour depends on whatever is behind it — which this
//! surface does not know, because the backdrop is an arbitrary painted subtree
//! rather than a single colour. Publishing the ratio of the *declared* colour
//! there would be a number that looks authoritative and is wrong in exactly the
//! cases a caller cares about, so it is withheld instead.
//!
//! [`TextStyle::bg_color`]: pinion_core::style::TextStyle::bg_color

use pinion_core::contrast::contrast_ratio;
use pinion_core::scene::{Scene, TextNode, effective_style_at};
use pinion_core::style::Color;
use pinion_text::LayoutCache;
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;

/// One painted background band.
#[derive(Debug, Clone, Serialize)]
pub struct TextBackgroundBand {
    /// The paint tag of the text node the band belongs to, when it has one.
    pub tag: Option<String>,
    /// UTF-8 byte offset of the first byte of the declaration this band was cut
    /// from (inclusive).
    pub start: u32,
    /// UTF-8 byte offset one past its last byte.
    pub end: u32,
    /// Window-absolute x of the band's left edge.
    pub x: i64,
    /// Window-absolute y of the band's top edge.
    pub y: i64,
    /// Painted width.
    pub width: u32,
    /// Painted height — the visual line's box.
    pub height: u32,
    /// The background colour painted.
    pub color: ColorWire,
    /// The foreground the run draws on top of it — resolved for `start`, the
    /// same way the shaper resolves it.
    pub fg_color: ColorWire,
    /// WCAG 2.x contrast of `fg_color` against `color`, `1.0..=21.0`. `null`
    /// when `color` is translucent; see `contrast_note` and the module doc.
    pub contrast: Option<f64>,
    /// Why `contrast` is absent, when it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contrast_note: Option<String>,
}

/// A colour on the wire — the `{r,g,b,a}` shape every other pinion method uses.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ColorWire {
    /// Red channel, `0..=255`.
    pub r: u8,
    /// Green channel, `0..=255`.
    pub g: u8,
    /// Blue channel, `0..=255`.
    pub b: u8,
    /// Alpha channel, `0..=255`.
    pub a: u8,
}

impl From<Color> for ColorWire {
    fn from(c: Color) -> Self {
        Self {
            r: c.r,
            g: c.g,
            b: c.b,
            a: c.a,
        }
    }
}

/// Response payload for `scene/text_backgrounds`.
#[derive(Debug, Clone, Serialize)]
pub struct TextBackgroundsOutcome {
    /// Every painted band in the scene, in paint order — so a band later in
    /// the list is one a reader would see drawn over an earlier one, should
    /// they overlap.
    pub bands: Vec<TextBackgroundBand>,
}

/// The note `contrast` carries when it is withheld.
const TRANSLUCENT_NOTE: &str = "background is not opaque; its rendered colour depends on the backdrop, \
     which this surface does not know";

/// Typed errors the [`handle_scene_text_backgrounds`] dispatcher can return.
/// The variant name rides in `error.data` so an agent pattern-matches rather
/// than parsing prose.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextBackgroundsError {
    /// The embedder installed no band list on the dispatch context.
    ///
    /// Distinct from an empty list, and the distinction is the useful part: an
    /// empty list means "this frame highlights nothing", while this means "this
    /// host cannot answer" — a host that never shapes (`pinion-tui`) or a
    /// fixture with no shape cache. A caller asserting "no unreadable
    /// highlights" must not read the second as the first.
    TextBackgroundsUnavailable,
}

/// Collect every painted background band in `scene`, in paint order.
///
/// Called by the embedder before dispatch (the `text_cache_stats` pattern),
/// because the answer needs BOTH the painted scene and the shape cache and only
/// the shell holds them together.
///
/// `cache` must be the shell's own — the one the painter shaped through — so
/// every band here is a cache HIT on an entry the frame already derived, not a
/// re-shape. That is what makes this the painter's answer rather than a second
/// opinion that could disagree with the pixels.
#[must_use]
pub fn collect_bands(scene: &Scene, cache: &mut LayoutCache) -> Vec<TextBackgroundBand> {
    // A background is rare, and asking costs a cache lookup plus (on the first
    // ask) a derivation. Skipping the leaves that declare none keeps this
    // method O(highlighted text) rather than O(scene).
    //
    // R1546 — this skip is COST-ONLY and deliberately not asserted. Removing it
    // changes no published answer: a leaf declaring no background derives an
    // empty band list, so the response is identical either way. The only
    // observable difference is `background_builds`, which would then count
    // every text leaf in the scene rather than the highlighted ones — and
    // pinning that number would make an unrelated scene edit fail this method's
    // tests. So the reason lives here rather than in an assertion: a
    // counterfactual that deletes this filter SHOULD pass.
    //
    // R1551 — collected first, then shaped, so the walk's borrow of `scene` and
    // the derivation's mutable borrow of `cache` stay apart.
    let mut leaves: Vec<(&pinion_core::scene::TextNode, i64, i64)> = Vec::new();
    scene.for_each_text_leaf(|t, x, y| {
        if t.style.bg_color.is_some() || t.runs.iter().any(|r| r.style.bg_color.is_some()) {
            leaves.push((t, x, y));
        }
    });
    let mut bands = Vec::new();
    for (t, x, y) in leaves {
        bands_of(t, x, y, cache, &mut bands);
    }
    bands
}

/// Build the `scene/text_backgrounds` response from the bands the embedder
/// collected with [`collect_bands`].
///
/// # Errors
///
/// - [`TextBackgroundsError::TextBackgroundsUnavailable`] — the embedder
///   installed no band list.
/// - A serialization failure, unreachable in practice for owned strings and
///   numbers; surfaced rather than unwrapped so an RPC handler never panics
///   the shell.
pub fn handle_scene_text_backgrounds(
    bands: Option<&[TextBackgroundBand]>,
) -> Result<Value, RpcError> {
    let Some(bands) = bands else {
        return Err(RpcError::invalid_params(
            "text background bands unavailable for this embedder",
        )
        .with_data_string("TextBackgroundsUnavailable"));
    };
    serde_json::to_value(TextBackgroundsOutcome {
        bands: bands.to_vec(),
    })
    .map_err(RpcError::internal_error)
}

/// The bands one painted text leaf contributes.
///
/// `(x_off, y_off)` is the window-absolute offset `Scene::for_each_text_leaf`
/// resolved for this leaf, so a highlight inside a scroll reports where it is
/// on screen rather than where it is in its own content tree.
fn bands_of(
    t: &TextNode,
    x_off: i64,
    y_off: i64,
    cache: &mut LayoutCache,
    out: &mut Vec<TextBackgroundBand>,
) {
    let max_width = if t.rect.w > 0 { Some(t.rect.w) } else { None };
    let derived = cache
        .backgrounds(&t.content, &t.style, &t.runs, max_width)
        .to_vec();
    for band in derived {
        let fg = effective_style_at(&t.style, &t.runs, band.start as usize).fg_color;
        let opaque = band.color.a == u8::MAX;
        out.push(TextBackgroundBand {
            tag: t.tag.as_ref().map(std::string::ToString::to_string),
            start: band.start,
            end: band.end,
            x: x_off + i64::from(t.rect.x) + round_to_i64(band.x),
            y: y_off + i64::from(t.rect.y) + round_to_i64(band.y),
            width: round_to_u32(band.width),
            height: round_to_u32(band.height),
            color: band.color.into(),
            fg_color: fg.into(),
            contrast: opaque.then(|| f64::from(contrast_ratio(fg, band.color))),
            contrast_note: (!opaque).then(|| TRANSLUCENT_NOTE.to_owned()),
        });
    }
}

/// The magnitude a layout coordinate is clamped to before it becomes a wire
/// integer.
///
/// A billion pixels is past any window, any scroll extent and any font size
/// this framework can be given, so the clamp only ever fires on a value that is
/// already nonsense (a NaN's neighbour, an overflowed advance). Bounding it
/// here means the casts below cannot wrap, and a bogus band reports an absurd
/// coordinate rather than a plausible wrong one.
const COORD_LIMIT: f32 = 1e9;

/// Layout-space f32 to a wire integer, rounded to the nearest pixel and
/// clamped. The paint frame is integral, so a band that reports `36` is the
/// column a screenshot comparison will find it in.
#[allow(
    clippy::cast_possible_truncation,
    reason = "clamped to +/-COORD_LIMIT, far inside i64, before the cast"
)]
fn round_to_i64(v: f32) -> i64 {
    if v.is_nan() {
        return 0;
    }
    v.round().clamp(-COORD_LIMIT, COORD_LIMIT) as i64
}

/// As [`round_to_i64`], for a non-negative extent. A negative width is not a
/// value any consumer can act on, so it floors at zero rather than wrapping.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to [0, COORD_LIMIT], far inside u32, before the cast"
)]
fn round_to_u32(v: f32) -> u32 {
    if v.is_nan() {
        return 0;
    }
    v.round().clamp(0.0, COORD_LIMIT) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::{ContainerNode, Rect, ScrollNode, StyleRun, TextNode};
    use pinion_core::style::TextStyle;

    const HL: Color = Color::rgb(0xFF, 0xF1, 0x76);

    fn highlighted(tag: &'static str, content: &str, at: Rect) -> Scene {
        let base = TextStyle::new();
        let mut node = TextNode::new(content.to_owned(), at);
        node.runs = vec![StyleRun::new(0, 3, base.with_bg_color(HL))];
        node.tag = Some(tag.into());
        Scene::Text(node)
    }

    fn bands_of(scene: &Scene) -> Vec<Value> {
        let mut cache = LayoutCache::new();
        let collected = collect_bands(scene, &mut cache);
        let value = handle_scene_text_backgrounds(Some(&collected)).expect("ok");
        value["bands"].as_array().expect("array").clone()
    }

    /// An embedder that installed nothing is told so, rather than being
    /// answered with an empty list — "this host cannot shape" and "this frame
    /// highlights nothing" are different facts and a caller asserting on
    /// readability must not read one as the other.
    #[test]
    fn r1546_an_absent_band_list_is_unavailable_not_empty() {
        let err = handle_scene_text_backgrounds(None).expect_err("unavailable");
        assert_eq!(
            err.data,
            Some(Value::String("TextBackgroundsUnavailable".to_owned())),
        );
    }

    #[test]
    fn an_unpainted_binding_answers_with_an_empty_list() {
        let value = handle_scene_text_backgrounds(Some(&[])).expect("ok");
        assert_eq!(value["bands"].as_array().expect("array").len(), 0);
    }

    #[test]
    fn text_declaring_no_background_contributes_no_band() {
        let scene = Scene::Text(TextNode::new("plain".to_owned(), Rect::new(0, 0, 200, 30)));
        assert_eq!(bands_of(&scene).len(), 0);
    }

    /// The published band carries the tag, the declared byte range, the colour
    /// painted, and a positive extent — the four things a caller needs to
    /// locate a highlight without a screenshot.
    #[test]
    fn r1546_a_declared_background_is_published_with_its_painted_extent() {
        let scene = highlighted("note", "Row label", Rect::new(10, 20, 200, 30));
        let bands = bands_of(&scene);
        assert_eq!(bands.len(), 1, "{bands:?}");
        let b = &bands[0];
        assert_eq!(b["tag"], "note");
        assert_eq!(b["start"], 0);
        assert_eq!(b["end"], 3);
        assert_eq!(b["color"]["r"], 0xFF);
        assert_eq!(b["color"]["a"], 0xFF);
        assert!(b["width"].as_u64().expect("width") > 0);
        assert!(b["height"].as_u64().expect("height") > 0);
    }

    /// The band is reported in window-absolute coordinates: it moves with the
    /// node it belongs to. Asserted as a DIFFERENCE between two placements of
    /// the same text, so it holds without pinning a font's advance.
    #[test]
    fn r1546_the_extent_is_window_absolute() {
        let near = bands_of(&highlighted("n", "Row label", Rect::new(10, 20, 200, 30)));
        let far = bands_of(&highlighted("n", "Row label", Rect::new(70, 45, 200, 30)));
        assert_eq!(
            far[0]["x"].as_i64().expect("x") - near[0]["x"].as_i64().expect("x"),
            60,
        );
        assert_eq!(
            far[0]["y"].as_i64().expect("y") - near[0]["y"].as_i64().expect("y"),
            25,
        );
    }

    /// A scroll offset is folded in, the way every other absolute rect in this
    /// tree folds it — so a highlight inside a scrolled list reports where it
    /// is on screen, not where it would be if nothing had scrolled.
    #[test]
    fn r1546_a_scroll_offset_moves_the_published_band() {
        let build = |offset_y: i32| {
            let inner = highlighted("n", "Row label", Rect::new(0, 100, 200, 30));
            let mut scroll = ScrollNode::new(Rect::new(0, 0, 200, 50), inner);
            scroll.offset_y = offset_y;
            bands_of(&Scene::Scroll(scroll))
        };
        let top = build(0);
        let scrolled = build(40);
        assert_eq!(
            top[0]["y"].as_i64().expect("y") - scrolled[0]["y"].as_i64().expect("y"),
            40,
        );
    }

    /// An opaque background publishes its WCAG ratio; black on this yellow is
    /// well past the 4.5 body-text bar, which is the assertion a caller makes.
    #[test]
    fn r1546_an_opaque_background_publishes_its_contrast() {
        let bands = bands_of(&highlighted("n", "Row label", Rect::new(0, 0, 200, 30)));
        let ratio = bands[0]["contrast"].as_f64().expect("a ratio");
        assert!((4.5..=21.0).contains(&ratio), "ratio {ratio}");
        assert!(bands[0].get("contrast_note").is_none());
    }

    /// A translucent background withholds the ratio and says why, rather than
    /// publishing the declared colour's ratio — which would be authoritative-
    /// looking and wrong by however much the backdrop shows through.
    #[test]
    fn r1546_a_translucent_background_withholds_the_contrast_with_a_reason() {
        let base = TextStyle::new();
        let mut node = TextNode::new("Row label".to_owned(), Rect::new(0, 0, 200, 30));
        node.runs = vec![StyleRun::new(0, 3, base.with_bg_color(HL.with_alpha(0x80)))];
        let bands = bands_of(&Scene::Text(node));
        assert_eq!(bands.len(), 1);
        assert!(bands[0]["contrast"].is_null());
        assert!(
            bands[0]["contrast_note"]
                .as_str()
                .expect("a stated reason")
                .contains("backdrop"),
        );
    }

    /// The foreground published beside the background is the one the SHAPER
    /// resolves for those bytes, not the node's base style — otherwise the
    /// ratio would describe a pairing that is never drawn.
    #[test]
    fn r1546_the_published_foreground_is_the_runs_own() {
        let base = TextStyle::new();
        let mut run = base.clone().with_bg_color(HL);
        run.fg_color = Color::rgb(0x30, 0x30, 0x30);
        let mut node = TextNode::new("Row label".to_owned(), Rect::new(0, 0, 200, 30));
        node.runs = vec![StyleRun::new(0, 3, run)];
        let bands = bands_of(&Scene::Text(node));
        assert_eq!(bands[0]["fg_color"]["r"], 0x30);
    }

    #[test]
    fn r1546_bands_are_reported_in_paint_order() {
        let scene = Scene::Container(ContainerNode::new(vec![
            highlighted("first", "Row label", Rect::new(0, 0, 200, 30)),
            highlighted("second", "Row label", Rect::new(0, 40, 200, 30)),
        ]));
        let bands = bands_of(&scene);
        assert_eq!(bands.len(), 2);
        assert_eq!(bands[0]["tag"], "first");
        assert_eq!(bands[1]["tag"], "second");
    }
}
