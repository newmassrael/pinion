//! R1067 §5.37.11 — production font-load bridge: an OS system font → a parsed,
//! *selected* §5.37 [`Font`].
//!
//! This is the connecting layer the §5.37 self-hosted text engine has lacked.
//! The engine ([`pinion_text_font`]) can shape + rasterize, and the
//! R1063–R1066 paint seam (`paint_adapter::draw_atlased_glyphs`) can turn its
//! glyph atlas into real pixels — but nothing supplied a *production* font: the
//! bundled Noto / Nanum files are §5.37.1 parser fixtures, and committing a
//! production font is disallowed. [`pinion_platform_fonts`] (§5.37.11) enumerates
//! the OS's installed font files; this module is where the parser lives, so this
//! is also where **font selection** happens — picking a default regular sans by
//! the font's OWN parsed metadata (`name` / `OS/2` tables), never by guessing
//! from a filename. Enumeration is OS knowledge (the platform layer); selection
//! is font knowledge (here).
//!
//! [`load_system_font`] is the single connector. The R1068 opt-in `Scene::Text`
//! paint arm ([`crate::paint_adapter::to_vello_with_text_engine`]) consumes a
//! [`SelfHostedTextEngine`] built from it; the parley path stays the default.

use crate::layout::TextMeasure;
use pinion_core::scene::StyleRun;
use pinion_core::style::{LineHeight, TextAlign, TextStyle};
use pinion_text_font::{Font, shape_paragraph_with_fallback};
use std::path::PathBuf;

/// R1068 §5.37 — a self-hosted text engine handle that caches the parsed
/// production [`Font`] across frames, so the opt-in `Scene::Text` paint arm
/// ([`crate::paint_adapter::to_vello_with_text_engine`]) never re-discovers or
/// re-parses the font per frame.
///
/// What this caches and what it does NOT: it holds the parsed [`Font`] (parsing
/// is the per-frame cost this removes). It does **not** yet cache the rasterized
/// glyph atlas — the arm re-runs `render_paragraph_atlased` each paint; a
/// cross-frame glyph-atlas cache is a later campaign step. It also holds a
/// single font today (the R1068 single-style arm); a fallback stack is the
/// multi-font step. Construct once at app / window init via
/// [`SelfHostedTextEngine::from_system_font`].
pub struct SelfHostedTextEngine {
    font: Font,
}

impl SelfHostedTextEngine {
    /// Build an engine from the selected default system font ([`load_system_font`]).
    ///
    /// # Errors
    ///
    /// Returns [`LoadFontError::NoSystemFont`] when no usable system font is found.
    pub fn from_system_font() -> Result<Self, LoadFontError> {
        Ok(Self {
            font: load_system_font()?,
        })
    }

    /// Build an engine from an already-parsed [`Font`] — the deterministic
    /// constructor a test drives with a bundled fixture (system discovery is
    /// environment-dependent), and the seam a future caller uses to supply a
    /// specific (e.g. embedded brand) font.
    #[must_use]
    pub fn from_font(font: Font) -> Self {
        Self { font }
    }

    /// The engine's font.
    #[must_use]
    pub fn font(&self) -> &Font {
        &self.font
    }
}

/// R1070 §5.37 — eligibility SSOT, shared by the paint arm
/// (`crate::paint_adapter::self_hosted_eligible`) and the measure arm
/// ([`SelfHostedTextEngine`]'s [`TextMeasure`] impl).
///
/// These are the NECESSARY conditions for a `Scene::Text` leaf to be both
/// *rendered and sized* by the §5.37 self-hosted engine so paint and measure
/// register exactly — every one is a case the arm would otherwise diverge from
/// parley:
///
/// - **single style** (`runs` empty) — styled runs are the multi-style step;
/// - **no hard line break** (`'\n'`) — the arm is one line, not a paragraph;
/// - **default `Start` alignment** — the arm pens from the box left, so Center /
///   End would shift versus parley;
/// - **`Normal` line height** — the arm's baseline matches parley's natural line
///   box only in `Normal` mode; a fixed / multiplied height moves parley's
///   baseline by leading the arm does not model;
/// - **undecorated** — underline / strikethrough are not drawn by the arm.
///
/// Necessary, not sufficient: both arms additionally decline a single line that
/// would soft-wrap (see [`SelfHostedTextEngine::measure_text`] and
/// `paint_text_self_hosted`). Everything excluded here stays on parley for BOTH
/// measure and paint, so the two never disagree on which path renders a leaf.
#[must_use]
pub fn self_hosted_text_eligible(content: &str, style: &TextStyle, runs: &[StyleRun]) -> bool {
    runs.is_empty()
        && !content.contains('\n')
        && matches!(style.text_align, TextAlign::Start)
        && matches!(style.line_height, LineHeight::Normal)
        && !style.decoration.underline
        && !style.decoration.strikethrough
}

