//! R740 §5.16 — backend-agnostic image-asset decode substrate.
//!
//! The first slice of the asset pipeline (clearing the `Scene::Image`
//! paint no-op). This crate owns the *backend-agnostic* half: turning
//! encoded image bytes (PNG for now) into a flat RGBA8 buffer plus its
//! dimensions. It deliberately knows nothing about GPUs, vello, or
//! peniko — the vello-side cache (`pinion_runtime::image_cache`) wraps a
//! [`DecodedImage`]'s `Arc`-shared pixels in a `peniko::Blob` at paint
//! time, so the decoded buffer is shared (never re-copied per frame) and
//! a future TUI / Phase-C GPU-upload consumer reuses the same decode.
//!
//! ## Scope (honest)
//!
//! - **PNG only.** `image`'s `png` feature is the only one enabled. JPEG
//!   / WebP / GIF are additive `image` features added behind a real
//!   consumer ([[abstraction-needs-second-consumer]]).
//! - **Synchronous.** Decode is a blocking CPU call. The render path
//!   decodes once per source and caches the result, so the blocking cost
//!   is paid a single time. Async / streaming loading (progress, network
//!   sources) is a Phase-C axis, not this slice.
//! - **RGBA8, straight (un-premultiplied) alpha.** The single canonical
//!   in-memory format; the vello side declares `ImageAlphaType::Alpha`
//!   to match.

use std::sync::Arc;

/// A decoded, ready-to-upload raster image: a flat RGBA8 pixel buffer
/// (`width * height * 4` bytes, row-major, straight alpha) plus its
/// dimensions.
///
/// `pixels` is `Arc`-shared so the vello-side cache can wrap it in a
/// `peniko::Blob` without copying, and so multiple `Scene::Image` nodes
/// referencing the same decoded source share one buffer.
#[derive(Clone, Debug)]
pub struct DecodedImage {
    width: u32,
    height: u32,
    pixels: Arc<Vec<u8>>,
}

impl DecodedImage {
    /// Width in pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The flat RGBA8 buffer (`width * height * 4` bytes, row-major,
    /// straight alpha).
    #[must_use]
    pub fn pixels(&self) -> &Arc<Vec<u8>> {
        &self.pixels
    }

    /// Construct directly from a known-good RGBA8 buffer. Used by the
    /// vello cache after a decode and by tests; `decode_image` is the
    /// normal entry point. Returns `None` when the buffer length does
    /// not match `width * height * 4` (the invariant downstream GPU
    /// upload relies on).
    #[must_use]
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Option<Self> {
        let expected = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        if pixels.len() != expected {
            return None;
        }
        Some(Self {
            width,
            height,
            pixels: Arc::new(pixels),
        })
    }
}

/// Why a decode failed. Kept small and `non_exhaustive` so additional
/// codecs can add variants without breaking callers.
#[non_exhaustive]
#[derive(Debug)]
pub enum DecodeError {
    /// The `image` crate could not decode the bytes (corrupt data,
    /// unsupported format, truncated file). Carries the underlying
    /// message for diagnostics.
    Decode(String),
    /// The decoded dimensions overflow the `width * height * 4` buffer
    /// size computation (a defensive guard against malformed headers).
    DimensionOverflow,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decode(msg) => write!(f, "image decode failed: {msg}"),
            Self::DimensionOverflow => write!(f, "image dimensions overflow buffer size"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Decode encoded image bytes (PNG) into a straight-alpha RGBA8
/// [`DecodedImage`].
///
/// The format is sniffed from the byte content by the `image` crate, so
/// the caller does not pass a hint; only the `png` feature is compiled
/// in, so non-PNG bytes return [`DecodeError::Decode`] rather than
/// silently mis-decoding.
///
/// # Errors
///
/// Returns [`DecodeError::Decode`] when the bytes are not a decodable
/// image, or [`DecodeError::DimensionOverflow`] when the decoded
/// dimensions overflow the buffer-size computation.
pub fn decode_image(bytes: &[u8]) -> Result<DecodedImage, DecodeError> {
    let decoded = image::load_from_memory(bytes).map_err(|e| DecodeError::Decode(e.to_string()))?;
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    DecodedImage::from_rgba8(width, height, rgba.into_raw()).ok_or(DecodeError::DimensionOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2x2 RGBA8 test image encoded as PNG, built at test time so the
    /// decode round-trip is exercised end to end without a fixture file.
    fn encode_2x2_png() -> Vec<u8> {
        use image::ImageEncoder;
        // Red / green / blue / opaque-white, one pixel each.
        let raw: Vec<u8> = vec![
            255, 0, 0, 255, // (0,0) red
            0, 255, 0, 255, // (1,0) green
            0, 0, 255, 255, // (0,1) blue
            255, 255, 255, 255, // (1,1) white
        ];
        let mut out = Vec::new();
        {
            let encoder = image::codecs::png::PngEncoder::new(&mut out);
            encoder
                .write_image(&raw, 2, 2, image::ExtendedColorType::Rgba8)
                .expect("encode 2x2 png");
        }
        out
    }

    #[test]
    fn decodes_png_to_rgba8() {
        let png = encode_2x2_png();
        let img = decode_image(&png).expect("valid png decodes");
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);
        assert_eq!(img.pixels().len(), 2 * 2 * 4, "RGBA8 buffer = w*h*4");
        // Top-left pixel is the red we encoded (straight alpha preserved).
        assert_eq!(&img.pixels()[0..4], &[255, 0, 0, 255]);
        // Bottom-right is opaque white.
        assert_eq!(&img.pixels()[12..16], &[255, 255, 255, 255]);
    }

    #[test]
    fn rejects_non_image_bytes() {
        let err = decode_image(b"not an image at all").unwrap_err();
        assert!(
            matches!(err, DecodeError::Decode(_)),
            "garbage bytes → Decode error"
        );
    }

    #[test]
    fn from_rgba8_enforces_buffer_length() {
        // 2x2 needs 16 bytes; a 15-byte buffer is rejected.
        assert!(DecodedImage::from_rgba8(2, 2, vec![0; 15]).is_none());
        assert!(DecodedImage::from_rgba8(2, 2, vec![0; 16]).is_some());
    }

    #[test]
    fn shared_pixels_clone_is_cheap_arc() {
        let img = DecodedImage::from_rgba8(1, 1, vec![1, 2, 3, 4]).unwrap();
        let clone = img.clone();
        // Same backing allocation (Arc shared, not copied).
        assert!(Arc::ptr_eq(img.pixels(), clone.pixels()));
    }
}
