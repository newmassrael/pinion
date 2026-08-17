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
//!
//! # R1710 — the answer is what was GRANTED
//!
//! Pre-R1710 this module echoed the request back and set `requested: true`,
//! a field whose documented `false` state was unreachable (a missing closure
//! returns `Err`). Measured on this host: a window declaring a floor of
//! 1440x900, asked for 1560x880 over this method, answered `height: 880` and
//! then sat at 900 — the window manager had enforced the floor. On the bare
//! display every gate in this tree runs on, nothing enforced it and the same
//! request produced a genuinely 880-tall window, so no test could see the
//! divergence.
//!
//! Now the ask is resolved against the window's declared
//! [`pinion_core::size_grant::SizeBounds`] HERE, the resolved size is what the
//! closure forwards, and the response carries the resolution: the granted
//! size, the size asked for, and per axis which declared bound moved it. See
//! [`pinion_core::size_grant`] for why the granted extent is derived from the
//! bound rather than stored beside it.

use pinion_core::size_grant::{Bound, Grant, SizeBounds};
use serde::{Deserialize, Serialize};

/// Request params for `scene/resize`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResizeParams {
    /// Target logical width in CSS pixels.
    pub width: u32,
    /// Target logical height in CSS pixels.
    pub height: u32,
    /// R1710 §2 #3 — resolve the request and answer, without touching the
    /// window. Absent is `false` (every pre-R1710 caller is byte-unchanged).
    ///
    /// The dry run and the real path call the same resolution, so "what would
    /// you grant" cannot disagree with "what did you grant". It is also how an
    /// agent reads a window's floor without a probe: ask for `1x1` and read
    /// the bound each axis reports back.
    #[serde(default)]
    pub dry_run: bool,
}

/// Response payload for `scene/resize`: what the window was actually asked to
/// take, what the caller asked for, and per axis which declared bound decided
/// the difference.
///
/// [`width`](Self::width) / [`height`](Self::height) are the **granted** size.
/// For an ask inside the declared bounds — every pre-R1710 call — they are the
/// asked-for numbers, unchanged.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResizeOutcome {
    /// Granted logical width: what the window was asked to take.
    pub width: u32,
    /// Granted logical height: what the window was asked to take.
    pub height: u32,
    /// The size the caller asked for, as `[width, height]`. Equal to the
    /// granted pair whenever [`as_asked`](Self::as_asked) is `true`.
    pub asked: (u32, u32),
    /// Which declared bound decided the width.
    pub width_bound: Bound,
    /// Which declared bound decided the height.
    pub height_bound: Bound,
    /// Was the whole ask granted as asked? Derived from the two bounds and
    /// published because it is what a caller branches on.
    pub as_asked: bool,
    /// Was the granted size forwarded to the window system?
    ///
    /// `false` means and only means [`ResizeParams::dry_run`]. An unreachable
    /// closure is an `Err` ([`ResizeError::ClosureUnavailable`]), not a `false`
    /// here — one value answering two reasons is the defect R1707 named, and
    /// the field this replaces (`requested`) could never be `false` at all.
    pub applied: bool,
}

