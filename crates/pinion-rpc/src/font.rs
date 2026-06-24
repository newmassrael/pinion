//! `font/*` RPC method dispatch (§5.37.2, R50.X.1 minimal 3 method).
//!
//! Three typed handlers expose the §5.37.1 OpenType parser results
//! to AI agents over JSON-RPC 2.0:
//!
//!   * `font/parse(bytes)` → `{ font_id }` — register a font binary
//!   * `font/family_name(font_id)` → `{ name }` — query metadata
//!   * `font/glyph_id_for(font_id, codepoint)` → `{ glyph_id }`
//!
//! [`FontRegistry`] is the per-server stateful handle store. AI
//! agents call `font/parse` once to obtain a `font_id`, then issue
//! follow-up queries with the handle. The registry uses
//! `RwLock<HashMap<u32, Arc<Font>>>` so concurrent reads (the
//! dominant access pattern in introspect workflows) do not serialise,
//! and an `AtomicU32` counter for handle allocation — `1`-indexed,
//! `0` reserved as an invalid sentinel.
//!
//! Wire envelope (JSON-RPC 2.0 per §5.7) lives in [`crate::dispatch`];
//! this module holds only the typed Rust API plus the registry.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};

use pinion_text_font::{
    Component, ComponentArgs, ComponentTransform, Font, Glyph, GlyphHeader, GlyphPoint, ParseError,
};
use serde::{Deserialize, Serialize};

/// Per-server font handle store (§5.37.2 ratify).
///
/// Allocates `u32` handles starting at `1`; `0` is reserved as an
/// invalid sentinel. Handles never recycle within a registry's
/// lifetime — explicit cleanup waits for `font/dispose` (R50.X.3).
pub struct FontRegistry {
    inner: RwLock<HashMap<u32, Arc<Font>>>,
    next_id: AtomicU32,
}

impl FontRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            next_id: AtomicU32::new(1),
        }
    }

    /// Currently registered font count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.read().map_or(0, |g| g.len())
    }

    /// True if no fonts are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn get(&self, id: u32) -> Option<Arc<Font>> {
        self.inner.read().ok()?.get(&id).cloned()
    }

    fn insert(&self, font: Font) -> Result<u32, FontError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if id == u32::MAX {
            return Err(FontError::RegistryExhausted);
        }
        self.inner
            .write()
            .map_err(|_| FontError::RegistryPoisoned)?
            .insert(id, Arc::new(font));
        Ok(id)
    }

    /// Remove a handle from the registry; return whether it existed.
    /// The `0` sentinel is always rejected (returns `Ok(false)`).
    fn remove(&self, id: u32) -> Result<bool, FontError> {
        if id == 0 {
            return Ok(false);
        }
        Ok(self
            .inner
            .write()
            .map_err(|_| FontError::RegistryPoisoned)?
            .remove(&id)
            .is_some())
    }

    /// Snapshot the currently-allocated handles, ascending.
    fn snapshot_ids(&self) -> Result<Vec<u32>, FontError> {
        let mut ids: Vec<u32> = self
            .inner
            .read()
            .map_err(|_| FontError::RegistryPoisoned)?
            .keys()
            .copied()
            .collect();
        ids.sort_unstable();
        Ok(ids)
    }
}

impl Default for FontRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Reasons `font/*` handlers can fail.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontError {
    /// `font_id == 0` (reserved sentinel) or unknown handle.
    NotFound { font_id: u32 },
    /// `glyph_id >= num_glyphs` — out of the font's glyph index range.
    GlyphIdOutOfRange { glyph_id: u16, num_glyphs: u16 },
    /// OpenType parse failed — wraps the [`ParseError`] detail.
    Parse(ParseError),
    /// Counter reached `u32::MAX` — server-lifecycle exhaustion.
    RegistryExhausted,
    /// Internal `RwLock` poisoned (a writer panicked while holding it).
    RegistryPoisoned,
}

impl From<ParseError> for FontError {
    fn from(err: ParseError) -> Self {
        FontError::Parse(err)
    }
}

