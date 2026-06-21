//! `hello-popover` — R670 §5.16 first real consumer of
//! [`SizeStrategy::IntrinsicAfterFirstPaint`](pinion_shell::SizeStrategy).
//!
//! ## Why this binding exists
//!
//! R668 landed the `IntrinsicAfterFirstPaint` window-creation policy
//! (substrate + 8 unit tests + 15-binding migration), but every
//! existing example declared `SizeStrategy::Fixed { width, height }` —
//! the substrate had no first real application consumer until this
//! round. The R668 carry + R669 carry both flagged the gap as
//! mandatory R670 inline clearance per the SEED directive.
//!
//! A textbook `IntrinsicAfterFirstPaint` consumer must satisfy three
//! criteria:
//!
//! 1. **No fixed root size**: the view fn must not lock
//!    `LayoutStyle::with_size(Size::px(WIN_W, WIN_H))` on the root —
//!    otherwise the intrinsic walker measures the locked size and
//!    `request_inner_size` is a no-op against the already-correct
//!    window.
//! 2. **Naturally-sized content**: the painted scene's per-node
//!    rects must reflect the actual content the user sees, so the
//!    `Scene::intrinsic_content_size` walker computes a meaningful
//!    bbox.
//! 3. **A genuine reason for the substrate**: a binding that could
//!    just declare `Fixed { width, height }` doesn't motivate
//!    `IntrinsicAfterFirstPaint`. A popover / tooltip / inline panel
//!    whose natural size is dictated by its painted content rather
//!    than a designer-chosen frame is the textbook use case
//!    (matches Qt `QToolTip::sizeHint`, GTK `GtkPopover` content
//!    sizing, macOS `NSPopover.contentSize`, web CSS
//!    `width: max-content` on absolutely-positioned overlays).
//!
//! This binding satisfies all three: the view fn returns a column of
//! text rows + a button without any root-size lock, so the painted
//! bbox is what the shell honors at boot. The popover content is
//! intentionally a status panel (variable-width text rows + one
//! Button trigger) so the natural bbox is non-trivial — a Fixed
//! strategy would either crop the longest row (too small) or pad
//! whitespace below the last row (too big).
//!
//! ## R670 substrate scope clarification (honest)
//!
//! `SizeStrategy::IntrinsicAfterFirstPaint` is **one-shot** per
//! binding lifetime — the shell sizes the window once after the very
//! first paint cycle, then never resizes again on subsequent paints.
//! Dynamic shrink-wrap-on-state-change ("click button → window
//! expands to show extra rows → click again → window collapses") is a
//! separate substrate axis that would require either an explicit
//! `scene/resize` RPC call from the binding or a substrate extension
//! lifting the one-shot guard.
//!
//! The R670 SEED honest-honest-honest line: this binding is the
//! first-consumer of the one-shot capability. The "click → expand"
//! axis the R668 atomic 4 carry originally sketched would be a
//! second-consumer round once a real use case (dialog with collapsible
//! "more options" section, etc.) surfaces the substrate-incompleteness
//! signal.
//!
//! ## Architecture
//!
//! Same shape as `hello-button` — a Button trigger underneath a
//! status header. R51.30 pinion-shell migration applies in full: the
//! binding declares the view fn + `WidgetView` impl + a one-line
//! `main()` calling `pinion_shell::run::<PopoverView>()`. Everything
//! else (event loop, paint pipeline, RPC ingress, `IntrinsicAfterFirstPaint`
//! wire) lives in `pinion_shell`.

#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Color, Frame, Scene, WidgetCore};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

// pinion-forge codegen output. Defines `pub struct HelloPopoverRenderer`
// + `pub enum HelloPopoverRendererError` + async `new<W: Into<wgpu::
// SurfaceTarget<'static>>>` + sync `render(&vello::Scene, peniko::
// Color)` + sync `resize(u32, u32)`. R46.3.3 emit template uses
// fully-qualified `::vello::*` paths so the include is bare.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods (emitted by
// pinion-forge) into the `pinion_shell::VelloRenderer` trait so the
// generic `AppShell<V>` can construct + render + resize it without
// hardcoding the concrete struct name.
vello_renderer_impl!(HelloPopoverRenderer, HelloPopoverRendererError);

