//! `hello-checkbox` — R51.32 §5.38 paint-side N=3 amortization
//! evidence on the pinion-shell substrate (R51.30).
//!
//! Pre-R51.30 a 3rd visual binary would have repeated the ~650 LOC
//! App boilerplate of hello-button + hello-toggle. With pinion-shell
//! in place this binary is ~120 LOC: the view fn (checkbox + label
//! row), the `CheckboxView` `WidgetView` impl, the renderer trait
//! bridge, and a 3-line `main`. The framework primitive amortizes
//! the third time exactly as the R51.30 changelog predicted: no
//! further substrate refactor was required.
//!
//! Visual contract: a 24×24 rounded square with the (state, checked)
//! cross-product encoded as fill + optional inner `\u{2713}` (CHECK
//! MARK) glyph, followed by a label. The check glyph is rendered
//! through `Scene::Text` via the parley/swash shaping pipeline that
//! the shell threads through the shared `LayoutCache` — no
//! special-case bitmap or SVG path, just Unicode + parley.

use pinion_core::external::IntrospectValue;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widgets::checkbox::{CheckboxEvent, CheckboxExternal, CheckboxState};
use pinion_core::{Color, Frame, Scene, WidgetCore};
#[cfg(test)]
use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloCheckboxRenderer, HelloCheckboxRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 180;
/// (R57.X.checkbox §5.50) [`ThemeProvider`] cache key — matches the
/// `"app"` convention shared across the example gallery.
const THEME_TAG: &str = "app";
const BOX_SIZE: u32 = 24;
const BOX_RADIUS: u32 = 4;
const ROW_GAP: u32 = 10;

/// (R57.X.checkbox §5.50) Material 3 Checkbox accent ramp — when
/// checked, the box fill resolves to `ColorRole::Accent` (idle) and
/// layers M3 state-layer overlays (hover = 8 %, pressed = 12 %,
/// disabled = 38 % fade toward `Surface`).
fn checkbox_accent_for(theme: &Theme, state: CheckboxState) -> Color {
    let base = theme.resolve(ColorRole::Accent);
    match state {
        CheckboxState::Idle => base,
        CheckboxState::Hover => base.lerp(theme.resolve(ColorRole::OnSurface), 0.08),
        CheckboxState::Pressed => base.lerp(theme.resolve(ColorRole::OnSurface), 0.12),
        CheckboxState::Disabled => base.lerp(theme.resolve(ColorRole::Surface), 0.38),
    }
}

/// (R57.X.checkbox §5.50) Border color — anchors on
/// `ColorRole::Outline` with the same state-layer treatment as the
/// accent ramp so checked + unchecked share the M3 overlay weights.
fn checkbox_outline_for(theme: &Theme, state: CheckboxState) -> Color {
    let base = theme.resolve(ColorRole::Outline);
    match state {
        CheckboxState::Idle => base,
        CheckboxState::Hover => base.lerp(theme.resolve(ColorRole::OnSurface), 0.08),
        CheckboxState::Pressed => base.lerp(theme.resolve(ColorRole::OnSurface), 0.12),
        CheckboxState::Disabled => base.lerp(theme.resolve(ColorRole::Surface), 0.38),
    }
}

