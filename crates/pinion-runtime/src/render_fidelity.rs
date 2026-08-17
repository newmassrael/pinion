//! R1036 §5.16 §5.41 §2 #7 — AI-first render-fidelity introspection record
//! (PR-17).
//!
//! ## The introspection gap this closes
//!
//! §2 #7 promises that `scene/snapshot {from:"paint"}` is "the exact tree that
//! produced the pixels on screen". But the addressed window's stored
//! `last_paint_scene` is *dual-purpose*: it also feeds RPC hit-testing geometry,
//! and the R684 post-dispatch finalize REPLACES it with a query-time
//! recompute after every producer-running RPC method (`scene/drag`,
//! `scene/click`, `scene/layout`, …). That recompute equals the displayed frame
//! in normal operation — and DIVERGES from it exactly when a frame failed to
//! reach the screen (a missed settle repaint, a stale swapchain present). So an
//! AI polling `from:paint` to diagnose a render divergence reads *current state*,
//! not the *displayed frame*, and is blind to the very bug it is chasing. That
//! is why PR-17 concluded "data is not enough" and fell back to pixel-counting —
//! the limit was contamination, not an inherent one.
//!
//! ## The primitive
//!
//! This module records a SEPARATE, uncontaminated fingerprint of each frame at
//! the winit present boundary ([`RenderFidelity`]) — written ONLY by
//! `AppShell::render_window` after `renderer.render`, never by an RPC recompute.
//! The `scene/render_fidelity` RPC method projects it (in `pinion-rpc`) and
//! compares it against a fresh recompute of current producer state, so an AI
//! gets a deterministic, pixel-free divergence verdict through the RPC surface:
//!
//! - `paint_seq` advances once per real present — frozen after the AI's reflow
//!   ⟹ the settle frame never painted (missing repaint).
//! - `present_ok` is the `renderer.render` outcome — `false` ⟹ a present failure.
//! - `grids` is each `TextGrid`'s used-row count + content fingerprint of the
//!   frame that was actually ENCODED — diverging from current state ⟹ the
//!   displayed frame is stale even though `from:state` looks settled.
//!
//! Pixel-level present staleness (encoded frame correct, X surface stale) is the
//! one residue that the encoded-scene record cannot see; for that the AI pairs
//! this record with the `root.get_image` screenshot RPC — still fully AI-driven,
//! no human in the loop.

use core::hash::Hasher;
use std::sync::OnceLock;
use std::time::Instant;

use pinion_core::scene::Scene;

/// Per-`TextGrid` fingerprint of one presented frame. `used_rows` is the count
/// of rows carrying ink (the metric that rises during a reflow and must fall
/// back when it settles); `content_hash` fingerprints every cell cluster so two
/// frames with identical content are identical and a stale frame is caught.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridFidelity {
    /// The grid's §5.20 tag, or `"<untagged>"` for an untagged grid.
    pub tag: String,
    /// Layout-resolved paint width / height of the grid node, in logical px.
    pub rect_w: u32,
    /// See [`Self::rect_w`].
    pub rect_h: u32,
    /// Grid dimensions (columns / rows) of the projected buffer.
    pub cols: u16,
    /// See [`Self::cols`].
    pub rows: u16,
    /// Count of rows that carry at least one non-whitespace cell.
    pub used_rows: u16,
    /// Order-sensitive fingerprint of every cell cluster in the grid.
    pub content_hash: u64,
}

/// R1709 §5.16 — what the window could say about putting frames on the screen
/// at the moment this frame was recorded.
///
/// # Why the names are strings here
///
/// The vocabulary — which statuses exist, which of them are the surface
/// breaking rather than the window waiting, and which rung of the recovery
/// ladder each earns — is defined exactly once, in `pinion_gpu`. This crate
/// is backend-agnostic (a TUI backend has no swapchain, and pinion-runtime
/// must not grow a `wgpu` dependency to say so), so it carries that
/// vocabulary's own spellings rather than a second enum that could drift.
///
/// `None` means "nothing is currently wrong", not "unknown": a window whose
/// last attempt reached the screen has no missed reason and owes no rung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PresentHealth {
    /// Frames that did not reach the screen since the last one that did,
    /// counting waits as well as breakages. `0` ⟺ the last attempt
    /// presented.
    pub missed_in_a_row: u32,
    /// Of those, how many were the surface breaking rather than waiting.
    pub broken_in_a_row: u32,
    /// Why the most recent missed frame missed.
    pub last_missed: Option<&'static str>,
    /// Which rung of the recovery ladder the most recent breakage earned.
    pub last_rung: Option<&'static str>,
    /// How many times this window's surface has had to be remade, over the
    /// window's whole life. Cumulative — it survives recovery, because a
    /// window that is healthy now having needed four rebuilds to get there
    /// is a different fact from one that has needed none.
    pub rebuilds: u32,
}

