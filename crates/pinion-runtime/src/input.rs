//! R48 §5.35 input dispatch primitive — cursor/key → widget routing.
//!
//! [`InputRouter`] owns the framework-side input retention and dispatch
//! that R47 (hello-button hit-test fix) had to implement at the
//! application level. By moving it into pinion-runtime, every example
//! and every future widget catalog entry (R47+ Slider / Toggle /
//! `TextField`) shares the same routing — the R47-class bug (cursor on
//! background still drives the widget SCXML) cannot reappear in
//! application code because application code no longer owns the
//! routing.
//!
//! ## Lifecycle
//!
//! ```text
//!   ┌─ winit CursorMoved ──────┐
//!   │                          ▼
//!   │   router.cursor_moved(x, y, &mut state_scene)
//!   │       │  re-resolve hover_target from last paint scene
//!   │       │  PointerEnter/Leave dispatch on tag transition
//!   │       ▼
//!   ┌─ winit MouseInput Press ─┐
//!   │   router.pointer_down(&mut state_scene)
//!   │       │  PointerDown to hover_target (no-op when none)
//!   │       ▼
//!   ┌─ winit MouseInput Release┐
//!   │   router.pointer_up(&mut state_scene)
//!   │       │  PointerUp to hover_target (no-op when none)
//!   │       ▼
//!   ┌─ winit CursorLeft ───────┐
//!   │   router.cursor_left(&mut state_scene)
//!   │       │  drop cursor, rollback in-flight Hover
//!   │       ▼
//!   ┌─ post-render ────────────┐
//!   │   router.update_paint_scene(paint_scene, &mut state_scene)
//!   │       │  retain paint scene, refresh hover_target
//!   │       │  (handles window resize moving a widget under
//!   │       │   a stationary cursor)
//!   └──────────────────────────┘
//! ```
//!
//! ## Tag matching
//!
//! The hit-test walks the *paint* scene's tagged Container / Box /
//! Path / Image / Text nodes (§5.20 [`Scene::tag`]) — these carry the
//! visual layout for the cursor to land on. The dispatch target is the
//! corresponding *state* scene's [`ExternalNode`] with the same tag —
//! that node carries the live SCXML statechart (or any other §5.15
//! introspectable handle). Application code only needs to keep the
//! two scenes' tags in sync: the same `"main_btn"` literal on the
//! paint Container and the state [`ExternalNode`].
//!
//! ## Out of scope (R48 carry-forward)
//!
//! - Multi-target dispatch (capture / bubble). The current router
//!   picks the deepest tagged ancestor and dispatches once.
//! - Focus tab order + keyboard dispatch. v0 routes pointer events
//!   only; key events stay with the application until the focus model
//!   lands (carry).
//! - Touch / gesture (pinch, multi-finger). winit `Touch` event is
//!   not yet wired; the routing axis for those is a separate carry.

use pinion_core::external::IntrospectValue;
use pinion_core::scene::{ExternalNode, Scene};

/// Framework-side input dispatch primitive. Owns retained paint scene,
/// cursor state, and hover target; dispatches winit-side input events
/// to the state scene's matching [`ExternalNode`] via the
/// `introspect_mut().invoke("send", Text(<event name>))` channel
/// (§5.15 item 5 input forwarding).
///
/// Application code calls into the router on every winit input event
/// and once per frame to refresh the retained paint scene. The router
/// does the hit-test, decides which widget should receive each event,
/// and dispatches through the same channel that `pinion_rpc::dispatch`
/// uses for AI-driven `scene/invoke` calls — the §2 invariant #2
/// ("RPC headless as AI primary path") stays literal: a human cursor
/// and an AI agent both reach the SCXML through the same
/// `invoke("send", ...)` path.
///
/// Widget catalog R47+ (`Slider` / `Toggle` / `TextField`) plugs in by
/// attaching a tag on its paint Container and a matching tag on its
/// state [`ExternalNode`]. No application-level hit-test code is
/// needed — adding a new widget cannot reintroduce the R47-class bug
/// because the routing primitive is framework-owned.
#[derive(Debug, Default)]
pub struct InputRouter {
    /// Last-rendered paint scene (post-layout). `None` until the
    /// first [`update_paint_scene`](Self::update_paint_scene) call.
    /// The router holds it across input events so hit-tests don't
    /// need a fresh `view()` rebuild per cursor move.
    last_paint_scene: Option<Scene>,
    /// Cursor position in window physical pixels. `None` when the
    /// cursor is outside the window or has never entered.
    cursor: Option<(f64, f64)>,
    /// Tag of the widget currently under the cursor. `None` when the
    /// cursor is over no tagged region (background, or no cursor).
    /// Drives `PointerEnter` / `PointerLeave` dispatch and gates
    /// `PointerDown` / `PointerUp`.
    hover_target: Option<String>,
}

