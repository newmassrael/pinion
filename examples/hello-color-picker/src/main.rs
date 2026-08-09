//! `hello-color-picker` — R709 §5.38 `ColorPicker`: the first real
//! consumer of the R708 gradient-fill substrate and the first consumer
//! of the new §5.38 `ColorArea` 2-D pad widget.
//!
//! ## What this demonstrates
//!
//! A canonical HSV colour picker decomposes into two slider-class
//! controls (there is no WAI-ARIA `colorpicker` role; react-aria and
//! native pickers model it as a 1-D hue slider plus a 2-D
//! saturation/value area, both `role="slider"`):
//!
//! * a **2-D saturation/value pad** (`tag = "sv_pad"`, the primary
//!   external) — the new `ColorArea` widget. One drag thumb writes a
//!   normalised `(x, y) = (saturation, value)` pair. It is painted by
//!   *layering gradients* (R708): a pure-hue base fill, a horizontal
//!   white→transparent overlay (the saturation axis), and a vertical
//!   transparent→black overlay (the value axis). Top-left is white,
//!   top-right is the fully-saturated hue, the whole bottom edge is
//!   black — the standard SV square, built entirely from
//!   `BoxStyle.gradient` + alpha stops;
//! * a **hue bar** (`tag = "hue_bar"`, an extra external) — a §5.38
//!   `SliderExternal` painted with the 7-stop rainbow gradient (the
//!   `hello-gradient` hue strip, generalised into an interactive
//!   control);
//! * a **live preview swatch** filled with `Color::from_hsv(hue,
//!   saturation, value)` (the new §5.50 colour-space constructor), plus
//!   a hex read-out.
//!
//! ## AI-first introspection
//!
//! Both controls expose their normalised value through the §5.15
//! introspect channel: `scene/query {path: "/sv_pad/external/x"}` /
//! `.../y` and `.../hue_bar/external/value`. An agent drives the picker
//! purely through `scene/intervene` (set an axis) or `scene/click` /
//! `scene/drag` (pointer) and reads the selected colour back without a
//! single pixel — `tools/demos/r709_color_picker.py` does exactly that,
//! then a live-pixel pass samples the SV square's corners to prove the
//! gradient layering renders ([[introspection-from-paint-not-screen]]).

use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::marks::MarkSet;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, FlexDirection, Gradient, JustifyContent, LayoutStyle,
    Size, TextStyle, scale_normalized_to_px,
};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::color_area::{ColorAreaExternal, ColorAreaState};
use pinion_core::widgets::slider::SliderExternal;
use pinion_core::{ColorRole, Frame, Modifiers, Scene, WidgetCore, WidgetStateName, use_theme};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

// pinion-forge codegen: `pub struct HelloColorPickerRenderer` + async
// `new` + sync `render` / `resize`. Identical include shape to
// hello-gradient.
include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloColorPickerRenderer, HelloColorPickerRendererError);

const WIN_W: u32 = 380;
const WIN_H: u32 = 360;

const THEME_TAG: &str = "app";

/// The 2-D saturation/value pad — the primary `ColorArea` external's
/// hit-test surface. Square so the live-pixel guard can predict each
/// corner's colour.
const SV_TAG: &str = "sv_pad";
const SV_SIZE: u32 = 220;

/// The hue bar — an extra `SliderExternal`. Same width as the SV pad,
/// stacked below it (the raster editor layout).
const HUE_TAG: &str = "hue_bar";
const HUE_W: u32 = 220;
const HUE_H: u32 = 24;
const HUE_THUMB_W: u32 = 6;

/// SV-pad selection thumb: a two-ring circle (black outer + white
/// inner) so it stays visible over both the white and the black
/// corners of the square.
const SV_THUMB: u32 = 14;

const SWATCH_W: u32 = 120;
const SWATCH_H: u32 = 72;
const SWATCH_RADIUS: u32 = 12;