/// JSON wire shape for `font/parse` params (§5.7 transport).
///
/// The typed Rust API [`parse`] takes the byte vector directly; this
/// struct documents the on-the-wire envelope plus carries the serde
/// `Deserialize` so external Rust callers that drive the dispatcher
/// through raw JSON can round-trip the shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseParams {
    /// OpenType binary payload (TTF/OTF byte stream).
    pub bytes: Vec<u8>,
}

/// Outcome for `font/parse`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseOutcome {
    /// Newly-allocated handle. Use in subsequent `font/*` calls.
    pub font_id: u32,
}

/// Register a font binary and return its handle.
///
/// # Errors
///
/// * [`FontError::Parse`] — malformed OpenType binary.
/// * [`FontError::RegistryExhausted`] — counter wraps at `u32::MAX`.
/// * [`FontError::RegistryPoisoned`] — internal lock poisoned.
pub fn parse(registry: &FontRegistry, bytes: Vec<u8>) -> Result<ParseOutcome, FontError> {
    let font = Font::from_bytes(bytes)?;
    let font_id = registry.insert(font)?;
    Ok(ParseOutcome { font_id })
}

/// JSON wire shape for `font/family_name` params.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FamilyNameParams {
    pub font_id: u32,
}

/// Outcome for `font/family_name`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FamilyNameOutcome {
    /// `None` when the font's `name` table has no family entry.
    pub name: Option<String>,
}

/// Look up the font's family name (`name` table id 1).
///
/// # Errors
///
/// [`FontError::NotFound`] when `font_id` is `0` or unknown.
pub fn family_name(registry: &FontRegistry, font_id: u32) -> Result<FamilyNameOutcome, FontError> {
    let font = lookup(registry, font_id)?;
    Ok(FamilyNameOutcome {
        name: font.family_name(),
    })
}

/// JSON wire shape for `font/glyph_id_for` params.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GlyphIdForParams {
    pub font_id: u32,
    pub codepoint: u32,
}

/// Outcome for `font/glyph_id_for`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlyphIdForOutcome {
    /// `None` when no `cmap` subtable maps the codepoint; `Some(0)` =
    /// `.notdef` (explicit fallback glyph).
    pub glyph_id: Option<u16>,
}

/// Map a Unicode codepoint to a glyph id via the font's best `cmap`
/// subtable.
///
/// # Errors
///
/// [`FontError::NotFound`] when `font_id` is `0` or unknown.
pub fn glyph_id_for(
    registry: &FontRegistry,
    font_id: u32,
    codepoint: u32,
) -> Result<GlyphIdForOutcome, FontError> {
    let font = lookup(registry, font_id)?;
    Ok(GlyphIdForOutcome {
        glyph_id: font.glyph_id_for(codepoint),
    })
}

/// Shared handle resolver — rejects the `0` sentinel and unknown ids.
fn lookup(registry: &FontRegistry, font_id: u32) -> Result<Arc<Font>, FontError> {
    if font_id == 0 {
        return Err(FontError::NotFound { font_id: 0 });
    }
    registry.get(font_id).ok_or(FontError::NotFound { font_id })
}

// ─── R50.X.2 extended methods (§5.37.2 R50.X.2) ────────────────────────────

/// JSON wire shape for `font/glyph_outline` params.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GlyphOutlineParams {
    pub font_id: u32,
    pub glyph_id: u16,
}

/// Outcome for `font/glyph_outline` — mirror of [`Glyph`] with serde
/// support. `pinion-text-font` deliberately stays serde-free (§5.37.1
/// 외부 lib 0 정신), so the wire shape lives here and is constructed via
/// [`From<&Glyph>`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum GlyphOutlineOutcome {
    /// `loca[i] == loca[i+1]` — glyph has no visual representation.
    Empty,
    /// Flattened contour glyph with absolute-coordinate points.
    Simple {
        header: GlyphHeaderInfo,
        end_pts_of_contours: Vec<u16>,
        instructions: Vec<u8>,
        points: Vec<GlyphPointInfo>,
    },
    /// Composite glyph referring to other glyph indices via component
    /// records. `raw_body` is deliberately omitted — internal source-
    /// of-truth, AI agents query the parsed `components` view.
    Composite {
        header: GlyphHeaderInfo,
        components: Vec<ComponentInfo>,
        instructions: Vec<u8>,
    },
}

