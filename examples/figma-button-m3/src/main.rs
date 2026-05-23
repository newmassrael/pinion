//! `figma-button-m3` — R635 §5.7 first Figma → pinion design-parity
//! binding.
//!
//! Re-creates the Material 3 "Filled / Enabled" Button variant from
//! the Material 3 Design Kit Community Figma file (node `51553:5180`)
//! as a static pinion paint scene. Exact transcription:
//!
//! - 109 × 40 px button rect
//! - fill: `#675AA4` (M3 Primary; Figma `r=0.4039 g=0.3137 b=0.6431 a=1.0`)
//! - `cornerRadius: 100` (fully rounded — M3 default for filled buttons)
//! - HORIZONTAL auto-layout, padding L=16, R=16, T=10, B=10
//! - label: `"Button"` Roboto Medium 14px white (leading + trailing
//!   18×18 icon slots elided to placeholders — Figma `INSTANCE`
//!   nodes are external-component references that need a glyph /
//!   svg substrate; tracked in the R636+ axis queue)
//! - state-layer overlay (white, opacity = 0 at rest) elided —
//!   M3 hover / pressed visibility, irrelevant for the Enabled
//!   first-frame snapshot
//!
//! Spec was extracted via `pinion figma-verify qluPDRsuDuPM3deySb0ejR`
//! (R634), then narrowed by node id to the single COMPONENT under
//! the `Buttons` frame. See `tools/figma-spec/button-m3.md` (TBD)
//! for the full mapping table when R636+ lands the substrate gap
//! audit.
//!
//! ## R635 scope (intentionally static)
//!
//! The binding ignores Button SCXML state — `read_state` always
//! returns `ButtonState::Idle`, the view fn paints the same Filled
//! / Enabled Scene regardless. The first design-parity loop is
//! about *pixel reproduction*, not interactivity; hover / pressed /
//! disabled variants are separate Figma COMPONENT nodes that lands
//! as their own bindings once the Filled / Enabled spec verifies
//! end-to-end.
//!
//! ## R637 screenshot capture
//!
//! ```sh
//! PINION_SCREENSHOT=/tmp/pinion-btn.png cargo run -p figma-button-m3
//! ```
//!
//! The R637 `pinion_shell::run` env hook bypasses winit, drives the
//! initial paint scene through the wgpu + vello headless renderer,
//! and writes the PNG. Pair with the R636 reference fetch
//! (`pinion figma-fetch-image qluPDRsuDuPM3deySb0ejR 51553:5180 -o
//! /tmp/figma-btn-ref.png --scale 2`) for the side-by-side diff
//! R638 `pinion figma-diff` will consume.