/// R1070 §5.37 — opt-in self-hosted MEASURE arm: size a single-style `Scene::Text`
/// leaf by the §5.37 engine so its box registers with the §5.37 paint arm
/// ([`crate::paint_adapter::to_vello_with_text_engine`]), closing the R1068
/// paint-only gap where a string wider than the §5.37 advance overflowed the
/// parley measured box.
impl TextMeasure for SelfHostedTextEngine {
    /// Returns `None` (defer to the parley measure — byte-identical to pre-R1070)
    /// when the leaf is ineligible ([`self_hosted_text_eligible`]); the font is
    /// malformed (`units_per_em` 0) or the size non-positive; nothing shapes
    /// (empty content); or the single line would soft-wrap inside a bounded
    /// `max_width`. That soft-wrap decline is the exact mirror of
    /// `paint_text_self_hosted`'s — so whenever measure picks §5.37 the paint arm
    /// also paints §5.37 (advance ≤ box, no overflow), and whenever measure defers
    /// the paint arm declines too (advance > box). All-whitespace content shapes to
    /// blank glyphs: measure sizes its whitespace advance while the paint arm
    /// declines on no ink — harmless, since nothing inks to overflow.
    ///
    /// On the §5.37 path the box is `(advance, line_box_height)` where
    /// `line_box_height = (ascender − descender + line_gap)·px/upem` — the same
    /// `Normal` line box whose first baseline `(ascender + line_gap/2)·px/upem` the
    /// paint arm pens at, so the ascent above and the descent + remaining
    /// half-leading below fit the box exactly (full vertical coherence).
    fn measure_text(
        &self,
        content: &str,
        style: &TextStyle,
        runs: &[StyleRun],
        max_width: Option<u32>,
    ) -> Option<(f32, f32)> {
        if !self_hosted_text_eligible(content, style, runs) {
            return None;
        }
        let font = &self.font;
        // `units_per_em` is read from `head` at parse; 0 would be a malformed font.
        let upem = f64::from(font.units_per_em());
        if upem <= 0.0 {
            return None;
        }
        #[allow(
            clippy::cast_precision_loss,
            reason = "font_size_px <= 2^24 px in practice — exact in f32"
        )]
        let px = style.font_size_px as f32;
        if px <= 0.0 {
            return None;
        }
        let shaped = shape_paragraph_with_fallback(&[font], content, px);
        if shaped.glyphs.is_empty() {
            return None;
        }
        // Soft-wrap decline mirror: a bounded box the single §5.37 line overflows is
        // exactly what parley wraps to multiple lines — defer so the measured height
        // reflects the wrap (and the paint arm, seeing advance > rect.w, also declines).
        if let Some(w) = max_width {
            #[allow(
                clippy::cast_precision_loss,
                reason = "max_width <= 2^24 px in practice — exact in f32"
            )]
            let bound = w as f32;
            if shaped.advance > bound {
                return None;
            }
        }
        // §5.37 line box height from the font's own vertical metrics (descender is
        // negative, so `ascender − descender + line_gap` is the full em box).
        let line_box_height = (f64::from(font.ascender()) - f64::from(font.descender())
            + f64::from(font.line_gap()))
            * f64::from(px)
            / upem;
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a single line box height is a small positive px value — fits f32"
        )]
        let line_box_height = line_box_height as f32;
        Some((shaped.advance, line_box_height))
    }
}

/// Why [`load_system_font`] could not produce a [`Font`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadFontError {
    /// No usable (enumerable + parseable) font was found in the OS's standard
    /// font directories. On Linux this means the machine has no parseable
    /// `.ttf` / `.otf` installed; on macOS / Windows it is the current (deferred)
    /// state of [`pinion_platform_fonts::system_font_dirs`], which returns no
    /// directories there. Per-candidate parse failures are skipped (a corrupt
    /// font file does not abort selection), so this is the only failure surfaced.
    NoSystemFont,
}

impl std::fmt::Display for LoadFontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSystemFont => {
                f.write_str("no usable system font found in the OS font directories")
            }
        }
    }
}

impl std::error::Error for LoadFontError {}

/// Preferred default sans-serif families, matched against a font's PARSED
/// `name`-table family (authoritative), not its filename. A best-effort "give me
/// a reasonable default UI font"; [`select_default_sans`] falls back to the first
/// regular non-mono face, then the first parseable font, when none is installed.
const PREFERRED_SANS_FAMILIES: &[&str] = &[
    "DejaVu Sans",
    "Noto Sans",
    "Liberation Sans",
    "FreeSans",
    "Ubuntu",
    "Cantarell",
    "Open Sans",
    "Roboto",
    "Arial",
];

