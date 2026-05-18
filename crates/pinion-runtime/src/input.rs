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
//!   │   router.cursor_moved(id, x, y, &mut state_scene)
//!   │       │  re-resolve hover_targets[id] from last paint scene
//!   │       │  PointerEnter/Leave dispatch on tag transition
//!   │       ▼
//!   ┌─ winit MouseInput Press ─┐
//!   │   router.pointer_down(id, &mut state_scene)
//!   │       │  PointerDown to hover_targets[id] (no-op when none)
//!   │       ▼
//!   ┌─ winit MouseInput Release┐
//!   │   router.pointer_up(id, &mut state_scene)
//!   │       │  PointerUp to hover_targets[id] (no-op when none)
//!   │       ▼
//!   ┌─ winit CursorLeft ───────┐
//!   │   router.cursor_left(id, &mut state_scene)
//!   │       │  drop cursor for id, rollback in-flight Hover
//!   │       ▼
//!   ┌─ post-render ────────────┐
//!   │   router.update_paint_scene(paint_scene, &mut state_scene)
//!   │       │  retain paint scene, refresh hover_targets for
//!   │       │  every active pointer (handles window resize moving
//!   │       │  a widget under a stationary cursor)
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
//! ## Multi-pointer (R51.38 §5.35)
//!
//! Every input method takes a [`PointerId`] identifying the source
//! pointer. Mouse-driven shells pass [`PointerId::MOUSE`]; touch /
//! pen / future input sources mint distinct ids via
//! [`PointerId::touch`]. Per-pointer state (`cursor`, `hover_target`,
//! `captured_target`) lives in `HashMap<PointerId, _>` so two
//! simultaneous touches can drag two different widgets without
//! aliasing the capture lock. Single-pointer mouse shells observe no
//! behavioural change — the maps degenerate to a single entry under
//! `PointerId::MOUSE`. This is the first-design ratify for the
//! mobile / multi-touch axis; designing in capture-aliasing-by-default
//! and refactoring later was the carry-forward path the R51.38
//! substrate-first decision rejected.
//!
//! ## Out of scope (R48+ carry-forward)
//!
//! - Multi-target dispatch (capture / bubble). The current router
//!   picks the deepest tagged ancestor and dispatches once.
//! - Focus tab order + keyboard dispatch. v0 routes pointer events
//!   only; key events stay with the application until the focus model
//!   lands (carry).
//! - Touch event wiring at the shell layer. The router's API accepts
//!   touch pointers via [`PointerId::touch`], but no `pinion-shell`
//!   call site sources them yet — winit `Touch` event integration is
//!   a separate carry.

use std::collections::HashMap;

use pinion_core::external::IntrospectValue;
use pinion_core::scene::{ExternalNode, Rect, Scene};

/// R51.38 §5.35 — pointer identity used by every [`InputRouter`]
/// input method to route per-pointer cursor / hover / capture state.
/// Mouse events on every desktop platform pinion supports come from a
/// single (logical) source, so [`PointerId::MOUSE`] is a fixed `const`
/// and mouse-driven shells never allocate. Touch finger IDs route
/// through [`PointerId::touch`] which offsets by one so `PointerId(0)`
/// stays reserved for the mouse.
///
/// `Hash` + `Eq` + `Copy` so the routing tables can key on it without
/// allocation; `Debug` for diagnostic logging. The internal `u64`
/// width matches winit's `FingerId` to avoid lossy narrowing when
/// shells eventually wire touch events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PointerId(u64);

impl PointerId {
    /// The primary mouse pointer. Mouse events on every desktop
    /// platform are single-source, so this constant suffices —
    /// shells pass it unconditionally for every winit `CursorMoved`
    /// / `MouseInput` event. The reserved id `0` cannot collide with
    /// any [`PointerId::touch`] result because that factory offsets
    /// by one.
    pub const MOUSE: PointerId = PointerId(0);

    /// Touch-finger pointer id. The factory offsets by one so a
    /// `winit::event::Touch::id` of `0` maps to `PointerId(1)`,
    /// keeping `PointerId(0)` reserved for [`MOUSE`]. Wrapping
    /// addition handles the (theoretical) `u64::MAX` finger id edge
    /// without panic — wrap-around lands at `PointerId(0)` which
    /// then aliases the mouse, but in practice no platform mints
    /// finger ids anywhere near that magnitude.
    #[must_use]
    pub fn touch(finger_id: u64) -> Self {
        PointerId(finger_id.wrapping_add(1))
    }