use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Color, Frame, Scene, WidgetCore};
use pinion_a11y::{AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_shell::{vello_renderer_impl, WidgetView};

// pinion-forge codegen output. Defines `pub struct FigmaButtonM3Renderer`
// + `pub enum FigmaButtonM3RendererError` plus the Vello-backed
// async `new` / sync `render` / sync `resize` methods.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so the generic `AppShell<V>`
// can construct + render + resize.
vello_renderer_impl!(FigmaButtonM3Renderer, FigmaButtonM3RendererError);

// Window slightly larger than the button itself so the surrounding
// canvas has breathing room for the design-parity inspection.
const WIN_W: u32 = 320;
const WIN_H: u32 = 160;

/// Figma Filled / Enabled Button spec — exact transcription from the
/// Material 3 Design Kit Community file (node `51553:5180`). Each
/// `const` mirrors a single Figma field so a future spec drift
/// surfaces as a single-line diff here.
const BTN_W: u32 = 109;
const BTN_H: u32 = 40;
const BTN_RADIUS: u32 = 100;
// Figma color: r=0.4039 g=0.3137 b=0.6431 → (103, 80, 164) sRGB byte
// per the standard 255× quantization R628 `quantize_unit_byte` uses
// internally; the trio matches the M3 token `Primary` (#675AA4).
const BTN_FILL: Color = Color::rgb(103, 80, 164);
const LABEL_FG: Color = Color::rgb(255, 255, 255);
const LABEL_PX: u32 = 14;
// Background canvas tone behind the button — neutral dark so the
// `#675AA4` button contrasts visually during the inspection. Not a
// Figma spec value; pinion-side framing only.
const CANVAS_BG: Color = Color::rgb(0x1F, 0x1F, 0x1F);

/// R635 §5.7 — paint the Filled / Enabled Button.
///
/// `_state` and `_frame` are unused (R635a is static; see module
/// docs). [`Frame`] is `Copy`, so the free fn takes it by value
/// per the workspace `clippy::pedantic` `trivially_copy_pass_by_ref`
/// rule; the `WidgetCore::view` trait shim below dereferences the
/// `&Frame` argument when forwarding.
fn view(_state: ButtonState, _frame: Frame) -> Scene {
    let label = Scene::Text(TextNode::styled(
        "Button",
        Rect::default(),
        TextStyle::new().with_size_px(LABEL_PX).with_fg(LABEL_FG),
    ));
    let button = Scene::Container(
        ContainerNode::new(vec![label])
            // Framework dispatch identifier — matches `WidgetCore::tag()`
            // so the shell's `InputRouter` can hit-test pointer events
            // (currently no-op since R635a is non-interactive).
            .with_tag("figma_button_m3")
            .with_style(BoxStyle::filled(BTN_FILL).with_corner_radius(BTN_RADIUS))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(BTN_W, BTN_H))
                    .with_padding(Rect::new(16, 10, 16, 10)),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![button])
            .with_style(BoxStyle::filled(CANVAS_BG))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// Unit struct carrying the `WidgetCore` / `WidgetA11y` / `WidgetView`
/// impls. Per the R51.30 shell contract every method is associated
/// (`fn`, not `&self`) so the shell instantiates `AppShell<FigmaButtonView>`
/// without holding a value of this type.
struct FigmaButtonView;

impl WidgetCore for FigmaButtonView {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn tag() -> &'static str {
        "figma_button_m3"
    }

    /// R635a is static — always report `Idle` so the view fn paints
    /// the Filled / Enabled rendering regardless of pointer events.
    fn read_state(_scene: &Scene) -> ButtonState {
        ButtonState::Idle
    }

    fn view(state: ButtonState, frame: &Frame) -> Scene {
        view(state, *frame)
    }

    /// R635a does not route any events; the shell still requires the
    /// trait method, so the sentinel `"__noop__"` makes any accidental
    /// dispatch a no-op in the SCXML side.
    fn event_name(_event: ButtonEvent) -> &'static str {
        "__noop__"
    }

    fn title() -> &'static str {
        "Figma Material 3 Filled Button (R635)"
    }

    fn keybinding(_key: &str) -> Option<ButtonEvent> {
        None
    }

    fn apply_key(
        _scene: &mut Scene,
        _focused: Option<&str>,
        _key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        false
    }
}

impl WidgetA11y for FigmaButtonView {
    /// R635a — single `AriaRole::Button` AT node. `focused` flag is
    /// the only state surfaced; hover / pressed / disabled are
    /// intentionally clamped to false to mirror the static
    /// Filled / Enabled rendering.
    fn access_node(_state: &ButtonState, focused: Option<&str>) -> Vec<AccessNode> {
        let focused_here = focused == Some(<Self as WidgetCore>::tag());
        let access_state = AccessState {
            focused: focused_here,
            disabled: false,
            hovered: false,
            pressed: false,
            checked: None,
        };
        vec![
            AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Button)
                .with_state(access_state),
        ]
    }
}

impl WidgetView for FigmaButtonView {
    type Renderer = FigmaButtonM3Renderer;

    fn initial_size() -> (u32, u32) {
        (WIN_W, WIN_H)
    }
}

fn main() {
    pinion_shell::run::<FigmaButtonView>();
}