/// view-fn (§6.3): pure sync mapping `(CheckboxState, bool) -> Scene`.
///
/// Layout: a horizontal row [checkbox-box] [label] centered in the
/// window. The checkbox visual is a `Container` (not a `Box`) so the
/// optional `\u{2713}` glyph can render as a centered child when the
/// `checked` value is true. The container carries the dispatch tag
/// `main_checkbox` so the shell's `InputRouter` routes pointer
/// events to the matching `Scene::External("main_checkbox")`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: CheckboxState, checked: bool, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let box_fill = if checked {
        checkbox_accent_for(&theme, state)
    } else {
        // (R57.X.checkbox §5.50) Unchecked column: transparent fill
        // so the border + parent surface mark the empty state.
        Color::TRANSPARENT
    };
    let border_color = checkbox_outline_for(&theme, state);
    let mut children: Vec<Scene> = Vec::new();
    if checked {
        let glyph_color = match state {
            CheckboxState::Disabled => theme.resolve(ColorRole::OnSurfaceMuted),
            _ => theme.resolve(ColorRole::OnAccent),
        };
        children.push(Scene::Text(
            // R51.81 §5.40 — the check-glyph is pure decoration
            // (visual mark for the `checked` state). `Presentational`
            // tells `enrich_names_from_scene` to skip it so the AT
            // exposes the linguistic label ("Receive newsletter")
            // instead of "✓". Replaces the pre-R51.81 `aria_label`
            // Band-Aid on the outer `ContainerNode`.
            TextNode::styled(
                "\u{2713}",
                Rect::default(),
                TextStyle::new().with_size_px(18).with_fg(glyph_color),
            )
            .with_role(pinion_core::scene::TextRole::Presentational),
        ));
    }
    let box_visual = Scene::Container(
        ContainerNode::new(children)
            .with_style(
                BoxStyle::filled(box_fill)
                    .with_corner_radius(BOX_RADIUS)
                    .with_border(Border::new(border_color, 2)),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(BOX_SIZE, BOX_SIZE)),
            ),
    );
    let label_color = match state {
        CheckboxState::Disabled => theme.resolve(ColorRole::OnSurfaceMuted),
        _ => theme.resolve(ColorRole::OnSurface),
    };
    let label = Scene::Text(TextNode::styled(
        "Receive newsletter",
        Rect::default(),
        TextStyle::new().with_size_px(16).with_fg(label_color),
    ));
    let row = Scene::Container(
        ContainerNode::new(vec![box_visual, label])
            .with_tag("main_checkbox")
            // R51.81 §5.40 — the check-glyph TextNode declares
            // `TextRole::Presentational`, so `enrich_names_from_scene`
            // skips past it and lands on the "Receive newsletter"
            // label naturally. No `aria_label` override needed: the
            // role marker is the textbook fix for in-scene
            // decoration glyphs.
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(ROW_GAP),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![row])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// R654 §5.16 Cat A cascade retrofit. The R653-extended `#[widget]`
/// derives the mechanical `WidgetCore` / `WidgetA11y` / `WidgetView`
/// trio (`state_flags(checked = bool_field(1))` + `access_value =
/// bool_field(1)`); only the binding-specific methods (`view` /
/// `read_state` / `event_name` / `keybinding` / `apply_key` /
/// `fmt_state_log`) stay as inherent fns the macro forwards to.
/// No reducer — Checkbox carries no theme-flip side effect (vs
/// hello-toggle / hello-theme which do via `update` flag).
#[widget(
    tag = "main_checkbox",
    state = (CheckboxState, bool),
    event = CheckboxEvent,
    title = "pinion hello-checkbox (R654 §5.16 #[widget] retrofit)",
    renderer = HelloCheckboxRenderer,
    initial_size = (WIN_W, WIN_H),
    external = CheckboxExternal::new,
    role = CheckBox,
    state_flags(
        hovered = Hover,
        pressed = Pressed,
        disabled = Disabled,
        checked = bool_field(1),
    ),
    access_value = bool_field(1),
    apply_key,
    keybinding,
    fmt_state_log,
)]
struct CheckboxView;

impl CheckboxView {
    fn read_state(scene: &Scene) -> (CheckboxState, bool) {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                let state = if let Some(IntrospectValue::Text(name)) = intro.query("state") {
                    parse_checkbox_state(&name)
                } else {
                    CheckboxState::Idle
                };
                let checked = matches!(intro.query("checked"), Some(IntrospectValue::Bool(true)));
                return (state, checked);
            }
        }
        (CheckboxState::Idle, false)
    }

    fn view(state: (CheckboxState, bool), frame: Frame) -> Scene {
        view(state.0, state.1, &frame)
    }

    fn event_name(event: CheckboxEvent) -> &'static str {
        match event {
            CheckboxEvent::PointerEnter => "PointerEnter",
            CheckboxEvent::PointerLeave => "PointerLeave",
            CheckboxEvent::PointerDown => "PointerDown",
            CheckboxEvent::PointerUp => "PointerUp",
            CheckboxEvent::Disable => "Disable",
            CheckboxEvent::Enable => "Enable",
            _ => "__internal__",
        }
    }

    fn keybinding(key: &str) -> Option<CheckboxEvent> {
        match key {
            "d" => Some(CheckboxEvent::Disable),
            "e" => Some(CheckboxEvent::Enable),
            _ => None,
        }
    }

    /// R51.55 §5.39 — ARIA Checkbox keyboard activation. Space on
    /// the focused checkbox fires `KeyboardActivate`, which flips
    /// the checked sidecar and emits the `"checked"` intent in
    /// parity with a pointer click. Pure ARIA checkboxes accept
    /// only Space (Enter is reserved for form submit in the broader
    /// ARIA model) — Enter does not reach this hook.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(Self::tag()) {
            return false;
        }
        if key != "Space" {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        intro
            .invoke("send", IntrospectValue::Text("KeyboardActivate".to_string()))
            .is_ok()
    }

    fn fmt_state_log(state: (CheckboxState, bool)) -> String {
        format!(
            "{} / {}",
            checkbox_state_name(state.0),
            if state.1 { "checked" } else { "unchecked" },
        )
    }
}

