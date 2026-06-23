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

use pinion_core::scene::{Rect, Scene, TextNode};
use pinion_core::style::{Color, TextStyle};
use pinion_runtime::paint_adapter::{
    draw_atlased_glyphs, draw_atlased_glyphs_styled, draw_coverage, to_vello_with_text_engine,
};
use pinion_runtime::text_engine::{LoadFontError, SelfHostedTextEngine, load_system_font};
use pinion_shell::headless_screenshot::HeadlessScreenshot;
use pinion_text::LayoutCache;
use pinion_text_font::{Font, render_paragraph, render_paragraph_atlased, shape_paragraph};
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
        min_x >= lo && min_y >= lo && max_x <= MARGIN + width + 1 && max_y <= MARGIN + height + 1,
        "ink bbox ({min_x},{min_y})-({max_x},{max_y}) escaped the placed footprint \
         at margin {MARGIN} (mask {width}x{height}) — draw_coverage mis-placed the mask"
    );

    // (3) The MARGIN gutter is black — a far-corner control proving the blit is
    // local to the footprint and did not flood the surface.
    assert!(!is_ink(0, 0), "top-left gutter must stay black (no flood)");

    // (4) Shape fidelity: the glyph's NEGATIVE SPACE survived to the surface.
    // Assertions (1)-(2) bound where ink lands but do NOT distinguish the glyph
    // shape from a solid rectangle — a buggy draw_coverage that ignored
    // coverage.alpha and filled the whole footprint would pass them (its ink
    // bbox equals the footprint). "Hi" has interstitial background (the gap
    // between the H and the i, the counters / the space above the i's stem), so
    // at least one pixel strictly INSIDE the ink bbox must be background. A
    // solid-rectangle blit has none and fails here — this is what pins that the
    // engine's per-pixel mask shape, not just its bounding box, reached pixels.
    let mut interior_background = false;
    'scan: for y in (min_y + 1)..max_y {
        for x in (min_x + 1)..max_x {
            if !is_ink(x, y) {
                interior_background = true;
                break 'scan;
            }
        }
    }
    assert!(
        interior_background,
        "no background pixel inside the ink bbox ({min_x},{min_y})-({max_x},{max_y}) — \
         the mask SHAPE did not reach the surface (a solid-rectangle blit looks like this)"
    );
}

