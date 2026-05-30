//! `hello-gradient` — R708 §5.50 consumer of the gradient-fill
//! rendering substrate (the first non-solid [`BoxStyle`] paint).
//!
//! ## What this demonstrates
//!
//! `BoxStyle::gradient` (R708) carries an optional [`Gradient`] overlay
//! that the Vello `paint_adapter` lowers to a `peniko::Gradient` —
//! linear or radial, box-relative UV geometry, an unbounded stop ramp,
//! and a [`Extend`] mode. This binding paints:
//!
//! * a **horizontal hue strip** (`tag = "hue_strip"`) — a 7-stop linear
//!   rainbow, the direct precursor of the R709 `ColorPicker` hue bar;
//! * a **demo swatch** (`tag = "demo_swatch"`) that the Toggle switches
//!   between a vertical *linear* gradient (Off) and a *radial* gradient
//!   (On), exercising both [`GradientKind`] arms and proving the §5.16
//!   R682 paint-cache re-keys when a gradient changes (the manual
//!   `Hash for BoxStyle` folds the gradient in);
//! * a rounded gradient panel behind the swatch.
//!
//! ## Why a Toggle
//!
//! A gradient gallery is intrinsically stateless, but the `AppShell`
//! drives a [`WidgetView`] with a statechart `External`. Rather than
//! invent a one-off widget ([[abstraction-needs-second-consumer]]), the
//! binding reuses the §5.38 [`ToggleExternal`] purely as the *mode bit*:
//! the Off/On sidecar selects linear vs radial for the demo swatch. The
//! Toggle is the RPC-drivable, AT-exposed control; the gradients are the
//! substrate under test.
//!
//! ## Verification (substrate-first)
//!
//! * `scene/snapshot` exposes every gradient as structured data —
//!   `tools/demos/r708_gradient.py` reads back the hue strip's stop
//!   colours / offsets and the swatch's geometry kind without OCR
//!   (§2 #7 scene-as-data, [[ai-first-rpc-introspection-obligation]]).
//! * `tools/demos/r708_gradient_pixel.py` captures the live window and
//!   samples the rendered hue strip at the exact stop offsets — the
//!   live-pixel guard a structural query cannot replace
//!   ([[introspection-from-paint-not-screen]], R706/R707.3 precedent).

use pinion_core::external::IntrospectValue;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, Gradient, JustifyContent, LayoutStyle, Size,
    TextStyle,
};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{ColorRole, Frame, Scene, WidgetCore, WidgetStateName, use_theme};
#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

