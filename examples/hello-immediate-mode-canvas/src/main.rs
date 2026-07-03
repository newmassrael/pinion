//! `hello-immediate-mode-canvas` — R681 §2 #4 first real consumer
//! of [`Scene::ImmediateModeNode`]
//! and the [`ImmediateMode`] trait.
//!
//! ## Why this binding exists
//!
//! Substrate land sequence: atomic 0 added the
//! [`Scene::ImmediateModeNode`] variant plus the [`ImmediateMode`]
//! trait plus the [`ImmediatePainter`] surface. Atomic 1 wired the
//! per-window paint cycle (substrate tick walker plus Vello paint
//! adapter branch). Atomic 2 brought the `ControlFlow::WaitUntil`
//! game-loop pacing. Atomic 3 added the per-window
//! `WindowFramePolicy` override map. **This binding is the first
//! real application consumer** — a canonical "retained widget tree
//! hosts one immediate-mode subtree" composition that exercises
//! every wire on the chain at once.
//!
//! Northern-star context: this is the **§2 #4 dual-execution model's
//! first visible Phase A → Phase C bridge**. Phase A (retained
//! widgets, idle ~30fps, input-driven) is where every existing
//! hello-* binding lives. Phase C (immediate-mode game loop, 60fps+,
//! continuous paint) is the entry into the game-engine substrate
//! tier. This binding sits at the seam — a Button trigger
//! (retained, input-driven) under a rotating triangle canvas
//! (immediate-mode, continuous-paint). Both render correctly in
//! the same window, the same paint cycle, the same `Scene` tree;
//! the substrate's
//! [`Scene::has_immediate_mode_subtree`](pinion_core::scene::Scene::has_immediate_mode_subtree)
//! signal arms the per-window game-loop pacing automatically.
//!
//! ## Architecture
//!
//! - **Retained side**: a header [`TextNode`] + a `Dismiss`
//!   [`Button`](pinion_core::widgets::button) trigger underneath the
//!   immediate-mode canvas. The button has no functional effect on
//!   the canvas in this first-cut consumer (the substrate consumer
//!   round comes when an "interactive game with retained UI panel
//!   driving immediate-mode state" use case surfaces).
//! - **Immediate side**: a `RotatingTriangleDriver`
//!   ([`pinion_core::scene::ImmediateMode`] impl) whose state is one
//!   `f32 angle` field. `tick(dt)` advances `angle` by
//!   `dt.as_secs_f32() * ROTATION_RAD_PER_SEC` (mod `TAU`). `paint`
//!   computes three viewport-local triangle vertices around the
//!   viewport centre at radius `~min(w,h)*0.4`, rotated by the
//!   current `angle`, then fills with the M3 `Accent` colour role
//!   through the backend-agnostic [`ImmediatePainter::fill_triangle`].
//! - **`Owner::cache` wiring**: the driver lives in
//!   [`Owner::cache`](pinion_core::Owner::cache) keyed by
//!   `("canvas_driver")` so the same instance survives view-fn
//!   re-runs. The view fn pulls the same `Rc<RefCell<...>>` each
//!   frame and clones it into the `Scene::ImmediateModeNode`; the
//!   substrate tick walker borrows it mutably for one `tick(dt)`
//!   call per paint cycle, then the paint adapter calls
//!   [`ImmediateMode::paint`]
//!   to encode the triangle into vello.

use std::cell::RefCell;
use std::f32::consts::TAU;
use std::rc::Rc;
use std::time::Duration;

#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_core::scene::{
    ContainerNode, ImmediateMode, ImmediateModeNode, ImmediatePainter, Rect, TextNode,
};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Color, Frame, Owner, Scene, WidgetCore};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

// pinion-forge codegen output. Defines `pub struct
// HelloImmediateModeCanvasRenderer` + `pub enum
// HelloImmediateModeCanvasRendererError` + async `new<W: Into<...>>`
// + sync `render(&vello::Scene, peniko::Color)` + sync `resize(u32,
// u32)`. R46.3.3 emit template uses fully-qualified `::vello::*`
// paths so the include is bare.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 bridge from the inherent renderer methods to the
// `pinion_shell::VelloRenderer` trait.
vello_renderer_impl!(
    HelloImmediateModeCanvasRenderer,
    HelloImmediateModeCanvasRendererError
);

/// (R681 §2 #4) Window size. The canvas needs enough vertical room
/// to host a header text + the immediate-mode viewport + a Button
/// trigger underneath. Fixed for this first-cut consumer; a future
/// round could compose the immediate-mode binding with
/// `SizeStrategy::IntrinsicAfterFirstPaint` (hello-popover R670
/// substrate) to honour the natural canvas bbox at boot.
const WIN_W: u32 = 320;
const WIN_H: u32 = 420;