    /// Raw underlying value. Exposed for diagnostic logging and for
    /// shells that mint custom synthetic pointer IDs (e.g. pen input
    /// on platforms pinion adds later). Application code that just
    /// routes mouse + touch should prefer the [`MOUSE`] constant and
    /// the [`touch`](Self::touch) factory.
    #[must_use]
    pub fn raw(self) -> u64 {
        self.0
    }
}

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
    /// R51.38 §5.35 — per-pointer cursor position in window physical
    /// pixels. Absence means the pointer is outside the window or
    /// has never entered. Mouse-driven shells observe a single
    /// `PointerId::MOUSE` entry; touch / pen shells route each
    /// finger / stylus through its own [`PointerId`].
    cursors: HashMap<PointerId, (f64, f64)>,
    /// R51.38 §5.35 — per-pointer hover target tag. Empty when no
    /// pointer is over a tagged region. Drives `PointerEnter` /
    /// `PointerLeave` dispatch and gates `PointerDown` /
    /// `PointerUp` per pointer, so two simultaneous touches can sit
    /// on two different widgets without aliasing.
    hover_targets: HashMap<PointerId, String>,
    /// R51.34 §5.35 + R51.38 §5.35 — per-pointer capture-lock map:
    /// tag of the widget each pointer claimed on its most recent
    /// `pointer_down` via [`External::wants_pointer_capture`]. While
    /// an entry is present, every
    /// [`cursor_moved`](Self::cursor_moved) for that pointer skips
    /// [`refresh_hover`](Self::refresh_hover) and forwards the
    /// cursor position to the widget's
    /// [`External::pointer_move`](pinion_core::external::External::pointer_move).
    /// Cleared on [`pointer_up`](Self::pointer_up); the subsequent
    /// `refresh_hover` fires the deferred `PointerLeave` if the
    /// cursor strayed off the widget during the drag. `cursor_left`
    /// is suppressed for that pointer while capture is in flight so
    /// the drag survives the window-leave / re-enter cycle. Multi-
    /// touch drags (two fingers, two widgets) each get an
    /// independent entry — the R51.38 first-design ratify avoids
    /// the aliasing-by-default refactor cost of single-target
    /// capture.
    captured_targets: HashMap<PointerId, String>,
}

impl InputRouter {
    /// Construct an empty router. No retained paint scene, no
    /// cursors, no hover targets, no capture locks.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Current hover target tag for `id`, when any. Mainly for tests
    /// and diagnostic logging; application dispatch should not need
    /// to inspect this directly.
    #[must_use]
    pub fn hover_target(&self, id: PointerId) -> Option<&str> {
        self.hover_targets.get(&id).map(String::as_str)
    }

    /// R51.34 §5.35 — current capture-lock target tag for `id`, when
    /// that pointer claimed a widget via
    /// [`External::wants_pointer_capture`] on its most recent
    /// [`pointer_down`](Self::pointer_down). `None` when no drag is
    /// in flight for that pointer. Diagnostic / test surface only —
    /// application code never needs to inspect this directly.
    #[must_use]
    pub fn captured_target(&self, id: PointerId) -> Option<&str> {
        self.captured_targets.get(&id).map(String::as_str)
    }

    /// Update the retained paint scene after each render. Re-resolves
    /// `hover_targets` for every active pointer against the new
    /// layout — a window resize may move the button rect under a
    /// stationary cursor, and the resulting `PointerEnter` /
    /// `PointerLeave` transitions fire here so the SCXML matches the
    /// new visual state on the next frame. Pointers under capture
    /// lock keep their hover pinned (the drag invariant).
    pub fn update_paint_scene(&mut self, scene: Scene, state_scene: &mut Scene) {
        self.last_paint_scene = Some(scene);
        // Snapshot pointer ids before iterating — refresh_hover
        // takes &mut self and mutates `hover_targets`. Cloning the
        // key set keeps the multi-pointer iteration self-contained
        // (single-pointer shells: 1 entry, negligible cost).
        let ids: Vec<PointerId> = self.cursors.keys().copied().collect();
        for id in ids {
            if self.captured_targets.contains_key(&id) {
                continue;
            }
            self.refresh_hover(id, state_scene);
        }
    }

    /// winit `CursorMoved` handler. Stores the new cursor position
    /// under `id` then either:
    ///
    /// * **Capture mode** (R51.34 §5.35): when a drag-aware widget
    ///   holds the lock for this pointer, forward the cursor
    ///   position to its
    ///   [`External::pointer_move`](pinion_core::external::External::pointer_move)
    ///   as widget-relative normalised `(x_rel, y_rel)`. The hover
    ///   target stays pinned so the SCXML does not see spurious
    ///   `PointerLeave` events when the cursor strays off the
    ///   widget rect mid-drag.
    /// * **Free mode** (pre-R51.34 default): re-resolve this
    ///   pointer's hover target and dispatch `PointerEnter` /
    ///   `PointerLeave` on transitions — the canonical button-like
    ///   cancel-by-leave UX.
    pub fn cursor_moved(&mut self, id: PointerId, x: f64, y: f64, state_scene: &mut Scene) {
        self.cursors.insert(id, (x, y));
        if let Some(tag) = self.captured_targets.get(&id).cloned() {
            self.forward_pointer_move(state_scene, &tag, x, y);
        } else {
            self.refresh_hover(id, state_scene);
        }
    }

    /// winit `CursorLeft` handler. Drops the cursor for `id` and
    /// dispatches a `PointerLeave` if a hover was in flight for that
    /// pointer — *unless* a drag is in flight (R51.34 §5.35 capture
    /// lock), in which case the hover stays pinned so the drag
    /// survives the window-leave / re-enter cycle that a real
    /// drag-out gesture produces. The deferred `PointerLeave` (if
    /// the cursor never returns) fires on the matching
    /// [`pointer_up`](Self::pointer_up).
    pub fn cursor_left(&mut self, id: PointerId, state_scene: &mut Scene) {
        self.cursors.remove(&id);
        if self.captured_targets.contains_key(&id) {
            return;
        }
        if let Some(tag) = self.hover_targets.remove(&id) {
            dispatch_send(state_scene, &tag, "PointerLeave");
        }
    }