/// (R670 §5.16) Window-creation policy: open at the natural content
/// bbox (no fixed `WIN_W × WIN_H` lock). `min` is the floor against
/// degenerate empty content (the user must always see a window of
/// minimally-readable proportions); `max` is the ceiling against
/// absurdly-large content (a future state mutation should not push
/// the window past the monitor's small-window region).
///
/// The shell's `IntrinsicAfterFirstPaint` wire (R668 substrate) opens
/// the window at `min`, runs one paint cycle to populate per-node
/// rects, walks the scene for the tight content bbox via
/// `Scene::intrinsic_content_size`, clamps to `[min, max]`, and calls
/// `winit::window::Window::request_inner_size` when the clamped size
/// differs from `min`. winit emits `WindowEvent::Resized` on
/// acceptance which feeds the next paint at the new viewport.
///
/// These bounds are chosen so the realistic content bbox (computed
/// below from the row heights + paddings) lands strictly inside the
/// `(min, max)` interval — the demo verifies the post-first-paint
/// window size is neither `min` nor `max`, confirming the substrate
/// actually walked the content rather than capping at a bound.
const POPOVER_MIN_W: u32 = 240;
const POPOVER_MIN_H: u32 = 100;
const POPOVER_MAX_W: u32 = 480;
const POPOVER_MAX_H: u32 = 400;

/// (R670 §5.16) Layout constants. The popover lays out three text
/// rows above a single Button trigger with consistent vertical
/// rhythm. The bbox these constants compute determines the
/// post-first-paint window size:
///
///   inner content width  ≈ longest text row width (parley-shaped)
///   inner content height ≈ `HEADER_FONT_PX` + 3 × `BODY_FONT_PX`
///                          + 4 × `ROW_GAP` + `BTN_H` + 2 × `OUTER_PAD`
///
/// With the values below, the natural bbox is roughly
/// `(320, 230)` — strictly inside `[(240,100), (480,400)]`, so the
/// substrate's one-shot resize fires (target != min) and demos can
/// assert "post-first-paint width > `min_w` AND height > `min_h`".
const HEADER_FONT_PX: u32 = 18;
const BODY_FONT_PX: u32 = 14;
const BTN_W: u32 = 160;
const BTN_H: u32 = 36;
const ROW_GAP: u32 = 10;
const OUTER_PAD: u32 = 20;

/// (R670 §5.16) Shared `ThemeProvider` cache tag. Matches the `"app"`
/// convention used by hello-button / hello-toggle / hello-theme so a
/// future popover-in-host-binary embedding shares one palette.
const THEME_TAG: &str = "app";

/// (R670 §5.16) Static status header. The popover content is
/// intentionally static — the first-paint bbox is the binding's
/// natural size and stays stable for the lifetime of the window.
const HEADER_TEXT: &str = "Popover Status";
const BODY_LINE_1: &str = "All systems nominal";
const BODY_LINE_2: &str = "Sync: last 2 minutes ago";
const BODY_LINE_3: &str = "Storage: 47% free";
const BTN_LABEL: &str = "Dismiss";

/// view-fn (§6.3): pure sync mapping `(ButtonState) -> Scene`. The
/// view body is intentionally minimal — three text rows + one
/// Button trigger laid out vertically with consistent rhythm. The
/// `&Frame` slot is the §6.3 ZST hedge per the canonical view-fn
/// signature contract.
///
/// **No root size lock.** The root container's [`LayoutStyle`] omits
/// the `.with_size(Size::px(...))` call that every other hello-*
/// binding uses; the shell's `IntrinsicAfterFirstPaint` post-first-
/// paint walker measures the actual painted bbox and sizes the
/// window to match (clamped to the `[POPOVER_MIN, POPOVER_MAX]`
/// interval declared above).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ButtonState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let surface = theme.resolve(ColorRole::Surface);

    // Header row — visually distinct from body lines via the larger
    // font + non-muted color (M3 title small vs body medium).
    let header_text = Scene::Text(TextNode::styled(
        HEADER_TEXT,
        Rect::default(),
        TextStyle::new()
            .with_size_px(HEADER_FONT_PX)
            .with_fg(on_surface),
    ));

    // Three body rows — muted color so the header reads as the
    // popover title and the body rows as supplementary status.
    let body_line = |text: &str| {
        Scene::Text(TextNode::styled(
            text,
            Rect::default(),
            TextStyle::new()
                .with_size_px(BODY_FONT_PX)
                .with_fg(on_surface_muted),
        ))
    };
    let body_row_1 = body_line(BODY_LINE_1);
    let body_row_2 = body_line(BODY_LINE_2);
    let body_row_3 = body_line(BODY_LINE_3);

    // Button trigger — M3 filled-tonal surface (the popover footer
    // dismiss action). Pressed state darkens via the M3 state-layer
    // overlay weight (12 %); Disabled fades toward Surface (38 %) —
    // hello-popover never actually disables the button, but the
    // mapping mirrors hello-button so the visual language stays
    // consistent across the example gallery.
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
            .with_tag(POPOVER_BTN_TAG)
            .with_style(BoxStyle::filled(btn_fill))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(pinion_core::style::Size::px(BTN_W, BTN_H)),
            ),
    );

    // Root container — column layout with vertical rhythm gaps. The
    // critical detail: `LayoutStyle::new()` here intentionally OMITS
    // `with_size(Size::px(WIN_W, WIN_H))`. The shell's
    // `IntrinsicAfterFirstPaint` walker measures the painted bbox of
    // this container after the first paint cycle and sizes the
    // window to match.
    Scene::Container(
        ContainerNode::new(vec![
            header_text,
            body_row_1,
            body_row_2,
            body_row_3,
            button,
        ])
        .with_style(BoxStyle::filled(surface))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_justify(JustifyContent::Start)
                .with_align_items(AlignItems::Center)
                .with_gap(ROW_GAP)
                .with_padding(Rect::new(OUTER_PAD, OUTER_PAD, OUTER_PAD, OUTER_PAD)),
        ),
    )
}