/// (R681 §2 #4) Immediate-mode canvas viewport (logical pixels).
/// The viewport rect is positioned at `(0, HEADER_BAND_H)` so the
/// rotating triangle paints below the header text without
/// overlapping. Wired to the [`Scene::ImmediateModeNode`] each
/// view-fn call so the post-layout paint cycle can dispatch tick +
/// paint against the correct screen-space rect.
const CANVAS_VIEWPORT_W: u32 = 280;
const CANVAS_VIEWPORT_H: u32 = 280;

const HEADER_FONT_PX: u32 = 16;
const BODY_FONT_PX: u32 = 13;
const BTN_W: u32 = 160;
const BTN_H: u32 = 36;
const ROW_GAP: u32 = 12;
const OUTER_PAD: u32 = 20;

/// (R681 §2 #4) Rotation rate in radians per second. ~0.75 Hz at
/// `1.5 * π` rad/s rounds to one full revolution every ~2.7s — fast
/// enough that the user perceives continuous motion on a 60fps
/// pipeline, slow enough that demo assertions on the per-frame
/// angle delta can observe a finite, non-trivial value at any
/// scheduled snapshot.
const ROTATION_RAD_PER_SEC: f32 = std::f32::consts::PI * 1.5;

/// Shared `ThemeProvider` cache tag matching the `"app"` convention
/// hello-button / hello-toggle / hello-theme use.
const THEME_TAG: &str = "app";

/// (R681 §2 #4) `Owner::cache` slot key for the canvas
/// `ImmediateMode` driver. The view fn pulls the same
/// `Rc<RefCell<RotatingTriangleDriver>>` from this slot on every
/// render so the per-frame tick advances the same state instance.
const CANVAS_DRIVER_CACHE_KEY: &str = "hello_immediate_mode_canvas.driver";

/// (R681 §2 #4) Tags addressing each element of the paint tree —
/// the composite-paint-root convention (R55.G.17) requires the
/// painted scene to contain a node tagged [`CanvasView::tag()`]
/// somewhere; we attach `CANVAS_TAG` to the root Container.
/// `CANVAS_NODE_TAG` is the §5.20 intent tag on the immediate-mode
/// node itself so RPC `scene/query {path: ".../canvas_node"}`
/// addresses it directly, and `CANVAS_BTN_TAG` is the button
/// trigger's tag the `InputRouter` hit-tests pointer events
/// against.
const CANVAS_TAG: &str = "canvas_view";
const CANVAS_NODE_TAG: &str = "canvas_node";
const CANVAS_BTN_TAG: &str = "canvas_btn";

const HEADER_TEXT: &str = "Rotating Triangle (R681 immediate-mode)";
const HINT_TEXT: &str = "60 fps via ControlFlow::WaitUntil";
const BTN_LABEL: &str = "Dismiss";

/// R681 §2 #4 — the canvas's `ImmediateMode` driver. One `f32`
/// angle field, advanced on `tick(dt)`, painted as three rotating
/// vertices on `paint(&mut dyn ImmediatePainter)`. The state lives
/// inside the driver so multiple view-fn re-runs observe the same
/// rotation phase.
#[derive(Debug, Default)]
struct RotatingTriangleDriver {
    /// Current rotation angle in radians. Advanced by every
    /// [`ImmediateMode::tick`] call; mod `TAU` on each step so the
    /// value stays in `[0, 2π)` for any sequence of paint cycles
    /// (numerical stability against accumulated drift across hours
    /// of continuous paint).
    angle: f32,
    /// Fill colour for the triangle, resolved from the active
    /// theme by the view fn each frame and written here. Default
    /// (black) is the bootstrap state before the first view fn
    /// call publishes a real palette entry.
    fill: Color,
}

impl ImmediateMode for RotatingTriangleDriver {
    fn tick(&mut self, dt: Duration) {
        let dt_secs = dt.as_secs_f32();
        // Wrap modulo TAU so angle stays in [0, 2π) across hours of
        // continuous paint — numerical stability guard against f32
        // drift on long-running sessions.
        self.angle = (self.angle + dt_secs * ROTATION_RAD_PER_SEC).rem_euclid(TAU);
    }