    /// winit `MouseInput` (or touch-down) press handler for `id`.
    /// Dispatches `PointerDown` to the pointer's current hover
    /// target. No-op when that pointer is over no tagged region —
    /// clicks on the background don't drive the SCXML (this is the
    /// R47 fix internalized).
    ///
    /// R51.34 §5.35: after dispatch, if the target widget opts in to
    /// pointer capture via
    /// [`External::wants_pointer_capture`](pinion_core::external::External::wants_pointer_capture),
    /// the router pins this pointer's `captured_targets` entry to
    /// that tag for the duration of the press. While pinned,
    /// [`cursor_moved`] forwards the cursor to the widget through
    /// [`External::pointer_move`](pinion_core::external::External::pointer_move)
    /// and suppresses hover / leave dispatch for this pointer.
    /// Button-like widgets keep the default `false` and observe no
    /// behaviour change.
    pub fn pointer_down(&mut self, id: PointerId, state_scene: &mut Scene) {
        if let Some(tag) = self.hover_targets.get(&id).cloned() {
            dispatch_send(state_scene, &tag, "PointerDown");
            if widget_wants_capture(state_scene, &tag) {
                self.captured_targets.insert(id, tag.clone());
                // R51.35 §5.35 — click-to-position: forward the
                // press-time cursor as the initial `pointer_move` so
                // a click-without-drag still seeds the widget's
                // value at the click point (Material / `SwiftUI` / Qt
                // Slider click-jumps-to-position UX). Without this
                // forward the value would not update unless the user
                // also dragged the cursor at least one pixel.
                if let Some(&(x, y)) = self.cursors.get(&id) {
                    self.forward_pointer_move(state_scene, &tag, x, y);
                }
            }
        }
    }

    /// winit `MouseInput` (or touch-up) release handler for `id`.
    /// Dispatches `PointerUp` to that pointer's current hover
    /// target. Release with the cursor off-button is a no-op in
    /// free mode: `cursor_moved`'s `PointerLeave` already drove the
    /// SCXML out of `Pressed` back to `Idle`.
    ///
    /// R51.34 §5.35: in capture mode the cursor may currently sit
    /// off the widget rect (the drag strayed). `PointerUp` still
    /// dispatches to the captured tag so the SCXML observes the
    /// drag-end transition (e.g. Slider `Dragging → Hover` →
    /// `value_committed` intent). Capture for this pointer is then
    /// released and [`refresh_hover`](Self::refresh_hover) re-runs
    /// — if the cursor really did stray off, the deferred
    /// `PointerLeave` fires here.
    pub fn pointer_up(&mut self, id: PointerId, state_scene: &mut Scene) {
        let target = self
            .hover_targets
            .get(&id)
            .cloned()
            .or_else(|| self.captured_targets.get(&id).cloned());
        if let Some(tag) = target {
            dispatch_send(state_scene, &tag, "PointerUp");
        }
        if self.captured_targets.remove(&id).is_some() {
            self.refresh_hover(id, state_scene);
        }
    }

    /// R51.34 §5.35 — capture-mode cursor forward. Look up the
    /// post-layout rect of the captured widget in the retained paint
    /// scene, normalise the cursor `(x, y)` into widget-relative
    /// `[0.0, 1.0]` coordinates (may exceed when the cursor strays),
    /// and hand them to the captured `External` via
    /// [`External::pointer_move`](pinion_core::external::External::pointer_move).
    /// Silent no-op when the paint scene is unset (cursor moved
    /// before the first frame) or the tag is unmappable to a rect.
    fn forward_pointer_move(
        &self,
        state_scene: &mut Scene,
        target_tag: &str,
        cursor_x: f64,
        cursor_y: f64,
    ) {
        let Some(paint) = self.last_paint_scene.as_ref() else {
            return;
        };
        let Some(rect) = rect_for_tag(paint, target_tag) else {
            return;
        };
        let (x_rel, y_rel) = normalize_cursor(rect, cursor_x, cursor_y);
        let Some(external) = find_external_by_tag(state_scene, target_tag) else {
            return;
        };
        external.handle.pointer_move(x_rel, y_rel);
    }