const TITLE_FONT_PX: u32 = 18;
const LABEL_FONT_PX: u32 = 13;
const STATUS_FONT_PX: u32 = 12;
const COL_GAP: u32 = 20;
const ROW_GAP: u32 = 14;

/// Keyboard nudge step for the arrow keys (5 % of each normalised
/// axis), matching the §5.38 Slider's `apply_key` increment.
const STEP: f32 = 0.05;

// ── Hue-bar stop ramp ──────────────────────────────────────────────
// Seven sRGB-primary stops at exact sixth offsets (the hello-gradient
// hue strip, now an interactive slider track). The colour AT a stop is
// the stop colour regardless of interpolation space, so the live-pixel
// guard predicts the hue at any sixth.
const HUE_RED: Color = Color::rgb(0xff, 0x00, 0x00);
const HUE_YELLOW: Color = Color::rgb(0xff, 0xff, 0x00);
const HUE_GREEN: Color = Color::rgb(0x00, 0xff, 0x00);
const HUE_CYAN: Color = Color::rgb(0x00, 0xff, 0xff);
const HUE_BLUE: Color = Color::rgb(0x00, 0x00, 0xff);
const HUE_MAGENTA: Color = Color::rgb(0xff, 0x00, 0xff);

/// The §5.50 hue-bar gradient: a left-to-right linear ramp through the
/// sRGB colour wheel. `value = 0.0` (left) selects red, `value = 1.0`
/// (right) wraps back to red.
fn hue_bar_gradient() -> Gradient {
    Gradient::horizontal()
        .with_stop(0.0, HUE_RED)
        .with_stop(1.0 / 6.0, HUE_YELLOW)
        .with_stop(2.0 / 6.0, HUE_GREEN)
        .with_stop(3.0 / 6.0, HUE_CYAN)
        .with_stop(4.0 / 6.0, HUE_BLUE)
        .with_stop(5.0 / 6.0, HUE_MAGENTA)
        .with_stop(1.0, HUE_RED)
}

/// Saturation axis: a horizontal white→transparent overlay. At the
/// left edge (`x = 0`) opaque white desaturates the hue to grey; at the
/// right edge (`x = 1`) it is fully transparent so the pure hue shows.
/// Alpha-bearing stops (R708 lowers `Color.a` straight to peniko).
fn saturation_overlay() -> Gradient {
    Gradient::horizontal()
        .with_stop(0.0, Color::rgba(0xff, 0xff, 0xff, 0xff))
        .with_stop(1.0, Color::rgba(0xff, 0xff, 0xff, 0x00))
}

/// Value axis: a vertical transparent→black overlay. At the top edge
/// (`y_rel = 0`, the brightest value) it is transparent; at the bottom
/// edge it is opaque black, so the whole bottom row reads black
/// regardless of saturation.
fn value_overlay() -> Gradient {
    Gradient::vertical()
        .with_stop(0.0, Color::rgba(0x00, 0x00, 0x00, 0x00))
        .with_stop(1.0, Color::rgba(0x00, 0x00, 0x00, 0xff))
}

/// Map the normalised hue axis (`0.0..=1.0`) to a degree value.
fn hue_degrees(hue: f32) -> f32 {
    hue * 360.0
}

/// A full-cover absolute overlay box carrying `gradient`, sized to the
/// SV square and pinned at the pad origin. Stacked in child order over
/// the pure-hue base fill.
fn sv_overlay(gradient: Gradient) -> Scene {
    Scene::Box(
        BoxNode::new(
            Rect::default(),
            // Transparent solid fallback: the gradient supplies every
            // pixel, but a dropped-gradient regression would then read
            // as fully transparent (showing the base) rather than a
            // surprise opaque block.
            BoxStyle::filled(Color::rgba(0, 0, 0, 0)).with_gradient(gradient),
        )
        .with_layout(
            LayoutStyle::new()
                .with_size(Size::px(SV_SIZE, SV_SIZE))
                .with_absolute_position(0, 0),
        ),
    )
}