    fn paint(&mut self, painter: &mut dyn ImmediatePainter) {
        let (w_u, h_u) = painter.viewport_size();
        if w_u == 0 || h_u == 0 {
            return;
        }
        // Cast viewport size to f32 for trigonometry. Within the
        // GUI surface range (well under 2^24 px) the cast is exact;
        // the `#[allow]` documents the intentional precision waiver.
        #[allow(clippy::cast_precision_loss)]
        let w = w_u as f32;
        #[allow(clippy::cast_precision_loss)]
        let h = h_u as f32;
        let cx = w * 0.5;
        let cy = h * 0.5;
        let radius = w.min(h) * 0.4;
        // Three triangle vertices at 120° intervals around the centre,
        // rotated by the current angle. Vertex 0 starts at angle = 0
        // (positive x-axis); vertices 1 and 2 at +2π/3 and +4π/3.
        let v0 = (
            cx + radius * self.angle.cos(),
            cy + radius * self.angle.sin(),
        );
        let v1 = (
            cx + radius * (self.angle + TAU / 3.0).cos(),
            cy + radius * (self.angle + TAU / 3.0).sin(),
        );
        let v2 = (
            cx + radius * (self.angle + 2.0 * TAU / 3.0).cos(),
            cy + radius * (self.angle + 2.0 * TAU / 3.0).sin(),
        );
        painter.fill_triangle(v0, v1, v2, self.fill);
    }
}

/// (R681 §2 #4) [`Owner::cache`] hook for the canvas driver. Pulls
/// the same `Rc<RefCell<RotatingTriangleDriver>>` from the cache
/// slot on every view-fn call so the per-frame tick advances the
/// same state instance across the binding lifetime.
fn use_canvas_driver() -> Rc<RefCell<RotatingTriangleDriver>> {
    let owner =
        Owner::current().expect("use_canvas_driver must run inside Owner::run wrap (view fn)");
    owner.cache(CANVAS_DRIVER_CACHE_KEY, || {
        RefCell::new(RotatingTriangleDriver::default())
    })
}

/// view-fn (§6.3) — pure sync mapping `(ButtonState) -> Scene`.
/// Builds the composite paint scene:
///
/// - root [`Scene::Container`] (column layout, viewport-sized).
/// - header [`Scene::Text`] (`HEADER_TEXT` + theme `OnSurface`).
/// - hint  [`Scene::Text`] (`HINT_TEXT` + theme `OnSurfaceMuted`).
/// - canvas [`Scene::ImmediateModeNode`] hosting the rotating
///   triangle driver. The viewport rect is sized to
///   `(CANVAS_VIEWPORT_W, CANVAS_VIEWPORT_H)` via the layout sidecar
///   so the §5.21 taffy pass resolves the post-layout `viewport`
///   the backend bridge paints into. The driver handle is the
///   stable cache entry returned by [`use_canvas_driver`].
/// - dismiss [`Scene::Container`] wrapping the Button label
///   (M3 `SurfaceContainerHighest` fill with state-layer overlays).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ButtonState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let surface = theme.resolve(ColorRole::Surface);
    let primary = theme.resolve(ColorRole::Accent);

    // Publish the M3 Primary colour into the driver so the next
    // paint call uses the live palette. Mutates the driver behind
    // the `RefCell`; the substrate's tick walker takes the
    // `borrow_mut` immediately after this view fn returns.
    let driver = use_canvas_driver();
    driver.borrow_mut().fill = primary;

    let header_text = Scene::Text(TextNode::styled(
        HEADER_TEXT,
        Rect::default(),
        TextStyle::new()
            .with_size_px(HEADER_FONT_PX)
            .with_fg(on_surface),
    ));
    let hint_text = Scene::Text(TextNode::styled(
        HINT_TEXT,
        Rect::default(),
        TextStyle::new()
            .with_size_px(BODY_FONT_PX)
            .with_fg(on_surface_muted),
    ));

    // ImmediateModeNode — the §2 #4 immediate-mode subtree.
    let handle: Rc<RefCell<dyn ImmediateMode>> = driver;
    let canvas = Scene::ImmediateModeNode(
        ImmediateModeNode::new(handle, Rect::default())
            .with_tag(CANVAS_NODE_TAG)
            .with_layout(
                LayoutStyle::new().with_size(Size::px(CANVAS_VIEWPORT_W, CANVAS_VIEWPORT_H)),
            ),
    );

    // Dismiss Button — `ButtonState` mapped to M3 surface tier +
    // state-layer overlay (same convention hello-button uses).
    let btn_fill: Color = match state {
        ButtonState::Idle | ButtonState::Hover => theme.resolve(ColorRole::SurfaceContainerHighest),
        ButtonState::Pressed => theme
            .resolve(ColorRole::SurfaceContainerHighest)
            .lerp(on_surface, 0.12),
        ButtonState::Disabled => theme
            .resolve(ColorRole::SurfaceContainerHighest)
            .lerp(surface, 0.38),
    };
    let btn_label = Scene::Text(TextNode::styled(
        BTN_LABEL,
        Rect::default(),
        TextStyle::new()
            .with_size_px(BODY_FONT_PX)
            .with_fg(on_surface),
    ));
    let button = Scene::Container(
        ContainerNode::new(vec![btn_label])
            .with_tag(CANVAS_BTN_TAG)
            .with_style(BoxStyle::filled(btn_fill))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    // (R1031.1 §5.39) hand-composed button (immediate-mode view)
                    // is a Tab stop — the composing view owns the focus opt-in.
                    .with_focusable(true)
                    .with_size(Size::px(BTN_W, BTN_H)),
            ),
    );

    Scene::Container(
        ContainerNode::new(vec![header_text, hint_text, canvas, button])
            .with_tag(CANVAS_TAG)
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Center)
                    .with_gap(ROW_GAP)
                    .with_padding(Rect::new(OUTER_PAD, OUTER_PAD, OUTER_PAD, OUTER_PAD))
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