/// R1065 §5.37.9 → §5.16 — does the per-glyph GlyphAtlas reach pixels as separate
/// quads? The production-direction successor to draw_coverage: instead of blitting
/// one whole-paragraph mask, `draw_atlased_glyphs` uploads each atlas once and
/// draws one quad per glyph sampling its sub-rect. This forcing consumer shapes
/// "Hi" through `render_paragraph_atlased`, paints it with `draw_atlased_glyphs`
/// onto a black target, renders HEADLESSLY to RGBA8, and asserts the two glyphs
/// land as two horizontally-separated ink clusters within the placed footprint —
/// proving (a) the atlas reached pixels, (b) placement is correct through the GPU,
/// and (c) the glyphs are distinct quads with a background gap (no inter-glyph
/// atlas bleed), not one merged blob.
///
/// `#[ignore]` / lavapipe like the sibling seam test (see that test's header).
#[test]
#[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
fn self_hosted_atlas_reaches_pixels_per_glyph() {
    // --- §5.37: shape + atlas-place "Hi" (no composite — keep per-glyph). ----
    let font = Font::from_bytes(NOTO.to_vec()).expect("parse NotoSans fixture");
    let shaped = shape_paragraph(&font, "Hi", PX);
    let rendered = render_paragraph_atlased(&[&font], &shaped, PX).expect("atlas-render paragraph");

    // Premise: "Hi" is two inked glyphs, each a placement into atlas 0.
    assert_eq!(rendered.placed.len(), 2, "Hi should atlas-place two glyphs");

    // --- footprint: the union of the glyph quads (what composite() would size
    // the mask to), pinned so its top-left ink lands at (MARGIN, MARGIN). -----
    let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for p in &rendered.placed {
        let g = &p.glyph;
        let (gx, gy) = (p.pen_x + g.left, p.pen_y + g.top);
        left = left.min(gx);
        top = top.min(gy);
        right = right.max(gx + i32::try_from(g.width).expect("glyph width fits i32"));
        bottom = bottom.max(gy + i32::try_from(g.height).expect("glyph height fits i32"));
    }
    let width = u32::try_from(right - left).expect("footprint width fits u32");
    let height = u32::try_from(bottom - top).expect("footprint height fits u32");
    let pen_x = f64::from(MARGIN) - f64::from(left);
    let pen_y = f64::from(MARGIN) - f64::from(top);
    let buf_w = width + 2 * MARGIN;
    let buf_h = height + 2 * MARGIN;

    // --- paint the atlas per-glyph onto a black target, render headlessly. ---
    let mut scene = VelloScene::new();
    draw_atlased_glyphs(
        &mut scene,
        &rendered,
        Color::rgb(255, 255, 255),
        pen_x,
        pen_y,
        Affine::IDENTITY,
    );

    let mut shot = match HeadlessScreenshot::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping: no wgpu adapter ({e})");
            return;
        }
    };
    let rgba = shot
        .render_to_rgba8(&scene, buf_w, buf_h, PenikoColor::BLACK)
        .expect("headless render");
    assert_eq!(rgba.len(), (buf_w * buf_h * 4) as usize);

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

    // (1) The atlas reached pixels.
    assert!(ink_count > 0, "the per-glyph atlas produced no visible ink");

    // (2) All ink lies within the placed footprint (1px GPU-AA slop) — a
    // mis-placed quad or a wrong brush_transform would scatter ink outside.
    let lo = MARGIN - 1;
    assert!(
        min_x >= lo && min_y >= lo && max_x <= MARGIN + width + 1 && max_y <= MARGIN + height + 1,
        "ink bbox ({min_x},{min_y})-({max_x},{max_y}) escaped the placed footprint \
         at margin {MARGIN} ({width}x{height}) — a glyph quad / brush_transform is wrong"
    );

    // (3) The gutter is black (no flood).
    assert!(!is_ink(0, 0), "top-left gutter must stay black (no flood)");

    // (4) The glyphs are TWO horizontally-separated clusters: at least one column
    // strictly between the ink extremes is fully background (the H/i advance gap).
    // This rules out a single whole-paragraph blit, a merged blob, or a grossly
    // mis-placed quad — the atlas path's defining witness: two quads sampling two
    // atlas sub-rects. (It does NOT bound sub-pixel edge bleed — the advance gap
    // is several px, wider than any <=1px fringe; the integer-quad no-bleed
    // property is argued at `draw_atlased_glyphs`, not asserted here.)
    let column_has_ink = |x: u32| (0..buf_h).any(|y| is_ink(x, y));
    let empty_column_between = ((min_x + 1)..max_x).any(|x| !column_has_ink(x));
    assert!(
        empty_column_between,
        "no fully-background column between the glyphs (ink x {min_x}..{max_x}) — \
         the two glyph quads merged (a single blit or a grossly mis-placed quad)"
    );
}

