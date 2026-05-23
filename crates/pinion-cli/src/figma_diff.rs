//! R638 §5.7 — `pinion figma-diff` sub-command.
//!
//! Pixel-diff side of the Figma → pinion design-parity loop. Pre-R638
//! the workflow stopped after R637 saved the pinion-rendered PNG —
//! comparing it against the Figma reference (R636 output) required
//! eyeballing the two images in an external viewer. R638 closes the
//! loop with a typed CLI that decodes both PNGs, optionally resizes
//! one to match the other, computes per-channel MAE / max-delta /
//! exact-match-percent, and writes an RGB diff visualization PNG.
//!
//! ## Wire shape
//!
//! ```text
//! $ pinion figma-diff /tmp/pinion-btn.png /tmp/figma-btn-ref.png \
//!     --resize b-to-a -o /tmp/btn-diff.png
//! image-a: 320 x 160 RGBA
//! image-b: 218 x 80 RGBA (resized to 320 x 160 via Lanczos3)
//! mean abs delta: R=12.4 G=11.1 B=13.7 A=0.0
//! max abs delta:  R=247 G=234 B=255 A=0
//! exact match:    61.2% (31_309 / 51_200 pixels)
//! wrote diff visualization: /tmp/btn-diff.png
//! ```
//!
//! ## Dim mismatch policy
//!
//! The diff is only meaningful when both images share dimensions.
//! - Default: dim mismatch is a hard error (the canonical case is two
//!   bit-aligned images from the same logical render).
//! - `--resize a-to-b`: resample image-a to image-b's dimensions
//!   (Lanczos3 — image-rs `imageops::resize` with `FilterType::Lanczos3`).
//! - `--resize b-to-a`: resample image-b to image-a's dimensions
//!   (the Figma → pinion workflow default: pinion's 320×160 canvas
//!   carries the framing, Figma's tight 109×40 ref gets upscaled).
//!
//! ## Diff visualization
//!
//! Per-pixel summed abs channel delta `d = |dr| + |dg| + |db|` mapped
//! to greyscale → red-channel intensity. `d = 0` (identical) →
//! white; `d = 765` (max possible 3 × 255) → saturated red. Output
//! is RGB (no alpha) so the diff stays single-purpose visual debug
//! aid, not a substitute for the source image.
//!
//! ## Metrics rationale
//!
//! - **Per-channel MAE** — directly comparable to Figma's documented
//!   color spec (`#675AA4` = R=103 G=80 B=164); deviation per channel
//!   surfaces "wrong color" vs "wrong placement" classifications.
//! - **Max abs delta** — single-pixel outliers (font-rendering edge
//!   antialiasing, single-bit rounding); high max + low MAE = "small
//!   localized difference"; high MAE = "wholesale mismatch".
//! - **Exact-match percent** — bit-identical pixel count; useful for
//!   regression guards (R635-class static binding should stay near
//!   100% after substrate changes).
//!
//! SSIM (`imageproc::stats::ssim`) is intentionally deferred — MAE
//! plus exact-match-percent surfaces Figma → pinion divergence in
//! every case observed so far (R635 first binding land); SSIM gains
//! signal only when "perceptual similarity" outranks per-channel
//! bit-exactness, which is not yet the design-parity loop's focus.

use std::path::PathBuf;

use clap::Args;
use image::imageops::FilterType;
use image::{DynamicImage, ImageReader, RgbImage, RgbaImage};

/// Which side to resample when dimensions differ. `None` is the
/// canonical case (matching dims) and surfaces a typed error when
/// the images mismatch; the two `Some` variants opt in to Lanczos3
/// resampling toward the target dim.
#[derive(Debug, Clone, Copy)]
pub enum ResizeMode {
    /// Resample image-a to image-b's dimensions.
    AtoB,
    /// Resample image-b to image-a's dimensions.
    BtoA,
}

impl std::str::FromStr for ResizeMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "a-to-b" => Ok(Self::AtoB),
            "b-to-a" => Ok(Self::BtoA),
            other => Err(format!(
                "unknown resize mode {other:?}; expected `a-to-b` or `b-to-a`"
            )),
        }
    }
}

/// Arguments for the `figma-diff` sub-command.
#[derive(Args)]
pub struct FigmaDiffArgs {
    /// First image (canonical: pinion output from R637
    /// `PINION_SCREENSHOT=...`).
    pub image_a: PathBuf,

