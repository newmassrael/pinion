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

use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::checkbox::{CheckboxEvent, CheckboxExternal, CheckboxState};
use pinion_core::{Color, Frame, Scene, WidgetCore};
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloCheckboxRenderer, HelloCheckboxRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 180;
const BG_FILL: Color = Color::rgb(0x20, 0x30, 0x40);
const BOX_SIZE: u32 = 24;
const BOX_RADIUS: u32 = 4;
const ROW_GAP: u32 = 10;

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
    let box_fill = match (state, checked) {
        // Unchecked column: transparent fill (the border + the bg
        // showing through marks the empty state). Vello's transparent
        // colour is `Color::rgba(0, 0, 0, 0)`.
        (_, false) => Color::rgba(0, 0, 0, 0),
        // Checked column: blue accent for Idle/Hover, darker for
        // Pressed, muted brown-grey for Disabled (same chromatic-mute
        // convention hello-toggle uses).
        (CheckboxState::Idle, true) => Color::rgb(0x30, 0x70, 0xd0),
        (CheckboxState::Hover, true) => Color::rgb(0x40, 0x80, 0xe0),
        (CheckboxState::Pressed, true) => Color::rgb(0x20, 0x50, 0xa0),
        (CheckboxState::Disabled, true) => Color::rgb(0x4a, 0x42, 0x38),
    };
    // Border colour mirrors the fill polarity: when checked the
    // border matches the fill (no visible outline against the same
    // colour); when unchecked a white outline marks the click target.
    // Hover slightly brightens the outline; Pressed darkens; Disabled
    // mutes.
    let border_color = match state {
        CheckboxState::Idle => Color::rgb(0xc0, 0xc0, 0xc0),
        CheckboxState::Hover => Color::rgb(0xe0, 0xe0, 0xe0),
        CheckboxState::Pressed => Color::rgb(0x90, 0x90, 0x90),
        CheckboxState::Disabled => Color::rgb(0x70, 0x66, 0x58),
    };
    let mut children: Vec<Scene> = Vec::new();
    if checked {
        let glyph_color = match state {
            CheckboxState::Disabled => Color::rgb(0xc0, 0xb0, 0x98),
            _ => Color::rgb(0xff, 0xff, 0xff),
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
        CheckboxState::Disabled => Color::rgb(0x90, 0x86, 0x78),
        _ => Color::rgb(0xe0, 0xe0, 0xe0),
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
            .with_style(BoxStyle::filled(BG_FILL))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

struct CheckboxView;

impl WidgetCore for CheckboxView {
    type State = (CheckboxState, bool);
    type Event = CheckboxEvent;

    fn create_external() -> Box<dyn External> {
        Box::new(CheckboxExternal::new())
    }

    fn tag() -> &'static str {
        "main_checkbox"
    }

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

    fn view(state: (CheckboxState, bool), frame: &Frame) -> Scene {
        view(state.0, state.1, frame)
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

    fn title() -> &'static str {
        "pinion hello-checkbox (R51.32 §5.38 pinion-shell)"
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
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str, _modifiers: pinion_core::Modifiers) -> bool {
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

    fn fmt_state_log(state: &(CheckboxState, bool)) -> String {
        format!(
            "{} / {}",
            checkbox_state_name(state.0),
            if state.1 { "checked" } else { "unchecked" },
        )
    }
}

impl WidgetA11y for CheckboxView {
    /// R51.64 §5.40 — AccessKit semantic tree contribution. Emits a
    /// single `AriaRole::CheckBox` node; `value` and `state.checked`
    /// both carry the boolean checked state (lockstep with the
    /// introspect schema's `value` key).
    ///
    /// R51.81 §5.40 — the accessible name is derived by
    /// `enrich_names_from_scene` walking the paint scene for the
    /// first non-presentational `TextNode::content`. The check-glyph
    /// `TextNode` carries `TextRole::Presentational` so it is skipped;
    /// the DFS lands on the "Receive newsletter" label naturally. No
    /// duplicate string literal lives in `access_node`.
    fn access_node(state: &(CheckboxState, bool), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, checked) = (state.0, state.1);
        let access_state = AccessState {
            focused: focused == Some(<Self as WidgetCore>::tag()),
            disabled: matches!(interaction, CheckboxState::Disabled),
            hovered: matches!(interaction, CheckboxState::Hover),
            pressed: matches!(interaction, CheckboxState::Pressed),
            checked: Some(checked),
        };
        vec![AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::CheckBox)
            .with_value(AccessValue::Bool(checked))
            .with_state(access_state)]
    }
}

impl WidgetView for CheckboxView {
    type Renderer = HelloCheckboxRenderer;

    fn initial_size() -> (u32, u32) {
        (WIN_W, WIN_H)
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
        let scene = view(s, c, &Frame::new());
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
}