/// One ring of the SV thumb: a hollow bordered circle at `(left, top)`.
fn sv_thumb_ring(left: u32, top: u32, size: u32, border: Color) -> Scene {
    Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(Color::rgba(0, 0, 0, 0))
                .with_border(Border::new(border, 2))
                .with_corner_radius(size / 2),
        )
        .with_layout(
            LayoutStyle::new()
                .with_size(Size::px(size, size))
                .with_absolute_position(left, top),
        ),
    )
}

/// R1618 — the two reasons the saturation/value pad looks the way it does, and
/// they live in DIFFERENT externals: the base colour is the hue slider's, the
/// thumb is this pad's own. Named once so the paint and the wire agree by
/// construction rather than by two matching string literals.
const MARK_HUE_BASE: &str = "hue_base";
const MARK_SV_THUMB: &str = "sv_thumb";

/// view-fn (§6.3): pure sync mapping the composite picker state to a
/// `Scene`. `state = (sv_interaction, saturation, value, hue)`.
#[allow(clippy::trivially_copy_pass_by_ref, clippy::too_many_lines)]
fn view(state: PickerState, _frame: &Frame) -> Scene {
    let (sv_state, saturation, value, hue) = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);

    let deg = hue_degrees(hue);
    let pure_hue = Color::from_hsv(deg, 1.0, 1.0);
    let selected = Color::from_hsv(deg, saturation, value);

    let title = Scene::Text(TextNode::styled(
        "Colour picker",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_fg(on_surface),
    ));

    // ── SV pad: pure-hue base + saturation + value overlays + thumb ──
    // Thumb leading-pixel position: saturation drives x left→right,
    // value drives y with the top = brightest inversion. The thumb is
    // kept fully inside the square (RANGE = SV_SIZE - SV_THUMB).
    let sv_range = SV_SIZE - SV_THUMB;
    let thumb_x = scale_normalized_to_px(saturation, sv_range);
    let thumb_y = scale_normalized_to_px(1.0 - value, sv_range);
    let outer = sv_thumb_ring(thumb_x, thumb_y, SV_THUMB, Color::rgb(0x00, 0x00, 0x00));
    let inner = sv_thumb_ring(
        thumb_x + 2,
        thumb_y + 2,
        SV_THUMB.saturating_sub(4),
        Color::rgb(0xff, 0xff, 0xff),
    );
    let sv_pad = Scene::Container(
        ContainerNode::new(vec![
            sv_overlay(saturation_overlay()),
            sv_overlay(value_overlay()),
            outer,
            inner,
        ])
        .with_tag(SV_TAG)
        // R1618 — the pad is where two externals MEET, and until now nothing
        // could say so. Its base fill is the hue the HUE slider holds; its
        // thumb is the saturation and value the SV pad holds. A client asking
        // "why is this pad this colour" got half an answer from either
        // external and had to compose the other half itself — which means
        // re-implementing the framework's composition on the client side.
        //
        // Declared base-first, so the last reason is the one nearest the
        // pixels a person is looking at: the same last-wins order every mark
        // set resolves in.
        .with_marks(
            MarkSet::whole()
                .because(MARK_HUE_BASE)
                .because(MARK_SV_THUMB),
        )
        .with_aria_label("Saturation and brightness")
        // Base = the fully-saturated, full-value hue; the overlays
        // carve saturation (white) and value (black) out of it.
        .with_style(BoxStyle::filled(pure_hue))
        // R1020 §5.39 — the SV pad is a Tab stop (painted before the hue bar,
        // matching the [SV_TAG, HUE_TAG] order); opt it into the enumeration.
        .with_layout(
            LayoutStyle::new()
                .with_focusable(true)
                .with_size(Size::px(SV_SIZE, SV_SIZE)),
        ),
    );

    // ── Hue bar: rainbow gradient track + leading thumb ─────────────
    let hue_thumb_x = scale_normalized_to_px(hue, HUE_W - HUE_THUMB_W);
    let hue_thumb = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(Color::rgba(0xff, 0xff, 0xff, 0xff))
                .with_border(Border::new(Color::rgb(0x00, 0x00, 0x00), 1))
                .with_corner_radius(2),
        )
        .with_layout(
            LayoutStyle::new()
                .with_size(Size::px(HUE_THUMB_W, HUE_H))
                .with_absolute_position(hue_thumb_x, 0),
        ),
    );
    let hue_bar = Scene::Container(
        ContainerNode::new(vec![hue_thumb])
            .with_tag(HUE_TAG)
            .with_aria_label("Hue")
            .with_style(
                BoxStyle::filled(Color::rgb(0x00, 0x00, 0x00)).with_gradient(hue_bar_gradient()),
            )
            // R1020 §5.39 — the hue bar is a Tab stop; opt it into the
            // scene-derived enumeration.
            .with_layout(
                LayoutStyle::new()
                    .with_focusable(true)
                    .with_size(Size::px(HUE_W, HUE_H)),
            ),
    );

    let controls = Scene::Container(
        ContainerNode::new(vec![sv_pad, hue_bar]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Start)
                .with_gap(ROW_GAP),
        ),
    );

    // ── Preview column: swatch + hex read-out ───────────────────────
    let swatch = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(selected)
                .with_border(Border::new(theme.resolve(ColorRole::OnSurfaceMuted), 1))
                .with_corner_radius(SWATCH_RADIUS),
        )
        .with_tag("preview_swatch")
        .with_layout(LayoutStyle::new().with_size(Size::px(SWATCH_W, SWATCH_H))),
    );
    let hex = Scene::Text(TextNode::styled(
        hex_label(selected),
        Rect::default(),
        TextStyle::new()
            .with_size_px(LABEL_FONT_PX)
            .with_fg(on_surface),
    ));
    let hsv_label = Scene::Text(TextNode::styled(
        format!(
            "H {:.0}\u{00b0}  S {:.0}%  V {:.0}%",
            deg,
            saturation * 100.0,
            value * 100.0
        ),
        Rect::default(),
        TextStyle::new()
            .with_size_px(LABEL_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    let preview = Scene::Container(
        ContainerNode::new(vec![swatch, hex, hsv_label]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Start)
                .with_gap(8),
        ),
    );

    let body = Scene::Container(
        ContainerNode::new(vec![controls, preview]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Start)
                .with_gap(COL_GAP),
        ),
    );

    let status = Scene::Text(TextNode::styled(
        format!("{} | {}", sv_state.as_name(), hex_label(selected)),
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    Scene::Container(
        ContainerNode::new(vec![title, body, status])
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

/// `#rrggbb` upper-case hex for the selected colour (alpha is always
/// opaque from `Color::from_hsv`, so the swatch read-out omits it).
fn hex_label(c: Color) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

// ── Composite state + external readback ────────────────────────────

/// `(sv_interaction_state, saturation, value, hue)` — every field is
/// `Copy`, so the tuple satisfies `WidgetCore::State`. The two SV axes
/// and the SV interaction state come from the primary
/// `ColorAreaExternal` (`sv_pad`); the hue comes from the extra
/// `SliderExternal` (`hue_bar`).
type PickerState = (ColorAreaState, f32, f32, f32);

/// Read the SV pad's interaction state + `(x, y)` from the `sv_pad`
/// external. Defaults to `(Idle, 0, 0)` before the external exists.
fn read_sv(scene: &Scene) -> (ColorAreaState, f32, f32) {
    if let Some(node) = scene.find_external_with_tag(SV_TAG) {
        if let Some(intro) = node.handle.introspect() {
            let st = match intro.query("state") {
                Some(IntrospectValue::Text(name)) => ColorAreaState::from_name_or_default(&name),
                _ => ColorAreaState::Idle,
            };
            let x = intro.query("x").and_then(|v| v.as_f32()).unwrap_or(0.0);
            let y = intro.query("y").and_then(|v| v.as_f32()).unwrap_or(0.0);
            return (st, x, y);
        }
    }
    (ColorAreaState::Idle, 0.0, 0.0)
}

/// Read the hue bar's normalised value from the `hue_bar` external.
fn read_hue(scene: &Scene) -> f32 {
    scene
        .find_external_with_tag(HUE_TAG)
        .and_then(|node| node.handle.introspect())
        .and_then(|intro| intro.query("value"))
        .and_then(|v| v.as_f32())
        .unwrap_or(0.0)
}

struct ColorPickerView;

impl WidgetCore for ColorPickerView {
    type State = PickerState;
    // The picker is driven by the pointer router (drag) + the arrow
    // keys (apply_key); no keybinding-channel widget event flows
    // through `event_name` (mirrors hello-dialog / hello-menu).
    type Event = ();

    /// Primary external = the SV pad, seeded at the centre `(0.5, 0.5)`
    /// so the first paint shows a mid saturation/value selection with
    /// the thumb away from every corner (keeps the four SV corners
    /// clear for the live-pixel guard, which renders the boot frame).
    fn create_external() -> Box<dyn External> {
        let mut sv = ColorAreaExternal::new();
        sv.set_xy(0.5, 0.5);
        Box::new(sv)
    }

    /// Extra external = the hue bar, seeded at red (`value = 0.0`).
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let hue = SliderExternal::new();
        vec![ExtraExternal::new(HUE_TAG, Box::new(hue))]
    }

    fn tag() -> &'static str {
        SV_TAG
    }

    fn read_state(scene: &Scene) -> PickerState {
        let (st, x, y) = read_sv(scene);
        (st, x, y, read_hue(scene))
    }

    fn view(state: PickerState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-color-picker (R709 §5.38 ColorArea + gradient)"
    }

    /// W3C/ARIA slider keyboard support. On the SV pad the arrows nudge
    /// both axes (Left/Right = saturation, Up/Down = value); on the hue
    /// bar they nudge the single hue axis. Each writes through the
    /// §5.15 `intervene` side door — the same path `scene/intervene`
    /// uses — so an AI client observes keyboard and drag mutations
    /// identically.
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str, _mods: Modifiers) -> bool {
        match focused {
            Some(SV_TAG) => nudge_sv(scene, key),
            Some(HUE_TAG) => nudge_axis(scene, HUE_TAG, "value", key),
            _ => false,
        }
    }

    fn fmt_state_log(state: &PickerState) -> String {
        format!(
            "{} | {}",
            state.0.as_name(),
            hex_label(Color::from_hsv(hue_degrees(state.3), state.1, state.2,))
        )
    }
}

/// Nudge the SV pad: Left/Right step saturation (`x`), Up/Down step
/// value (`y`, up = brighter), Home/End jump saturation to the edges.
fn nudge_sv(scene: &mut Scene, key: &str) -> bool {
    let Some(node) = scene.find_external_with_tag_mut(SV_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    let x = intro.query("x").and_then(|v| v.as_f32()).unwrap_or(0.0);
    let y = intro.query("y").and_then(|v| v.as_f32()).unwrap_or(0.0);
    let (axis, next) = match key {
        "ArrowLeft" => ("x", (x - STEP).clamp(0.0, 1.0)),
        "ArrowRight" => ("x", (x + STEP).clamp(0.0, 1.0)),
        "ArrowDown" => ("y", (y - STEP).clamp(0.0, 1.0)),
        "ArrowUp" => ("y", (y + STEP).clamp(0.0, 1.0)),
        "Home" => ("x", 0.0),
        "End" => ("x", 1.0),
        _ => return false,
    };
    intro
        .intervene(axis, IntrospectValue::Float(f64::from(next)))
        .is_ok()
}

/// Nudge a single-axis slider-class external (the hue bar). Mirrors the
/// hello-slider `apply_key` arrow / Home / End / `PageUp` / `PageDown`
/// set.
fn nudge_axis(scene: &mut Scene, tag: &str, path: &str, key: &str) -> bool {
    let Some(node) = scene.find_external_with_tag_mut(tag) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    let cur = intro.query(path).and_then(|v| v.as_f32()).unwrap_or(0.0);
    let next = match key {
        "ArrowLeft" | "ArrowDown" => (cur - STEP).clamp(0.0, 1.0),
        "ArrowRight" | "ArrowUp" => (cur + STEP).clamp(0.0, 1.0),
        "Home" => 0.0,
        "End" => 1.0,
        "PageDown" => (cur - 0.10).clamp(0.0, 1.0),
        "PageUp" => (cur + 0.10).clamp(0.0, 1.0),
        _ => return false,
    };
    intro
        .intervene(path, IntrospectValue::Float(f64::from(next)))
        .is_ok()
}

impl WidgetA11y for ColorPickerView {
    /// Both controls report `role="slider"` — the react-aria / native
    /// convention for a colour picker (there is no `colorpicker` role).
    /// The SV pad's `aria-valuetext` carries the selected hex colour
    /// (a 2-D area has no single meaningful `aria-valuenow`); the hue
    /// bar carries its normalised `aria-valuenow`. Full 2-D ARIA (two
    /// thumbs / per-axis value text) is a refinement deferred until a
    /// second 2-D consumer exists.
    fn access_node(state: &PickerState, focused: Option<&str>) -> Vec<AccessNode> {
        let (sv_state, saturation, value, hue) = *state;
        let selected = Color::from_hsv(hue_degrees(hue), saturation, value);
        let sv_node = AccessNode::new(SV_TAG, AriaRole::Slider)
            .with_name("Saturation and brightness")
            .with_value(AccessValue::Text(hex_label(selected)))
            .with_state(AccessState {
                focused: focused == Some(SV_TAG),
                ..AccessState::from_interaction(sv_state, None)
            });
        let hue_node = AccessNode::new(HUE_TAG, AriaRole::Slider)
            .with_name("Hue")
            .with_value(AccessValue::Float {
                value: hue,
                min: 0.0,
                max: 1.0,
            })
            .with_state(AccessState {
                focused: focused == Some(HUE_TAG),
                ..AccessState::default()
            });
        vec![sv_node, hue_node]
    }
}

impl WidgetView for ColorPickerView {
    type Renderer = HelloColorPickerRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<ColorPickerView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;
    use pinion_core::scene::ExternalNode;
    use pinion_core::style::GradientKind;

    fn rendered(state: PickerState) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(state, &Frame::new()))
    }

    fn find_box<'a>(scene: &'a Scene, tag: &str) -> Option<&'a BoxNode> {
        match scene {
            Scene::Box(b) if b.tag.as_deref() == Some(tag) => Some(b),
            Scene::Container(c) => c.children.iter().find_map(|ch| find_box(ch, tag)),
            Scene::Scroll(s) => find_box(&s.content, tag),
            _ => None,
        }
    }

    fn find_container<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        match scene {
            Scene::Container(c) if c.tag.as_deref() == Some(tag) => Some(c),
            Scene::Container(c) => c.children.iter().find_map(|ch| find_container(ch, tag)),
            Scene::Scroll(s) => find_container(&s.content, tag),
            _ => None,
        }
    }

    #[test]
    fn from_hsv_preview_matches_axes() {
        // Fully saturated, bright red.
        assert_eq!(
            Color::from_hsv(hue_degrees(0.0), 1.0, 1.0),
            Color::rgb(255, 0, 0)
        );
        // Bottom of the SV square (value 0) is always black.
        assert_eq!(
            Color::from_hsv(hue_degrees(0.33), 1.0, 0.0),
            Color::rgb(0, 0, 0)
        );
        // Left of the SV square (saturation 0) is the grey value ramp.
        assert_eq!(
            Color::from_hsv(hue_degrees(0.5), 0.0, 1.0),
            Color::rgb(255, 255, 255)
        );
    }

    #[test]
    fn sv_pad_layers_two_alpha_gradients_over_pure_hue() {
        // hue 0 (red) base, saturation 1, value 1.
        let scene = rendered((ColorAreaState::Idle, 1.0, 1.0, 0.0));
        let pad = find_container(&scene, SV_TAG).expect("sv pad present");
        // Base fill = the pure hue.
        assert_eq!(pad.style.fill, Color::rgb(255, 0, 0));
        // Two gradient overlays present: a horizontal saturation ramp
        // and a vertical value ramp, both alpha-bearing.
        let grads: Vec<&Gradient> = pad
            .children
            .iter()
            .filter_map(|c| match c {
                Scene::Box(b) => b.style.gradient.as_ref(),
                _ => None,
            })
            .collect();
        assert_eq!(grads.len(), 2, "saturation + value overlays");
        // Saturation overlay: white opaque -> white transparent.
        assert!(matches!(grads[0].kind, GradientKind::Linear { start, end }
            if start == (0.0, 0.0) && end == (1.0, 0.0)));
        assert_eq!(grads[0].stops[0].color, Color::rgba(255, 255, 255, 255));
        assert_eq!(grads[0].stops[1].color, Color::rgba(255, 255, 255, 0));
        // Value overlay: black transparent (top) -> black opaque (bottom).
        assert!(matches!(grads[1].kind, GradientKind::Linear { start, end }
            if start == (0.0, 0.0) && end == (0.0, 1.0)));
        assert_eq!(grads[1].stops[0].color, Color::rgba(0, 0, 0, 0));
        assert_eq!(grads[1].stops[1].color, Color::rgba(0, 0, 0, 255));
    }

    #[test]
    fn hue_bar_carries_seven_stop_rainbow() {
        let scene = rendered((ColorAreaState::Idle, 1.0, 1.0, 0.0));
        let bar = find_container(&scene, HUE_TAG).expect("hue bar present");
        let grad = bar.style.gradient.as_ref().expect("hue gradient");
        assert_eq!(grad.stops.len(), 7);
        assert_eq!(grad.stops[0].color, HUE_RED);
        assert_eq!(grad.stops[3].color, HUE_CYAN);
        assert_eq!(grad.stops[6].color, HUE_RED);
    }

    #[test]
    fn preview_swatch_tracks_selected_hsv() {
        // hue 0.5 -> cyan family; saturation 1, value 1 -> pure cyan.
        let scene = rendered((ColorAreaState::Idle, 1.0, 1.0, 0.5));
        let swatch = find_box(&scene, "preview_swatch").expect("swatch present");
        assert_eq!(swatch.style.fill, Color::from_hsv(180.0, 1.0, 1.0));
    }

    #[test]
    fn sv_thumb_moves_with_axes() {
        // Two scenes differing only in saturation must move the SV
        // thumb horizontally (different absolute_position left).
        let low = rendered((ColorAreaState::Idle, 0.1, 0.5, 0.0));
        let high = rendered((ColorAreaState::Idle, 0.9, 0.5, 0.0));
        // The outer thumb ring is the first absolute box without a
        // gradient inside the pad.
        let left_of = |scene: &Scene| -> u32 {
            let pad = find_container(scene, SV_TAG).unwrap();
            pad.children
                .iter()
                .find_map(|c| match c {
                    Scene::Box(b)
                        if b.style.gradient.is_none() && b.layout.absolute_position.is_some() =>
                    {
                        Some(b.layout.absolute_position.unwrap().0)
                    }
                    _ => None,
                })
                .unwrap()
        };
        assert!(left_of(&high) > left_of(&low));
    }

    #[test]
    fn read_state_recovers_axes_from_both_externals() {
        // Build the exact multi-external scene shape the shell wraps.
        let mut sv = ColorAreaExternal::new();
        sv.set_xy(0.6, 0.4);
        let mut hue = SliderExternal::new();
        hue.set_value(0.25);
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::External(ExternalNode::new(Box::new(sv)).with_tag(SV_TAG)),
            Scene::External(ExternalNode::new(Box::new(hue)).with_tag(HUE_TAG)),
        ]));
        let (_st, sat, val, h) = ColorPickerView::read_state(&scene);
        assert!((sat - 0.6).abs() < 1e-6);
        assert!((val - 0.4).abs() < 1e-6);
        assert!((h - 0.25).abs() < 1e-6);
    }

    #[test]
    fn both_controls_report_slider_role() {
        let nodes = ColorPickerView::access_node(&(ColorAreaState::Idle, 1.0, 1.0, 0.0), None);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].role, AriaRole::Slider);
        assert_eq!(nodes[0].tag, SV_TAG);
        assert_eq!(nodes[1].role, AriaRole::Slider);
        assert_eq!(nodes[1].tag, HUE_TAG);
    }

    #[test]
    fn view_contains_composite_paint_root_tag() {
        let scene = rendered((ColorAreaState::Idle, 1.0, 1.0, 0.0));
        assert!(scene.contains_tag(SV_TAG));
    }

    /// R1618 — the pad publishes WHY it is that colour, and a counterfactual
    /// found that nothing but the demo checked it.
    ///
    /// Two of this round's ten counterfactuals passed against the whole Rust
    /// suite: deleting the container arm that carries this publication, and
    /// swapping the two reasons' declaration order. Both are the round's
    /// subject, and both were covered only by a demo that runs a subprocess —
    /// which is to say, not by the gate a round is judged at.
    ///
    /// The order is asserted as an ORDER, not as a set, because declaration
    /// order IS the resolution rule: the last reason is the one a painter
    /// obeyed where two overlap, so writing them the other way round is a
    /// behaviour change and not a formatting choice.
    #[test]
    fn r1618_the_pad_publishes_both_externals_reasons_in_order() {
        use pinion_core::marks::{MarksLookup, domain};
        use pinion_core::reactive::Owner;

        Owner::new().run(|| {
            let scene = view((ColorAreaState::Idle, 0.6, 0.4, 0.25), &Frame::new());
            let MarksLookup::Published(runs) = scene.marks_for_tag(SV_TAG) else {
                panic!("the pad is where two externals meet and must say so");
            };
            assert_eq!(
                runs.domain(),
                domain::NODE,
                "a uniformly painted node counts ITSELF; a byte or character \
                 domain here would hand a client a plausible wrong index space",
            );
            let names: Vec<&str> = runs.runs().iter().map(|r| r.name).collect();
            assert_eq!(
                names,
                vec![MARK_HUE_BASE, MARK_SV_THUMB],
                "both reasons, base first — the LAST declared is what the \
                 painter obeyed",
            );
            assert_eq!(
                runs.names_at(0),
                names,
                "and the stack at the one position is that same order",
            );
            for run in runs.runs() {
                assert_eq!(
                    (run.start, run.end),
                    (0, 1),
                    "a node-domain run covers the one position there is",
                );
            }
            // The hue bar beside it is a container that was never asked, and
            // it answers with its CHANNEL rather than with silence — because
            // for a container the primary answer is still "the attribution of
            // what is in me belongs to my children", which is where a caller
            // should look next. A leaf box with no children has no such
            // redirection to offer, so an unasked one is `Silent`. Two
            // different negatives because there are two different facts.
            assert_eq!(
                scene.marks_for_tag(HUE_TAG),
                MarksLookup::NoChannel(pinion_core::marks::MarksChannel::Structural),
            );
        });
    }
}