    /// Second image (canonical: Figma reference from R636
    /// `pinion figma-fetch-image`).
    pub image_b: PathBuf,

    /// Output path for the diff visualization PNG. When omitted only
    /// the per-channel metrics are printed (no diff PNG written).
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Resize one image to match the other's dimensions via Lanczos3
    /// resampling. Omit to require matching dims (typed error on
    /// mismatch).
    #[arg(long)]
    pub resize: Option<ResizeMode>,
}

/// Per-channel diff metrics surfaced both to stderr and as the
/// return value for downstream consumers (future RPC wrapper, etc.).
#[derive(Debug, Clone, Copy)]
pub struct DiffMetrics {
    /// Mean absolute delta per channel: `R`, `G`, `B`, `A`. Range
    /// `[0.0, 255.0]`; smaller = closer match.
    pub mean_abs: [f64; 4],
    /// Max absolute delta per channel (single-pixel worst case).
    pub max_abs: [u8; 4],
    /// Pixels with all four channels exactly identical, as a
    /// percentage in `[0.0, 100.0]`.
    pub exact_match_pct: f64,
    /// Pixel count (denominator of `exact_match_pct`).
    pub pixel_count: u64,
    /// Pixels with all four channels exactly identical (numerator
    /// of `exact_match_pct`).
    pub exact_match_count: u64,
}

/// R638 §5.7 — execute the `figma-diff` sub-command.
///
/// # Errors
///
/// - I/O / decode error on either input PNG
/// - Dimension mismatch when `--resize` is omitted
/// - I/O / encode error on the diff PNG output (when `--output`
///   is supplied)
pub fn run(args: &FigmaDiffArgs) -> Result<(), Box<dyn std::error::Error>> {
    let img_a = decode_rgba8(&args.image_a)?;
    let img_b = decode_rgba8(&args.image_b)?;
    eprintln!(
        "image-a: {} x {} RGBA",
        img_a.width(),
        img_a.height()
    );
    let img_a_dims = (img_a.width(), img_a.height());
    let img_b_dims_orig = (img_b.width(), img_b.height());

    let (img_a, img_b) = match (args.resize, img_a_dims == img_b_dims_orig) {
        (_, true) => {
            eprintln!(
                "image-b: {} x {} RGBA",
                img_b.width(),
                img_b.height(),
            );
            (img_a, img_b)
        }
        (None, false) => {
            return Err(format!(
                "dimension mismatch: a={}x{} b={}x{}; pass `--resize a-to-b` \
                 or `--resize b-to-a` to opt in to Lanczos3 resampling",
                img_a_dims.0, img_a_dims.1, img_b_dims_orig.0, img_b_dims_orig.1,
            )
            .into());
        }
        (Some(ResizeMode::BtoA), false) => {
            eprintln!(
                "image-b: {} x {} RGBA (resized to {} x {} via Lanczos3)",
                img_b_dims_orig.0, img_b_dims_orig.1, img_a_dims.0, img_a_dims.1,
            );
            let resized = image::imageops::resize(
                &img_b,
                img_a_dims.0,
                img_a_dims.1,
                FilterType::Lanczos3,
            );
            (img_a, resized)
        }
        (Some(ResizeMode::AtoB), false) => {
            eprintln!(
                "image-b: {} x {} RGBA",
                img_b_dims_orig.0, img_b_dims_orig.1,
            );
            let resized = image::imageops::resize(
                &img_a,
                img_b_dims_orig.0,
                img_b_dims_orig.1,
                FilterType::Lanczos3,
            );
            (resized, img_b)
        }
    };

    let metrics = compute_metrics(&img_a, &img_b);
    eprintln!(
        "mean abs delta: R={:.1} G={:.1} B={:.1} A={:.1}",
        metrics.mean_abs[0], metrics.mean_abs[1], metrics.mean_abs[2], metrics.mean_abs[3],
    );
    eprintln!(
        "max abs delta:  R={} G={} B={} A={}",
        metrics.max_abs[0], metrics.max_abs[1], metrics.max_abs[2], metrics.max_abs[3],
    );
    eprintln!(
        "exact match:    {:.1}% ({} / {} pixels)",
        metrics.exact_match_pct, metrics.exact_match_count, metrics.pixel_count,
    );

    if let Some(out_path) = &args.output {
        let diff = render_diff_visualization(&img_a, &img_b);
        encode_rgb_png(&diff, out_path)?;
        eprintln!("wrote diff visualization: {}", out_path.display());
    }

    Ok(())
}

