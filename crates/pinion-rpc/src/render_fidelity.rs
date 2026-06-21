//! `scene/render_fidelity` RPC method dispatch — R1036 §5.16 §5.7 §2 #7 (PR-17).
//!
//! ## What this answers that `scene/snapshot {from:"paint"}` cannot
//!
//! §2 #7 promises introspection equals the displayed frame. But the stored
//! `last_paint_scene` that `from:"paint"` serializes is overwritten by the R684
//! post-dispatch finalize with a *query-time recompute* after any
//! producer-running RPC method — so it equals current state, not the displayed
//! frame, and DIVERGES from the displayed frame exactly when a frame failed to
//! reach the screen (a missed settle repaint, a stale present). An AI polling
//! `from:"paint"` to diagnose a render divergence is therefore blind to it; that
//! contamination is why PR-17 had to fall back to pixel-counting.
//!
//! `scene/render_fidelity` exposes a SEPARATE, uncontaminated fingerprint of the
//! frame the window last PRESENTED (recorded only on the winit paint path) and
//! compares it against a fresh recompute of current producer state — giving an
//! AI a deterministic, pixel-free divergence verdict purely through RPC:
//!
//! - `paint_seq` advances once per real present — read it before and after
//!   driving an interaction; no advance ⟹ the settled state never painted.
//! - `present_ok` is the `renderer.render` outcome — `false` ⟹ a present failure.
//! - `diverged` is `true` when any `TextGrid`'s displayed fingerprint differs
//!   from its current-state fingerprint — the §2 #7 violation, in data.
//!
//! The one residue this cannot see is pixel-level present staleness where the
//! encoded frame is correct but the X surface is stale (`displayed == state`,
//! yet the screen lags). For that the AI pairs this with the `root.get_image`
//! screenshot RPC — still fully AI-driven, no human in the loop.
//!
//! ## Wire shape
//!
//! Request: `{ "jsonrpc": "2.0", "method": "scene/render_fidelity", "params": {}, "id": 1 }`
//!
//! Response `result`:
//!
//! ```json
//! {
//!   "paint_seq": 142,
//!   "presented_at_ms": 90210,
//!   "present_ok": true,
//!   "viewport_w": 960, "viewport_h": 600,
//!   "displayed": [
//!     { "tag": "pane.1#grid", "rect_w": 480, "rect_h": 600,
//!       "cols": 80, "rows": 40, "used_rows": 12, "content_hash": "ab12cd34ef560078" }
//!   ],
//!   "state": [
//!     { "tag": "pane.1#grid", "rect_w": 480, "rect_h": 600,
//!       "cols": 80, "rows": 40, "used_rows": 4, "content_hash": "00aa11bb22cc33dd" }
//!   ],
//!   "diverged": true,
//!   "divergences": [
//!     { "tag": "pane.1#grid", "displayed_used_rows": 12, "state_used_rows": 4,
//!       "displayed_content_hash": "ab12cd34ef560078", "state_content_hash": "00aa11bb22cc33dd" }
//!   ]
//! }
//! ```
//!
//! - `displayed` is the PRESENTED frame; `state` is the current producer state
//!   recomputed at the same viewport. `state` / `diverged` / `divergences` are
//!   omitted (`null`) when no paint producer is wired (headless opt-out) — the
//!   displayed record alone is still returned.
//! - `content_hash` is a 16-hex-digit fingerprint of every cell cluster; equal
//!   hashes ⟹ identical content, so a client compares without re-deriving it.

use pinion_runtime::{GridFidelity, RenderFidelity};
use serde::Serialize;

/// Typed errors [`render_fidelity`] can return. Maps onto JSON-RPC `-32602`
/// with the variant name in `error.data` so AI agents pattern-match without
/// parsing prose.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderFidelityError {
    /// The embedder did not install a per-window [`RenderFidelity`] record —
    /// the window has not painted yet, or the embedder opted out of
    /// render-fidelity observability (headless fixture).
    RenderFidelityUnavailable,
}