impl ResizeOutcome {
    /// Project a resolved [`Grant`] onto the wire.
    fn of(grant: Grant, applied: bool) -> Self {
        let (width, height) = grant.size();
        Self {
            width,
            height,
            asked: grant.asked(),
            width_bound: grant.width(),
            height_bound: grant.height(),
            as_asked: grant.is_as_asked(),
            applied,
        }
    }
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

/// Resolve a resize request against the window's declared bounds and — unless
/// the request is a dry run — forward the resolved size through the registered
/// closure.
///
/// `bounds` is what the addressed window declares; pass
/// [`SizeBounds::UNBOUNDED`] for a surface that declares nothing, which grants
/// every ask as asked.
///
/// A dry run still requires the closure: "would you grant this" is a question
/// about a window that CAN be resized, and answering it for a surface with no
/// resize plumbing would make `applied: false` mean two different things.
///
/// # Errors
///
/// See [`ResizeError`] for the failure surface.
pub fn resize<F>(
    params: ResizeParams,
    bounds: SizeBounds,
    resize_request: Option<&mut F>,
) -> Result<ResizeOutcome, ResizeError>
where
    F: FnMut(u32, u32) + ?Sized,
{
    // Refused BEFORE resolution, and deliberately not rescued by a floor: a
    // zero extent is a malformed request (every backend refuses a zero-sized
    // window), so a floor quietly turning it into a legal size would hide the
    // caller's typo instead of naming it.
    if params.width == 0 || params.height == 0 {
        return Err(ResizeError::InvalidSize);
    }
    let closure = resize_request.ok_or(ResizeError::ClosureUnavailable)?;
    let grant = bounds.resolve((params.width, params.height));
    if !params.dry_run {
        let (w, h) = grant.size();
        closure(w, h);
    }
    Ok(ResizeOutcome::of(grant, !params.dry_run))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn params(width: u32, height: u32) -> ResizeParams {
        ResizeParams {
            width,
            height,
            dry_run: false,
        }
    }

    #[test]
    fn resize_requires_closure() {
        let err = resize::<dyn FnMut(u32, u32)>(params(320, 200), SizeBounds::UNBOUNDED, None)
            .unwrap_err();
        assert_eq!(err, ResizeError::ClosureUnavailable);
    }

    #[test]
    fn resize_rejects_zero_size() {
        let mut closure = |_w: u32, _h: u32| {};
        let err = resize(params(0, 200), SizeBounds::UNBOUNDED, Some(&mut closure)).unwrap_err();
        assert_eq!(err, ResizeError::InvalidSize);
    }

    #[test]
    fn r1710_a_zero_ask_is_refused_even_under_a_floor_that_could_rescue_it() {
        let mut closure = |_w: u32, _h: u32| {};
        let err = resize(
            params(0, 200),
            SizeBounds::floored((1440, 900)),
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
        let outcome = resize(params(345, 211), SizeBounds::UNBOUNDED, Some(&mut closure)).unwrap();
        assert_eq!((outcome.width, outcome.height), (345, 211));
        assert_eq!(outcome.asked, (345, 211));
        assert!(outcome.as_asked);
        assert!(outcome.applied);
        assert_eq!(outcome.width_bound, Bound::AsAsked);
        assert_eq!(captured.get(), (345, 211));
    }

    #[test]
    fn r1710_an_ask_below_the_floor_forwards_the_floor_and_says_so() {
        // The measured defect: this used to forward 880 and answer 880 while
        // the window sat at 900 under any window manager that enforces the
        // declared floor — and reached a genuinely 880-tall window on the bare
        // display CI runs on, so the two disagreed and neither was checked.
        let captured = Cell::new((0_u32, 0_u32));
        let mut closure = |w: u32, h: u32| {
            captured.set((w, h));
        };
        let outcome = resize(
            params(1560, 880),
            SizeBounds::floored((1440, 900)),
            Some(&mut closure),
        )
        .unwrap();
        assert_eq!(
            captured.get(),
            (1560, 900),
            "the window system is asked for the size it can grant"
        );
        assert_eq!((outcome.width, outcome.height), (1560, 900));
        assert_eq!(outcome.asked, (1560, 880));
        assert!(!outcome.as_asked);
        assert_eq!(outcome.width_bound, Bound::AsAsked);
        assert_eq!(outcome.height_bound, Bound::Floor { at: 900 });
        assert!(outcome.applied);
    }

    #[test]
    fn r1710_a_dry_run_answers_the_same_grant_and_touches_nothing() {
        let calls = Cell::new(0_u32);
        let mut closure = |_w: u32, _h: u32| calls.set(calls.get() + 1);
        let bounds = SizeBounds::floored((1440, 900));
        let dry = resize(
            ResizeParams {
                width: 1560,
                height: 880,
                dry_run: true,
            },
            bounds,
            Some(&mut closure),
        )
        .unwrap();
        assert_eq!(calls.get(), 0, "a dry run does not reach the window");
        assert!(!dry.applied);

        let real = resize(params(1560, 880), bounds, Some(&mut closure)).unwrap();
        assert_eq!(calls.get(), 1);
        // §2 #3 — the two answers differ in `applied` and in NOTHING else.
        assert_eq!(
            ResizeOutcome {
                applied: true,
                ..dry
            },
            real
        );
    }

    #[test]
    fn r1710_a_dry_run_still_needs_a_resizable_surface() {
        // Otherwise `applied: false` would mean both "you asked me not to" and
        // "I could not" — the two-reasons-one-value shape R1707 named.
        let err = resize::<dyn FnMut(u32, u32)>(
            ResizeParams {
                width: 10,
                height: 10,
                dry_run: true,
            },
            SizeBounds::UNBOUNDED,
            None,
        )
        .unwrap_err();
        assert_eq!(err, ResizeError::ClosureUnavailable);
    }

    #[test]
    fn r1710_params_default_dry_run_to_false_on_the_wire() {
        // Every pre-R1710 caller sends two keys. Deserializing must keep
        // meaning "apply it", not become an accidental no-op.
        let p: ResizeParams = serde_json::from_str(r#"{"width":800,"height":600}"#).unwrap();
        assert_eq!(p, params(800, 600));
        assert!(!p.dry_run);
    }

    #[test]
    fn r1710_the_outcome_key_set_is_what_the_census_declares() {
        let outcome = resize(
            params(1340, 880),
            SizeBounds::floored((1440, 900)),
            Some(&mut |_w: u32, _h: u32| {}),
        )
        .unwrap();
        let v = serde_json::to_value(outcome).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "applied",
                "as_asked",
                "asked",
                "height",
                "height_bound",
                "width",
                "width_bound",
            ]
        );
        assert_eq!(v["asked"], serde_json::json!([1340, 880]));
        assert_eq!(
            v["width_bound"],
            serde_json::json!({"kind": "floor", "at": 1440})
        );
    }
}