/// Decode the file at `path` and return its RGBA8 representation.
/// Greyscale / palette / RGB-only PNGs are widened to RGBA8 so the
/// downstream diff math stays single-pathed.
fn decode_rgba8(path: &PathBuf) -> Result<RgbaImage, Box<dyn std::error::Error>> {
    let reader = ImageReader::open(path).map_err(|err| {
        format!("open {path}: {err}", path = path.display())
    })?;
    let decoded = reader
        .with_guessed_format()
        .map_err(|err| format!("guess format of {}: {err}", path.display()))?
        .decode()
        .map_err(|err| format!("decode {}: {err}", path.display()))?;
    Ok(DynamicImage::ImageRgba8(decoded.to_rgba8()).to_rgba8())
}

/// Compute per-channel MAE / max / exact-match against two same-size
/// RGBA8 images. Caller is responsible for matching dims; this fn
/// panics if dims differ (private — called only post-resize).
fn compute_metrics(a: &RgbaImage, b: &RgbaImage) -> DiffMetrics {
    assert_eq!(a.dimensions(), b.dimensions(), "dims must match");
    let pixel_count = u64::from(a.width()) * u64::from(a.height());
    let mut sum_abs = [0_u64; 4];
    let mut max_abs = [0_u8; 4];
    let mut exact_match_count = 0_u64;
    for (pa, pb) in a.pixels().zip(b.pixels()) {
        let mut all_equal = true;
        for ch in 0..4 {
            let da = pa.0[ch];
            let db = pb.0[ch];
            let diff = da.abs_diff(db);
            sum_abs[ch] += u64::from(diff);
            if diff > max_abs[ch] {
                max_abs[ch] = diff;
            }
            if diff != 0 {
                all_equal = false;
            }
        }
        if all_equal {
            exact_match_count += 1;
        }
    }
    // `pixel_count > 0` for any non-degenerate PNG (the image crate
    // rejects 0×0 at decode time), but defensive divide guard keeps
    // the fn total without an explicit panic path. `u64_to_f64`
    // centralises the `clippy::cast_precision_loss` allow with the
    // pixel-count justification (max practical image: 8K × 8K × 255
    // = 16e9 ≪ 2^53 f64 mantissa ceiling).
    let denom = u64_to_f64(pixel_count.max(1));
    let mean_abs = [
        u64_to_f64(sum_abs[0]) / denom,
        u64_to_f64(sum_abs[1]) / denom,
        u64_to_f64(sum_abs[2]) / denom,
        u64_to_f64(sum_abs[3]) / denom,
    ];
    let exact_match_pct = 100.0 * u64_to_f64(exact_match_count) / denom;
    DiffMetrics {
        mean_abs,
        max_abs,
        exact_match_pct,
        pixel_count,
        exact_match_count,
    }
}

/// Build the diff visualization: per-pixel `|dr| + |dg| + |db|`
/// summed RGB channel delta mapped to a white→red gradient. Output
/// is RGB (no alpha — the diff is a debug visualization, alpha
/// matching is already surfaced through the metrics).
fn render_diff_visualization(image_a: &RgbaImage, image_b: &RgbaImage) -> RgbImage {
    let (width, height) = image_a.dimensions();
    let mut out = RgbImage::new(width, height);
    for (x, y, pixel_a) in image_a.enumerate_pixels() {
        let pixel_b = image_b.get_pixel(x, y);
        let summed = u32::from(pixel_a.0[0].abs_diff(pixel_b.0[0]))
            + u32::from(pixel_a.0[1].abs_diff(pixel_b.0[1]))
            + u32::from(pixel_a.0[2].abs_diff(pixel_b.0[2]));
        // Map [0, 765] → fade from white (identical) to saturated
        // red (max delta). Hold R at 255 and dim G+B by the delta
        // intensity so closer-to-white = better match,
        // closer-to-red = larger delta.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "`(summed * 255 / 765).min(255)` is bounded to [0, 255] by construction; the u8 cast is safe"
        )]
        let intensity = (summed * 255 / 765).min(255) as u8;
        let red = 255;
        let green = 255 - intensity;
        let blue = 255 - intensity;
        out.put_pixel(x, y, image::Rgb([red, green, blue]));
    }
    out
}

