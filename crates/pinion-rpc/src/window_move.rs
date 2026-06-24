//! `scene/window_move` RPC method — the WRITE peer of the R1087
//! `scene/windows` read (§5.16 §5.41 §2 #7 PR-31).
//!
//! `scene/windows` lets an AI agent *read* where each declared
//! floating window is placed; `scene/window_move` lets it *write* that
//! placement. Together they complete the introspect + intervene pair
//! for the floating-panel-as-positioned-window model: an agent can
//! reposition a torn-off panel symbolically, exactly as a user would by
//! dragging the title bar.
//!
//! The application registers a `reposition_request` closure on
//! [`crate::DispatchContext`]; the closure looks the declared window up
//! by id and, when found, writes the new logical-pixel outer position
//! into the binding's `Signal<Vec<WindowSpec>>`. The reconcile move
//! pass (R1087) then drives the live OS window via
//! `Window::set_outer_position`.
//!
//! Asynchrony mirrors `scene/resize`: the signal write fires the
//! reconcile effect, and the actual OS move lands on the next event-loop
//! iteration. `scene/window_move` returns once the closure has written
//! the signal — AI clients pair it with `scene/windows` to confirm the
//! declared position took.
//!
//! Units: `x` / `y` are CSS-style logical pixels (the spec's §5.21
//! coordinate system throughout), matching `WindowSpec.position` and the
//! `scene/windows` read payload.

use serde::{Deserialize, Serialize};

/// Request params for `scene/window_move`.
///
/// The move target is named `window_id` — deliberately NOT `window`,
/// which is the reserved per-dispatch `{window: "<id>"}` SCOPE key
/// ([`crate::Request::window_scope`]). The move target and the dispatch
/// scope are distinct concepts (a `scene/window_move` is a global
/// signal write, not a window-scoped read), and naming the target
/// `window` would route it through the unknown-window scope gate before
/// this handler ran — surfacing `unknown_window` instead of the precise
/// [`WindowMoveError::UnknownWindow`]. `window_id` addresses the same
/// window the `scene/windows` read reports under its `id` field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowMoveParams {
    /// The declared window id (the `scene/windows` `id` field).
    pub window_id: String,
    /// Target outer position, logical x in CSS pixels.
    pub x: i32,
    /// Target outer position, logical y in CSS pixels.
    pub y: i32,
}

/// Response payload for `scene/window_move`. Echoes the requested
/// placement (a confirmation anchor in the response envelope, like
/// [`crate::ResizeOutcome`]); the actual OS move follows asynchronously on
/// reconcile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowMoveOutcome {
    /// Echoes the moved window's id.
    pub window_id: String,
    /// Echoes the requested logical x.
    pub x: i32,
    /// Echoes the requested logical y.
    pub y: i32,
    /// Whether the application's `reposition_request` closure wrote the
    /// declared position. `true` once the matching window's spec has the
    /// new position; a miss surfaces [`WindowMoveError::UnknownWindow`]
    /// rather than `false`, so this is `true` on the success path.
    pub requested: bool,
}

/// Reasons `scene/window_move` can fail.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowMoveError {
    /// The dispatcher was invoked without a `reposition_request` closure
    /// registered — a single-window or TUI binding that declares no
    /// `windows_signal`. [`crate::DispatchContext::with_reposition_request`]
    /// is the application-side surface that registers one.
    ClosureUnavailable,
    /// No declared window carries the requested id (a stale handle, or a
    /// window the binding never declared a position for). The closure ran
    /// and found no spec to write.
    UnknownWindow,
}

/// Write a declared window's outer position via the registered closure.
///
/// The closure returns `true` when it found a declared window with the
/// requested id and wrote its position; `false` is mapped to
/// [`WindowMoveError::UnknownWindow`] so the wire surfaces a precise
/// miss rather than a silent no-op.
///
/// # Errors
///
/// See [`WindowMoveError`] for the failure surface.
pub fn window_move<F>(
    params: WindowMoveParams,
    reposition_request: Option<&mut F>,
) -> Result<WindowMoveOutcome, WindowMoveError>
where
    F: FnMut(&str, i32, i32) -> bool + ?Sized,
{
    let closure = reposition_request.ok_or(WindowMoveError::ClosureUnavailable)?;
    if closure(&params.window_id, params.x, params.y) {
        Ok(WindowMoveOutcome {
            window_id: params.window_id,
            x: params.x,
            y: params.y,
            requested: true,
        })
    } else {
        Err(WindowMoveError::UnknownWindow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn window_move_requires_closure() {
        let err = window_move::<dyn FnMut(&str, i32, i32) -> bool>(
            WindowMoveParams {
                window_id: "torn-1".into(),
                x: 100,
                y: 80,
            },
            None,
        )
        .unwrap_err();
        assert_eq!(err, WindowMoveError::ClosureUnavailable);
    }

    #[test]
    fn window_move_unknown_window_when_closure_reports_miss() {
        let mut closure = |_id: &str, _x: i32, _y: i32| false;
        let err = window_move(
            WindowMoveParams {
                window_id: "ghost".into(),
                x: 10,
                y: 20,
            },
            Some(&mut closure),
        )
        .unwrap_err();
        assert_eq!(err, WindowMoveError::UnknownWindow);
    }

    #[test]
    fn window_move_invokes_closure_with_requested_position() {
        let captured: Cell<(String, i32, i32)> = Cell::new((String::new(), 0, 0));
        let mut closure = |id: &str, x: i32, y: i32| {
            captured.set((id.to_owned(), x, y));
            true
        };
        let outcome = window_move(
            WindowMoveParams {
                window_id: "torn-2".into(),
                x: 240,
                y: 160,
            },
            Some(&mut closure),
        )
        .unwrap();
        assert!(outcome.requested);
        assert_eq!(outcome.window_id, "torn-2");
        assert_eq!((outcome.x, outcome.y), (240, 160));
        assert_eq!(captured.into_inner(), ("torn-2".to_owned(), 240, 160));
    }
}
