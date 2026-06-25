//! `scene/cross_window_drop` RPC method — introspect the R1098 cross-window
//! drop resolution (§5.51 PR-33).
//!
//! The per-window `InputRouter::resolve_drop_point` sees only its own window's
//! painted scene, so a drag captured by one window (a settled floating panel)
//! cannot resolve a dock zone in another window (the main dock) — the gap PR-33
//! closes. [`pinion_runtime::resolve_cross_window_drop`] is the cross-window
//! peer: it maps an absolute desktop cursor across every window's scene +
//! outer position to the window that owns the drop target it lands on.
//!
//! This method makes that resolution AI-observable (§2 #7): an agent asks "if a
//! drag releases at absolute desktop `(x, y)`, which window's dock zone does it
//! land on?" and gets back the target window id + the drop point in that
//! window's local frame — the same `{window, tag, x_rel, y_rel}` a live
//! cross-window redock will carry into its intent.
//!
//! ## Why the shell pre-resolves
//!
//! [`pinion_core::scene::Scene`] is deliberately not `Clone` (it holds
//! `Box<dyn External>`), so the resolution cannot own cloned per-window scenes.
//! The shell therefore resolves in place — borrowing every window's stored
//! paint scene `&self`, before the dispatch borrow split — and threads the
//! small owned [`pinion_runtime::CrossWindowDrop`] result into the dispatch
//! context. This handler validates the request carried a cursor and serializes
//! that pre-resolved result.
//!
//! Units: `x` / `y` are absolute **desktop** logical pixels (window outer
//! positions plus the in-window cursor), the same coordinate space
//! `scene/windows` reports each window's `position` in.

use serde::{Deserialize, Serialize};

/// Request params for `scene/cross_window_drop`: the absolute desktop cursor to
/// resolve, in logical pixels.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct CrossWindowDropParams {
    /// Absolute desktop logical-pixel x (a window outer-x plus an in-window x).
    pub x: f64,
    /// Absolute desktop logical-pixel y.
    pub y: f64,
}

/// One resolved cross-window drop target in the response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResolvedCrossWindowDrop {
    /// Spec id of the window whose drop target the cursor landed on.
    pub window: String,
    /// The drop target's paint tag (a dock zone) in that window.
    pub tag: String,
    /// Cursor x normalised over the tag's rect (`0.0`..`1.0`).
    pub x_rel: f32,
    /// Cursor y normalised over the tag's rect (`0.0`..`1.0`).
    pub y_rel: f32,
}

/// Response payload: the resolved drop, or `null` when the cursor lands on no
/// window's opted-in drop target (a cross-window release there floats rather
/// than redocks).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CrossWindowDropOutcome {
    /// The resolved target, or `None` (serialised `null`) when the cursor is
    /// over no window's drop zone.
    pub drop: Option<ResolvedCrossWindowDrop>,
}

/// Reasons `scene/cross_window_drop` can fail.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossWindowDropError {
    /// The request carried no `{x, y}` cursor (or a malformed one).
    MissingCursor,
}

/// Validate the request cursor and format the shell's pre-resolved
/// cross-window drop.
///
/// The shell resolves with the SAME cursor before the dispatch borrow split
/// (it alone holds every window's scene); this handler is the rpc-layer gate:
/// it rejects a request with no cursor and serialises the pre-resolved result
/// (`Some` → the target, `None` → `null`).
///
/// # Errors
///
/// [`CrossWindowDropError::MissingCursor`] when `params` is absent or carries
/// no numeric `{x, y}`.
pub fn cross_window_drop(
    params: Option<&serde_json::Value>,
    resolved: Option<pinion_runtime::CrossWindowDrop>,
) -> Result<CrossWindowDropOutcome, CrossWindowDropError> {
    // The request must name a cursor; the resolution itself happened in the
    // shell against the SAME params, so a well-formed request and the
    // pre-resolved answer agree by construction.
    let _params = parse_params(params).ok_or(CrossWindowDropError::MissingCursor)?;
    Ok(CrossWindowDropOutcome {
        drop: resolved.map(|d| ResolvedCrossWindowDrop {
            window: d.window,
            tag: d.point.tag,
            x_rel: d.point.x_rel,
            y_rel: d.point.y_rel,
        }),
    })
}

/// Parse the `{x, y}` cursor from request params — the SSOT both the shell
/// (to resolve) and the handler (to validate) read, so they cannot disagree
/// about what cursor a request named.
#[must_use]
pub fn parse_params(params: Option<&serde_json::Value>) -> Option<CrossWindowDropParams> {
    serde_json::from_value(params?.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::DropPoint;
    use pinion_runtime::CrossWindowDrop;

    fn cursor(x: f64, y: f64) -> serde_json::Value {
        serde_json::json!({ "x": x, "y": y })
    }

    #[test]
    fn missing_cursor_rejects() {
        assert_eq!(
            cross_window_drop(None, None).unwrap_err(),
            CrossWindowDropError::MissingCursor,
        );
        // A params object without x/y is also a miss.
        let bad = serde_json::json!({ "nope": 1 });
        assert_eq!(
            cross_window_drop(Some(&bad), None).unwrap_err(),
            CrossWindowDropError::MissingCursor,
        );
    }

    #[test]
    fn formats_a_resolved_drop() {
        let resolved = CrossWindowDrop {
            window: "main".into(),
            point: DropPoint {
                tag: "main_dock".into(),
                x_rel: 0.25,
                y_rel: 0.75,
            },
        };
        let c = cursor(550.0, 450.0);
        let out = cross_window_drop(Some(&c), Some(resolved)).unwrap();
        let drop = out.drop.expect("a drop was resolved");
        assert_eq!(drop.window, "main");
        assert_eq!(drop.tag, "main_dock");
        assert!((drop.x_rel - 0.25).abs() < 1e-6 && (drop.y_rel - 0.75).abs() < 1e-6);
    }

    #[test]
    fn null_when_cursor_over_no_drop_target() {
        // A well-formed request whose cursor mapped into no window's drop zone
        // serialises `drop: null` (the release floats, it does not redock).
        let c = cursor(9000.0, 9000.0);
        let out = cross_window_drop(Some(&c), None).unwrap();
        assert!(out.drop.is_none());
    }
}