impl InputRouter {
    /// Construct an empty router. No retained paint scene, no cursor,
    /// no hover target.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Current hover target tag, when any. Mainly for tests and
    /// diagnostic logging; application dispatch should not need to
    /// inspect this.
    #[must_use]
    pub fn hover_target(&self) -> Option<&str> {
        self.hover_target.as_deref()
    }

    /// Update the retained paint scene after each render. Re-resolves
    /// `hover_target` against the new layout — a window resize may
    /// move the button rect under a stationary cursor, and the
    /// resulting `PointerEnter` / `PointerLeave` transitions fire
    /// here so the SCXML matches the new visual state on the next
    /// frame.
    pub fn update_paint_scene(&mut self, scene: Scene, state_scene: &mut Scene) {
        self.last_paint_scene = Some(scene);
        self.refresh_hover(state_scene);
    }

    /// winit `CursorMoved` handler. Stores the new cursor position,
    /// re-resolves the hover target, and dispatches the resulting
    /// `PointerEnter` / `PointerLeave` to the state scene.
    pub fn cursor_moved(&mut self, x: f64, y: f64, state_scene: &mut Scene) {
        self.cursor = Some((x, y));
        self.refresh_hover(state_scene);
    }

    /// winit `CursorLeft` handler. Drops the cursor and dispatches a
    /// `PointerLeave` if a hover was in flight. The SCXML observes
    /// the same Leave transition regardless of whether the cursor
    /// left the window or merely crossed off the widget rect.
    pub fn cursor_left(&mut self, state_scene: &mut Scene) {
        self.cursor = None;
        if let Some(tag) = self.hover_target.take() {
            dispatch_send(state_scene, &tag, "PointerLeave");
        }
    }

    /// winit `MouseInput` left-button press handler. Dispatches
    /// `PointerDown` to the current hover target. No-op when the
    /// cursor is off all tagged regions — clicks on the background
    /// don't drive the SCXML (this is the R47 fix internalized).
    pub fn pointer_down(&mut self, state_scene: &mut Scene) {
        if let Some(tag) = self.hover_target.clone() {
            dispatch_send(state_scene, &tag, "PointerDown");
        }
    }

    /// winit `MouseInput` left-button release handler. Dispatches
    /// `PointerUp` to the current hover target. Release with the
    /// cursor off-button is a no-op: `cursor_moved`'s `PointerLeave`
    /// already drove the SCXML out of `Pressed` back to `Idle`.
    pub fn pointer_up(&mut self, state_scene: &mut Scene) {
        if let Some(tag) = self.hover_target.clone() {
            dispatch_send(state_scene, &tag, "PointerUp");
        }
    }

    /// Recompute `hover_target` from the current cursor and the
    /// retained paint scene. Dispatches `PointerLeave` for the old
    /// target (if any) then `PointerEnter` for the new target (if
    /// any) so consumers always see the leave-before-enter ordering
    /// even when the cursor crosses directly from one tagged widget
    /// to another.
    fn refresh_hover(&mut self, state_scene: &mut Scene) {
        let now = match (self.cursor, &self.last_paint_scene) {
            (Some((x, y)), Some(scene)) => resolve_hover_tag(scene, x, y),
            _ => None,
        };
        if self.hover_target == now {
            return;
        }
        if let Some(prev) = self.hover_target.take() {
            dispatch_send(state_scene, &prev, "PointerLeave");
        }
        self.hover_target.clone_from(&now);
        if let Some(target) = now {
            dispatch_send(state_scene, &target, "PointerEnter");
        }
    }
}

/// Hit-test `paint_scene` at `(x, y)` and return the deepest tagged
/// ancestor's tag. Returns `None` when no node in the hit path
/// carries a tag (the cursor is over a fully untagged region —
/// usually the background, possibly some untagged decoration).
///
/// The walk is deepest-first because the visual nesting matches the
/// expected dispatch target — a tagged label inside a tagged button
/// dispatches to the label first (if anyone tags labels), falling
/// back to the button container.
fn resolve_hover_tag(paint_scene: &Scene, x: f64, y: f64) -> Option<String> {
    let xu = floor_clamp_u32(x);
    let yu = floor_clamp_u32(y);
    let hit = paint_scene.hit_test(xu, yu)?;
    // Walk segments deepest-first: the longer the prefix, the deeper
    // the ancestor. The root (empty prefix) is the last fallback.
    for k in (0..=hit.segments.len()).rev() {
        let Some(scene) = paint_scene.lookup_path_ref(&hit.segments[..k]) else {
            continue;
        };
        if let Some(tag) = scene.tag() {
            return Some(tag.to_string());
        }
    }
    None
}