/// Wire view of one `TextGrid`'s fingerprint. Mirrors [`GridFidelity`] with the
/// hash hex-encoded for JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GridFidelityView {
    /// The grid's tag, or `"<untagged>"`.
    pub tag: String,
    /// Layout-resolved grid node width / height, logical px.
    pub rect_w: u32,
    /// See [`Self::rect_w`].
    pub rect_h: u32,
    /// Grid columns / rows.
    pub cols: u16,
    /// See [`Self::cols`].
    pub rows: u16,
    /// Rows carrying ink.
    pub used_rows: u16,
    /// 16-hex-digit cell-cluster fingerprint.
    pub content_hash: String,
}

impl From<&GridFidelity> for GridFidelityView {
    fn from(g: &GridFidelity) -> Self {
        Self {
            tag: g.tag.clone(),
            rect_w: g.rect_w,
            rect_h: g.rect_h,
            cols: g.cols,
            rows: g.rows,
            used_rows: g.used_rows,
            content_hash: format!("{:016x}", g.content_hash),
        }
    }
}

/// One `TextGrid` whose presented fingerprint differs from current state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GridDivergence {
    /// The diverging grid's tag.
    pub tag: String,
    /// Used-row count in the PRESENTED frame.
    pub displayed_used_rows: u16,
    /// Used-row count in current state.
    pub state_used_rows: u16,
    /// Presented-frame content fingerprint (16 hex digits).
    pub displayed_content_hash: String,
    /// Current-state content fingerprint (16 hex digits).
    pub state_content_hash: String,
}

/// Snapshot returned by [`render_fidelity`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenderFidelityOutcome {
    /// Monotonic present counter (starts at 1) — read before/after driving an
    /// interaction; no advance ⟹ no frame presented for the settled state.
    pub paint_seq: u64,
    /// Process-relative millisecond stamp of the present.
    pub presented_at_ms: u64,
    /// `renderer.render` outcome for the presented frame.
    pub present_ok: bool,
    /// Viewport the frame was encoded at (state is recomputed here).
    pub viewport_w: u32,
    /// See [`Self::viewport_w`].
    pub viewport_h: u32,
    /// Per-`TextGrid` fingerprints of the PRESENTED frame.
    pub displayed: Vec<GridFidelityView>,
    /// Per-`TextGrid` fingerprints of current producer state, recomputed at the
    /// same viewport. `None` when no paint producer is wired (headless opt-out).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<Vec<GridFidelityView>>,
    /// `true` when any grid's displayed fingerprint differs from its current
    /// state — the §2 #7 displayed-≠-state violation. `None` when no producer
    /// was available to compare against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diverged: Option<bool>,
    /// Per-grid detail for every divergence (empty when none / no producer).
    pub divergences: Vec<GridDivergence>,
}

/// Project a presented-frame [`RenderFidelity`] record (and optional current
/// state grids) onto the wire-shaped [`RenderFidelityOutcome`], computing the
/// per-grid divergence verdict.
///
/// `state_grids` is `None` when no paint producer is wired — the displayed
/// record is still returned, with `state` / `diverged` omitted.
///
/// # Errors
///
/// - [`RenderFidelityError::RenderFidelityUnavailable`] — the embedder did not
///   register a record on the dispatch context (window never painted).
pub fn render_fidelity(
    record: Option<&RenderFidelity>,
    state_grids: Option<Vec<GridFidelity>>,
) -> Result<RenderFidelityOutcome, RenderFidelityError> {
    let Some(record) = record else {
        return Err(RenderFidelityError::RenderFidelityUnavailable);
    };
    let displayed: Vec<GridFidelityView> =
        record.grids.iter().map(GridFidelityView::from).collect();

    let (state, diverged, divergences) = match state_grids {
        Some(state) => {
            let mut divs = Vec::new();
            for d in &record.grids {
                // Compare by tag; a grid present in one frame but not the other
                // (a pane appearing / disappearing) is itself a divergence.
                match state.iter().find(|s| s.tag == d.tag) {
                    Some(s) if s.used_rows == d.used_rows && s.content_hash == d.content_hash => {}
                    Some(s) => divs.push(divergence(d, Some(s))),
                    None => divs.push(divergence(d, None)),
                }
            }
            let state_views = state.iter().map(GridFidelityView::from).collect();
            (Some(state_views), Some(!divs.is_empty()), divs)
        }
        None => (None, None, Vec::new()),
    };

    Ok(RenderFidelityOutcome {
        paint_seq: record.paint_seq,
        presented_at_ms: u64::try_from(record.presented_at_ms).unwrap_or(u64::MAX),
        present_ok: record.present_ok,
        viewport_w: record.viewport_w,
        viewport_h: record.viewport_h,
        displayed,
        state,
        diverged,
        divergences,
    })
}

