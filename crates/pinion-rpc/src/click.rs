//! `scene/click` RPC method dispatch (§5.12 method 2 of 7, R16 slice 11).
//!
//! Synthesizes a [`PointerEvent::Down`] at logical coords `(x, y)`
//! against the `External` at the resolved window's scene root, and
//! consults [`External::handles_event`] for the policy answer.
//!
//! v0 semantics intentionally narrow:
//!   * path syntax: `/[window[id]/]external` — same prefix shape as
//!     `scene/query` (§5.18 short-circuit), the trailing `/external`
//!     selects the scene-root `External`.
//!   * only the *down* half of a click is synthesized — the framework
//!     does not yet own a real event pipeline, so the response reports
//!     the External's `handles_event` policy verdict, not an actual
//!     state transition.
//!   * scene-tree traversal (clicking a `Button` widget nested under
//!     `Scene::Container`) lands when §5.3 DSL settles a path syntax for
//!     the introspectable variants.

use pinion_core::event::{Coord, Event, PointerEvent};
use pinion_core::Scene;

use crate::path::{self, PathError};

/// Outcome of a [`click`] call.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClickOutcome {
    /// `true` when the `External` declared it would handle the event
    /// (returned `true` from `handles_event`).
    pub handled: bool,
}

/// Reasons [`click`] can fail.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickError {
    /// Window-prefix parsing failed.
    Path(PathError),
    /// Scene path does not match a v0-supported click target.
    UnsupportedPath,
    /// Expected an `External` at the scene root, found a different
    /// primitive.
    NoExternalAtPath,
}

impl From<PathError> for ClickError {
    fn from(err: PathError) -> Self {
        ClickError::Path(err)
    }
}

/// Synthesize a click against `scene` at `(x, y)` logical coords.
///
/// See module docs for v0 semantics (probe-only, no state mutation).
///
/// # Errors
///
/// Returns [`ClickError`] when the path is malformed, the scene root is
/// not an `External`, or the path is not v0-supported.
pub fn click(
    scene: &Scene,
    raw_path: &str,
    x: f32,
    y: f32,
) -> Result<ClickOutcome, ClickError> {
    let resolved = path::resolve(raw_path)?;
    let _ = resolved.window;

    // v0: scene-path must be exactly "/external". Empty path (no /external
    // segment) and any other shape are unsupported until §5.3 DSL settles
    // richer scene addressing.
    if resolved.scene_path != "/external" {
        return Err(ClickError::UnsupportedPath);
    }

    let Scene::External(node) = scene else {
        return Err(ClickError::NoExternalAtPath);
    };

    let event = Event::Pointer(PointerEvent::Down {
        coord: Coord::logical(x, y),
    });
    let handled = node.handle.handles_event(&event);
    Ok(ClickOutcome { handled })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::{
        Backend, BackendFallback, BackendSupport, CountedExternal, External, RepaintOwner,
        StubExternal, ThreadOwnership,
    };
    use pinion_core::scene::{BoxNode, ExternalNode};

    /// Test fixture: an `External` that *claims* every event via
    /// `handles_event`. Lets us exercise the `handled=true` path of the
    /// click dispatch without modifying the production `Stub`/`Counted`
    /// no-op posture.
    #[derive(Debug, Default)]
    struct EventCapturingExternal;

    impl External for EventCapturingExternal {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::External
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn handles_event(&self, _event: &Event) -> bool {
            true
        }
    }

    fn external_scene(handle: Box<dyn External>) -> Scene {
        Scene::External(ExternalNode::new(handle))
    }

    #[test]
    fn stub_external_does_not_handle() {
        let scene = external_scene(Box::new(StubExternal::new()));
        let outcome = click(&scene, "/external", 10.0, 20.0).unwrap();
        assert!(!outcome.handled);
    }

    #[test]
    fn counted_external_does_not_handle_by_default() {
        let scene = external_scene(Box::new(CountedExternal::new(0)));
        let outcome = click(&scene, "/external", 0.0, 0.0).unwrap();
        assert!(!outcome.handled);
    }

    #[test]
    fn capturing_external_handles() {
        let scene = external_scene(Box::new(EventCapturingExternal));
        let outcome = click(&scene, "/external", 1.5, 2.5).unwrap();
        assert!(outcome.handled);
    }

    #[test]
    fn window_prefix_short_circuits_to_first_window() {
        let scene = external_scene(Box::new(EventCapturingExternal));
        let outcome = click(&scene, "/window[main]/external", 0.0, 0.0).unwrap();
        assert!(outcome.handled);
    }

    #[test]
    fn box_at_root_reports_no_external() {
        let scene = Scene::Box(BoxNode::new());
        let err = click(&scene, "/external", 0.0, 0.0).unwrap_err();
        assert_eq!(err, ClickError::NoExternalAtPath);
    }

    #[test]
    fn unsupported_scene_path_rejected() {
        let scene = external_scene(Box::new(StubExternal::new()));
        let err = click(&scene, "/external/extra", 0.0, 0.0).unwrap_err();
        assert_eq!(err, ClickError::UnsupportedPath);
    }

    #[test]
    fn malformed_window_prefix_surfaces_as_path_error() {
        let scene = external_scene(Box::new(StubExternal::new()));
        let err = click(&scene, "/window[main/external", 0.0, 0.0).unwrap_err();
        assert_eq!(err, ClickError::Path(PathError::MalformedPrefix));
    }
}