/// Enumerate the OS's installed fonts and select a default regular sans, parsed
/// into a §5.37 [`Font`].
///
/// Selection ([`select_default_sans`]) is driven by each candidate's parsed
/// metadata, so the result does not depend on filenames. The font is not cached
/// here — [`SelfHostedTextEngine`] owns the cached handle; a per-frame caller
/// must cache itself.
///
/// # Errors
///
/// [`LoadFontError::NoSystemFont`] when no enumerable, parseable font exists.
pub fn load_system_font() -> Result<Font, LoadFontError> {
    select_default_sans(&pinion_platform_fonts::enumerate_fonts())
        .ok_or(LoadFontError::NoSystemFont)
}

/// Choose a default regular sans from enumerated font-file `paths`, parsing each
/// candidate so the choice rests on the font's own `name` / `OS/2` metadata.
///
/// Walks the (sorted, deterministic) candidates in three tiers: a
/// [`PREFERRED_SANS_FAMILIES`] regular face wins immediately; otherwise the first
/// regular-weight non-monospace face; otherwise the first parseable font. A
/// candidate that fails to parse is skipped (a corrupt file is not fatal).
fn select_default_sans(paths: &[PathBuf]) -> Option<Font> {
    let mut first_regular: Option<Font> = None;
    let mut first_parseable: Option<Font> = None;
    for path in paths {
        let Ok(bytes) = pinion_platform_fonts::read_font_bytes(path) else {
            continue;
        };
        let Ok(font) = Font::from_bytes(bytes) else {
            continue;
        };
        let regular_sans = is_regular(&font) && !font.is_monospace();
        if regular_sans && is_preferred_sans(&font) {
            return Some(font);
        }
        if regular_sans {
            first_regular.get_or_insert(font);
        } else {
            first_parseable.get_or_insert(font);
        }
    }
    first_regular.or(first_parseable)
}

/// `true` when the font's parsed `name`-table family is a [`PREFERRED_SANS_FAMILIES`]
/// member (case-insensitive, exact family match — not a substring of a filename).
fn is_preferred_sans(font: &Font) -> bool {
    font.family_name().is_some_and(|family| {
        PREFERRED_SANS_FAMILIES
            .iter()
            .any(|preferred| family.eq_ignore_ascii_case(preferred))
    })
}

/// `true` when the font declares itself a regular (upright, non-bold) face — read
/// from its `name`-table subfamily (the font's own style label), falling back to
/// the `OS/2` `usWeightClass` (`weight_class`) when the subfamily name is absent.
fn is_regular(font: &Font) -> bool {
    match font.subfamily_name() {
        Some(subfamily) => matches!(
            subfamily.to_ascii_lowercase().as_str(),
            "regular" | "book" | "roman" | "normal"
        ),
        None => font.weight_class() == 400,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTO_FIXTURE: &[u8] =
        include_bytes!("../../pinion-text-font/tests/fonts/NotoSans-Regular.ttf");

    #[test]
    fn selection_predicates_read_parsed_metadata_not_filename() {
        // Deterministic forcing consumer for the selection logic: the bundled
        // NotoSans-Regular fixture's PARSED metadata classifies it as a preferred
        // regular sans. This proves selection rests on the `name`/`OS2` tables —
        // independent of the file's path (the fixture is loaded from bytes).
        let font = Font::from_bytes(NOTO_FIXTURE.to_vec()).expect("parse NotoSans fixture");
        assert!(
            is_preferred_sans(&font),
            "parsed family name should match a preferred sans (got {:?})",
            font.family_name()
        );
        assert!(
            is_regular(&font),
            "parsed subfamily should be regular (got {:?})",
            font.subfamily_name()
        );
        assert!(!font.is_monospace(), "NotoSans is proportional");
    }

    #[test]
    fn load_system_font_yields_a_shapeable_font_when_present() {
        // Forcing consumer (default gate): drive the real production path —
        // enumerate → parse → select → shape — not a call-and-ignore smoke test.
        // A CI runner with fonts installed exercises the whole chain; a font-less
        // environment surfaces `NoSystemFont` (the realgpu seam test carries the
        // pixel-level end-to-end proof). The assertions are font-agnostic (any
        // sans-serif shapes ASCII), so they are deterministic across machines —
        // sidestepping the system-font pixel-metric debt.
        match load_system_font() {
            Ok(font) => {
                assert!(font.num_glyphs() > 0, "a real font has glyphs");
                let shaped = pinion_text_font::shape_paragraph(&font, "Text", 16.0);
                assert!(
                    !shaped.glyphs.is_empty(),
                    "a selected system font must shape ASCII text into glyphs"
                );
            }
            Err(LoadFontError::NoSystemFont) => {
                // Acceptable: no usable font installed in this environment.
            }
        }
    }

    #[test]
    fn load_font_error_displays() {
        // The public error type formats for diagnostics (it is the bridge's
        // surfaced failure mode).
        assert_eq!(
            LoadFontError::NoSystemFont.to_string(),
            "no usable system font found in the OS font directories"
        );
    }
}