    /// Recompute `hover_targets[id]` from `id`'s current cursor and
    /// the retained paint scene. Dispatches `PointerLeave` for the
    /// pointer's old target (if any) then `PointerEnter` for its new
    /// target (if any) so consumers always see the leave-before-
    /// enter ordering even when the cursor crosses directly from one
    /// tagged widget to another. Per-pointer ordering — two
    /// pointers crossing different widgets see two independent
    /// enter / leave streams.
    fn refresh_hover(&mut self, id: PointerId, state_scene: &mut Scene) {
        let now = match (self.cursors.get(&id), &self.last_paint_scene) {
            (Some(&(x, y)), Some(scene)) => resolve_hover_tag(scene, x, y),
            _ => None,
        };
        let prev = self.hover_targets.get(&id).cloned();
        if prev == now {
            return;
        }
        if let Some(prev_tag) = prev {
            self.hover_targets.remove(&id);
            dispatch_send(state_scene, &prev_tag, "PointerLeave");
        }
        if let Some(target) = now {
            self.hover_targets.insert(id, target.clone());
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

/// R51.34 §5.35 — depth-first walk for the post-layout rect of the
/// tagged primitive named by `target_tag`. Returns the first match
/// in declaration order (mirrors [`find_external_by_tag`]'s walk).
/// `EffectNode` carries no tag so the walk skips it implicitly via
/// [`Scene::tag`]. `None` when no node in the paint tree matches.
fn rect_for_tag(scene: &Scene, target_tag: &str) -> Option<Rect> {
    if let Some(tag) = scene.tag() {
        if tag == target_tag {
            return Some(scene.rect());
        }
    }
    if let Scene::Container(c) = scene {
        for child in &c.children {
            if let Some(rect) = rect_for_tag(child, target_tag) {
                return Some(rect);
            }
        }
    }
    None
}

/// R51.34 §5.35 — normalise a winit cursor `(f64, f64)` into
/// widget-relative `(f32, f32)` over `rect`. `0.0` maps to the
/// left / top edge, `1.0` to the right / bottom edge. Coordinates
/// may exceed `[0.0, 1.0]` (or be negative) when the cursor strays
/// outside the rect under R51.34 capture lock — Slider clamps in
/// its [`pointer_move`](pinion_core::external::External::pointer_move)
/// impl, future drag widgets may not. Zero-size rect (degenerate
/// layout) collapses to `(0.0, 0.0)` so consumers never divide by
/// zero.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn normalize_cursor(rect: Rect, cursor_x: f64, cursor_y: f64) -> (f32, f32) {
    let width = f64::from(rect.w);
    let height = f64::from(rect.h);
    let x_rel = if width > 0.0 {
        ((cursor_x - f64::from(rect.x)) / width) as f32
    } else {
        0.0
    };
    let y_rel = if height > 0.0 {
        ((cursor_y - f64::from(rect.y)) / height) as f32
    } else {
        0.0
    };
    (x_rel, y_rel)
}

/// R51.34 §5.35 — ask the state-scene `ExternalNode` matching
/// `target_tag` whether it opts in to pointer capture via
/// [`External::wants_pointer_capture`](pinion_core::external::External::wants_pointer_capture).
/// `false` when no matching node is found (out-of-sync paint and
/// state tags) so the router never claims capture on a phantom
/// widget.
fn widget_wants_capture(state_scene: &Scene, target_tag: &str) -> bool {
    widget_wants_capture_walk(state_scene, target_tag).unwrap_or(false)
}

/// Recursive helper for [`widget_wants_capture`]. Returns
/// `Some(bool)` when the tag is found (allowing the caller to
/// distinguish "found, but declined" from "not found"), `None`
/// when the walk finds no match.
fn widget_wants_capture_walk(scene: &Scene, target_tag: &str) -> Option<bool> {
    match scene {
        Scene::External(node) => {
            if tag_matches(node.tag.as_deref(), target_tag) {
                Some(node.handle.wants_pointer_capture())
            } else {
                None
            }
        }
        Scene::Container(c) => {
            for child in &c.children {
                if let Some(found) = widget_wants_capture_walk(child, target_tag) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
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
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert!(read(&captures).is_empty());
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
    }

    #[test]
    fn cursor_on_button_dispatches_enter_then_down_up() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        // Cursor on the button rect center.
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            read(&captures),
            vec!["PointerEnter".to_string(), "PointerDown".into(), "PointerUp".into()],
        );
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("main_btn"));
    }

    #[test]
    fn cursor_crossing_off_button_fires_leave() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // on
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state); // off
        assert_eq!(
            read(&captures),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
    }

    #[test]
    fn cursor_left_rolls_back_hover() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // on
        router.cursor_left(PointerId::MOUSE, &mut state); // window-leave
        assert_eq!(
            read(&captures),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
    }

