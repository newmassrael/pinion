// Prose mentions of type / token names (RGBA8, NotoSans, …) read fine
// un-backticked in a test binding.
#![allow(clippy::doc_markdown)]

//! R1063 §5.37 → production-paint seam — does the self-hosted text engine reach
//! real pixels?
//!
//! The §5.37 engine (`pinion-text-font`) shapes + rasterises text entirely on
//! the CPU to one [`Coverage`](pinion_text_font::Coverage) AA mask, with zero
//! external deps. Until now it had NO pixel consumer — every layer was a test
//! forcing-consumer asserting CPU geometry, never pixels on a surface. R1063
//! adds the seam: `paint_adapter::draw_coverage` uploads the mask as a
//! `peniko::ImageData` and blits it through Vello's image path.
//!
//! This is the end-to-end forcing consumer for that seam. It shapes `"Hi"`
//! through the real engine (`shape_paragraph` → `render_paragraph` → Coverage),
//! blits it with `draw_coverage` onto a black target, renders HEADLESSLY to
//! RGBA8, and asserts (1) the engine produced visible ink and (2) that ink lands
//! exactly within the mask's positioned footprint — i.e. `draw_coverage`'s
//! placement math is correct through the GPU, not just on paper. A broken
//! conversion would yield no ink; a broken placement would scatter it outside
//! the footprint.
//!
//! `#[ignore]` like the other headless-GPU tests (wgpu cold-boot is too slow for
//! the default suite); run with
//! `cargo test --test text_engine_paint_seam -- --ignored`. Force the software
//! adapter locally to match CI: `WGPU_ADAPTER_NAME=llvmpipe` (or the lavapipe
//! ICD) — a hardware pass does not imply a CI software pass.

use pinion_core::style::Color;
use pinion_runtime::paint_adapter::draw_coverage;
use pinion_shell::headless_screenshot::HeadlessScreenshot;
use pinion_text_font::{Font, render_paragraph, shape_paragraph};
use vello::Scene as VelloScene;
use vello::kurbo::Affine;
use vello::peniko::Color as PenikoColor;

/// NotoSans (Latin) — the §5.37.1 parser test fixture, reused here to drive the
/// shaper. (Production font policy for §5.37 is a separate, later decision; this
/// is a dev forcing consumer.)
const NOTO: &[u8] = include_bytes!("../../pinion-text-font/tests/fonts/NotoSans-Regular.ttf");

const PX: f32 = 32.0;
/// Margin (device px) left around the positioned ink on every side, so the
/// buffer is sized to contain the mask regardless of the font's exact metrics.
const MARGIN: u32 = 8;

#[test]
#[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
fn self_hosted_engine_coverage_reaches_pixels_at_placed_footprint() {
    // --- §5.37: shape + rasterise "Hi" to one CPU coverage mask. -----------
    let font = Font::from_bytes(NOTO.to_vec()).expect("parse NotoSans fixture");
    let shaped = shape_paragraph(&font, "Hi", PX);
    let cov = render_paragraph(&[&font], &shaped, PX).expect("rasterise paragraph");

    // Premise guard: the mask must actually carry ink, else the pixel assertions
    // below would pass vacuously.
    assert!(!cov.is_empty(), "render_paragraph produced an empty mask");
    let mask_mass: u64 = cov.alpha.iter().map(|&a| u64::from(a)).sum();
    assert!(mask_mass > 0, "the coverage mask has zero ink mass");

    // --- placement: pin the mask's top-left ink pixel at (MARGIN, MARGIN) ---
    // so the ink footprint is exactly [MARGIN, MARGIN+width] × [MARGIN,
    // MARGIN+height], with a MARGIN gutter on every side. pen + cov.{left,top}
    // is where draw_coverage lands the bitmap's top-left (see `Coverage`).
    let pen_x = f64::from(MARGIN) - f64::from(cov.left);
    let pen_y = f64::from(MARGIN) - f64::from(cov.top);
    let width = u32::try_from(cov.width).expect("mask width fits u32");
    let height = u32::try_from(cov.height).expect("mask height fits u32");
    let buf_w = width + 2 * MARGIN;
    let buf_h = height + 2 * MARGIN;

    // --- blit through the R1063 seam onto a black target, render headlessly. -
    let mut scene = VelloScene::new();
    draw_coverage(
        &mut scene,
        &cov,
        Color::rgb(255, 255, 255),
        pen_x,
        pen_y,
        Affine::IDENTITY,
    );

    let mut shot = match HeadlessScreenshot::new() {
        Ok(s) => s,
        // No GPU / software adapter on this host — skip rather than fail (mirrors
        // the sibling render_target_reuse_gpu test's stance on a bare dev box).
        Err(e) => {
            eprintln!("skipping: no wgpu adapter ({e})");
            return;
        }
    };
    let rgba = shot
        .render_to_rgba8(&scene, buf_w, buf_h, PenikoColor::BLACK)
        .expect("headless render");
    assert_eq!(rgba.len(), (buf_w * buf_h * 4) as usize);

    // --- assertions: ink exists AND falls within the positioned footprint. --
    // A pixel counts as ink when it is bright white (the fully-covered glyph
    // stems; mid-coverage AA edges premultiply to gray and are excluded, which
    // is fine — we only need the placement bound, not an exact mask match).
    let is_ink = |x: u32, y: u32| -> bool {
        let i = ((y * buf_w + x) * 4) as usize;
        rgba[i] > 180 && rgba[i + 1] > 180 && rgba[i + 2] > 180
    };

    let mut ink_count = 0u32;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in 0..buf_h {
        for x in 0..buf_w {
            if is_ink(x, y) {
                ink_count += 1;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }

    // (1) The engine reached pixels at all.
    assert!(
        ink_count > 0,
        "the self-hosted engine's coverage produced no visible ink on the surface"
    );

    // (2) Every inked pixel lies within the mask's positioned footprint, allowing
    // a 1px slop for GPU area anti-aliasing at the bitmap edge. If draw_coverage
    // placed the bitmap wrong, ink would land outside this box.
    let lo = MARGIN - 1;
    assert!(
        min_x >= lo && min_y >= lo && max_x <= MARGIN + width && max_y <= MARGIN + height,
        "ink bbox ({min_x},{min_y})-({max_x},{max_y}) escaped the placed footprint \
         at margin {MARGIN} (mask {width}x{height}) — draw_coverage mis-placed the mask"
    );

    // (3) The MARGIN gutter is black — a far-corner control proving the blit is
    // local to the footprint and did not flood the surface.
    assert!(!is_ink(0, 0), "top-left gutter must stay black (no flood)");
}
