//! `hello-elevation` — R710 §5.50 consumer of the drop-shadow
//! rendering substrate (the first `BoxShadow` paint).
//!
//! ## What this demonstrates
//!
//! `BoxStyle::shadows` (R710) carries a `Vec<BoxShadow>` painted behind
//! a box — the CSS `box-shadow` / Flutter `List<BoxShadow>` model. Each
//! `BoxShadow` (colour, offset, blur, spread) lowers to Vello's
//! native blurred-rounded-rect primitive. This binding paints a
//! **Material-style elevation gallery**:
//!
//! * three static cards (`card_low` / `card_mid` / `card_high`) at
//!   rising elevation levels, each casting a *key + ambient* shadow
//!   pair — the reason the substrate is a `Vec` and not a single
//!   `Option<BoxShadow>` (Material elevation composes two casts);
//! * an interactive **raise card** (`tag = "main_toggle"`) that the
//!   Toggle lifts between the resting (level 1) and raised (level 5)
//!   elevation, exercising both shadow lists and proving the §5.16 R682
//!   paint-cache re-keys when a shadow changes (the manual
//!   `Hash for BoxStyle` folds the shadow list in).
//!
//! The elevation ramp (`pinion_widget_paint::elevation`, R711 lift) is a
//! *Material-style* key + ambient model parameterised by level — a
//! design choice, not a claim of bit-exact MD3 dp tokens; the substrate
//! under test is the generic `BoxShadow` primitive (cf. R708:
//! `Gradient` is the substrate, the hue ramp is the consumer's choice).
//!
//! ## Why a Toggle
//!
//! An elevation gallery is intrinsically stateless, but the `AppShell`
//! drives a `WidgetView` with a statechart `External`. Rather than
//! invent a one-off widget ([[abstraction-needs-second-consumer]]), the
//! binding reuses the §5.38 [`ToggleExternal`] purely as the
//! *rest/raised bit*. The Toggle is the RPC-drivable, AT-exposed
//! control; the shadows are the substrate under test.
//!
//! ## Verification (substrate-first)
//!
//! * `scene/snapshot` exposes every card's `shadows` list as structured
//!   data — `tools/demos/r710_elevation.py` reads back the per-card
//!   blur / offset / colour and watches the raise card's list change on
//!   toggle, all without OCR (§2 #7, [[ai-first-rpc-introspection-obligation]]).
//! * the same demo captures the live window (`PINION_SCREENSHOT`) and
//!   samples the background just below each card — the cast darkens the
//!   surface monotonically with elevation, the live-pixel guard a
//!   structural query cannot replace ([[introspection-from-paint-not-screen]],
//!   R706/R707.3/R708/R709 precedent).

#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_core::external::IntrospectValue;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{ColorRole, Frame, Scene, WidgetStateName, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;
// R711 §5.50 — the lifted shared elevation ramp (this binding is one of
// four consumers: dialog / menu / drawer panels + this gallery).
use pinion_widget_paint::elevation::elevation;

