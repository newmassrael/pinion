//! `hello-window-chrome` — R1121 §5.16 §5.39 client-side window chrome demo.
//!
//! Declares a single BORDERLESS window (`WindowSpec::with_decorations(false)`).
//! Because the OS draws no title bar, the shell injects pinion's own chrome
//! strip (title + minimize / maximize / close buttons + a drag grip) across the
//! top and insets this binding's content below it. Every chrome control is a
//! real, introspectable `Scene` node — visible in `scene/snapshot` and driven by
//! the same hit-test the live pointer uses, so an AI agent observes and clicks
//! the window controls (the §2 #7 reason custom chrome beats OS chrome).
//!
//! Run it (visible window): `cargo run -p hello-window-chrome`. Drag the dark
//! bar to move the window; the buttons at the top-right minimize / maximize /
//! close it. R1122: drag the window's side / bottom edges or bottom corners to
//! resize it (the chrome restores the drag-resize the OS frame would provide).
//! R1195: the outermost 6 px of the top edge also resize the window (VS Code /
//! Win11 — a title bar is moved from its bulk, resized from its very edge).
//! R1123: the maximize button switches
//! to the "restore" (two-square) glyph while maximized, and the resize border
//! is dropped — a maximized window fills the work area, so edge-resize is off.

use pinion_a11y::WidgetA11y;
use pinion_core::external::{External, StubExternal};
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, TextStyle,
};
use pinion_core::widgets::button::{ButtonEvent, ButtonState};
use pinion_core::{Frame, WidgetCore};
use pinion_shell::{
    SizeStrategy, WidgetView, WindowChromeStyle, WindowPolicy, WindowSpec, vello_renderer_impl,
};

// pinion-forge codegen output: `pub struct WindowChromeRenderer` + error +
// async `new` + sync `render` / `resize`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// Bridge the codegen renderer into `pinion_shell::VelloRenderer`.
vello_renderer_impl!(WindowChromeRenderer, WindowChromeRendererError);

const WIN_W: u32 = 640;
const WIN_H: u32 = 420;
const CONTENT_TAG: &str = "content";

/// view-fn (§6.3): the window's content, painted BELOW the chrome strip the
/// shell insets it under. A light surface with two centred captions so the
/// borderless body is visibly distinct from the dark chrome bar on top.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: ButtonState, _frame: &Frame) -> Scene {
    let mut head = TextStyle::new();
    head.fg_color = Color::rgb(0x20, 0x20, 0x20);
    head.font_size_px = 18;

    let mut sub = TextStyle::new();
    sub.fg_color = Color::rgb(0x55, 0x55, 0x55);
    sub.font_size_px = 13;

    let body = ContainerNode::new(vec![
        Scene::Text(TextNode::styled(
            "Borderless window — pinion draws the title bar above.",
            Rect::new(0, 0, 560, 28),
            head,
        )),
        Scene::Text(TextNode::styled(
            "Drag the dark bar to move. Minimize / maximize / close are at the top-right.",
            Rect::new(0, 0, 560, 22),
            sub,
        )),
    ])
    .with_style(BoxStyle::filled(Color::rgb(0xF2, 0xF2, 0xF2)))
    .with_layout(
        LayoutStyle::new()
            .flex(FlexDirection::Column)
            .with_justify(JustifyContent::Center)
            .with_align_items(AlignItems::Center)
            .with_gap(12),
    );
    Scene::Container(body)
}

/// Hand-rolled `WidgetView` (not the `#[widget]` macro) so it can override
/// [`WidgetView::windows`] to declare a borderless window — the trigger for
/// the shell's client-side chrome.
struct ChromeDemo;

impl WidgetCore for ChromeDemo {
    type State = ButtonState;
    type Event = ButtonEvent;
    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal)
    }
    fn tag() -> &'static str {
        CONTENT_TAG
    }
    fn read_state(_scene: &Scene) -> Self::State {
        ButtonState::Idle
    }
    fn view(state: Self::State, frame: &Frame) -> Scene {
        view(state, frame)
    }
    fn event_name(_event: Self::Event) -> &'static str {
        "__internal__"
    }
    fn title() -> &'static str {
        "pinion window chrome demo"
    }
}

impl WidgetA11y for ChromeDemo {}

impl WidgetView for ChromeDemo {
    type Renderer = WindowChromeRenderer;
    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
    fn windows() -> Vec<WindowSpec> {
        // R1121.1 — two ORTHOGONAL declarations: `decorations(false)` turns off
        // the OS frame, and `window_chrome` (below) turns ON pinion's own
        // chrome. A naked borderless window would set the first and omit the
        // second.
        vec![
            WindowSpec::new(
                "main",
                "pinion window chrome demo",
                SizeStrategy::Fixed {
                    width: WIN_W,
                    height: WIN_H,
                },
            )
            .with_decorations(false),
        ]
    }
    fn window_policy(_window_id: &str) -> WindowPolicy {
        WindowPolicy::new().with_chrome(WindowChromeStyle::default())
    }
}

fn main() {
    pinion_shell::run::<ChromeDemo>();
}
