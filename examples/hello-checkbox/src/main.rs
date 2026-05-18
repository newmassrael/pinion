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
use pinion_core::{Color, Frame, Scene};
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
        children.push(Scene::Text(TextNode::styled(
            "\u{2713}",
            Rect::default(),
            TextStyle::new().with_size_px(18).with_fg(glyph_color),
        )));
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

impl WidgetView for CheckboxView {
    type State = (CheckboxState, bool);
    type Event = CheckboxEvent;
    type Renderer = HelloCheckboxRenderer;

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

    fn initial_size() -> (u32, u32) {
        (WIN_W, WIN_H)
    }

    fn keybinding(key: &str) -> Option<CheckboxEvent> {
        match key {
            "d" => Some(CheckboxEvent::Disable),
            "e" => Some(CheckboxEvent::Enable),
            _ => None,
        }
    }

    fn fmt_state_log(state: &(CheckboxState, bool)) -> String {
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