// pinion-forge codegen output: `pub struct HelloElevationRenderer` +
// `HelloElevationRendererError` + async `new<...>` + sync `render` /
// `resize`. R46.3.3 emit template uses fully-qualified `::vello::*`
// paths so the include is bare.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so `AppShell<V>` can build /
// render / resize it. Identical pattern to hello-gradient.
vello_renderer_impl!(HelloElevationRenderer, HelloElevationRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 440;

const THEME_TAG: &str = "app";

// Card geometry. The vertical gap between cards is wide enough that a
// level-5 cast lands in the background gap below its card (where the
// pixel guard samples) without reaching the next card.
const CARD_W: u32 = 200;
const CARD_H: u32 = 40;
const CARD_RADIUS: u32 = 12;
const COLUMN_GAP: u32 = 30;

const TITLE_FONT_PX: u32 = 18;
const CARD_FONT_PX: u32 = 14;
const STATUS_FONT_PX: u32 = 12;

// Static gallery elevation levels.
const LEVEL_LOW: u8 = 1;
const LEVEL_MID: u8 = 3;
const LEVEL_HIGH: u8 = 5;
// Interactive raise-card endpoints.
const REST_LEVEL: u8 = 1;
const RAISED_LEVEL: u8 = 5;

/// Material 3 state-layer overlay weights (linear-sRGB lerp toward
/// [`ColorRole::OnSurface`]) for the raise-card chrome — 8 % hover /
/// 12 % pressed, matching the rest of the widget gallery.
const HOVER_OVERLAY_T: f32 = 0.08;
const PRESSED_OVERLAY_T: f32 = 0.12;
const DISABLED_OVERLAY_T: f32 = 0.50;

// ── Material-style elevation ramp ──────────────────────────────────
// The per-level key + ambient shadow list now lives in the shared
// `pinion_widget_paint::elevation` module (R711 lift). This gallery is
// one of its four consumers (the others are the dialog / menu / drawer
// panels), so the ramp has a single authoritative home rather than a
// per-binding copy ([[abstraction-needs-second-consumer]]).

/// One labelled elevation card: a rounded surface chip casting
/// `elevation(level)`. `tag` makes the card addressable for
/// `scene/snapshot` so the demo can read back its shadow list and rect.
fn elevation_card(tag: &'static str, level: u8, label: &str, fill: Color, fg: Color) -> Scene {
    let text = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new().with_size_px(CARD_FONT_PX).with_fg(fg),
    ));
    Scene::Container(
        ContainerNode::new(vec![text])
            .with_tag(tag)
            .with_style(
                BoxStyle::filled(fill)
                    .with_corner_radius(CARD_RADIUS)
                    .with_shadows(elevation(level)),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(CARD_W, CARD_H)),
            ),
    )
}

/// view-fn (§6.3): pure sync mapping `(ToggleState, bool) -> Scene`.
/// `on` lifts the raise card from the resting to the raised elevation.
/// `&Frame` is the §6.3 ZST hedge. The shell calls `compute_layout`
/// before paint, so the view need not resolve rects.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ToggleState, on: bool, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let card_fill = theme.resolve(ColorRole::SurfaceContainerHighest);

    let title = Scene::Text(TextNode::styled(
        "Elevation gallery",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_fg(on_surface),
    ));

    // Static gallery — three rising elevations, each a key + ambient
    // cast (Vec<BoxShadow>). The cards are pure introspectable visuals;
    // only the raise card below is a hit target.
    let card_low = elevation_card("card_low", LEVEL_LOW, "Level 1", card_fill, on_surface);
    let card_mid = elevation_card("card_mid", LEVEL_MID, "Level 3", card_fill, on_surface);
    let card_high = elevation_card("card_high", LEVEL_HIGH, "Level 5", card_fill, on_surface);

    // Interactive raise card — the Toggle hit handle (`main_toggle`).
    // Off rests at level 1; On lifts to level 5. Hover / Pressed lerp
    // the chip fill toward on-surface at the M3 state-layer weights.
    let raise_level = if on { RAISED_LEVEL } else { REST_LEVEL };
    let raise_fill: Color = match state {
        ToggleState::Idle => card_fill,
        ToggleState::Hover => card_fill.lerp(on_surface, HOVER_OVERLAY_T),
        ToggleState::Pressed => card_fill.lerp(on_surface, PRESSED_OVERLAY_T),
        ToggleState::Disabled => card_fill.lerp(surface, DISABLED_OVERLAY_T),
    };
    let raise_label = Scene::Text(TextNode::styled(
        if on { "Raised" } else { "Resting" },
        Rect::default(),
        TextStyle::new()
            .with_size_px(CARD_FONT_PX)
            .with_fg(on_surface),
    ));
    let raise_card = Scene::Container(
        ContainerNode::new(vec![raise_label])
            .with_tag("main_toggle")
            .with_aria_label("Card elevation")
            .with_style(
                BoxStyle::filled(raise_fill)
                    .with_corner_radius(CARD_RADIUS)
                    .with_shadows(elevation(raise_level)),
            )
            .with_layout(
                LayoutStyle::new()
                    .with_focusable(true)
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(CARD_W, CARD_H)),
            ),
    );

    let status = Scene::Text(TextNode::styled(
        format!(
            "{} | {}",
            state.as_name(),
            if on { "Raised (5)" } else { "Resting (1)" }
        ),
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    Scene::Container(
        ContainerNode::new(vec![
            title, card_low, card_mid, card_high, raise_card, status,
        ])
        .with_style(BoxStyle::filled(surface))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_gap(COLUMN_GAP),
        ),
    )
}