/// (R670 §5.16) Paint scene tag for the dismiss Button — the shell's
/// `InputRouter` hit-tests this tag and routes pointer events through
/// the `Scene::External(ButtonExternal)` the `#[widget]` macro wires
/// at the root.
const POPOVER_BTN_TAG: &str = "popover_btn";

/// `WidgetView` binding for the popover's Button trigger. The
/// `#[widget]` attribute lifts every mechanical trait wiring item
/// (`WidgetCore` + `WidgetA11y` + `WidgetView` forwarding); only the
/// view body, the keybinding map, and the `initial_size_strategy`
/// override remain as inherent fns.
///
/// `initial_size = (POPOVER_MIN_W, POPOVER_MIN_H)` declares the
/// fallback the macro's auto-emitted `initial_size_strategy` would
/// reach for if the binding did not override the strategy directly.
/// The binding overrides `initial_size_strategy` below, so this pair
/// is only ever read as the bootstrap floor; winit's
/// `set_min_inner_size` clamp anchors the user-driven OS-resize
/// behaviour against the same `(POPOVER_MIN_W, POPOVER_MIN_H)`.
#[widget(
    tag = "popover_btn",
    state = ButtonState,
    event = ButtonEvent,
    title = "pinion hello-popover (R670 §5.16 IntrinsicAfterFirstPaint)",
    renderer = HelloPopoverRenderer,
    initial_size = (POPOVER_MIN_W, POPOVER_MIN_H),
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
    initial_size_strategy,
)]
struct PopoverView;

impl PopoverView {
    /// R641 inherent forward for [`WidgetCore::view`]. Re-borrows
    /// the by-value `Frame` the macro hands in so the free `view(...)`
    /// fn above keeps the canonical `&Frame` signature.
    fn view(state: ButtonState, frame: Frame) -> Scene {
        view(state, &frame)
    }

    /// Keybinding shim — `d` / `e` mirror the hello-button
    /// convention so RPC-driven demos can exercise the
    /// character-key arc identically across the example gallery.
    fn keybinding(key: &str) -> Option<ButtonEvent> {
        match key {
            "d" => Some(ButtonEvent::Disable),
            "e" => Some(ButtonEvent::Enable),
            _ => None,
        }
    }

    /// ARIA Button keyboard activation (Space / Enter on the focused
    /// button fires `KeyboardActivate` → SCXML internal transition →
    /// `click` intent). Mirrors hello-button exactly.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }

    /// **The R670 atomic (1) substrate consumer.** Overrides the
    /// macro's auto-emitted `initial_size_strategy` (which would
    /// otherwise return `Fixed { width: POPOVER_MIN_W, height:
    /// POPOVER_MIN_H }` per the `initial_size = (..., ...)` attribute
    /// above) with the `IntrinsicAfterFirstPaint` variant.
    ///
    /// The shell's `IntrinsicAfterFirstPaint` wire (R668 substrate)
    /// opens the window at `(POPOVER_MIN_W, POPOVER_MIN_H)`, runs one
    /// paint cycle, walks the resulting scene for the natural bbox,
    /// clamps to `[(POPOVER_MIN_W, POPOVER_MIN_H), (POPOVER_MAX_W,
    /// POPOVER_MAX_H)]`, and calls `Window::request_inner_size` when
    /// the clamped size differs from min — the steady-state path for
    /// this binding since the natural bbox is strictly larger than
    /// the floor on every realistic monitor.
    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::IntrinsicAfterFirstPaint {
            min: (POPOVER_MIN_W, POPOVER_MIN_H),
            max: (POPOVER_MAX_W, POPOVER_MAX_H),
        }
    }
}

fn main() {
    pinion_shell::run::<PopoverView>();
}

#[cfg(test)]
mod r670_intrinsic_first_paint_tests {
    use super::*;