/// Dispatch a synthetic input event to the state scene's matching
/// `ExternalNode`. Walks the state scene depth-first; calls
/// `introspect_mut().invoke("send", Text(event_name))` on the first
/// node whose `tag` equals `target_tag`. Silent no-op when no
/// matching node is found — application's view-scene tag and
/// state-scene tag are out of sync, but routing keeps running rather
/// than panic.
fn dispatch_send(state_scene: &mut Scene, target_tag: &str, event_name: &str) {
    let Some(external) = find_external_by_tag(state_scene, target_tag) else {
        return;
    };
    let Some(intro) = external.handle.introspect_mut() else {
        return;
    };
    let _ = intro.invoke("send", IntrospectValue::Text(event_name.to_string()));
}

/// Depth-first search for an [`ExternalNode`] whose tag matches
/// `target_tag`. Returns the first match in declaration order
/// (matches [`walk_scene_and_drain`](crate::walk_scene_and_drain)'s
/// traversal direction). Containers recurse; non-container variants
/// compare their own tag (when applicable) and stop.
fn find_external_by_tag<'a>(scene: &'a mut Scene, target_tag: &str) -> Option<&'a mut ExternalNode> {
    match scene {
        Scene::External(node) => {
            if tag_matches(node.tag.as_deref(), target_tag) {
                Some(node)
            } else {
                None
            }
        }
        Scene::Container(c) => {
            for child in &mut c.children {
                if let Some(found) = find_external_by_tag(child, target_tag) {
                    return Some(found);
                }
            }
            None
        }
        // Box / Text / Path / Image / Effect cannot carry an
        // `External` handle, so they never produce a dispatch target.
        _ => None,
    }
}

/// Tag comparison helper. `ExternalNode.tag` is `Option<Cow<...>>`;
/// resolve the borrow then string-compare.
fn tag_matches(node_tag: Option<&str>, target: &str) -> bool {
    matches!(node_tag, Some(t) if t == target)
}