/// R1066 §5.37 → §5.16 — does per-glyph colour reach pixels distinctly? The
/// styled-run paint a code editor needs (syntax highlighting): one grayscale
/// atlas, glyphs in different colours. This forcing consumer paints "Hi" with the
/// H red and the i blue through `draw_atlased_glyphs_styled`, renders HEADLESSLY,
/// and asserts the red ink (the H) lies entirely LEFT of the blue ink (the i) —
/// proving each glyph's quad sampled its own `(atlas, colour)` tint, not one
/// shared colour. A uniform-colour path could not produce two colours; a
/// colour/glyph mis-mapping would interleave or swap them.
///
/// `#[ignore]` / lavapipe like the sibling seam tests (see the first test header).
#[test]
#[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
fn self_hosted_atlas_paints_per_glyph_color() {
    let font = Font::from_bytes(NOTO.to_vec()).expect("parse NotoSans fixture");
    let shaped = shape_paragraph(&font, "Hi", PX);
    let rendered = render_paragraph_atlased(&[&font], &shaped, PX).expect("atlas-render paragraph");
    assert_eq!(rendered.placed.len(), 2, "Hi should atlas-place two glyphs");

    // Footprint = union of the glyph quads, pinned to (MARGIN, MARGIN).
    let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for p in &rendered.placed {
        let g = &p.glyph;
        let (gx, gy) = (p.pen_x + g.left, p.pen_y + g.top);
        left = left.min(gx);
        top = top.min(gy);
        right = right.max(gx + i32::try_from(g.width).expect("glyph width fits i32"));
        bottom = bottom.max(gy + i32::try_from(g.height).expect("glyph height fits i32"));
    }
    let width = u32::try_from(right - left).expect("footprint width fits u32");
    let height = u32::try_from(bottom - top).expect("footprint height fits u32");
    let pen_x = f64::from(MARGIN) - f64::from(left);
    let pen_y = f64::from(MARGIN) - f64::from(top);
    let buf_w = width + 2 * MARGIN;
    let buf_h = height + 2 * MARGIN;

    // placed[0] = H (leftmost, LTR) -> red; placed[1] = i -> blue.
    let colors = [Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)];
    let mut scene = VelloScene::new();
    draw_atlased_glyphs_styled(
        &mut scene,
        &rendered,
        &colors,
        pen_x,
        pen_y,
        Affine::IDENTITY,
    );

    let mut shot = match HeadlessScreenshot::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping: no wgpu adapter ({e})");
            return;
        }
    };
    let rgba = shot
        .render_to_rgba8(&scene, buf_w, buf_h, PenikoColor::BLACK)
        .expect("headless render");

    // Channel-dominant classifiers (the AA fringe premultiplies to a dim tint, so
    // require a clearly-dominant channel with the other two suppressed).
    let is_red = |x: u32, y: u32| {
        let i = ((y * buf_w + x) * 4) as usize;
        rgba[i] > 150 && rgba[i + 1] < 80 && rgba[i + 2] < 80
    };
    let is_blue = |x: u32, y: u32| {
        let i = ((y * buf_w + x) * 4) as usize;
        rgba[i + 2] > 150 && rgba[i] < 80 && rgba[i + 1] < 80
    };
    let bbox = |pred: &dyn Fn(u32, u32) -> bool| {
        let (mut n, mut lo, mut hi) = (0u32, u32::MAX, 0u32);
        for y in 0..buf_h {
            for x in 0..buf_w {
                if pred(x, y) {
                    n += 1;
                    lo = lo.min(x);
                    hi = hi.max(x);
                }
            }
        }
        (n, lo, hi)
    };

    let (red_n, red_lo, red_hi) = bbox(&is_red);
    let (blue_n, blue_lo, blue_hi) = bbox(&is_blue);

    // Both colours reached pixels.
    assert!(
        red_n > 0,
        "no red ink — the H glyph did not take its colour"
    );
    assert!(
        blue_n > 0,
        "no blue ink — the i glyph did not take its colour"
    );

    // The red glyph (H) is entirely LEFT of the blue glyph (i): max red x < min
    // blue x. This is the per-glyph-colour witness — each quad sampled its own
    // (atlas, colour) tint at its own position. A uniform colour, a swapped
    // mapping, or bleed across the gap would break this ordering.
    assert!(
        red_hi < blue_lo,
        "red ink (x {red_lo}..{red_hi}) is not entirely left of blue ink \
         (x {blue_lo}..{blue_hi}) — per-glyph colour mis-mapped or bled"
    );
}