/// Build a [`GridDivergence`] for displayed grid `d` against (optional) current
/// state grid `s`. A `None` state grid (the tag vanished from current state)
/// reports zeroed state fields, which a client reads as "absent".
fn divergence(d: &GridFidelity, s: Option<&GridFidelity>) -> GridDivergence {
    GridDivergence {
        tag: d.tag.clone(),
        displayed_used_rows: d.used_rows,
        state_used_rows: s.map_or(0, |s| s.used_rows),
        displayed_content_hash: format!("{:016x}", d.content_hash),
        state_content_hash: format!("{:016x}", s.map_or(0, |s| s.content_hash)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(tag: &str, used_rows: u16, hash: u64) -> GridFidelity {
        GridFidelity {
            tag: tag.to_owned(),
            rect_w: 480,
            rect_h: 600,
            cols: 80,
            rows: 40,
            used_rows,
            content_hash: hash,
        }
    }

    fn record(grids: Vec<GridFidelity>) -> RenderFidelity {
        RenderFidelity {
            paint_seq: 7,
            presented_at_ms: 1234,
            present_ok: true,
            viewport_w: 960,
            viewport_h: 600,
            grids,
        }
    }

    #[test]
    fn missing_record_errors() {
        let err = render_fidelity(None, None).unwrap_err();
        assert_eq!(err, RenderFidelityError::RenderFidelityUnavailable);
    }

    #[test]
    fn no_producer_returns_displayed_only() {
        let rec = record(vec![grid("pane.1#grid", 3, 0xabcd)]);
        let out = render_fidelity(Some(&rec), None).unwrap();
        assert_eq!(out.displayed.len(), 1);
        assert_eq!(out.displayed[0].used_rows, 3);
        assert_eq!(out.displayed[0].content_hash, "000000000000abcd");
        assert!(out.state.is_none(), "no producer ⟹ no state");
        assert!(out.diverged.is_none(), "no producer ⟹ no verdict");
    }

    #[test]
    fn matching_state_reports_no_divergence() {
        let rec = record(vec![grid("pane.1#grid", 4, 0x1234)]);
        let state = vec![grid("pane.1#grid", 4, 0x1234)];
        let out = render_fidelity(Some(&rec), Some(state)).unwrap();
        assert_eq!(out.diverged, Some(false));
        assert!(out.divergences.is_empty());
    }

    #[test]
    fn stale_displayed_diverges_from_settled_state() {
        // PR-17 in data: presented frame shows 3 used rows (residue) while
        // current producer state has settled to 1 — same tag, same dims.
        let rec = record(vec![grid("pane.1#grid", 3, stale_hash())]);
        let state = vec![grid("pane.1#grid", 1, 0x5678)];
        let out = render_fidelity(Some(&rec), Some(state)).unwrap();
        assert_eq!(out.diverged, Some(true));
        assert_eq!(out.divergences.len(), 1);
        assert_eq!(out.divergences[0].displayed_used_rows, 3);
        assert_eq!(out.divergences[0].state_used_rows, 1);
    }

    #[test]
    fn serialized_omits_state_when_absent() {
        let rec = record(vec![grid("pane.1#grid", 2, 0x9)]);
        let out = render_fidelity(Some(&rec), None).unwrap();
        let json = serde_json::to_value(out).unwrap();
        assert!(
            json.get("state").is_none(),
            "state omitted when no producer; got {json}"
        );
        assert!(json.get("diverged").is_none());
        assert_eq!(
            json.get("paint_seq").and_then(serde_json::Value::as_u64),
            Some(7)
        );
    }

    fn stale_hash() -> u64 {
        0xdead_beef
    }
}