// pinion-forge codegen output: `pub struct HelloGradientRenderer` +
// `HelloGradientRendererError` + async `new<...>` + sync `render` /
// `resize`. R46.3.3 emit template uses fully-qualified `::vello::*`
// paths so the include is bare.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so `AppShell<V>` can build /
// render / resize it. Identical pattern to hello-toggle.
vello_renderer_impl!(HelloGradientRenderer, HelloGradientRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 320;

const THEME_TAG: &str = "app";

// Hue strip — a 320×28 sharp (radius 0) rectangle so the live-pixel
// guard can sample clean interior columns with no corner antialiasing.
const STRIP_W: u32 = 320;
const STRIP_H: u32 = 28;

// Demo swatch — a 160×96 rounded panel; the Toggle switches its fill
// between a vertical linear gradient and a centred radial gradient.
const SWATCH_W: u32 = 160;
const SWATCH_H: u32 = 96;
const SWATCH_RADIUS: u32 = 16;

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 12;
const SWITCH_FONT_PX: u32 = 14;
const ROW_GAP: u32 = 16;

/// Material 3 state-layer overlay weights (linear-sRGB lerp toward
/// [`ColorRole::OnSurface`]) for the switch chrome — 8 % hover / 12 %
/// pressed, matching the rest of the widget gallery.
const HOVER_OVERLAY_T: f32 = 0.08;
const PRESSED_OVERLAY_T: f32 = 0.12;
const DISABLED_OVERLAY_T: f32 = 0.50;

// ── Hue-strip stop ramp ────────────────────────────────────────────
// Seven sRGB-primary stops at exact sixth offsets so the live-pixel
// guard can predict the colour at any stop offset (the colour AT a stop
// is the stop colour regardless of the interpolation colour space).
const HUE_RED: Color = Color::rgb(0xff, 0x00, 0x00);
const HUE_YELLOW: Color = Color::rgb(0xff, 0xff, 0x00);
const HUE_GREEN: Color = Color::rgb(0x00, 0xff, 0x00);
const HUE_CYAN: Color = Color::rgb(0x00, 0xff, 0xff);
const HUE_BLUE: Color = Color::rgb(0x00, 0x00, 0xff);
const HUE_MAGENTA: Color = Color::rgb(0xff, 0x00, 0xff);

/// The §5.50 hue-bar gradient: a left-to-right linear ramp through the
/// sRGB colour wheel. Stops are the SSOT the demo + pixel guard assert
/// against — `r708_gradient.py` reads them back via `scene/snapshot`.
fn hue_strip_gradient() -> Gradient {
    Gradient::horizontal()
        .with_stop(0.0, HUE_RED)
        .with_stop(1.0 / 6.0, HUE_YELLOW)
        .with_stop(2.0 / 6.0, HUE_GREEN)
        .with_stop(3.0 / 6.0, HUE_CYAN)
        .with_stop(4.0 / 6.0, HUE_BLUE)
        .with_stop(5.0 / 6.0, HUE_MAGENTA)
        .with_stop(1.0, HUE_RED)
}

/// Off-mode demo swatch: a top-to-bottom linear gradient from `Accent`
/// to `Surface`.
fn vertical_swatch_gradient(accent: Color, surface: Color) -> Gradient {
    Gradient::vertical()
        .with_stop(0.0, accent)
        .with_stop(1.0, surface)
}

/// On-mode demo swatch: a centred radial gradient from `Accent` (centre)
/// out to `Surface` at the rect's shorter half-side.
fn radial_swatch_gradient(accent: Color, surface: Color) -> Gradient {
    Gradient::radial((0.5, 0.5), 0.5)
        .with_stop(0.0, accent)
        .with_stop(1.0, surface)
}

/// view-fn (§6.3): pure sync mapping `(ToggleState, bool) -> Scene`.
/// `on` selects the demo-swatch gradient kind (Off = linear vertical,
/// On = radial). `&Frame` is the §6.3 ZST hedge. The shell calls
/// `compute_layout` before paint, so the view need not resolve rects.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ToggleState, on: bool, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let accent = theme.resolve(ColorRole::Accent);

    let title = Scene::Text(TextNode::styled(
        "Gradient gallery",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_fg(on_surface),
    ));

    // Hue strip — sharp linear rainbow. `fill` is the solid fallback the
    // gradient overrides; kept opaque-black so a regression that dropped
    // the gradient would read as an obvious black bar, not transparent.
    let hue_strip = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(Color::rgb(0x00, 0x00, 0x00))
                .with_gradient(hue_strip_gradient()),
        )
        .with_tag("hue_strip")
        .with_layout(LayoutStyle::new().with_size(Size::px(STRIP_W, STRIP_H))),
    );

    // Demo swatch — linear (Off) or radial (On). The gradient change
    // re-keys the §5.16 paint-cache via the manual `Hash for BoxStyle`.
    let swatch_gradient = if on {
        radial_swatch_gradient(accent, surface)
    } else {
        vertical_swatch_gradient(accent, surface)
    };
    let swatch = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(surface)
                .with_gradient(swatch_gradient)
                .with_corner_radius(SWATCH_RADIUS),
        )
        .with_tag("demo_swatch")
        .with_layout(LayoutStyle::new().with_size(Size::px(SWATCH_W, SWATCH_H))),
    );

    // Mode switch — a rounded chip tagged `main_toggle` (the Toggle
    // hit-test handle). Off sits on the inactive container surface, On
    // on the accent; Hover / Pressed lerp toward on-surface at the M3
    // state-layer weights.
    let switch_base = if on {
        accent
    } else {
        theme.resolve(ColorRole::SurfaceContainerHighest)
    };
    let switch_fill: Color = match state {
        ToggleState::Idle => switch_base,
        ToggleState::Hover => switch_base.lerp(on_surface, HOVER_OVERLAY_T),
        ToggleState::Pressed => switch_base.lerp(on_surface, PRESSED_OVERLAY_T),
        ToggleState::Disabled => switch_base.lerp(surface, DISABLED_OVERLAY_T),
    };
    let switch_fg = if on {
        theme.resolve(ColorRole::OnAccent)
    } else {
        on_surface
    };
    let switch_label = Scene::Text(TextNode::styled(
        if on { "Radial" } else { "Linear" },
        Rect::default(),
        TextStyle::new()
            .with_size_px(SWITCH_FONT_PX)
            .with_fg(switch_fg),
    ));
    let mode_chip = Scene::Container(
        ContainerNode::new(vec![switch_label])
            .with_tag("main_toggle")
            .with_aria_label("Gradient kind")
            .with_style(BoxStyle::filled(switch_fill).with_corner_radius(18))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(120, 36)),
            ),
    );

    let status = Scene::Text(TextNode::styled(
        format!("{} | {}", state.as_name(), if on { "Radial" } else { "Linear" }),
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    Scene::Container(
        ContainerNode::new(vec![title, hue_strip, swatch, mode_chip, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(ROW_GAP),
            ),
    )
}

/// `WidgetView` binding. The §5.38 Toggle is reused as the linear/radial
/// mode bit (see the module doc); the `#[widget]` attribute derives the
/// mechanical [`WidgetCore`] / [`WidgetA11y`] / [`WidgetView`] trio. No
/// `update` reducer — unlike `hello-toggle` this binding has no theme
/// side-effect; the Off/On bit is read straight in [`view`].
///
/// [`WidgetCore`]: pinion_core::WidgetCore
/// [`WidgetA11y`]: pinion_a11y::WidgetA11y
/// [`WidgetView`]: pinion_shell::WidgetView
#[widget(
    tag = "main_toggle",
    state = (ToggleState, bool),
    event = ToggleEvent,
    title = "pinion hello-gradient (R708 §5.50 gradient-fill substrate)",
    renderer = HelloGradientRenderer,
    initial_size = (WIN_W, WIN_H),
    external = ToggleExternal::new,
    role = Switch,
    state_flags(
        hovered = Hover,
        pressed = Pressed,
        disabled = Disabled,
        checked = bool_field(1),
    ),
    access_value = bool_field(1),
    apply_key,
    keybinding,
)]
struct GradientView;