/// `WidgetView` binding. The `#[widget]` attribute lifts every
/// mechanical trait wiring item (`WidgetCore` + `WidgetA11y` +
/// `WidgetView` forwarding trio); only the view body + keybinding
/// shim + ARIA Button apply_key remain as inherent fns.
///
/// `tag = "canvas_btn"` matches the `with_tag(CANVAS_BTN_TAG)` on the
/// button container so the shell's `InputRouter` routes pointer
/// events through the `Scene::External(ButtonExternal)` root.
#[widget(
    tag = "canvas_btn",
    state = ButtonState,
    event = ButtonEvent,
    title = "pinion hello-immediate-mode-canvas (R681 §2 #4)",
    renderer = HelloImmediateModeCanvasRenderer,
    initial_size = (WIN_W, WIN_H),
    external = ButtonExternal::new,
    role = Button,
    state_flags(
        hovered = Hover,
        pressed = Pressed,
        disabled = Disabled,
    ),
    apply_key,
    keybinding,
    state_name_derive,
)]
struct CanvasView;

impl CanvasView {
    /// R641 inherent forward for [`WidgetCore::view`]. Re-borrows
    /// the by-value `Frame` the macro hands in so the free
    /// `view(...)` fn keeps the canonical `&Frame` signature.
    fn view(state: ButtonState, frame: Frame) -> Scene {
        view(state, &frame)
    }

    /// Keybinding shim mirroring hello-button.
    fn keybinding(key: &str) -> Option<ButtonEvent> {
        match key {
            "d" => Some(ButtonEvent::Disable),
            "e" => Some(ButtonEvent::Enable),
            _ => None,
        }
    }

    /// ARIA Button keyboard activation — Space / Enter on the
    /// focused button fires the SCXML `KeyboardActivate` transition.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }
}

fn main() {
    pinion_shell::run::<CanvasView>();
}

#[cfg(test)]
mod r681_immediate_mode_canvas_tests {
    use super::*;
    use pinion_core::scene::Scene as PinionScene;

    #[test]
    fn r681_widget_tag_matches_paint_tag() {
        assert_eq!(<CanvasView as WidgetCore>::tag(), CANVAS_BTN_TAG);
    }