/// Bounding-box header (`x_min`, `y_min`, `x_max`, `y_max`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlyphHeaderInfo {
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
}

/// One outline control point — absolute-coordinate `(x, y)` + on/off
/// curve marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlyphPointInfo {
    pub x: i16,
    pub y: i16,
    pub on_curve: bool,
}

/// Composite component record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentInfo {
    pub flags: u16,
    pub glyph_index: u16,
    pub args: ComponentArgsInfo,
    pub transform: ComponentTransformInfo,
}

/// Component placement arguments (offset vs point-match).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "tag")]
pub enum ComponentArgsInfo {
    Offset { x: i32, y: i32 },
    PointMatch { parent: u32, child: u32 },
}

/// Component transform matrix (raw F2DOT14 `i16` — divide by 16384 for
/// the floating-point value).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "tag")]
pub enum ComponentTransformInfo {
    Identity,
    Scale { scale: i16 },
    XYScale { x: i16, y: i16 },
    Matrix { xx: i16, xy: i16, yx: i16, yy: i16 },
}

impl From<&GlyphHeader> for GlyphHeaderInfo {
    fn from(h: &GlyphHeader) -> Self {
        Self {
            x_min: h.x_min,
            y_min: h.y_min,
            x_max: h.x_max,
            y_max: h.y_max,
        }
    }
}

impl From<&GlyphPoint> for GlyphPointInfo {
    fn from(p: &GlyphPoint) -> Self {
        Self {
            x: p.x,
            y: p.y,
            on_curve: p.on_curve,
        }
    }
}

impl From<&ComponentArgs> for ComponentArgsInfo {
    fn from(a: &ComponentArgs) -> Self {
        match *a {
            ComponentArgs::Offset { x, y } => Self::Offset { x, y },
            ComponentArgs::PointMatch { parent, child } => Self::PointMatch { parent, child },
        }
    }
}

impl From<&ComponentTransform> for ComponentTransformInfo {
    fn from(t: &ComponentTransform) -> Self {
        match *t {
            ComponentTransform::Identity => Self::Identity,
            ComponentTransform::Scale { scale } => Self::Scale { scale },
            ComponentTransform::XYScale { x, y } => Self::XYScale { x, y },
            ComponentTransform::Matrix { xx, xy, yx, yy } => Self::Matrix { xx, xy, yx, yy },
        }
    }
}

impl From<&Component> for ComponentInfo {
    fn from(c: &Component) -> Self {
        Self {
            flags: c.flags,
            glyph_index: c.glyph_index,
            args: (&c.args).into(),
            transform: (&c.transform).into(),
        }
    }
}

impl From<&Glyph> for GlyphOutlineOutcome {
    fn from(g: &Glyph) -> Self {
        match g {
            Glyph::Empty => Self::Empty,
            Glyph::Simple(s) => Self::Simple {
                header: (&s.header).into(),
                end_pts_of_contours: s.end_pts_of_contours.clone(),
                instructions: s.instructions.clone(),
                points: s.points.iter().map(Into::into).collect(),
            },
            Glyph::Composite(c) => Self::Composite {
                header: (&c.header).into(),
                components: c.components.iter().map(Into::into).collect(),
                instructions: c.instructions.clone(),
            },
        }
    }
}

/// Look up a glyph's outline by id.
///
/// # Errors
///
/// * [`FontError::NotFound`] — `font_id` is `0` or unknown.
/// * [`FontError::GlyphIdOutOfRange`] — `glyph_id >= num_glyphs`.
pub fn glyph_outline(
    registry: &FontRegistry,
    font_id: u32,
    glyph_id: u16,
) -> Result<GlyphOutlineOutcome, FontError> {
    let font = lookup(registry, font_id)?;
    let num_glyphs = font.num_glyphs();
    let glyph = font
        .glyph_outline(glyph_id)
        .ok_or(FontError::GlyphIdOutOfRange {
            glyph_id,
            num_glyphs,
        })?;
    Ok(glyph.into())
}

/// JSON wire shape for `font/cmap_subtables` params.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CmapSubtablesParams {
    pub font_id: u32,
}

