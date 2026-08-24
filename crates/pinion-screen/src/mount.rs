//! ★★★★★ R1724 — **an existing binding, placed.**
//!
//! [`Mount`] is the whole reason a screen is not a new kind of thing an author
//! writes. `hello-node-lab` is 20,655 lines of screen that already answers
//! every hook a page needs; what it lacked was a way to be *somewhere*. It is
//! mounted without one line of it changing.

use std::cell::Cell;
use std::marker::PhantomData;
use std::rc::Rc;

use pinion_a11y::{AccessAction, AccessFocus, AccessNode};
use pinion_core::command::Command;
use pinion_core::event::WheelDelta;
use pinion_core::input::{CompositionEvent, KeyPress, Modifiers};
use pinion_core::intent::Intent;
use pinion_core::reactive::Signal;
use pinion_core::scene::{ContainerNode, Rect};
use pinion_core::shrink::ShrinkPolicy;
use pinion_core::widget_core::ExtraExternal;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, WindowPolicy, WindowSpec};

use crate::Screen;

/// Mount an existing binding as one destination's page.
///
/// # The latch
///
/// A binding's cached projection travels through
/// [`WidgetCore::State`], which is `Copy` and
/// typed per binding — so it cannot travel across a roster of differently-typed
/// screens in a host's own `State`. The mount holds it instead: [`Screen::latch`]
/// reads it out of the state scene and parks it here, and every hook that would
/// have received `&state` reads it back.
///
/// A mount that has not been latched yet reads its binding's projection out of
/// an **empty scene**, which is the truthful answer rather than a placeholder:
/// before a screen is arrived at, none of its externals is in the state scene,
/// and that is exactly what an empty scene says. It is also what makes every
/// hook total — a page can be asked what it would show before anybody has gone
/// there, which is what a host doing layout for a rail seat does.
pub struct Mount<V: WidgetView> {
    latched: Cell<Option<<V as WidgetCore>::State>>,
    revision: Cell<u64>,
    binding: PhantomData<fn() -> V>,
}

impl<V: WidgetView> Default for Mount<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: WidgetView> Mount<V> {
    /// A mount of `V`, not yet latched.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            latched: Cell::new(None),
            revision: Cell::new(0),
            binding: PhantomData,
        }
    }

    /// The parked projection, or the binding's reading of an empty scene when
    /// nothing has been latched yet.
    fn state(&self) -> <V as WidgetCore>::State {
        self.latched
            .get()
            .unwrap_or_else(|| V::read_state(&Scene::Container(ContainerNode::new(Vec::new()))))
    }
}