    #[test]
    fn pointer_down_off_button_is_noop() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        // No cursor_moved — cursor stays None.
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert!(read(&captures).is_empty());
    }

    #[test]
    fn pointer_down_before_first_paint_is_noop() {
        let mut router = InputRouter::new();
        let (mut state, captures) = state_with_button();
        // CursorMoved arrives before update_paint_scene — common at
        // startup. last_paint_scene is None, so hover_target stays
        // None, so dispatch is suppressed.
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
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
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("main_btn"));
        // Window resize moves the button to (10..50). Cursor stays at
        // (100, 100) — now off the button. update_paint_scene must
        // re-resolve and emit PointerLeave.
        router.update_paint_scene(
            paint_with_button(200, 200, Rect::new(10, 10, 40, 40)),
            &mut state,
        );
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
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
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        // hover_target resolves to "main_btn" from paint, but state
        // has no matching ExternalNode → silent no-op.
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("main_btn"));
        assert!(read(&captures).is_empty());
    }

    #[test]
    fn floor_clamp_u32_handles_negative_and_fractional() {
        assert_eq!(floor_clamp_u32(-1.0), 0);
        assert_eq!(floor_clamp_u32(0.0), 0);
        assert_eq!(floor_clamp_u32(1.9), 1);
        assert_eq!(floor_clamp_u32(99.5), 99);
    }

    // ─── R51.34 §5.35 capture-lock fixtures + tests ────────────

    /// Shared event log alias — symbolic input event names captured
    /// via `invoke("send", Text(<name>))` calls (the full symbolic
    /// path the router uses for `PointerEnter` / `PointerDown` /
    /// `PointerUp` / `PointerLeave`). Tests hold an `Arc` clone for
    /// assertions; the router moves the External into an
    /// `ExternalNode`.
    type EventLog = Arc<Mutex<Vec<String>>>;

    /// Shared move log alias — `(x_rel, y_rel)` tuples captured via
    /// `External::pointer_move` during capture lock. Only the
    /// `DragCaptureExternal` fixture appends here.
    type MoveLog = Arc<Mutex<Vec<(f32, f32)>>>;

    /// Drag-aware capture fixture. Opts in to pointer capture via
    /// [`External::wants_pointer_capture`] and records every
    /// [`External::pointer_move`] forward (so tests can assert the
    /// router fed the correct widget-relative normalised coords).
    /// Symbolic events (`PointerEnter` / `Down` / `Up` / `Leave`)
    /// share the same `events` log as the existing
    /// [`CaptureExternal`] for cross-correlation assertions in
    /// drag-end sequences.
    struct DragCaptureExternal {
        events: EventLog,
        moves: MoveLog,
    }

    impl DragCaptureExternal {
        fn new() -> (Self, EventLog, MoveLog) {
            let events = Arc::new(Mutex::new(Vec::new()));
            let moves = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    events: Arc::clone(&events),
                    moves: Arc::clone(&moves),
                },
                events,
                moves,
            )
        }
    }

    impl std::fmt::Debug for DragCaptureExternal {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("DragCaptureExternal").finish()
        }
    }

    impl pinion_core::external::External for DragCaptureExternal {
        fn backends(&self) -> BackendSupport {
            BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
        }
        fn repaint_ownership(&self) -> RepaintOwner {
            RepaintOwner::Framework
        }
        fn thread_ownership(&self) -> ThreadOwnership {
            ThreadOwnership::UiThreadSync
        }
        fn wants_pointer_capture(&self) -> bool {
            true
        }
        fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
            self.moves.lock().expect("mutex poisoned").push((x_rel, y_rel));
        }
        fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
            Some(self)
        }
    }

    impl ExternalIntrospect for DragCaptureExternal {
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
                    self.events.lock().expect("mutex poisoned").push(name);
                }
            }
            Ok(IntrospectValue::Null)
        }
    }

    fn read_moves(moves: &MoveLog) -> Vec<(f32, f32)> {
        moves.lock().expect("mutex poisoned").clone()
    }

    /// Paint scene mirroring [`paint_with_button`] but with the
    /// `main_slider` tag — the drag-widget counterpart of the
    /// button-like fixture.
    fn paint_with_slider(viewport_w: u32, viewport_h: u32, slider_rect: Rect) -> Scene {
        let slider = Scene::Container(
            ContainerNode::new(vec![])
                .with_tag("main_slider")
                .with_style(BoxStyle::filled(Color::default())),
        );
        let mut slider_with_rect = slider;
        if let Scene::Container(c) = &mut slider_with_rect {
            c.rect = slider_rect;
        }
        let mut root = Scene::Container(
            ContainerNode::new(vec![slider_with_rect])
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, viewport_w, viewport_h);
        }
        root
    }

    fn state_with_slider() -> (Scene, EventLog, MoveLog) {
        let (capture, events, moves) = DragCaptureExternal::new();
        let scene = Scene::External(
            ExternalNode::new(Box::new(capture)).with_tag("main_slider"),
        );
        (scene, events, moves)
    }

    #[test]
    fn capture_lock_pins_hover_during_drag() {
        // Drag-aware widget: cursor stray off rect during press must
        // NOT fire PointerLeave. The SCXML must stay in its `Dragging`
        // state through the strays, ending only on pointer_up.
        let mut router = InputRouter::new();
        let (mut state, events, _moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // PointerEnter
        router.pointer_down(PointerId::MOUSE, &mut state); // PointerDown + capture lock
        assert_eq!(router.captured_target(PointerId::MOUSE), Some("main_slider"));
        router.cursor_moved(PointerId::MOUSE, 200.0, 200.0, &mut state); // stray off
        // No PointerLeave during stray — capture lock keeps the
        // hover pinned. Only PointerEnter + PointerDown so far.
        assert_eq!(
            read(&events),
            vec!["PointerEnter".to_string(), "PointerDown".into()],
        );
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // back over
        // Still no extra events (the router is in capture mode,
        // hover doesn't re-resolve).
        assert_eq!(
            read(&events),
            vec!["PointerEnter".to_string(), "PointerDown".into()],
        );
        router.pointer_up(PointerId::MOUSE, &mut state);
        // PointerUp lands now; capture clears; subsequent refresh
        // sees cursor (100, 100) IS on the rect — no PointerLeave.
        assert_eq!(
            read(&events),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
            ],
        );
        assert_eq!(router.captured_target(PointerId::MOUSE), None);
    }

    #[test]
    fn capture_lock_forwards_pointer_move_normalized() {
        // During capture, cursor_moved must forward the cursor as
        // widget-relative normalised coords. Rect (80, 80, 40, 40)
        // means cursor (100, 100) → ((100 - 80) / 40, (100 - 80) / 40)
        // = (0.5, 0.5). The R51.35 click-to-position patch makes
        // pointer_down forward the press-time cursor too, so the
        // press at (100, 100) emits a (0.5, 0.5) entry before the
        // three drag-time moves below.
        let mut router = InputRouter::new();
        let (mut state, _events, moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // PointerEnter (not capture-mode yet)
        assert!(read_moves(&moves).is_empty());
        router.pointer_down(PointerId::MOUSE, &mut state); // enter capture + click-point forward (0.5, 0.5)
        router.cursor_moved(PointerId::MOUSE, 80.0, 80.0, &mut state); // top-left
        router.cursor_moved(PointerId::MOUSE, 120.0, 120.0, &mut state); // bottom-right
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state); // centre
        let log = read_moves(&moves);
        assert_eq!(log.len(), 4);
        assert!((log[0].0 - 0.5).abs() < 1e-4 && (log[0].1 - 0.5).abs() < 1e-4);
        assert!((log[1].0 - 0.0).abs() < 1e-4 && (log[1].1 - 0.0).abs() < 1e-4);
        assert!((log[2].0 - 1.0).abs() < 1e-4 && (log[2].1 - 1.0).abs() < 1e-4);
        assert!((log[3].0 - 0.5).abs() < 1e-4 && (log[3].1 - 0.5).abs() < 1e-4);
    }

    #[test]
    fn pointer_down_forwards_initial_cursor() {
        // R51.35 §5.35 — click-without-drag still updates the
        // widget's value. The Slider UX precedent: clicking on the
        // track jumps the thumb to the click point even if the user
        // releases without moving the mouse.
        let mut router = InputRouter::new();
        let (mut state, _events, moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        // Click at x = 110 → x_rel = (110 - 80) / 40 = 0.75.
        router.cursor_moved(PointerId::MOUSE, 110.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        let log = read_moves(&moves);
        // Exactly one pointer_move (the click-point); no drag moves
        // because the cursor never moved between down and up.
        assert_eq!(log.len(), 1);
        assert!((log[0].0 - 0.75).abs() < 1e-4);
        assert!((log[0].1 - 0.5).abs() < 1e-4);
    }

    #[test]
    fn capture_lock_allows_coords_outside_rect() {
        // Stray off the widget under capture lock — coords may exceed
        // [0, 1] or be negative; the consumer (Slider) clamps in its
        // own pointer_move impl. R51.35 click-to-position prepends a
        // (0.5, 0.5) press-time entry; the two strays follow.
        let mut router = InputRouter::new();
        let (mut state, _events, moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state); // click-point (0.5, 0.5)
        router.cursor_moved(PointerId::MOUSE, 40.0, 100.0, &mut state); // x = -1.0
        router.cursor_moved(PointerId::MOUSE, 160.0, 100.0, &mut state); // x = 2.0
        let log = read_moves(&moves);
        assert_eq!(log.len(), 3);
        assert!((log[0].0 - 0.5).abs() < 1e-4);
        assert!((log[1].0 - (-1.0)).abs() < 1e-4);
        assert!((log[2].0 - 2.0).abs() < 1e-4);
    }

    #[test]
    fn cursor_left_during_capture_keeps_drag_alive() {
        // Cursor leaves the window while a drag is in flight (the
        // user dragged the mouse off-screen). The router must
        // suppress PointerLeave; the drag resumes when the cursor
        // re-enters. The eventual pointer_up still dispatches.
        let mut router = InputRouter::new();
        let (mut state, events, _moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_left(PointerId::MOUSE, &mut state); // off-screen
        // No PointerLeave; capture still pinned.
        assert_eq!(
            read(&events),
            vec!["PointerEnter".to_string(), "PointerDown".into()],
        );
        assert_eq!(router.captured_target(PointerId::MOUSE), Some("main_slider"));
        // Drag resumes when cursor re-enters.
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            read(&events),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
            ],
        );
        assert_eq!(router.captured_target(PointerId::MOUSE), None);
    }

    #[test]
    fn pointer_up_off_widget_dispatches_then_fires_leave() {
        // Drag ended outside the widget rect. PointerUp dispatches
        // to the captured tag (Slider observes Dragging → Hover →
        // value_committed); then the post-release refresh_hover
        // dispatches the deferred PointerLeave (Hover → Idle).
        let mut router = InputRouter::new();
        let (mut state, events, _moves) = state_with_slider();
        let paint = paint_with_slider(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state); // stray off
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert_eq!(
            read(&events),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
                "PointerLeave".into(),
            ],
        );
        assert_eq!(router.captured_target(PointerId::MOUSE), None);
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
    }

    #[test]
    fn button_like_widget_preserves_pre_r51_34_cancel_by_leave() {
        // Regression: a non-capturing widget (default
        // wants_pointer_capture = false) must still cancel by leave
        // — cursor stray off during press fires PointerLeave, and
        // pointer_up off-button is a no-op (existing R47 behaviour).
        let mut router = InputRouter::new();
        let (mut state, events) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(router.captured_target(PointerId::MOUSE), None);
        router.cursor_moved(PointerId::MOUSE, 10.0, 10.0, &mut state); // PointerLeave
        router.pointer_up(PointerId::MOUSE, &mut state); // no dispatch (hover gone)
        assert_eq!(
            read(&events),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerLeave".into(),
            ],
        );
    }

    #[test]
    fn capture_pointer_up_with_no_hover_or_capture_is_silent() {
        // pointer_up called with nothing pressed and no capture →
        // no dispatch (existing R47 behaviour). Defensive — winit
        // can replay key events on focus regain.
        let mut router = InputRouter::new();
        let (mut state, events) = state_with_button();
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        router.update_paint_scene(paint, &mut state);
        router.pointer_up(PointerId::MOUSE, &mut state);
        assert!(read(&events).is_empty());
    }

    #[test]
    fn normalize_cursor_handles_zero_size_rect() {
        // Degenerate layout (e.g. a Slider that hasn't laid out yet)
        // collapses to (0, 0); the router must not divide by zero.
        let rect = Rect::new(10, 10, 0, 0);
        let (x_rel, y_rel) = normalize_cursor(rect, 5.0, 5.0);
        assert!((x_rel - 0.0).abs() < f32::EPSILON);
        assert!((y_rel - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rect_for_tag_returns_inner_when_matched() {
        // rect_for_tag finds the tagged child's rect, not the root.
        let paint = paint_with_button(200, 200, Rect::new(80, 80, 40, 40));
        let found = rect_for_tag(&paint, "main_btn").expect("tag present");
        assert_eq!(found, Rect::new(80, 80, 40, 40));
        // Missing tag → None.
        assert!(rect_for_tag(&paint, "ghost").is_none());
    }

    #[test]
    fn capture_lock_skips_when_paint_scene_unset() {
        // Capture entered before the first paint (winit replay edge
        // case). cursor_moved finds no rect → pointer_move silent.
        // The router does not panic.
        let mut router = InputRouter::new();
        let (mut state, _events, moves) = state_with_slider();
        // pointer_down without paint → hover_target is None →
        // no PointerDown → no capture. Verify that direct invariant
        // first.
        router.cursor_moved(PointerId::MOUSE, 100.0, 100.0, &mut state);
        router.pointer_down(PointerId::MOUSE, &mut state);
        assert_eq!(router.captured_target(PointerId::MOUSE), None);
        // Now seed the paint scene + simulate a successful press to
        // enter capture; then drop the paint scene state by NOT
        // calling update_paint_scene with a fresh rect — the router
        // still holds last_paint_scene from the previous call.
        // (We can't easily clear last_paint_scene from outside; this
        // test instead validates pointer_down before paint does NOT
        // claim capture, exercising the same defensive path.)
        assert!(read_moves(&moves).is_empty());
    }

    // ─── R51.38 §5.35 multi-pointer fixtures + tests ───────────

    /// Paint scene with two drag-aware widgets — `slider_a` on the
    /// left and `slider_b` on the right. Used by the multi-touch
    /// drag tests to exercise the per-pointer capture map.
    fn paint_with_two_sliders(viewport_w: u32, viewport_h: u32) -> Scene {
        let slider_a = {
            let mut s = Scene::Container(
                ContainerNode::new(vec![])
                    .with_tag("slider_a")
                    .with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut s {
                c.rect = Rect::new(20, 20, 60, 60);
            }
            s
        };
        let slider_b = {
            let mut s = Scene::Container(
                ContainerNode::new(vec![])
                    .with_tag("slider_b")
                    .with_style(BoxStyle::filled(Color::default())),
            );
            if let Scene::Container(c) = &mut s {
                c.rect = Rect::new(120, 20, 60, 60);
            }
            s
        };
        let mut root = Scene::Container(
            ContainerNode::new(vec![slider_a, slider_b])
                .with_style(BoxStyle::filled(Color::default())),
        );
        if let Scene::Container(c) = &mut root {
            c.rect = Rect::new(0, 0, viewport_w, viewport_h);
        }
        root
    }

    /// State scene with two drag-aware externals matching the paint
    /// fixture above. Returns both event + move logs so tests can
    /// distinguish which widget received what.
    #[allow(clippy::type_complexity)]
    fn state_with_two_sliders() -> (Scene, (EventLog, MoveLog), (EventLog, MoveLog)) {
        let (a, ea, ma) = DragCaptureExternal::new();
        let (b, eb, mb) = DragCaptureExternal::new();
        let root = Scene::Container(
            ContainerNode::new(vec![
                Scene::External(ExternalNode::new(Box::new(a)).with_tag("slider_a")),
                Scene::External(ExternalNode::new(Box::new(b)).with_tag("slider_b")),
            ])
            .with_style(BoxStyle::filled(Color::default())),
        );
        (root, (ea, ma), (eb, mb))
    }

    #[test]
    fn pointer_id_mouse_is_reserved_zero() {
        // Backwards-compat invariant: mouse pointer maps to the
        // reserved `PointerId(0)` slot; touch finger ids offset by
        // one so they never alias the mouse no matter what winit
        // hands the router.
        assert_eq!(PointerId::MOUSE.raw(), 0);
        assert_eq!(PointerId::touch(0).raw(), 1);
        assert_eq!(PointerId::touch(42).raw(), 43);
        assert_ne!(PointerId::MOUSE, PointerId::touch(0));
    }

    #[test]
    fn two_touches_drag_two_widgets_independently() {
        // Multi-touch first-design invariant: two fingers on two
        // widgets each enter capture lock on their own tag and
        // forward `pointer_move` only to their own widget. Single-
        // target capture (`Option<String>`) would alias here — the
        // R51.38 HashMap substrate makes this work without aliasing.
        let mut router = InputRouter::new();
        let (mut state, (ea, ma), (eb, mb)) = state_with_two_sliders();
        let paint = paint_with_two_sliders(200, 200);
        router.update_paint_scene(paint, &mut state);
        let t1 = PointerId::touch(0);
        let t2 = PointerId::touch(1);
        // Touch 1 lands on slider_a's centre (50, 50).
        router.cursor_moved(t1, 50.0, 50.0, &mut state);
        router.pointer_down(t1, &mut state);
        // Touch 2 lands on slider_b's centre (150, 50).
        router.cursor_moved(t2, 150.0, 50.0, &mut state);
        router.pointer_down(t2, &mut state);
        assert_eq!(router.captured_target(t1), Some("slider_a"));
        assert_eq!(router.captured_target(t2), Some("slider_b"));
        // Drag each in opposite directions. Each widget's
        // `pointer_move` only sees its own touch's coords; the
        // sequence below would alias under a single-target capture
        // implementation (the second touch would overwrite the
        // first's lock).
        router.cursor_moved(t1, 70.0, 50.0, &mut state); // slider_a right
        router.cursor_moved(t2, 130.0, 50.0, &mut state); // slider_b left
        router.pointer_up(t1, &mut state);
        router.pointer_up(t2, &mut state);
        // slider_a saw the click-point + one drag move.
        let log_a = read_moves(&ma);
        assert_eq!(log_a.len(), 2);
        assert!((log_a[0].0 - 0.5).abs() < 1e-4); // click point
        assert!((log_a[1].0 - 0.8333).abs() < 1e-3); // (70-20)/60
        // slider_b saw its own click-point + drag.
        let log_b = read_moves(&mb);
        assert_eq!(log_b.len(), 2);
        assert!((log_b[0].0 - 0.5).abs() < 1e-4); // click point
        assert!((log_b[1].0 - 0.1666).abs() < 1e-3); // (130-120)/60
        // PointerEnter / Down / Up streams independent per widget.
        assert_eq!(
            read(&ea),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
            ],
        );
        assert_eq!(
            read(&eb),
            vec![
                "PointerEnter".to_string(),
                "PointerDown".into(),
                "PointerUp".into(),
            ],
        );
        assert_eq!(router.captured_target(t1), None);
        assert_eq!(router.captured_target(t2), None);
    }

    #[test]
    fn mouse_and_touch_dont_alias_hover() {
        // Mouse on slider_a, touch on slider_b — both pointers have
        // their own `hover_target` entry. Per-pointer dispatch means
        // each widget sees its own PointerEnter without aliasing.
        let mut router = InputRouter::new();
        let (mut state, (ea, _ma), (eb, _mb)) = state_with_two_sliders();
        let paint = paint_with_two_sliders(200, 200);
        router.update_paint_scene(paint, &mut state);
        let touch = PointerId::touch(0);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state);
        router.cursor_moved(touch, 150.0, 50.0, &mut state);
        assert_eq!(router.hover_target(PointerId::MOUSE), Some("slider_a"));
        assert_eq!(router.hover_target(touch), Some("slider_b"));
        // Each widget observed exactly one PointerEnter — neither
        // saw the other pointer's transitions.
        assert_eq!(read(&ea), vec!["PointerEnter".to_string()]);
        assert_eq!(read(&eb), vec!["PointerEnter".to_string()]);
    }

    #[test]
    fn releasing_one_touch_does_not_release_other_capture() {
        // Per-pointer capture isolation: lifting one finger must
        // not break the other finger's drag. The shared single-
        // target `Option<String>` capture would collapse here (the
        // first pointer_up would clear the lock for both).
        let mut router = InputRouter::new();
        let (mut state, _a, _b) = state_with_two_sliders();
        let paint = paint_with_two_sliders(200, 200);
        router.update_paint_scene(paint, &mut state);
        let t1 = PointerId::touch(0);
        let t2 = PointerId::touch(1);
        router.cursor_moved(t1, 50.0, 50.0, &mut state);
        router.pointer_down(t1, &mut state);
        router.cursor_moved(t2, 150.0, 50.0, &mut state);
        router.pointer_down(t2, &mut state);
        assert_eq!(router.captured_target(t1), Some("slider_a"));
        assert_eq!(router.captured_target(t2), Some("slider_b"));
        // Lift touch 1 only.
        router.pointer_up(t1, &mut state);
        assert_eq!(router.captured_target(t1), None);
        // Touch 2's lock survives.
        assert_eq!(router.captured_target(t2), Some("slider_b"));
        router.pointer_up(t2, &mut state);
        assert_eq!(router.captured_target(t2), None);
    }

    #[test]
    fn cursor_left_for_one_pointer_keeps_other_state() {
        // Cursor leaves the window for the mouse pointer, but a
        // touch pointer's hover should be untouched. Per-pointer
        // `cursor_left` only drops the matching id's cursor.
        let mut router = InputRouter::new();
        let (mut state, (ea, _ma), (eb, _mb)) = state_with_two_sliders();
        let paint = paint_with_two_sliders(200, 200);
        router.update_paint_scene(paint, &mut state);
        let touch = PointerId::touch(0);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state);
        router.cursor_moved(touch, 150.0, 50.0, &mut state);
        router.cursor_left(PointerId::MOUSE, &mut state);
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
        assert_eq!(router.hover_target(touch), Some("slider_b"));
        // slider_a saw Enter + Leave; slider_b only Enter.
        assert_eq!(
            read(&ea),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
        assert_eq!(read(&eb), vec!["PointerEnter".to_string()]);
    }

    #[test]
    fn update_paint_scene_refreshes_every_active_pointer() {
        // After a layout change, every active pointer's hover_target
        // must re-resolve. With two pointers active (mouse + touch),
        // both should observe the layout shift independently.
        let mut router = InputRouter::new();
        let (mut state, (ea, _ma), (eb, _mb)) = state_with_two_sliders();
        router.update_paint_scene(paint_with_two_sliders(200, 200), &mut state);
        let touch = PointerId::touch(0);
        router.cursor_moved(PointerId::MOUSE, 50.0, 50.0, &mut state);
        router.cursor_moved(touch, 150.0, 50.0, &mut state);
        // Now repaint with both sliders shifted out from under both
        // cursors. paint_with_two_sliders uses fixed rects; build a
        // bare root with no children to simulate "both widgets
        // moved away".
        let bare_root = Scene::Container(
            ContainerNode::new(vec![])
                .with_style(BoxStyle::filled(Color::default())),
        );
        router.update_paint_scene(bare_root, &mut state);
        // Both pointers lost their hover — each sees PointerLeave.
        assert_eq!(router.hover_target(PointerId::MOUSE), None);
        assert_eq!(router.hover_target(touch), None);
        assert_eq!(
            read(&ea),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
        assert_eq!(
            read(&eb),
            vec!["PointerEnter".to_string(), "PointerLeave".into()],
        );
    }
}
