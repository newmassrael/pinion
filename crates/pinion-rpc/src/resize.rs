//! `scene/resize` RPC method dispatch (§5.12 R47.7.4 — AI window-drag
//! simulation primitive).
//!
//! AI agents trigger an actual winit resize event chain — not the
//! hypothetical-viewport path of `scene/layout`. The application
//! registers a `resize_request` closure on `DispatchContext`; the
//! closure invokes `winit::window::Window::request_inner_size` and
//! `request_redraw`, after which winit emits a `Resized` event on
//! the next event-loop iteration and the application's `render()`
//! rebuilds the paint scene at the new size.
//!
//! Asynchrony: `request_inner_size` is non-blocking — winit may emit
//! the actual `Resized` event one or more frames later. AI clients
//! pair `scene/resize` with `scene/wait_for_frame` (R47.7.4.x carry)
//! when they need stable observation; `scene/resize` itself returns
//! once the closure has issued the request.
//!
//! Units: `width` / `height` are CSS-style logical pixels. The
//! closure decides whether to forward them as `LogicalSize` (`HiDPI`
//! aware) or `PhysicalSize`; pinion's convention is logical because
//! the spec's coordinate system (§5.21) is logical px throughout.

use serde::{Deserialize, Serialize};

/// Request params for `scene/resize`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResizeParams {
    /// Target logical width in CSS pixels.
    pub width: u32,
    /// Target logical height in CSS pixels.
    pub height: u32,
}

/// Response payload for `scene/resize`. Confirms the requested
/// dimensions echoed back; the actual `Resized` event from winit
/// follows asynchronously.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResizeOutcome {
    /// Echoes `ResizeParams.width` so AI clients have a confirmation
    /// anchor in the response envelope.
    pub width: u32,
    pub height: u32,
    /// Whether the application's `resize_request` closure was
    /// successfully invoked. `false` means the dispatcher could not
    /// reach the closure — usually a `DispatchContext` shape error,
    /// reported separately as `ResizeError::ClosureUnavailable`.
    pub requested: bool,
}

/// Reasons `scene/resize` can fail.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeError {
    /// The dispatcher was invoked without a `resize_request` closure
    /// registered. `DispatchContext::with_resize_request` is the
    /// application-side surface that registers one.
    ClosureUnavailable,
    /// `width` or `height` is zero — winit rejects zero-sized windows
    /// on every backend and the closure short-circuits.
    InvalidSize,
}

/// Trigger an actual winit window resize via the registered closure.
///
/// # Errors
///
/// See [`ResizeError`] for the failure surface.
pub fn resize<F>(
    params: ResizeParams,
    resize_request: Option<&mut F>,
) -> Result<ResizeOutcome, ResizeError>
where
    F: FnMut(u32, u32) + ?Sized,
{
    if params.width == 0 || params.height == 0 {
        return Err(ResizeError::InvalidSize);
    }
    let closure = resize_request.ok_or(ResizeError::ClosureUnavailable)?;
    closure(params.width, params.height);
    Ok(ResizeOutcome {
        width: params.width,
        height: params.height,
        requested: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn resize_requires_closure() {
        let err = resize::<dyn FnMut(u32, u32)>(
            ResizeParams {
                width: 320,
                height: 200,
            },
            None,
        )
        .unwrap_err();
        assert_eq!(err, ResizeError::ClosureUnavailable);
    }

    #[test]
    fn resize_rejects_zero_size() {
        let mut closure = |_w: u32, _h: u32| {};
        let err = resize(
            ResizeParams {
                width: 0,
                height: 200,
            },
            Some(&mut closure),
        )
        .unwrap_err();
        assert_eq!(err, ResizeError::InvalidSize);
    }

    #[test]
    fn resize_invokes_closure_with_requested_size() {
        let captured = Cell::new((0_u32, 0_u32));
        let mut closure = |w: u32, h: u32| {
            captured.set((w, h));
        };
        let outcome = resize(
            ResizeParams {
                width: 345,
                height: 211,
            },
            Some(&mut closure),
        )
        .unwrap();
        assert_eq!(outcome.width, 345);
        assert_eq!(outcome.height, 211);
        assert!(outcome.requested);
        assert_eq!(captured.get(), (345, 211));
    }
}
