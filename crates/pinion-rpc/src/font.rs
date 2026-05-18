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

use pinion_text_font::{Font, ParseError};
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
pub fn parse(
    registry: &FontRegistry,
    bytes: Vec<u8>,
) -> Result<ParseOutcome, FontError> {
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
pub fn family_name(
    registry: &FontRegistry,
    font_id: u32,
) -> Result<FamilyNameOutcome, FontError> {
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
    registry
        .get(font_id)
        .ok_or(FontError::NotFound { font_id })
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
}