impl<V: WidgetView> Screen for Mount<V> {
    fn tag(&self) -> &'static str {
        V::tag()
    }

    fn title(&self) -> &'static str {
        V::title()
    }

    /// ★★★★★ R1738 — the binding's own verdict, unchanged by being mounted.
    ///
    /// That it is the *same* value the standalone binary publishes is the point:
    /// a section that reproduces its specification when run in its own window
    /// and is never asked when it is a page would be two builds wearing one
    /// name.
    fn conformance(&self) -> Option<pinion_core::conformance::DocumentReport> {
        V::conformance()
    }

    // R1808 — the binding's own answer, so a host walking the application never
    // has to know that this particular screen's surfaces exclude each other.
    fn poses(&self) -> usize {
        V::poses()
    }

    fn pose(&self, nth: usize) {
        V::pose(nth);
    }

    fn latch(&self, state_scene: &Scene) -> u64 {
        let next = V::read_state(state_scene);
        if self.latched.get() != Some(next) {
            self.latched.set(Some(next));
            // Wrapping because a revision is a change detector and not a count:
            // at one bump per frame at 144 Hz this wraps after 4 billion years,
            // and a debug-build overflow panic is not a failure mode worth
            // shipping for a number nobody reads as a quantity.
            self.revision.set(self.revision.get().wrapping_add(1));
        }
        self.revision.get()
    }

    fn fmt_state_log(&self) -> String {
        V::fmt_state_log(&self.state())
    }

    fn externals(&self) -> Vec<ExtraExternal> {
        // The primary first, when the binding has one: `CoreShell` composes a
        // binding's root as `[primary, ...extras]` and a screen's surfaces
        // arrive at the host in that same order, so a roster does not reorder
        // what a binding declared.
        let mut surfaces = Vec::new();
        if let Some(primary) = V::primary_surface() {
            surfaces.push(ExtraExternal::new(primary.tag, (primary.factory)()));
        }
        surfaces.extend(V::create_extra_externals());
        surfaces
    }

    fn reconcile_frame(&self) {
        V::reconcile_frame();
    }

    fn view(&self, frame: &Frame) -> Scene {
        V::view(self.state(), frame)
    }

    fn shrink_policy(&self) -> Option<ShrinkPolicy> {
        V::shrink_policy()
    }

    fn view_for_window(&self, window_id: &str, frame: &Frame) -> Scene {
        V::view_for_window(window_id, self.state(), frame)
    }

    fn windows(&self) -> Vec<WindowSpec> {
        V::windows()
    }

    fn windows_signal(&self) -> Option<Rc<Signal<Vec<WindowSpec>>>> {
        V::windows_signal()
    }

    fn window_policy(&self, window_id: &str) -> WindowPolicy {
        V::window_policy(window_id)
    }

    fn window_close_requested(&self, window_id: &str) -> bool {
        V::window_close_requested(window_id)
    }

    fn keybinding(&self, key: &str) -> Option<&'static str> {
        V::keybinding(key).map(V::event_name)
    }

    fn apply_key(
        &self,
        state_scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: Modifiers,
    ) -> bool {
        V::apply_key(state_scene, focused, key, modifiers)
    }

    fn apply_key_repeat(
        &self,
        state_scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: Modifiers,
        repeat: bool,
    ) -> bool {
        V::apply_key_repeat(state_scene, focused, key, modifiers, repeat)
    }

    fn apply_key_press(
        &self,
        state_scene: &mut Scene,
        focused: Option<&str>,
        press: &KeyPress<'_>,
    ) -> bool {
        V::apply_key_press(state_scene, focused, press)
    }

    fn forward_key_to_external(
        &self,
        state_scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
    ) -> bool {
        V::forward_key_to_external(state_scene, focused, key)
    }

    fn apply_composition(
        &self,
        state_scene: &mut Scene,
        focused: Option<&str>,
        event: &CompositionEvent,
    ) -> bool {
        V::apply_composition(state_scene, focused, event)
    }

    fn apply_middle_click(
        &self,
        state_scene: &mut Scene,
        focused: Option<&str>,
        modifiers: Modifiers,
    ) -> bool {
        V::apply_middle_click(state_scene, focused, modifiers)
    }

    fn apply_secondary_click(&self, state_scene: &mut Scene, x: f32, y: f32) -> bool {
        V::apply_secondary_click(state_scene, x, y)
    }

    fn apply_wheel(
        &self,
        paint_scene: &Scene,
        cursor: (f64, f64),
        delta: WheelDelta,
        modifiers: Modifiers,
    ) -> bool {
        V::apply_wheel(paint_scene, cursor, delta, modifiers)
    }

    fn position_caret_for_point(
        &self,
        state_scene: &Scene,
        focused: Option<&str>,
        hit_tag: Option<&str>,
        x: f32,
        y: f32,
        extend: bool,
    ) -> Option<usize> {
        V::position_caret_for_point(&self.state(), state_scene, focused, hit_tag, x, y, extend)
    }

    fn select_drag_to_point(
        &self,
        state_scene: &Scene,
        focused: Option<&str>,
        anchor: usize,
        x: f32,
        y: f32,
    ) -> bool {
        V::select_drag_to_point(&self.state(), state_scene, focused, anchor, x, y)
    }

    fn dock_drop_preview(
        &self,
        source_panel: &str,
        target_tag: &str,
        panel_rect: Rect,
        x_rel: f32,
        y_rel: f32,
    ) -> Option<Scene> {
        V::dock_drop_preview(source_panel, target_tag, panel_rect, x_rel, y_rel)
    }

    fn drag_image_style(&self, label: &str) -> Option<pinion_overlay::DragImageStyle> {
        V::drag_image_style(label)
    }

    fn on_file_hover(&self, window_id: &str, path: &str) -> bool {
        V::on_file_hover(window_id, &self.state(), path)
    }

    fn on_file_hover_cancel(&self, window_id: &str) -> bool {
        V::on_file_hover_cancel(window_id, &self.state())
    }

    fn on_file_drop(&self, window_id: &str, path: &str) -> bool {
        V::on_file_drop(window_id, &self.state(), path)
    }

    fn access_node(&self, focused: Option<&str>) -> Vec<AccessNode> {
        V::access_node(&self.state(), focused)
    }

    fn access_node_for_window(&self, window_id: &str, focused: Option<&str>) -> Vec<AccessNode> {
        V::access_node_for_window(window_id, &self.state(), focused)
    }

    fn access_focus_target(&self, focused: Option<&str>) -> Option<AccessFocus> {
        V::access_focus_target(&self.state(), focused)
    }

    fn access_child_invoke(
        &self,
        state_scene: &mut Scene,
        parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        V::access_child_invoke(state_scene, parent_tag, sub_tag, action)
    }

    fn focus_ring_style(&self, focused_tag: &str) -> Option<pinion_overlay::FocusRingStyle> {
        V::focus_ring_style(focused_tag)
    }

    fn ime_caret_rect(
        &self,
        state_scene: &Scene,
        focused: Option<&str>,
    ) -> Option<pinion_text::CaretRect> {
        V::ime_caret_rect(&self.state(), state_scene, focused)
    }

    fn update(&self, intent: &Intent) -> Vec<Command> {
        V::update(self.state(), intent)
    }
}