    #[test]
    fn r681_access_node_is_aria_button() {
        let nodes = <CanvasView as WidgetA11y>::access_node(&ButtonState::Idle, None);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, AriaRole::Button);
        assert_eq!(nodes[0].tag, "canvas_btn");
    }

    #[test]
    fn r681_view_root_carries_composite_paint_root_tag() {
        // R55.G.17 composite paint-root tag convention — the view
        // scene must contain at least one node tagged
        // `<CanvasView as WidgetCore>::tag()` ("canvas_btn") so the
        // input router resolves pointer events. The button
        // container carries the tag; the test confirms the
        // contains_tag walker finds it.
        let owner = Owner::new();
        let scene = owner.run(|| view(ButtonState::Idle, &Frame::new()));
        assert!(scene.contains_tag(CANVAS_BTN_TAG));
    }

    #[test]
    fn r681_view_contains_immediate_mode_subtree() {
        let owner = Owner::new();
        let scene = owner.run(|| view(ButtonState::Idle, &Frame::new()));
        assert!(
            scene.has_immediate_mode_subtree(),
            "view fn must emit at least one Scene::ImmediateModeNode for R681 first-consumer status",
        );
    }

    #[test]
    fn r681_view_canvas_node_tag_resolves_through_walker() {
        // The immediate-mode node carries its own §5.20 tag so RPC
        // `scene/query` can address it without walking the children
        // by index. `contains_tag` traverses through Container into
        // ImmediateModeNode and matches its tag.
        let owner = Owner::new();
        let scene = owner.run(|| view(ButtonState::Idle, &Frame::new()));
        assert!(scene.contains_tag(CANVAS_NODE_TAG));
    }

    #[test]
    fn r681_use_canvas_driver_returns_same_handle_across_view_runs() {
        // The driver lives in Owner::cache so the same
        // `Rc<RefCell<RotatingTriangleDriver>>` survives view-fn
        // re-runs. Pin the cache hit by running the view fn twice
        // and asserting the resulting ImmediateModeNode handles
        // are pointer-equal.
        let owner = Owner::new();
        let scene_a = owner.run(|| view(ButtonState::Idle, &Frame::new()));
        let scene_b = owner.run(|| view(ButtonState::Idle, &Frame::new()));
        let handle_a = first_immediate_handle(&scene_a).expect("scene_a has ImmediateModeNode");
        let handle_b = first_immediate_handle(&scene_b).expect("scene_b has ImmediateModeNode");
        assert!(
            Rc::ptr_eq(&handle_a, &handle_b),
            "Owner::cache must return the same driver Rc across view-fn re-runs",
        );
    }

    #[test]
    fn r681_driver_tick_advances_angle_modulo_tau() {
        let mut driver = RotatingTriangleDriver::default();
        // Initial angle is the f32 default (+0.0). Bit-pattern
        // compare so the f32 equality is exact without violating
        // clippy::float_cmp on the pedantic baseline.
        assert_eq!(driver.angle.to_bits(), 0.0_f32.to_bits());
        driver.tick(Duration::from_secs_f32(1.0 / ROTATION_RAD_PER_SEC));
        // 1/ROTATION_RAD_PER_SEC seconds of rotation = exactly 1 radian.
        let one_rad_diff = (driver.angle - 1.0_f32).abs();
        assert!(one_rad_diff < 1e-4, "angle = {} != ~1.0", driver.angle);
        // Tick by an enormous dt — angle must stay in [0, TAU).
        driver.tick(Duration::from_secs(1_000));
        assert!(driver.angle >= 0.0 && driver.angle < TAU);
    }

    #[test]
    fn r681_painted_scene_layout_runs_through_immediate_node_viewport() {
        // R681 atomic 1 — drive the paint pipeline through
        // compute_layout so the post-layout viewport on the
        // ImmediateModeNode reflects what the shell measures at
        // boot. The taffy pass resolves the viewport from the
        // layout sidecar `with_size(CANVAS_VIEWPORT_W,
        // CANVAS_VIEWPORT_H)` against the column flex parent —
        // the result must equal the requested size (or larger
        // dimension on free axes).
        let owner = Owner::new();
        let mut scene = owner.run(|| view(ButtonState::Idle, &Frame::new()));
        let mut text_cache = pinion_text::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut text_cache, WIN_W, WIN_H);
        let node =
            walk_first_immediate_node(&scene).expect("painted scene contains ImmediateModeNode");
        // Taffy clamps the canvas size against the column-flex
        // parent's content area (window minus padding minus the
        // header / hint / button row heights + gap rhythm). The
        // exact pixel dimensions depend on the parley-shaped text
        // row heights against the current font / DPI, so the
        // assertion is a sanity floor (the canvas occupies a
        // meaningful area, not zero) and a ceiling (it does not
        // exceed the requested size sidecar).
        assert!(
            node.viewport.w > 0 && node.viewport.h > 0,
            "post-layout viewport must be non-zero so the paint adapter has area to encode; got {:?}",
            node.viewport,
        );
        assert!(
            node.viewport.w <= CANVAS_VIEWPORT_W,
            "post-layout width {} must not exceed declared sidecar {}",
            node.viewport.w,
            CANVAS_VIEWPORT_W,
        );
        assert!(
            node.viewport.h <= CANVAS_VIEWPORT_H,
            "post-layout height {} must not exceed declared sidecar {}",
            node.viewport.h,
            CANVAS_VIEWPORT_H,
        );
    }

    fn first_immediate_handle(scene: &PinionScene) -> Option<Rc<RefCell<dyn ImmediateMode>>> {
        walk_first_immediate_node(scene).map(|n| n.handle.clone())
    }

    fn walk_first_immediate_node(scene: &PinionScene) -> Option<&ImmediateModeNode> {
        match scene {
            PinionScene::ImmediateModeNode(n) => Some(n),
            PinionScene::Container(c) => c.children.iter().find_map(walk_first_immediate_node),
            PinionScene::Scroll(s) => walk_first_immediate_node(&s.content),
            _ => None,
        }
    }
}