/// R1067 §5.37.11 — does a font *discovered from the OS* reach real pixels? The
/// sibling tests above all shape the bundled NotoSans parser FIXTURE; this one
/// closes the production-connection loop end to end:
/// `text_engine::load_system_font` (OS discovery via `pinion-platform-fonts` →
/// `Font::from_bytes`) → `shape_paragraph` → `render_paragraph_atlased` →
/// `draw_atlased_glyphs` → headless RGBA8. It proves the whole R1067 chain — a
/// real installed system font, not a committed fixture, becomes pixels through
/// the §5.37 engine and the R1065 atlas seam.
///
/// Assertions are font-AGNOSTIC (which system font resolves varies per machine):
/// ink exists, ink lands inside the placed glyphs' own footprint, the gutter
/// stays black, and the glyph SHAPE (not a solid rectangle) survived. No exact
/// metric / glyph-count assertion — that would reintroduce the system-font
/// pixel-determinism debt. Skips cleanly when the host has no installed font (the
/// default-gate `text_engine` + `pinion-platform-fonts` tests carry the non-GPU
/// proof) or no wgpu adapter.
///
/// `#[ignore]` / lavapipe like the sibling seam tests (see the first test header).
#[test]
#[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
fn discovered_system_font_reaches_pixels() {
    // --- §5.37.11: discover + parse a real OS font, then shape "Hi". ---------
    let font = match load_system_font() {
        Ok(font) => font,
        Err(LoadFontError::NoSystemFont) => {
            eprintln!("skipping: no system font installed on this host");
            return;
        }
    };
    let shaped = shape_paragraph(&font, "Hi", PX);
    let rendered = render_paragraph_atlased(&[&font], &shaped, PX).expect("atlas-render paragraph");
    assert!(
        !rendered.placed.is_empty(),
        "the system font shaped \"Hi\" into no glyphs"
    );

    // --- footprint = union of the glyph quads, pinned to (MARGIN, MARGIN). ---
    let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for p in &rendered.placed {
        let g = &p.glyph;
        let (gx, gy) = (p.pen_x + g.left, p.pen_y + g.top);
        left = left.min(gx);
        top = top.min(gy);
        right = right.max(gx + i32::try_from(g.width).expect("glyph width fits i32"));
        bottom = bottom.max(gy + i32::try_from(g.height).expect("glyph height fits i32"));
    }
    let width = u32::try_from(right - left).expect("footprint width fits u32");
    let height = u32::try_from(bottom - top).expect("footprint height fits u32");
    let pen_x = f64::from(MARGIN) - f64::from(left);
    let pen_y = f64::from(MARGIN) - f64::from(top);
    let buf_w = width + 2 * MARGIN;
    let buf_h = height + 2 * MARGIN;

    let mut scene = VelloScene::new();
    draw_atlased_glyphs(
        &mut scene,
        &rendered,
        Color::rgb(255, 255, 255),
        pen_x,
        pen_y,
        Affine::IDENTITY,
    );

    let mut shot = match HeadlessScreenshot::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping: no wgpu adapter ({e})");
            return;
        }
    };
    let rgba = shot
        .render_to_rgba8(&scene, buf_w, buf_h, PenikoColor::BLACK)
        .expect("headless render");
    assert_eq!(rgba.len(), (buf_w * buf_h * 4) as usize);

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

    // (1) The system font reached pixels.
    assert!(
        ink_count > 0,
        "the discovered system font produced no visible ink on the surface"
    );

    // (2) All ink lies within the placed footprint (1px GPU-AA slop).
    let lo = MARGIN - 1;
    assert!(
        min_x >= lo && min_y >= lo && max_x <= MARGIN + width + 1 && max_y <= MARGIN + height + 1,
        "ink bbox ({min_x},{min_y})-({max_x},{max_y}) escaped the placed footprint \
         at margin {MARGIN} ({width}x{height})"
    );

    // (3) The gutter is black (no flood).
    assert!(!is_ink(0, 0), "top-left gutter must stay black (no flood)");

    // (4) Shape fidelity: the glyph's negative space survived (not a solid blit).
    let mut interior_background = false;
    'scan: for y in (min_y + 1)..max_y {
        for x in (min_x + 1)..max_x {
            if !is_ink(x, y) {
                interior_background = true;
                break 'scan;
            }
        }
    }
    assert!(
        interior_background,
        "no background pixel inside the ink bbox — the system font's glyph SHAPE \
         did not reach the surface (a solid-rectangle blit looks like this)"
    );
}