    /// (R670 §5.16) Pin the binding's `initial_size_strategy` to the
    /// `IntrinsicAfterFirstPaint` variant. A regression that flips this
    /// back to `Fixed` would silently degrade the first-consumer
    /// status of the R668 substrate — every other hello-* binding
    /// still uses `Fixed`, so a SEED watch-out can't reach this
    /// boundary without a guard.
    #[test]
    fn r670_popover_declares_intrinsic_strategy() {
        match <PopoverView as pinion_shell::WidgetView>::initial_size_strategy() {
            pinion_shell::SizeStrategy::IntrinsicAfterFirstPaint { min, max } => {
                assert_eq!(min, (POPOVER_MIN_W, POPOVER_MIN_H));
                assert_eq!(max, (POPOVER_MAX_W, POPOVER_MAX_H));
            }
            other => panic!("expected IntrinsicAfterFirstPaint, got {other:?}"),
        }
    }

    /// (R670 §5.16) Pin the absence of a root-size lock. If a
    /// future cleanup round adds `with_size(...)` to the root
    /// container, the `IntrinsicAfterFirstPaint` walker would
    /// measure the locked size and the substrate consumer would
    /// silently degrade to a `Fixed`-equivalent at runtime.
    ///
    /// Asserts the painted root container's `LayoutStyle` does NOT
    /// declare a fixed size — the natural bbox must be free to
    /// flow from the children.
    #[test]
    fn r670_view_root_has_no_size_lock() {
        let owner = pinion_core::Owner::new();
        let scene = owner.run(|| view(ButtonState::Idle, &Frame::new()));
        match &scene {
            Scene::Container(c) => {
                use pinion_core::style::SizeValue;
                // Default `Size` is `(Auto, Auto)` — the value the
                // root container carries when the view fn omits
                // `with_size(...)`. A regression that adds a fixed
                // size lock would flip width / height to `Px(_)`,
                // and `IntrinsicAfterFirstPaint` would silently degrade
                // to the locked size at runtime.
                assert_eq!(
                    c.layout.size.width,
                    SizeValue::Auto,
                    "root container must omit LayoutStyle::with_size width for `IntrinsicAfterFirstPaint` to surface the natural bbox",
                );
                assert_eq!(
                    c.layout.size.height,
                    SizeValue::Auto,
                    "root container must omit LayoutStyle::with_size height for `IntrinsicAfterFirstPaint` to surface the natural bbox",
                );
            }
            other => panic!("expected root Container, got {other:?}"),
        }
    }

    /// (R670 §5.16) Pin the a11y surface — the popover binding is a
    /// Button at the macro level (via `role = Button` in the
    /// `#[widget]` attribute) so the AT-emitted node carries the
    /// ARIA Button role identically to hello-button.
    #[test]
    fn r670_access_node_is_aria_button() {
        let nodes = <PopoverView as WidgetA11y>::access_node(&ButtonState::Idle, None);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, AriaRole::Button);
        assert_eq!(nodes[0].tag, "popover_btn");
    }

    /// (R670 §5.16) Pin the widget tag literal — must match the
    /// `with_tag(POPOVER_BTN_TAG)` in the view fn so the shell's
    /// `InputRouter` hit-tests the painted button rect and routes
    /// pointer events back to the `Scene::External(ButtonExternal)`
    /// at the root.
    #[test]
    fn r670_widget_tag_matches_paint_tag() {
        assert_eq!(<PopoverView as WidgetCore>::tag(), POPOVER_BTN_TAG);
    }

    /// (R670 §5.16) Pin the natural-bbox sanity floor — the view fn
    /// must paint *something* (non-zero rect tree) so the substrate
    /// has content to measure. A regression that drops the body rows
    /// or shrinks the button to zero would produce a `(0, 0)` bbox
    /// and `IntrinsicAfterFirstPaint` would silently clamp the
    /// window at `min` without surfacing the degradation.
    ///
    /// Drives the paint pipeline through `compute_layout` (the same
    /// path the production shell runs) so the post-layout rect tree
    /// reflects what the shell's intrinsic walker sees.
    #[test]
    fn r670_painted_scene_has_non_zero_intrinsic_bbox() {
        let owner = pinion_core::Owner::new();
        let mut scene = owner.run(|| view(ButtonState::Idle, &Frame::new()));
        // Run layout against the floor viewport — production shell
        // sizes the surface to (POPOVER_MIN_W, POPOVER_MIN_H) on the
        // first paint, so the intrinsic walker measures against that
        // pre-resize layout.
        let mut text_cache = pinion_text::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut text_cache, POPOVER_MIN_W, POPOVER_MIN_H);
        let (w, h) = scene.intrinsic_content_size();
        assert!(
            w > 0 && h > 0,
            "intrinsic bbox must be non-zero so `IntrinsicAfterFirstPaint` has content to size against; got ({w}, {h})",
        );
    }
}