/// Centralised `u64 → f64` cast for the diff metric arithmetic.
/// `clippy::cast_precision_loss` fires on every `as f64` cast from
/// `u64`; the lint is correct in principle but irrelevant here —
/// our denominator is `width * height` and our numerator is
/// `sum_of_abs_deltas ≤ 255 * pixel_count`. For an 8K × 8K image
/// (well past the figma-diff use case) those land at ~6.7e7 and
/// ~1.7e10 respectively, both ≪ 2^53 ≈ 9e15 f64 mantissa ceiling.
/// One helper keeps the allow + rationale in a single spot rather
/// than 5 inline annotations.
#[allow(
    clippy::cast_precision_loss,
    reason = "pixel-count values stay well below 2^53; see fn docstring for the bound"
)]
fn u64_to_f64(v: u64) -> f64 {
    v as f64
}

/// Encode `img` as an 8-bit RGB PNG to `path`. Mirrors the `png`
/// crate usage in `pinion_shell::headless_screenshot::HeadlessScreenshot::render_to_png`
/// (R637) so encoder behavior matches across the substrate (which
/// produced the input pinion PNG) and the CLI (which produces the
/// diff PNG consumed downstream by humans / AI agents).
fn encode_rgb_png(img: &RgbImage, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::create(path).map_err(|err| {
        format!("create {path}: {err}", path = path.display())
    })?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, img.width(), img.height());
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut png_writer = encoder
        .write_header()
        .map_err(|err| format!("png header for {}: {err}", path.display()))?;
    png_writer
        .write_image_data(img.as_raw())
        .map_err(|err| format!("png body for {}: {err}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_images_report_100_percent_exact_match() {
        let mut a = RgbaImage::new(4, 4);
        for p in a.pixels_mut() {
            *p = image::Rgba([100, 150, 200, 255]);
        }
        let b = a.clone();
        let metrics = compute_metrics(&a, &b);
        assert_eq!(metrics.exact_match_count, 16);
        assert!((metrics.exact_match_pct - 100.0).abs() < 1e-9);
        assert_eq!(metrics.max_abs, [0, 0, 0, 0]);
        assert!(metrics.mean_abs.iter().all(|m| m.abs() < 1e-9));
    }

    #[test]
    fn single_channel_delta_surfaces_per_channel() {
        let mut a = RgbaImage::new(2, 2);
        let mut b = RgbaImage::new(2, 2);
        for p in a.pixels_mut() {
            *p = image::Rgba([100, 0, 0, 255]);
        }
        for p in b.pixels_mut() {
            *p = image::Rgba([110, 0, 0, 255]);
        }
        let metrics = compute_metrics(&a, &b);
        assert_eq!(metrics.max_abs, [10, 0, 0, 0]);
        assert!((metrics.mean_abs[0] - 10.0).abs() < 1e-9);
        assert!((metrics.mean_abs[1] - 0.0).abs() < 1e-9);
        assert_eq!(metrics.exact_match_count, 0);
        assert!((metrics.exact_match_pct - 0.0).abs() < 1e-9);
    }

    #[test]
    fn diff_visualization_renders_identical_pixels_as_white() {
        let mut a = RgbaImage::new(2, 2);
        for p in a.pixels_mut() {
            *p = image::Rgba([50, 100, 150, 255]);
        }
        let b = a.clone();
        let diff = render_diff_visualization(&a, &b);
        assert_eq!(diff.dimensions(), (2, 2));
        for p in diff.pixels() {
            assert_eq!(p.0, [255, 255, 255]);
        }
    }

    #[test]
    fn diff_visualization_renders_max_delta_as_saturated_red() {
        let mut a = RgbaImage::new(1, 1);
        let mut b = RgbaImage::new(1, 1);
        a.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));
        b.put_pixel(0, 0, image::Rgba([255, 255, 255, 255]));
        let diff = render_diff_visualization(&a, &b);
        assert_eq!(diff.get_pixel(0, 0).0, [255, 0, 0]);
    }

    #[test]
    fn resize_mode_parses_canonical_strings() {
        let a_to_b: ResizeMode = "a-to-b".parse().expect("parse");
        assert!(matches!(a_to_b, ResizeMode::AtoB));
        let b_to_a: ResizeMode = "b-to-a".parse().expect("parse");
        assert!(matches!(b_to_a, ResizeMode::BtoA));
        let err = "x-to-y".parse::<ResizeMode>().unwrap_err();
        assert!(err.contains("unknown resize mode"));
    }
}