/// Saturating cast from a winit cursor coordinate (`f64`) to the
/// `u32` accepted by [`Scene::hit_test`]. Negative values clamp to
/// `0` (cursor can never hit at sub-zero coords); fractional
/// precision is dropped (hit-test resolution is whole pixels at
/// R48). The allow-list documents what the saturating clamp protects
/// against, keeping the lint silenced only at this one call site.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn floor_clamp_u32(v: f64) -> u32 {
    v.max(0.0) as u32
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use pinion_core::external::{
        Backend, BackendFallback, BackendSupport, ExternalIntrospect, InterveneError,
        IntrospectSchema, InvokeError, RepaintOwner, ThreadOwnership,
    };
    use pinion_core::scene::{ContainerNode, Rect};
    use pinion_core::style::{BoxStyle, Color};

    /// Shared-state stub External — every `invoke("send", Text(name))`
    /// pushes `name` onto the held `Vec`. Constructed with
    /// [`CaptureExternal::new`] which returns the External *and* a
    /// matching `Arc<Mutex<...>>` handle the test holds for
    /// assertion. The router moves the External into an
    /// `ExternalNode`; the test keeps the Arc clone to read what
    /// arrived without re-extracting from the scene tree.
    struct CaptureExternal {
        captures: Arc<Mutex<Vec<String>>>,
    }

    impl CaptureExternal {
        fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
            let captures = Arc::new(Mutex::new(Vec::new()));
            (Self { captures: Arc::clone(&captures) }, captures)
        }
    }

    impl std::fmt::Debug for CaptureExternal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("CaptureExternal").finish()
        }
    }

    impl pinion_core::external::External for CaptureExternal {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
            Some(self)
        }
    }

    impl ExternalIntrospect for CaptureExternal {
        fn schema(&self) -> IntrospectSchema {
            IntrospectSchema::new(&[])
        }
        fn query(&self, _path: &str) -> Option<IntrospectValue> {
            None
        }
        fn intervene(
            &mut self,
            _path: &str,
            _value: IntrospectValue,
        ) -> Result<(), InterveneError> {
            Err(InterveneError::UnknownPath)
        }
        fn invoke(
            &mut self,
            method: &str,
            args: IntrospectValue,
        ) -> Result<IntrospectValue, InvokeError> {
            if method == "send" {
                if let IntrospectValue::Text(name) = args {
                    self.captures.lock().expect("mutex poisoned").push(name);
                }
            }
            Ok(IntrospectValue::Null)
        }
    }

    fn read(captures: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
        captures.lock().expect("mutex poisoned").clone()
    }

    /// Build a paint scene with one tagged button container of fixed
    /// size, centered in a wider background container. Matches the
    /// hello-button shape so tests use realistic coordinates.
    fn paint_with_button(viewport_w: u32, viewport_h: u32, btn_rect: Rect) -> Scene {
        let button = Scene::Container(
            ContainerNode::new(vec![])
                .with_tag("main_btn")
                .with_style(BoxStyle::filled(Color::default())),
        );
        // Manually set button rect (skip taffy layout for unit-test
        // determinism; this is the post-layout artifact the router
        // would normally receive from `compute_layout`).
        let mut button_with_rect = button;
        if let Scene::Container(c) = &mut button_with_rect {
            c.rect = btn_rect;
        }
        let mut root = Scene::Container(
            ContainerNode::new(vec![button_with_rect])
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, viewport_w, viewport_h);
        }
        root
    }

    /// Build a state scene with one [`ExternalNode`] tagged `main_btn`
    /// (`CaptureExternal` inside) — the dispatch target for the paint
    /// scene above. Returns the `Arc<Mutex>` handle so tests inspect
    /// the captures without re-walking the scene tree.
    fn state_with_button() -> (Scene, Arc<Mutex<Vec<String>>>) {
        let (capture, captures) = CaptureExternal::new();
        let scene = Scene::External(
            ExternalNode::new(Box::new(capture)).with_tag("main_btn"),
        );
        (scene, captures)
    }

    #[test]
    fn cursor_off_button_does_not_dispatch() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        // Cursor at (10, 10) — far from the button rect (80..120 x 80..120).
        router.cursor_moved(10.0, 10.0, &mut state);
        router.pointer_down(&mut state);
        router.pointer_up(&mut state);
        assert!(read(&captures).is_empty());
        assert_eq!(router.hover_target(), None);
    }

    #[test]
    fn cursor_on_button_dispatches_enter_then_down_up() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        // Cursor on the button rect center.
        router.cursor_moved(100.0, 100.0, &mut state);
        router.pointer_down(&mut state);
        router.pointer_up(&mut state);
        assert_eq!(
            read(&captures),
            vec!["PointerEnter".to_string(), "PointerDown".into(), "PointerUp".into()],
        );
        assert_eq!(router.hover_target(), Some("main_btn"));
    }

    #[test]
    fn cursor_crossing_off_button_fires_leave() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(100.0, 100.0, &mut state); // on
        router.cursor_moved(10.0, 10.0, &mut state); // off
        assert_eq!(
            read(&captures),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
        assert_eq!(router.hover_target(), None);
    }

    #[test]
    fn cursor_left_rolls_back_hover() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(100.0, 100.0, &mut state); // on
        router.cursor_left(&mut state); // window-leave
        assert_eq!(
            read(&captures),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
        assert_eq!(router.hover_target(), None);
    }

    #[test]
    fn pointer_down_off_button_is_noop() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        // No cursor_moved — cursor stays None.
        router.pointer_down(&mut state);
        assert!(read(&captures).is_empty());
    }

    #[test]
    fn pointer_down_before_first_paint_is_noop() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        // CursorMoved arrives before update_paint_scene — common at
        // startup. last_paint_scene is None, so hover_target stays
        // None, so dispatch is suppressed.
        router.cursor_moved(100.0, 100.0, &mut state);
        router.pointer_down(&mut state);
        assert!(read(&captures).is_empty());
    }

    #[test]
    fn resize_shifts_button_under_stationary_cursor() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        // First frame: button at (80..120) — cursor at (100, 100) hits.
        router.update_paint_scene(
            paint_with_button(200, 200, Rect::new(80, 80, 40, 40)),
            &mut state,
        );
        router.cursor_moved(100.0, 100.0, &mut state);
        assert_eq!(router.hover_target(), Some("main_btn"));
        // Window resize moves the button to (10..50). Cursor stays at
        // (100, 100) — now off the button. update_paint_scene must
        // re-resolve and emit PointerLeave.
        router.update_paint_scene(
            paint_with_button(200, 200, Rect::new(10, 10, 40, 40)),
            &mut state,
        );
        assert_eq!(router.hover_target(), None);
        assert_eq!(
            read(&captures),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
    }

    #[test]
    fn dispatch_to_missing_state_tag_is_silent() {
        let mut router = InputRouter::new();
        // State has a different tag than the paint scene's button.
        let (capture, captures) = CaptureExternal::new();
        let mut state = Scene::External(
            ExternalNode::new(Box::new(capture)).with_tag("other_widget"),
        );
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(100.0, 100.0, &mut state);
        // hover_target resolves to "main_btn" from paint, but state
        // has no matching ExternalNode → silent no-op.
        assert_eq!(router.hover_target(), Some("main_btn"));
        assert!(read(&captures).is_empty());
    }

    #[test]
    fn floor_clamp_u32_handles_negative_and_fractional() {
        assert_eq!(floor_clamp_u32(-1.0), 0);
        assert_eq!(floor_clamp_u32(0.0), 0);
        assert_eq!(floor_clamp_u32(1.9), 1);
        assert_eq!(floor_clamp_u32(99.5), 99);
    }
}