/// `WidgetView` binding. The §5.38 Toggle is reused as the rest/raised
/// elevation bit (see the module doc); the `#[widget]` attribute derives
/// the mechanical [`WidgetCore`] / [`WidgetA11y`] / [`WidgetView`] trio.
/// No `update` reducer — the Off/On bit is read straight in [`view`].
///
/// [`WidgetCore`]: pinion_core::WidgetCore
/// [`WidgetA11y`]: pinion_a11y::WidgetA11y
/// [`WidgetView`]: pinion_shell::WidgetView
#[widget(
    tag = "main_toggle",
    state = (ToggleState, bool),
    event = ToggleEvent,
    title = "pinion hello-elevation (R710 §5.50 drop-shadow substrate)",
    renderer = HelloElevationRenderer,
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
    apply_key = aria_activate,
    keybinding,
    event_name_derive,
)]
struct ElevationView;

impl ElevationView {
    /// Tuple-state introspect: SCXML state name via `query("state")` +
    /// the rest/raised sidecar via `query("value")`. Defaults to
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

    fn keybinding(key: &str) -> Option<ToggleEvent> {
        match key {
            "d" => Some(ToggleEvent::Disable),
            "e" => Some(ToggleEvent::Enable),
            _ => None,
        }
    }
}

fn main() {
    pinion_shell::run::<ElevationView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    /// Walk the scene for the first `Scene::Container` carrying `tag`.
    fn find_container<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        match scene {
            Scene::Container(c) if c.tag.as_deref() == Some(tag) => Some(c),
            Scene::Container(c) => c.children.iter().find_map(|ch| find_container(ch, tag)),
            _ => None,
        }
    }

    fn rendered(state: ToggleState, on: bool) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(state, on, &Frame::new()))
    }

    // The ramp internals (empty at 0, key+ambient pair, blur scales with
    // level) are tested at the SSOT in `pinion_widget_paint::elevation`;
    // this binding's tests focus on the consumer wiring.

    #[test]
    fn static_cards_carry_rising_shadow_lists() {
        let scene = rendered(ToggleState::Idle, false);
        for tag in ["card_low", "card_mid", "card_high"] {
            let card = find_container(&scene, tag).unwrap_or_else(|| panic!("{tag} present"));
            assert_eq!(card.style.shadows.len(), 2, "{tag} casts key + ambient");
        }
        let low = find_container(&scene, "card_low").unwrap();
        let high = find_container(&scene, "card_high").unwrap();
        assert!(
            high.style.shadows[0].blur > low.style.shadows[0].blur,
            "card_high casts a larger key shadow than card_low",
        );
    }

    #[test]
    fn raise_card_lifts_from_level_1_to_level_5() {
        let rest = rendered(ToggleState::Idle, false);
        let raised = rendered(ToggleState::Idle, true);
        let rest_card = find_container(&rest, "main_toggle").expect("raise card present");
        let raised_card = find_container(&raised, "main_toggle").expect("raise card present");
        assert!(
            raised_card.style.shadows[0].blur > rest_card.style.shadows[0].blur,
            "raising the card grows its key shadow",
        );
        // Both elevations are the documented endpoints.
        assert_eq!(rest_card.style.shadows, elevation(REST_LEVEL));
        assert_eq!(raised_card.style.shadows, elevation(RAISED_LEVEL));
    }

    #[test]
    fn raise_changes_re_key_the_paint_hash() {
        // The §5.16 R682 paint-cache keys off `Scene::paint_hash`, which
        // folds `BoxStyle::shadows` in via the manual `Hash for
        // BoxStyle`. Resting vs raised must therefore hash differently
        // or the cache would replay a stale fragment.
        let rest = rendered(ToggleState::Idle, false);
        let raised = rendered(ToggleState::Idle, true);
        assert_ne!(rest.paint_hash(), raised.paint_hash());
    }

    #[test]
    fn r55_g20_view_carries_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<ElevationView>(
            (ToggleState::Idle, false),
            &Frame::new(),
        );
    }

    #[test]
    fn raise_card_reports_switch_role() {
        let nodes = <ElevationView as WidgetA11y>::access_node(&(ToggleState::Idle, true), None);
        assert_eq!(nodes[0].role, AriaRole::Switch);
        assert_eq!(nodes[0].tag, "main_toggle");
    }
}