fn parse_checkbox_state(name: &str) -> CheckboxState {
    match name {
        "Hover" => CheckboxState::Hover,
        "Pressed" => CheckboxState::Pressed,
        "Disabled" => CheckboxState::Disabled,
        _ => CheckboxState::Idle,
    }
}

fn checkbox_state_name(state: CheckboxState) -> &'static str {
    match state {
        CheckboxState::Idle => "Idle",
        CheckboxState::Hover => "Hover",
        CheckboxState::Pressed => "Pressed",
        CheckboxState::Disabled => "Disabled",
    }
}

fn main() {
    pinion_shell::run::<CheckboxView>();
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    fn enriched(state: (CheckboxState, bool), focused: Option<&str>) -> Vec<AccessNode> {
        let (s, c) = state;
        // (R57.X.checkbox §5.50) `view` calls [`use_theme`] so the
        // call must run inside an Owner scope (callback-root-owner-
        // wrap discipline).
        let scene = pinion_core::Owner::new().run(|| view(s, c, &Frame::new()));
        let mut nodes = CheckboxView::access_node(&state, focused);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        nodes
    }

    #[test]
    fn unchecked_idle_emits_checkbox_role() {
        let nodes = enriched((CheckboxState::Idle, false), None);
        assert_eq!(nodes[0].role, AriaRole::CheckBox);
        assert_eq!(nodes[0].name.as_deref(), Some("Receive newsletter"));
        assert_eq!(nodes[0].state.checked, Some(false));
    }

    #[test]
    fn checked_idle_value_and_state_align() {
        let nodes = CheckboxView::access_node(&(CheckboxState::Idle, true), None);
        assert_eq!(nodes[0].value, Some(AccessValue::Bool(true)));
        assert_eq!(nodes[0].state.checked, Some(true));
    }

    #[test]
    fn hover_state_does_not_change_checked() {
        let nodes = CheckboxView::access_node(&(CheckboxState::Hover, true), None);
        assert!(nodes[0].state.hovered);
        assert_eq!(nodes[0].state.checked, Some(true));
    }

    #[test]
    fn focused_tag_sets_focused_flag() {
        let nodes = CheckboxView::access_node(
            &(CheckboxState::Idle, false),
            Some("main_checkbox"),
        );
        assert!(nodes[0].state.focused);
    }

    #[test]
    fn role_marker_skips_check_glyph_when_checked() {
        // R51.81 §5.40 — the check-glyph TextNode carries
        // `TextRole::Presentational` so `enrich_names_from_scene`
        // skips it during the DFS first-text scan and lands on the
        // linguistic "Receive newsletter" label. Pre-R51.81 the
        // glyph would have been picked first; an `aria_label`
        // override masked it as a Band-Aid (now removed).
        let nodes = enriched((CheckboxState::Idle, true), None);
        assert_eq!(
            nodes[0].name.as_deref(),
            Some("Receive newsletter"),
            "role marker must keep the check-glyph TextNode out of the DFS chain",
        );
    }

    #[test]
    fn r55_g20_view_contains_composite_paint_root_tag() {
        // R55.G.20 §5.49 — paint scene must carry the composite
        // `WidgetCore::tag()` so AI-side `{path: "main_checkbox"}`
        // input routing and `rect_for_tag` AT bounds attach resolve.
        //
        // R55.G.22 §5.49 — pinned via the framework helper which
        // calls `V::view` under an `Owner::new()` scope and asserts
        // `Scene::contains_tag(V::tag())`.
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<CheckboxView>(
            (CheckboxState::Idle, false),
            &Frame::new(),
        );
    }

    // ─────────────────────────────────────────────────────────────
    // R57.X.checkbox §5.50 — theme retrofit regression. Pins the
    // M3 Checkbox role mapping: checked=Accent ramp, border=Outline
    // ramp, both with state-layer overlays.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r57_x_checkbox_checked_idle_uses_accent_role() {
        let light = Theme::light();
        assert_eq!(
            checkbox_accent_for(&light, CheckboxState::Idle),
            light.accent,
        );
    }

    #[test]
    fn r57_x_checkbox_unchecked_outline_uses_outline_role() {
        let light = Theme::light();
        assert_eq!(
            checkbox_outline_for(&light, CheckboxState::Idle),
            light.outline,
        );
    }

    #[test]
    fn r57_x_checkbox_hover_overlay_lerps_toward_on_surface() {
        let theme = Theme::light();
        let expected = theme
            .resolve(ColorRole::Accent)
            .lerp(theme.resolve(ColorRole::OnSurface), 0.08);
        assert_eq!(
            checkbox_accent_for(&theme, CheckboxState::Hover),
            expected,
        );
    }
}