/// R1068 §5.37 → production `Scene::Text` — does the opt-in paint arm reach
/// pixels through the REAL paint walker? The R1067 test above drives the atlas
/// seam directly; this one drives the full production path:
/// `Scene::Text` → `to_vello_with_text_engine(Some(engine))` → `paint_text`'s
/// §5.37 arm → headless RGBA8. It proves the campaign's actual connection — a
/// `Scene::Text` node, painted by the production walker with the self-hosted
/// engine opted in, becomes pixels — not just that the low-level seam works.
///
/// Font-agnostic assertions (system font varies): ink exists, lands inside the
/// text node's box (proving the font-ascent baseline placement is correct), and
/// the gutter outside the box is black. Skips when no system font / no wgpu
/// adapter.
///
/// `#[ignore]` / lavapipe like the sibling seam tests (see the first test header).
#[test]
#[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
fn self_hosted_paint_arm_reaches_pixels_through_to_vello() {
    // A single-style label box, generous so the glyphs (Visible overflow) sit
    // inside it. The §5.37 arm activates for this node (single style, single
    // line, no decoration).
    const BOX_W: u32 = 240;
    const BOX_H: u32 = 40;

    let engine = match SelfHostedTextEngine::from_system_font() {
        Ok(engine) => engine,
        Err(LoadFontError::NoSystemFont) => {
            eprintln!("skipping: no system font installed on this host");
            return;
        }
    };

    let rect = Rect::new(MARGIN, MARGIN, BOX_W, BOX_H);
    // White text on the black render target so the ink classifier sees it
    // (the default fg_color is black).
    let scene = Scene::Text(TextNode::styled(
        "Hi",
        rect,
        TextStyle::new()
            .with_size_px(24)
            .with_fg(Color::rgb(255, 255, 255)),
    ));

    let mut vello = VelloScene::new();
    let mut cache = LayoutCache::new();
    to_vello_with_text_engine(&scene, &|_| None, &mut cache, Some(&engine), &mut vello);

    let buf_w = BOX_W + 2 * MARGIN;
    let buf_h = BOX_H + 2 * MARGIN;
    let mut shot = match HeadlessScreenshot::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping: no wgpu adapter ({e})");
            return;
        }
    };
    let rgba = shot
        .render_to_rgba8(&vello, buf_w, buf_h, PenikoColor::BLACK)
        .expect("headless render");
    assert_eq!(rgba.len(), (buf_w * buf_h * 4) as usize);

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

    // (1) The §5.37 arm reached pixels through the production walker.
    assert!(
        ink_count > 0,
        "the self-hosted paint arm produced no ink through to_vello_with_text_engine"
    );

    // (2) Ink lands inside the text node's box (1px slop): the §5.37 baseline
    // (font ascent below the box top) placed the glyphs within [MARGIN, MARGIN+H].
    assert!(
        min_x + 1 >= MARGIN
            && min_y + 1 >= MARGIN
            && max_x <= MARGIN + BOX_W + 1
            && max_y <= MARGIN + BOX_H + 1,
        "ink bbox ({min_x},{min_y})-({max_x},{max_y}) escaped the text box \
         ({MARGIN},{MARGIN})+{BOX_W}x{BOX_H} — baseline placement is wrong"
    );

    // (3) The gutter is black (no flood).
    assert!(!is_ink(0, 0), "top-left gutter must stay black (no flood)");

    // (4) The baseline math actually drove placement. "Hi" has no descenders, so
    // its lowest inked row sits on the baseline. The arm computes the baseline as
    // (ascender + line_gap/2) * px / upem below the box top — assert the lowest
    // ink row matches that, ±2px (AA + pixel snap). This pins R1068's baseline fix
    // (the Normal-line-box first baseline, incl. half-leading) rather than merely
    // "ink is somewhere in the box". font/px-agnostic: it reads the engine font's
    // own metrics.
    let f = engine.font();
    let upem = f64::from(f.units_per_em());
    let baseline_y =
        f64::from(MARGIN) + (f64::from(f.ascender()) + f64::from(f.line_gap()) / 2.0) * 24.0 / upem;
    assert!(
        (f64::from(max_y) - baseline_y).abs() <= 2.0,
        "lowest ink row {max_y} should sit on the computed baseline {baseline_y:.1} \
         (ascender+half-leading) — baseline formula did not drive placement"
    );
}
