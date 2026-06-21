// Prose mentions of widget type names (TextGrid, …) read fine un-backticked in
// a test binding.
#![allow(clippy::doc_markdown)]

//! R1036 PR-17 — does the GPU raster ACCUMULATE across frames sharing a target?
//!
//! The PR-17 residue ("리플로우로 사용 행수가 빠르게 변할 때 옛 행 픽셀이 안
//! 지워진 채 쌓이고") was narrowed by `scene/render_fidelity` to Category 3:
//! the *encoded* scene is correct (`diverged=False`) yet the framebuffer is
//! suspected to retain old rows. The live render path renders every frame into
//! ONE reused intermediate texture (`vello::util::RenderSurface::target_view`)
//! then blits to the swapchain. So the load-bearing question is: does
//! `render_to_texture` with a `base_color` FULLY overwrite that reused target,
//! or only the drawn coverage (which would leave frame N-1's rows where frame N
//! draws nothing — exactly the residue)?
//!
//! This test answers it HEADLESSLY and decisively: it renders the REAL
//! production paint of a [`Scene::TextGrid`] (via `paint_adapter::to_vello`) at
//! 5 inked rows, then 1 inked row, into the SAME reused texture
//! (`HeadlessScreenshot::render_sequence`), and asserts the 4 rows that were red
//! in frame 0 are dark in frame 1 — i.e. NO residue. A failure here would
//! reproduce PR-17's Category-3 mechanism inside pinion's raster; a pass proves
//! the raster is non-accumulating and isolates any live residue to the
//! swapchain-present / compositor layer below pinion's render logic.
//!
//! `#[ignore]` like the other headless-GPU tests (wgpu cold-boot is too slow for
//! the default suite); run with `cargo test --test render_target_reuse_gpu -- --ignored`.
//! Force the software adapter locally to match CI: `WGPU_ADAPTER_NAME=llvmpipe`
//! (or the lavapipe ICD) — a hardware pass does not imply a CI software pass.

use pinion_core::CellMetric;
use pinion_core::scene::{Rect, Scene, TextGridNode};
use pinion_core::style::Color;
use pinion_core::term_grid::{GridBuffer, TermCell, TermColor};
use pinion_runtime::paint_adapter::to_vello;
use pinion_shell::headless_screenshot::HeadlessScreenshot;
use pinion_text::LayoutCache;
use vello::Scene as VelloScene;
use vello::peniko::Color as PenikoColor;

const COLS: u16 = 20;
const ROWS: u16 = 10;
const CELL_W: u32 = 8;
const CELL_H: u32 = 16;
const W: u32 = COLS as u32 * CELL_W; // 160
const H: u32 = ROWS as u32 * CELL_H; // 160

/// A `COLS × ROWS` TextGrid whose rows in `red_rows` are filled cell-by-cell
/// with a solid red background (a stand-in for "inked prompt rows" — solid so
/// detection needs no glyph sampling). The node rect is the full target.
fn red_row_grid(red_rows: &[u16]) -> Scene {
    let red = TermColor::Rgb(Color::rgb(255, 0, 0));
    let mut buf = GridBuffer::new(COLS, ROWS);
    for &r in red_rows {
        buf = buf.with_row(
            r,
            (0..COLS).map(|_| TermCell::new(" ", TermColor::Default, red)),
        );
    }
    let mut node = TextGridNode::new(CellMetric::DEFAULT);
    node.rect = Rect::new(0, 0, W, H);
    node.cells = buf;
    Scene::TextGrid(node)
}

/// Encode a pinion scene through the production paint adapter into a vello scene.
fn encode(scene: &Scene, cache: &mut LayoutCache) -> VelloScene {
    let mut out = VelloScene::new();
    to_vello(scene, &|_b| None, cache, &mut out);
    out
}

/// `true` when the pixel at the vertical centre of grid row `row` is solid red
/// (the red cell background), `false` when it is dark (default background).
fn row_is_red(rgba8: &[u8], row: u16) -> bool {
    let px = W / 2;
    let py = u32::from(row) * CELL_H + CELL_H / 2;
    let idx = ((py * W + px) * 4) as usize;
    let (red, grn, blu) = (rgba8[idx], rgba8[idx + 1], rgba8[idx + 2]);
    red > 180 && grn < 80 && blu < 80
}

#[test]
#[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
fn render_to_texture_does_not_accumulate_across_reused_target() {
    let mut shot = match HeadlessScreenshot::new() {
        Ok(s) => s,
        // No GPU / software adapter on this host — skip rather than fail (the
        // sibling headless tests take the same stance via expect on CI runners
        // that DO have lavapipe; here we tolerate a bare dev box).
        Err(e) => {
            eprintln!("skipping: no wgpu adapter ({e})");
            return;
        }
    };
    let mut cache = LayoutCache::new();

    // Frame 0: 5 red rows (rows 0..=4) — the "stale" mid-reflow residue shape.
    let five = encode(&red_row_grid(&[0, 1, 2, 3, 4]), &mut cache);
    // Frame 1: 1 red row (row 0) — the "settled" shape. Rows 1..=4 draw only the
    // default background, so a coverage-only raster would leave them red.
    let one = encode(&red_row_grid(&[0]), &mut cache);

    let frames = shot
        .render_sequence(&[&five, &one], W, H, PenikoColor::BLACK)
        .expect("render sequence into reused target");
    assert_eq!(frames.len(), 2);

    // Frame 0 sanity: all five rows are red.
    for row in 0..5 {
        assert!(
            row_is_red(&frames[0], row),
            "frame 0 row {row} must be red (the 5-row scene)",
        );
    }

    // Frame 1 — the load-bearing assertion: row 0 stays red, rows 1..=4 are dark.
    // Residue (rows 1..=4 still red) would reproduce PR-17 Category 3 in the
    // raster; their being dark proves render_to_texture fully overwrites the
    // reused target (no accumulation).
    assert!(
        row_is_red(&frames[1], 0),
        "frame 1 row 0 is the surviving red row"
    );
    for row in 1..5 {
        assert!(
            !row_is_red(&frames[1], row),
            "frame 1 row {row} must NOT retain frame 0's red — that would be the \
             PR-17 residue inside the raster",
        );
    }
}