impl GradientView {
    /// Tuple-state introspect: SCXML state name via `query("state")` +
    /// the Off/On sidecar via `query("value")`. Defaults to
    /// `(Idle, false)` for a fresh External.
    fn read_state(scene: &Scene) -> (ToggleState, bool) {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                let state = if let Some(IntrospectValue::Text(name)) = intro.query("state") {
                    ToggleState::from_name_or_default(&name)
                } else {
                    ToggleState::Idle
                };
                let value = matches!(intro.query("value"), Some(IntrospectValue::Bool(true)));
                return (state, value);
            }
        }
        (ToggleState::Idle, false)
    }

    /// R641 §5.16 inherent view shim — unpacks the tuple and forwards to
    /// the free [`view`].
    fn view(state: (ToggleState, bool), frame: Frame) -> Scene {
        view(state.0, state.1, &frame)
    }

    fn event_name(event: ToggleEvent) -> &'static str {
        pinion_core::WidgetEventName::as_name(&event)
    }

    fn keybinding(key: &str) -> Option<ToggleEvent> {
        match key {
            "d" => Some(ToggleEvent::Disable),
            "e" => Some(ToggleEvent::Enable),
            _ => None,
        }
    }

    /// ARIA toggle-button keyboard activation (Space / Enter flips the
    /// Off ↔ On sidecar in parity with a pointer click).
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
    pinion_shell::run::<GradientView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;
    use pinion_core::style::GradientKind;

    /// Walk the scene for the first `Scene::Box` carrying `tag`.
    fn find_box<'a>(scene: &'a Scene, tag: &str) -> Option<&'a BoxNode> {
        match scene {
            Scene::Box(b) if b.tag.as_deref() == Some(tag) => Some(b),
            Scene::Container(c) => c.children.iter().find_map(|ch| find_box(ch, tag)),
            _ => None,
        }
    }

    fn rendered(state: ToggleState, on: bool) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(state, on, &Frame::new()))
    }

    #[test]
    fn hue_strip_carries_a_seven_stop_linear_gradient() {
        let scene = rendered(ToggleState::Idle, false);
        let strip = find_box(&scene, "hue_strip").expect("hue strip present");
        let gradient = strip.style.gradient.as_ref().expect("strip has a gradient");
        assert!(matches!(gradient.kind, GradientKind::Linear { .. }));
        assert_eq!(gradient.stops.len(), 7);
        assert_eq!(gradient.stops[0].color, HUE_RED);
        assert_eq!(gradient.stops[3].color, HUE_CYAN);
        assert_eq!(gradient.stops[6].color, HUE_RED);
        // Offsets ascend across the full [0,1] range.
        assert!((gradient.stops[0].offset - 0.0).abs() < f32::EPSILON);
        assert!((gradient.stops[6].offset - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn swatch_is_linear_when_off_and_radial_when_on() {
        let off = rendered(ToggleState::Idle, false);
        let off_swatch = find_box(&off, "demo_swatch").expect("swatch present");
        assert!(matches!(
            off_swatch.style.gradient.as_ref().unwrap().kind,
            GradientKind::Linear { .. }
        ));

        let on = rendered(ToggleState::Idle, true);
        let on_swatch = find_box(&on, "demo_swatch").expect("swatch present");
        assert!(matches!(
            on_swatch.style.gradient.as_ref().unwrap().kind,
            GradientKind::Radial { .. }
        ));
    }

    #[test]
    fn swatch_gradient_change_re_keys_the_paint_hash() {
        // The §5.16 R682 paint-cache keys off `Scene::paint_hash`, which
        // folds `BoxStyle::gradient` in via the manual `Hash for
        // BoxStyle`. Off vs On swatches must therefore hash differently
        // or the cache would replay a stale fragment.
        let off = rendered(ToggleState::Idle, false);
        let on = rendered(ToggleState::Idle, true);
        assert_ne!(off.paint_hash(), on.paint_hash());
    }

    #[test]
    fn r55_g20_view_carries_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<GradientView>(
            (ToggleState::Idle, false),
            &Frame::new(),
        );
    }

    #[test]
    fn switch_reports_switch_role() {
        let nodes = <GradientView as WidgetA11y>::access_node(&(ToggleState::Idle, true), None);
        assert_eq!(nodes[0].role, AriaRole::Switch);
        assert_eq!(nodes[0].tag, "main_toggle");
    }
}