/// One row in [`CmapSubtablesOutcome::subtables`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CmapSubtableInfo {
    pub platform_id: u16,
    pub encoding_id: u16,
    /// Subtable format header (uint16 at `subtable_offset`).
    pub format: u16,
    /// `true` if the parser recognised the format and parsed the
    /// subtable (Format 0 / 4 / 12 today); `false` for unsupported
    /// formats whose header is still surfaced for diagnostics.
    pub supported: bool,
}

/// Outcome for `font/cmap_subtables`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CmapSubtablesOutcome {
    pub version: u16,
    pub subtables: Vec<CmapSubtableInfo>,
}

/// List the `cmap` table's encoding records with parser support flags.
///
/// # Errors
///
/// [`FontError::NotFound`] — `font_id` is `0` or unknown.
pub fn cmap_subtables(
    registry: &FontRegistry,
    font_id: u32,
) -> Result<CmapSubtablesOutcome, FontError> {
    let font = lookup(registry, font_id)?;
    let subtables = font
        .cmap
        .encodings
        .iter()
        .enumerate()
        .map(|(i, enc)| CmapSubtableInfo {
            platform_id: enc.platform_id,
            encoding_id: enc.encoding_id,
            format: enc.subtable_format,
            supported: font
                .cmap
                .subtables
                .get(i)
                .and_then(Option::as_ref)
                .is_some(),
        })
        .collect();
    Ok(CmapSubtablesOutcome {
        version: font.cmap.version,
        subtables,
    })
}

/// JSON wire shape for `font/metrics` params.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MetricsParams {
    pub font_id: u32,
}

/// Aggregate font metrics from `head` / `hhea` / `maxp` / `OS/2` /
/// `post`. All values are raw design-space units; convert to pixels
/// via `(value * font_size) / units_per_em`.
///
/// `ascender` / `descender` / `line_gap` are the **raw `hhea`** table
/// values — NOT the `OS/2` `USE_TYPO_METRICS`-selected line-box metrics the
/// §5.37 engine uses for line layout (R1079). The two differ for a font
/// like `NanumGothic` that sets `USE_TYPO_METRICS` with typo metrics ≠ hhea;
/// the selector is `Font::vertical_line_metrics`, and the rendered line box
/// is observable as a laid-out `Scene::Text` rect via `scene/layout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsOutcome {
    pub units_per_em: u16,
    pub ascender: i16,
    pub descender: i16,
    pub line_gap: i16,
    pub num_glyphs: u16,
    pub weight_class: u16,
    pub is_monospace: bool,
}

/// Read the aggregate font metrics.
///
/// # Errors
///
/// [`FontError::NotFound`] — `font_id` is `0` or unknown.
pub fn metrics(registry: &FontRegistry, font_id: u32) -> Result<MetricsOutcome, FontError> {
    let font = lookup(registry, font_id)?;
    Ok(MetricsOutcome {
        units_per_em: font.units_per_em(),
        ascender: font.ascender(),
        descender: font.descender(),
        line_gap: font.line_gap(),
        num_glyphs: font.num_glyphs(),
        weight_class: font.weight_class(),
        is_monospace: font.is_monospace(),
    })
}

/// JSON wire shape for the three sibling name-accessor params
/// (`font/subfamily_name`, `font/full_name`, `font/postscript_name`).
/// Identical shape to [`FamilyNameParams`].
pub type NameAccessorParams = FamilyNameParams;

/// Outcome for `font/subfamily_name` — Name id 2 (typographic subfamily).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubfamilyNameOutcome {
    pub name: Option<String>,
}

/// Look up the typographic subfamily name (`name` table id 2).
///
/// # Errors
///
/// [`FontError::NotFound`] — `font_id` is `0` or unknown.
pub fn subfamily_name(
    registry: &FontRegistry,
    font_id: u32,
) -> Result<SubfamilyNameOutcome, FontError> {
    let font = lookup(registry, font_id)?;
    Ok(SubfamilyNameOutcome {
        name: font.subfamily_name(),
    })
}

/// Outcome for `font/full_name` — Name id 4 (full font name).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FullNameOutcome {
    pub name: Option<String>,
}

