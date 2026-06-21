//! R50.9 §5.37.9 — glyph atlas (CPU rasterize-once + shelf-pack).
//!
//! The caching layer between the rasterizer (§5.37.8) and a GPU text path: each
//! distinct `(glyph_id, size)` is rasterized at most once, its coverage packed
//! into a single shared grayscale bitmap, and its sub-rect + pen offset handed
//! back. The packed [`GlyphAtlas::alpha`] buffer is exactly the one image a GPU
//! text path uploads once and samples per glyph quad — so this is the last
//! self-hosted (§5.37) layer before paint-wiring replaces the §5.36 swash bridge.
//!
//! # Packing
//!
//! Shelf (row) packing: glyphs fill a row left-to-right until the next one would
//! overflow the atlas width, then a new shelf opens below at the running max row
//! height. Simple, deterministic, and good enough for monotonically-growing glyph
//! caches (no rectangle re-flow). The atlas grows on demand — wider (rare: a
//! glyph exceeds the current width) or taller (a new shelf) — so any glyph size
//! packs without a caller-side bound; nothing is silently dropped.
//!
//! # Scope
//!
//! CPU pack + cache only. GPU texture upload, SDF/MSDF, and eviction (the cache
//! grows unbounded — fine for a single run / a bounded UI glyph set) are later
//! layers. Production paint is still §5.36 swash; this atlas is exercised by
//! [`crate::shape::render_run`] (its first consumer) and tests until paint-wiring.

use crate::Font;
use crate::raster::RasterError;
use std::collections::HashMap;

/// A glyph's packed location in the atlas bitmap plus its pen-origin offset.
///
/// `(x, y, width, height)` is the glyph's sub-rect in [`GlyphAtlas::alpha`];
/// `(left, top)` is the same pen-origin offset a standalone
/// [`crate::Coverage`] carries (device px, baseline = row 0). A blank glyph
/// (space) has `width == height == 0` (nothing packed) but is still cached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasGlyph {
    /// Column of the glyph's left edge in the atlas bitmap.
    pub x: usize,
    /// Row of the glyph's top edge in the atlas bitmap.
    pub y: usize,
    /// Packed glyph width (px); 0 for a blank glyph.
    pub width: usize,
    /// Packed glyph height (px); 0 for a blank glyph.
    pub height: usize,
    /// Device-x of the glyph's left column relative to the pen origin.
    pub left: i32,
    /// Device-y of the glyph's top row relative to the baseline.
    pub top: i32,
}

/// Cache key: glyph id + the exact bit pattern of the requested `px_per_em`, so
/// the same size requested repeatedly is one entry (callers pass a consistent
/// size; bit-identity never merges genuinely different sizes).
type Key = (u16, u32);

/// A grayscale glyph atlas — rasterize each `(glyph, size)` once, pack its
/// coverage into one shared bitmap, return its sub-rect + pen offset.
#[derive(Debug, Clone, Default)]
pub struct GlyphAtlas {
    width: usize,
    height: usize,
    alpha: Vec<u8>,
    entries: HashMap<Key, AtlasGlyph>,
    // Shelf-packing cursor: the current row's left edge, top, and max height.
    shelf_x: usize,
    shelf_y: usize,
    shelf_h: usize,
}

impl GlyphAtlas {
    /// A new atlas whose shelves wrap at `width` px (a target row width — a glyph
    /// wider than this grows the atlas to fit). `width = 0` is allowed; the first
    /// packed glyph grows it.
    #[must_use]
    pub fn new(width: usize) -> Self {
        Self {
            width,
            ..Self::default()
        }
    }

    /// The atlas bitmap width (px).
    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    /// The atlas bitmap height (px) — grows as shelves are added.
    #[must_use]
    pub fn height(&self) -> usize {
        self.height
    }

    /// The packed coverage bitmap, row-major `width × height`, `0..=255`. The
    /// single image a GPU text path uploads.
    #[must_use]
    pub fn alpha(&self) -> &[u8] {
        &self.alpha
    }

