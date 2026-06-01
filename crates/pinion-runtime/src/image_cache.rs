//! R740 §5.16 — vello-side decoded-image cache.
//!
//! The GPU-adjacent half of the first asset-pipeline slice. The
//! backend-agnostic decode (encoded bytes → RGBA8) lives in
//! [`pinion_asset`]; this module owns the *vello* concern: resolving an
//! [`ImageNode::source`](pinion_core::scene::ImageNode) string to a
//! ready-to-draw `peniko::ImageData`, decoded **once** per source and
//! cached for every subsequent frame.
//!
//! The cache is owned by the shell (alongside the text
//! [`LayoutCache`](pinion_text::LayoutCache)) and threaded into
//! [`to_vello`](crate::paint_adapter::to_vello) so the pure paint walker
//! never performs IO on a cache hit — exactly the "codec / decoded-buffer
//! cache is carry-forward and resolved by the consumer rasterizer"
//! contract the [`ImageNode`](pinion_core::scene::ImageNode) doc states.
//!
//! ## Source model (first slice)
//!
//! `source` is treated as a **filesystem path** (the documented `file://`
//! locator form, minus the scheme for now). The file is read + decoded on
//! the first frame that paints it and the result — including a *negative*
//! result (missing file / undecodable) — is cached, so a broken source
//! costs one failed read, not one per frame. `https://` / `memory://`
//! schemes and a binding-registered in-memory store are additive axes for
//! a later round (no consumer yet, [[abstraction-needs-second-consumer]]).
//!
//! The decoded `peniko::ImageData` wraps the `Arc`-shared RGBA8 buffer in
//! a `peniko::Blob`, so each frame's lookup clones only the `Arc` (and the
//! handful of scalar fields), never the pixel data.

use std::collections::HashMap;
use std::sync::Arc;

use pinion_asset::DecodedImage;
use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

/// Per-shell cache mapping an image source string to its decoded
/// `peniko::ImageData` (or `None` when the source could not be loaded /
/// decoded — cached so the failure is not retried every frame).
#[derive(Default)]
pub struct ImageCache {
    entries: HashMap<String, Option<ImageData>>,
}

impl ImageCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self { entries: HashMap::new() }
    }

    /// Resolve `source` to a drawable `peniko::ImageData`, decoding +
    /// caching on the first call. Returns `None` (and caches the miss)
    /// when the source cannot be read or decoded. The returned value is a
    /// cheap clone (the pixel buffer is `Arc`-shared via `peniko::Blob`).
    pub fn resolve(&mut self, source: &str) -> Option<ImageData> {
        if let Some(slot) = self.entries.get(source) {
            return slot.clone();
        }
        let loaded = load_source(source).map(|d| to_image_data(&d));
        self.entries.insert(source.to_owned(), loaded.clone());
        loaded
    }

    /// Number of distinct sources resolved so far (hits + misses).
    /// Diagnostic / test surface.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no source has been resolved yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Insert a pre-decoded image directly under `source`, bypassing the
    /// filesystem. Lets a test (or a future in-memory `memory://`
    /// registrar) seed the cache deterministically without a fixture
    /// file on disk.
    pub fn insert_decoded(&mut self, source: impl Into<String>, image: &DecodedImage) {
        self.entries.insert(source.into(), Some(to_image_data(image)));
    }
}

/// Read + decode a filesystem `source` into a backend-agnostic
/// [`DecodedImage`]. `None` on any read or decode failure (the cache
/// records the miss).
fn load_source(source: &str) -> Option<DecodedImage> {
    let bytes = std::fs::read(source).ok()?;
    pinion_asset::decode_image(&bytes).ok()
}

/// Convert a backend-agnostic [`DecodedImage`] into a `peniko::ImageData`
/// by wrapping its `Arc`-shared RGBA8 buffer in a `peniko::Blob` (no
/// pixel copy). Straight (un-premultiplied) alpha, matching
/// [`pinion_asset`]'s output format.
fn to_image_data(decoded: &DecodedImage) -> ImageData {
    let pixels: Arc<dyn AsRef<[u8]> + Send + Sync> = decoded.pixels().clone();
    ImageData {
        data: Blob::new(pixels),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: decoded.width(),
        height: decoded.height(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded_2x2() -> DecodedImage {
        DecodedImage::from_rgba8(2, 2, vec![10; 2 * 2 * 4]).unwrap()
    }

    #[test]
    fn new_cache_is_empty() {
        let c = ImageCache::new();
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn insert_decoded_then_resolve_hits() {
        let mut c = ImageCache::new();
        c.insert_decoded("memory://x", &decoded_2x2());
        let data = c.resolve("memory://x").expect("seeded source resolves");
        assert_eq!(data.width, 2);
        assert_eq!(data.height, 2);
        assert_eq!(data.format, ImageFormat::Rgba8);
        assert_eq!(data.alpha_type, ImageAlphaType::Alpha);
        // Cloning the resolved data shares the same Blob backing.
        let again = c.resolve("memory://x").unwrap();
        assert_eq!(again.data.as_ref().len(), 2 * 2 * 4);
    }

    #[test]
    fn missing_file_caches_the_miss() {
        let mut c = ImageCache::new();
        assert!(c.resolve("/no/such/file/exists.png").is_none(), "missing → None");
        // The miss is cached (one entry), so a second resolve does not retry IO.
        assert_eq!(c.len(), 1);
        assert!(c.resolve("/no/such/file/exists.png").is_none());
        assert_eq!(c.len(), 1, "second resolve reuses the cached miss");
    }
}