/// Look up the full font name (`name` table id 4).
///
/// # Errors
///
/// [`FontError::NotFound`] — `font_id` is `0` or unknown.
pub fn full_name(registry: &FontRegistry, font_id: u32) -> Result<FullNameOutcome, FontError> {
    let font = lookup(registry, font_id)?;
    Ok(FullNameOutcome {
        name: font.full_name(),
    })
}

/// Outcome for `font/postscript_name` — Name id 6 (PostScript name).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostscriptNameOutcome {
    pub name: Option<String>,
}

/// Look up the PostScript name (`name` table id 6).
///
/// # Errors
///
/// [`FontError::NotFound`] — `font_id` is `0` or unknown.
pub fn postscript_name(
    registry: &FontRegistry,
    font_id: u32,
) -> Result<PostscriptNameOutcome, FontError> {
    let font = lookup(registry, font_id)?;
    Ok(PostscriptNameOutcome {
        name: font.postscript_name(),
    })
}

// ─── R50.X.3 lifecycle methods (§5.37.2 R50.X.3) ───────────────────────────

/// JSON wire shape for `font/dispose` params.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DisposeParams {
    pub font_id: u32,
}

/// Outcome for `font/dispose`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisposeOutcome {
    /// `true` if the handle existed and was removed; `false` if the
    /// handle was unknown (or `0`, the reserved sentinel).
    pub existed: bool,
}

/// Drop a font handle from the registry. The `0` sentinel and unknown
/// handles both surface as `existed: false` rather than an error so
/// idempotent cleanup is safe.
///
/// # Errors
///
/// [`FontError::RegistryPoisoned`] — internal lock poisoned.
pub fn dispose(registry: &FontRegistry, font_id: u32) -> Result<DisposeOutcome, FontError> {
    Ok(DisposeOutcome {
        existed: registry.remove(font_id)?,
    })
}

/// Outcome for `font/list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListOutcome {
    /// Currently-allocated handles, ascending.
    pub font_ids: Vec<u32>,
}