    /// Number of distinct `(glyph, size)` entries cached.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no glyph has been inserted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Look up `(glyph_id, px_per_em)`, rasterizing and packing it on a cache
    /// miss. Repeated calls for the same key return the same [`AtlasGlyph`]
    /// without re-rasterizing or re-packing.
    ///
    /// # Errors
    ///
    /// Propagates a [`RasterError`] from the underlying [`Font::rasterize_glyph`]
    /// (pathological size or a not-yet-supported composite).
    pub fn get_or_insert(
        &mut self,
        font: &Font,
        glyph_id: u16,
        px_per_em: f32,
    ) -> Result<AtlasGlyph, RasterError> {
        let key = (glyph_id, px_per_em.to_bits());
        if let Some(entry) = self.entries.get(&key) {
            return Ok(*entry);
        }
        let cov = font.rasterize_glyph(glyph_id, px_per_em)?;
        let entry = if cov.is_empty() {
            // Blank glyph (space): cached so it is not re-rasterized, but nothing
            // is packed — width/height 0. left/top carried for completeness.
            AtlasGlyph {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
                left: cov.left,
                top: cov.top,
            }
        } else {
            let (x, y) = self.pack(cov.width, cov.height);
            for row in 0..cov.height {
                let dst = (y + row) * self.width + x;
                let src = row * cov.width;
                self.alpha[dst..dst + cov.width].copy_from_slice(&cov.alpha[src..src + cov.width]);
            }
            AtlasGlyph {
                x,
                y,
                width: cov.width,
                height: cov.height,
                left: cov.left,
                top: cov.top,
            }
        };
        self.entries.insert(key, entry);
        Ok(entry)
    }

    /// Reserve a `w × h` slot via shelf packing, growing the atlas as needed, and
    /// return its top-left. Existing entries' logical `(x, y)` are preserved
    /// across growth (widening only rewrites row strides; height only appends
    /// rows), so previously-returned [`AtlasGlyph`] sub-rects stay valid.
    fn pack(&mut self, w: usize, h: usize) -> (usize, usize) {
        // Open a new shelf when the glyph overflows the current row (but never
        // for the first glyph on a fresh shelf — that case grows the width).
        if self.shelf_x > 0 && self.shelf_x + w > self.width {
            self.shelf_y += self.shelf_h;
            self.shelf_x = 0;
            self.shelf_h = 0;
        }
        self.ensure_width(self.shelf_x + w);
        self.ensure_height(self.shelf_y + h);
        let (x, y) = (self.shelf_x, self.shelf_y);
        self.shelf_x += w;
        self.shelf_h = self.shelf_h.max(h);
        (x, y)
    }

    /// Widen every existing row to `need` columns (rare — only when a glyph is
    /// wider than the current atlas width). Logical column indices are unchanged.
    fn ensure_width(&mut self, need: usize) {
        if need <= self.width {
            return;
        }
        let mut wider = vec![0u8; need * self.height];
        for row in 0..self.height {
            let src = row * self.width;
            let dst = row * need;
            wider[dst..dst + self.width].copy_from_slice(&self.alpha[src..src + self.width]);
        }
        self.alpha = wider;
        self.width = need;
    }

    /// Append rows so the atlas is at least `need` tall (a new shelf).
    fn ensure_height(&mut self, need: usize) {
        if need <= self.height {
            return;
        }
        self.alpha.resize(self.width * need, 0);
        self.height = need;
    }
}

#[cfg(test)]
mod tests {
    use super::GlyphAtlas;

    #[test]
    fn new_atlas_is_empty() {
        let atlas = GlyphAtlas::new(64);
        assert!(atlas.is_empty());
        assert_eq!(atlas.len(), 0);
        assert_eq!(atlas.width(), 64);
        assert_eq!(atlas.height(), 0);
    }
}