/// Uncontaminated fingerprint of the frame a window last PRESENTED. Written only
/// by the winit paint path, so it answers "what is actually displayed" without
/// the RPC-recompute contamination that `last_paint_scene` carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderFidelity {
    /// Monotonic present counter for this window (starts at 1 on the first
    /// recorded present). An AI reads it before and after driving an
    /// interaction: no advance ⟹ no new frame presented.
    pub paint_seq: u64,
    /// Process-relative millisecond stamp of this present (correlates with an
    /// AI's drive cadence without leaking an absolute wall clock).
    pub presented_at_ms: u128,
    /// `true` iff `renderer.render` returned `Ok` for this frame.
    pub present_ok: bool,
    /// Layout viewport the frame was encoded at — the size a fidelity check
    /// recomputes current state at for an apples-to-apples comparison.
    pub viewport_w: u32,
    /// See [`Self::viewport_w`].
    pub viewport_h: u32,
    /// Per-`TextGrid` fingerprints of the encoded frame.
    pub grids: Vec<GridFidelity>,
    /// R1709 — what the window could say about presenting when this frame
    /// was recorded.
    ///
    /// The companion [`Self::present_ok`] needs: that flag is one frame's
    /// outcome, and one frame's outcome cannot distinguish a blip from a
    /// window that has been dark for a hundred frames. It could not, and a
    /// permanently unpresentable surface went unnoticed for the life of the
    /// tree because of it.
    pub health: PresentHealth,
}

/// Process-relative millisecond clock, shared by every recorded present so the
/// stamps are comparable across windows.
#[must_use]
pub fn elapsed_ms() -> u128 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis()
}

/// `true` when row `row` of `grid` holds at least one cell whose cluster is not
/// pure whitespace — the per-row "this row carries ink" test.
fn row_has_ink(grid: &pinion_core::term_grid::GridBuffer, row: u16) -> bool {
    (0..grid.cols()).any(|col| {
        grid.cell(col, row)
            .is_some_and(|cell| !cell.cluster.trim().is_empty())
    })
}

/// Walk `scene` depth-first and collect a [`GridFidelity`] for every
/// [`Scene::TextGrid`], so a multi-pane window reports each terminal pane
/// independently.
#[must_use]
pub fn grid_fidelity(scene: &Scene) -> Vec<GridFidelity> {
    let mut out = Vec::new();
    collect_grid_fidelity(scene, &mut out);
    out
}

fn collect_grid_fidelity(scene: &Scene, out: &mut Vec<GridFidelity>) {
    match scene {
        Scene::TextGrid(n) => {
            let grid = n.cells();
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            let mut used_rows = 0u16;
            for row in 0..grid.rows() {
                if row_has_ink(grid, row) {
                    used_rows += 1;
                }
                for col in 0..grid.cols() {
                    if let Some(cell) = grid.cell(col, row) {
                        hasher.write(cell.cluster.as_bytes());
                        hasher.write_u8(0xff);
                    }
                }
            }
            out.push(GridFidelity {
                tag: n.tag.as_deref().unwrap_or("<untagged>").to_owned(),
                rect_w: n.rect.w,
                rect_h: n.rect.h,
                cols: grid.cols(),
                rows: grid.rows(),
                used_rows,
                content_hash: hasher.finish(),
            });
        }
        Scene::Container(c) => {
            for child in &c.children {
                collect_grid_fidelity(child, out);
            }
        }
        Scene::Scroll(s) => collect_grid_fidelity(&s.content, out),
        // Leaf / opaque variants hold no nested TextGrid. `Scene` is
        // `#[non_exhaustive]`, so a wildcard absorbs both these and any future
        // variant (a diagnostic walk, not the production paint recursion).
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::CellMetric;
    use pinion_core::scene::{Rect, TextGridNode};
    use pinion_core::term_grid::{GridBuffer, TermCell, TermColor};

    fn cell(ch: char) -> TermCell {
        TermCell::new(ch.to_string(), TermColor::Default, TermColor::Default)
    }

    /// A `cols × rows` grid with `text` written into each of `text_rows`.
    fn grid_with_rows(cols: u16, rows: u16, text_rows: &[u16], text: &str) -> Scene {
        let mut buf = GridBuffer::new(cols, rows);
        for &r in text_rows {
            buf = buf.with_row(r, text.chars().map(cell));
        }
        let mut node = TextGridNode::new(CellMetric::DEFAULT);
        node.tag = Some("grid".into());
        node.rect = Rect::new(0, 0, u32::from(cols) * 8, u32::from(rows) * 16);
        node.cells = buf;
        Scene::TextGrid(node)
    }

    #[test]
    fn used_rows_counts_only_inked_rows() {
        let scene = grid_with_rows(20, 5, &[2], "coin@coin");
        let grids = grid_fidelity(&scene);
        assert_eq!(grids.len(), 1);
        assert_eq!(grids[0].used_rows, 1, "exactly one row carries ink");
        assert_eq!(grids[0].rows, 5);
        assert_eq!(grids[0].tag, "grid");
    }

    #[test]
    fn content_hash_diverges_when_used_rows_differ() {
        // The PR-17 divergence in data: a 3-row residue vs a settled 1-row
        // frame must produce different fingerprints from the SAME grid dims.
        let stale = grid_fidelity(&grid_with_rows(20, 5, &[0, 1, 2], "coin@coin"));
        let settled = grid_fidelity(&grid_with_rows(20, 5, &[0], "coin@coin"));
        assert_eq!(stale[0].used_rows, 3);
        assert_eq!(settled[0].used_rows, 1);
        assert_ne!(
            stale[0].content_hash, settled[0].content_hash,
            "a stale multi-row frame must fingerprint differently from the settled frame"
        );
    }
}