/// Snapshot the currently-allocated font handles. Useful for AI
/// agents to enumerate the live registry state and reconcile after
/// session restart.
///
/// # Errors
///
/// [`FontError::RegistryPoisoned`] — internal lock poisoned.
pub fn list(registry: &FontRegistry) -> Result<ListOutcome, FontError> {
    Ok(ListOutcome {
        font_ids: registry.snapshot_ids()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTO_SANS: &[u8] =
        include_bytes!("../../pinion-text-font/tests/fonts/NotoSans-Regular.ttf");

    fn parse_noto(registry: &FontRegistry) -> u32 {
        parse(registry, NOTO_SANS.to_vec())
            .expect("Noto Sans fixture parses")
            .font_id
    }

    #[test]
    fn registry_starts_empty() {
        let registry = FontRegistry::new();
        assert_eq!(registry.len(), 0);
        assert!(registry.is_empty());
    }

    #[test]
    fn parse_noto_sans_allocates_handle() {
        let registry = FontRegistry::new();
        let out = parse(&registry, NOTO_SANS.to_vec()).unwrap();
        assert_ne!(out.font_id, 0, "0 reserved sentinel must not be allocated");
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn parse_rejects_empty_bytes() {
        let registry = FontRegistry::new();
        let err = parse(&registry, vec![]).unwrap_err();
        assert!(matches!(err, FontError::Parse(_)));
        assert_eq!(registry.len(), 0, "failed parse must not occupy a handle");
    }

    #[test]
    fn family_name_noto_sans() {
        let registry = FontRegistry::new();
        let id = parse_noto(&registry);
        let out = family_name(&registry, id).unwrap();
        assert_eq!(out.name.as_deref(), Some("Noto Sans"));
    }

    #[test]
    fn family_name_rejects_zero_id() {
        let registry = FontRegistry::new();
        let err = family_name(&registry, 0).unwrap_err();
        assert_eq!(err, FontError::NotFound { font_id: 0 });
    }

    #[test]
    fn family_name_rejects_unknown_id() {
        let registry = FontRegistry::new();
        let _ = parse_noto(&registry);
        let err = family_name(&registry, 9_999).unwrap_err();
        assert_eq!(err, FontError::NotFound { font_id: 9_999 });
    }

    #[test]
    fn glyph_id_for_letter_a_nonzero() {
        let registry = FontRegistry::new();
        let id = parse_noto(&registry);
        let out = glyph_id_for(&registry, id, 0x0041).unwrap();
        let gid = out.glyph_id.expect("'A' must be mapped in Noto Sans");
        assert_ne!(gid, 0, "'A' must not fall back to .notdef");
    }

    #[test]
    fn glyph_id_for_unmapped_codepoint() {
        // Private-use plane 16 last codepoint — Noto Sans does not
        // cover it, so cmap returns either None or .notdef.
        let registry = FontRegistry::new();
        let id = parse_noto(&registry);
        let out = glyph_id_for(&registry, id, 0x10_FFFE).unwrap();
        assert!(
            out.glyph_id.is_none() || out.glyph_id == Some(0),
            "U+10FFFE unexpectedly mapped: {:?}",
            out.glyph_id
        );
    }

    #[test]
    fn glyph_id_for_rejects_zero_id() {
        let registry = FontRegistry::new();
        let err = glyph_id_for(&registry, 0, 0x0041).unwrap_err();
        assert_eq!(err, FontError::NotFound { font_id: 0 });
    }

    #[test]
    fn handles_are_unique_per_parse() {
        let registry = FontRegistry::new();
        let a = parse_noto(&registry);
        let b = parse_noto(&registry);
        assert_ne!(a, b, "each parse must allocate a fresh handle");
        assert_eq!(registry.len(), 2);
    }

    // ─── R50.X.2 extended method tests ─────────────────────────────────

    #[test]
    fn glyph_outline_notdef_is_simple_for_noto_sans() {
        let registry = FontRegistry::new();
        let id = parse_noto(&registry);
        let outcome = glyph_outline(&registry, id, 0).unwrap();
        // Noto Sans .notdef is a simple hollow box outline.
        assert!(
            matches!(outcome, GlyphOutlineOutcome::Simple { .. }),
            "expected Simple for .notdef, got {outcome:?}",
        );
    }

    #[test]
    fn glyph_outline_letter_a_is_simple_with_points() {
        let registry = FontRegistry::new();
        let id = parse_noto(&registry);
        let gid = glyph_id_for(&registry, id, 0x0041)
            .unwrap()
            .glyph_id
            .unwrap();
        let outcome = glyph_outline(&registry, id, gid).unwrap();
        let GlyphOutlineOutcome::Simple {
            points,
            end_pts_of_contours,
            ..
        } = outcome
        else {
            panic!("'A' should be a simple glyph in Noto Sans, got non-simple");
        };
        assert!(!points.is_empty(), "'A' must have outline points");
        assert!(
            !end_pts_of_contours.is_empty(),
            "'A' must have at least one contour"
        );
    }

    #[test]
    fn glyph_outline_rejects_zero_font_id() {
        let registry = FontRegistry::new();
        let err = glyph_outline(&registry, 0, 0).unwrap_err();
        assert_eq!(err, FontError::NotFound { font_id: 0 });
    }

    #[test]
    fn glyph_outline_rejects_out_of_range_glyph_id() {
        let registry = FontRegistry::new();
        let id = parse_noto(&registry);
        let err = glyph_outline(&registry, id, u16::MAX).unwrap_err();
        assert!(
            matches!(err, FontError::GlyphIdOutOfRange { glyph_id, .. } if glyph_id == u16::MAX),
            "expected GlyphIdOutOfRange for u16::MAX, got {err:?}",
        );
    }

    #[test]
    fn cmap_subtables_noto_sans_lists_encodings() {
        let registry = FontRegistry::new();
        let id = parse_noto(&registry);
        let outcome = cmap_subtables(&registry, id).unwrap();
        assert!(
            !outcome.subtables.is_empty(),
            "Noto Sans must have at least one cmap subtable",
        );
        // Noto Sans must carry a supported (Format 0 / 4 / 12) subtable.
        let any_supported = outcome.subtables.iter().any(|s| s.supported);
        assert!(
            any_supported,
            "Noto Sans must have a parser-supported cmap subtable"
        );
    }

    #[test]
    fn metrics_noto_sans_units_per_em_1000() {
        let registry = FontRegistry::new();
        let id = parse_noto(&registry);
        let m = metrics(&registry, id).unwrap();
        assert_eq!(m.units_per_em, 1000, "Noto Sans uses 1000 UPEM");
        assert!(m.ascender > 0);
        assert!(m.descender < 0);
        assert!(m.num_glyphs > 0);
        assert_eq!(m.weight_class, 400, "Noto Sans Regular weight class is 400");
        assert!(!m.is_monospace);
    }

    #[test]
    fn subfamily_name_noto_sans_is_regular() {
        let registry = FontRegistry::new();
        let id = parse_noto(&registry);
        let out = subfamily_name(&registry, id).unwrap();
        assert_eq!(out.name.as_deref(), Some("Regular"));
    }

    #[test]
    fn full_name_noto_sans_contains_family() {
        let registry = FontRegistry::new();
        let id = parse_noto(&registry);
        let out = full_name(&registry, id).unwrap();
        let name = out.name.expect("full name present");
        assert!(name.contains("Noto Sans"), "full name = {name:?}");
    }

    #[test]
    fn postscript_name_noto_sans_starts_with_notosans() {
        let registry = FontRegistry::new();
        let id = parse_noto(&registry);
        let out = postscript_name(&registry, id).unwrap();
        let name = out.name.expect("postscript name present");
        assert!(name.starts_with("NotoSans"), "postscript name = {name:?}");
    }

    #[test]
    fn extended_methods_reject_unknown_font_id() {
        let registry = FontRegistry::new();
        for err in [
            glyph_outline(&registry, 999, 0).unwrap_err(),
            cmap_subtables(&registry, 999).unwrap_err(),
            metrics(&registry, 999).unwrap_err(),
            subfamily_name(&registry, 999).unwrap_err(),
            full_name(&registry, 999).unwrap_err(),
            postscript_name(&registry, 999).unwrap_err(),
        ] {
            assert_eq!(err, FontError::NotFound { font_id: 999 });
        }
    }

    // ─── R50.X.3 lifecycle tests ───────────────────────────────────────

    #[test]
    fn dispose_removes_known_handle() {
        let registry = FontRegistry::new();
        let id = parse_noto(&registry);
        let out = dispose(&registry, id).unwrap();
        assert!(out.existed);
        assert_eq!(registry.len(), 0);
        // After dispose, lookups fail.
        let err = family_name(&registry, id).unwrap_err();
        assert_eq!(err, FontError::NotFound { font_id: id });
    }

    #[test]
    fn dispose_unknown_handle_is_idempotent() {
        let registry = FontRegistry::new();
        let out = dispose(&registry, 9_999).unwrap();
        assert!(!out.existed);
    }

    #[test]
    fn dispose_zero_sentinel_is_idempotent() {
        let registry = FontRegistry::new();
        let out = dispose(&registry, 0).unwrap();
        assert!(!out.existed);
    }

    #[test]
    fn dispose_does_not_recycle_handles() {
        let registry = FontRegistry::new();
        let first = parse_noto(&registry);
        dispose(&registry, first).unwrap();
        let second = parse_noto(&registry);
        assert!(
            second > first,
            "next_id must continue ascending after dispose ({first} → {second})",
        );
    }

    #[test]
    fn list_empty_registry_returns_empty_vec() {
        let registry = FontRegistry::new();
        let out = list(&registry).unwrap();
        assert!(out.font_ids.is_empty());
    }

    #[test]
    fn list_returns_ascending_handles() {
        let registry = FontRegistry::new();
        let a = parse_noto(&registry);
        let b = parse_noto(&registry);
        let c = parse_noto(&registry);
        let out = list(&registry).unwrap();
        assert_eq!(out.font_ids, vec![a, b, c]);
    }

    #[test]
    fn list_excludes_disposed_handles() {
        let registry = FontRegistry::new();
        let a = parse_noto(&registry);
        let b = parse_noto(&registry);
        dispose(&registry, a).unwrap();
        let out = list(&registry).unwrap();
        assert_eq!(out.font_ids, vec![b]);
    }
}
