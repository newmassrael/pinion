//! `hello-pdf-export` — R908 §3 §5.53 consumer of the own-renderer
//! Scene → PDF pipeline.
//!
//! ## What this demonstrates
//!
//! pinion paints its own UI from a structured scene (inv #1: no opaque
//! paint callbacks), so it can render that scene to a *vector* PDF
//! without a rasterizer. This binding paints a document-style scene — a
//! title, body text, three primary-colour rounded swatches, a bordered
//! caption box, and a watermark a §5.38 [`ToggleExternal`] flips between
//! `DRAFT` and `FINAL` — and an AI client exports it to PDF over
//! `scene/export_pdf`, then reads the document's structure (page box,
//! object tree, fill operators, embedded text) straight back — no
//! pixels, the channel `scene/screenshot` cannot give until the §5.16
//! RHI ships.
//!
//! ## Why a Toggle
//!
//! The export is intrinsically stateless, but the `AppShell` drives a
//! [`WidgetView`] with a statechart `External`. Rather than invent a
//! one-off widget ([[abstraction-needs-second-consumer]]), the binding
//! reuses the Toggle purely as the *RPC-drivable repaint trigger* (the
//! hello-frame-profiler / hello-elevation Toggle-as-bit pattern). Each
//! flip changes the watermark text, so the demo can export, flip, export
//! again, and confirm the PDF tracks the live scene.
//!
//! ## Verification
//!
//! `tools/demos/r908_pdf_export.py` reads `scene/export_pdf` over RPC and
//! parses the returned document: the `%PDF` header + `%%EOF` trailer,
//! the `MediaBox` matching the page size, the catalog / pages / page /
//! font object tree, the `xref` table, the primary-colour fill
//! operators, the bordered box's stroke, and the watermark text — all
//! deterministic structure on a pure `Scene → bytes` function
//! ([[introspection-from-paint-not-screen]],
//! [[ai-first-rpc-introspection-obligation]]).

use pinion_core::external::IntrospectValue;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, FlexDirection, FontWeight, JustifyContent, LayoutStyle,
    Size, TextStyle,
};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{ColorRole, Frame, Scene, WidgetCore, WidgetStateName, use_theme};
#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

// pinion-forge codegen output: `pub struct HelloPdfExportRenderer` +
// error + async `new` + sync `render` / `resize`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so `AppShell<V>` can drive it.
vello_renderer_impl!(HelloPdfExportRenderer, HelloPdfExportRendererError);

const WIN_W: u32 = 420;
const WIN_H: u32 = 540;

const THEME_TAG: &str = "app";
const REPAINT_TAG: &str = "repaint";

const TITLE_FONT_PX: u32 = 22;
const BODY_FONT_PX: u32 = 13;
const CAPTION_FONT_PX: u32 = 12;
const WATERMARK_FONT_PX: u32 = 16;

const SWATCH_W: u32 = 96;
const SWATCH_H: u32 = 44;
const SWATCH_RADIUS: u32 = 8;

/// Material 3 state-layer overlay weights for the repaint chip chrome.
const HOVER_OVERLAY_T: f32 = 0.08;
const PRESSED_OVERLAY_T: f32 = 0.12;
const DISABLED_OVERLAY_T: f32 = 0.50;

/// Primary-colour swatches — pure channels so the exported `DeviceRGB`
/// fill operators are exact (`1 0 0 rg` / `0 1 0 rg` / `0 0 1 rg`),
/// which the demo asserts verbatim.
const SWATCH_COLORS: [Color; 3] = [
    Color::rgb(0xff, 0x00, 0x00),
    Color::rgb(0x00, 0xff, 0x00),
    Color::rgb(0x00, 0x00, 0xff),
];

/// Body text lines — fixed `&'static str`s (no per-paint `format!` +
/// leak; the view fn runs every frame).
const BODY_LINES: [&str; 3] = [
    "Vector export walks the structured scene,",
    "not a pixel buffer: rects, text, and color",
    "become PDF drawing operators directly.",
];

/// One coloured rounded swatch.
fn swatch(fill: Color) -> Scene {
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_style(BoxStyle::filled(fill).with_corner_radius(SWATCH_RADIUS))
            .with_layout(LayoutStyle::new().with_size(Size::px(SWATCH_W, SWATCH_H))),
    )
}

/// One left-aligned body text line.
fn body_line(text: &'static str, fg: Color) -> Scene {
    Scene::Text(TextNode::styled(
        text,
        Rect::default(),
        TextStyle::new().with_size_px(BODY_FONT_PX).with_fg(fg),
    ))
}

/// view-fn (§6.3): pure sync `(ToggleState, bool) -> Scene`. The `on`
/// bit flips the watermark label so each toggle click is a distinct
/// paint the exporter then reflects.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ToggleState, on: bool, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let outline = theme.resolve(ColorRole::OnSurfaceMuted);

    // Title — bold, large.
    let title = Scene::Text(TextNode::styled(
        "Quarterly Report",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_weight(FontWeight::BOLD)
            .with_fg(on_surface),
    ));

    // Row of three primary-colour rounded swatches.
    let swatches = Scene::Container(
        ContainerNode::new(SWATCH_COLORS.iter().map(|c| swatch(*c)).collect())
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(12),
            ),
    );

    // Body text block.
    let body = Scene::Container(
        ContainerNode::new(BODY_LINES.iter().map(|l| body_line(l, on_surface)).collect())
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column).with_gap(4)),
    );

    // Bordered caption box (exercises the PDF stroke path).
    let caption = Scene::Text(TextNode::styled(
        "Figure 1 - colour key",
        Rect::default(),
        TextStyle::new().with_size_px(CAPTION_FONT_PX).with_fg(outline),
    ));
    let bordered = Scene::Container(
        ContainerNode::new(vec![caption])
            .with_style(
                BoxStyle::filled(surface)
                    .with_border(Border::new(outline, 2))
                    .with_corner_radius(4),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(260, 40)),
            ),
    );

    // Watermark — the toggle-flipped state, the live-tracking proof.
    let watermark = Scene::Text(TextNode::styled(
        if on { "FINAL" } else { "DRAFT" },
        Rect::default(),
        TextStyle::new()
            .with_size_px(WATERMARK_FONT_PX)
            .with_weight(FontWeight::BOLD)
            .with_fg(outline),
    ));

    // The repaint trigger — the Toggle hit handle. Each click flips
    // `on`, re-runs the view, and re-paints (so a re-export reflects
    // the new watermark).
    let repaint_label = Scene::Text(TextNode::styled(
        "Toggle watermark (repaint)",
        Rect::default(),
        TextStyle::new().with_size_px(BODY_FONT_PX).with_fg(on_surface),
    ));
    let chip_base = theme.resolve(ColorRole::SurfaceContainerHighest);
    let repaint_fill: Color = match state {
        ToggleState::Idle => chip_base,
        ToggleState::Hover => chip_base.lerp(on_surface, HOVER_OVERLAY_T),
        ToggleState::Pressed => chip_base.lerp(on_surface, PRESSED_OVERLAY_T),
        ToggleState::Disabled => chip_base.lerp(surface, DISABLED_OVERLAY_T),
    };
    let repaint = Scene::Container(
        ContainerNode::new(vec![repaint_label])
            .with_tag(REPAINT_TAG)
            .with_aria_label("Toggle the document watermark")
            .with_style(BoxStyle::filled(repaint_fill).with_corner_radius(6))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(260, 30)),
            ),
    );

    Scene::Container(
        ContainerNode::new(vec![title, swatches, body, bordered, watermark, repaint])
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(14),
            ),
    )
}

/// `WidgetView` binding. The §5.38 Toggle is reused as the RPC-drivable
/// watermark/repaint trigger (see the module doc); `#[widget]` derives
/// the mechanical [`WidgetCore`] / [`WidgetA11y`] / [`WidgetView`] trio.
///
/// [`WidgetCore`]: pinion_core::WidgetCore
/// [`WidgetA11y`]: pinion_a11y::WidgetA11y
/// [`WidgetView`]: pinion_shell::WidgetView
#[widget(
    tag = "repaint",
    state = (ToggleState, bool),
    event = ToggleEvent,
    title = "pinion hello-pdf-export (R908 §5.53 scene -> PDF)",
    renderer = HelloPdfExportRenderer,
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
struct PdfExportView;

impl PdfExportView {
    /// Tuple-state introspect: SCXML state name + the watermark bit.
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

    /// R641 §5.16 inherent view shim — unpacks the tuple and forwards.
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
    /// watermark bit in parity with a pointer click).
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
    pinion_shell::run::<PdfExportView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    fn rendered(state: ToggleState, on: bool) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(state, on, &Frame::new()))
    }

    fn collect_text(scene: &Scene, out: &mut Vec<String>) {
        match scene {
            Scene::Text(t) => out.push(t.content.clone()),
            Scene::Container(c) => {
                for ch in &c.children {
                    collect_text(ch, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn view_carries_title_and_body() {
        let scene = rendered(ToggleState::Idle, false);
        let mut texts = Vec::new();
        collect_text(&scene, &mut texts);
        assert!(texts.iter().any(|t| t == "Quarterly Report"));
        for line in BODY_LINES {
            assert!(texts.iter().any(|t| t == line), "body line present: {line}");
        }
    }

    #[test]
    fn watermark_flips_with_the_bit() {
        let mut off = Vec::new();
        collect_text(&rendered(ToggleState::Idle, false), &mut off);
        assert!(off.iter().any(|t| t == "DRAFT") && !off.iter().any(|t| t == "FINAL"));

        let mut on = Vec::new();
        collect_text(&rendered(ToggleState::Idle, true), &mut on);
        assert!(on.iter().any(|t| t == "FINAL") && !on.iter().any(|t| t == "DRAFT"));
    }

    #[test]
    fn watermark_flip_re_keys_the_paint() {
        // Each click must change the painted tree so the re-export
        // reflects the new state (re-keys the paint hash).
        let off = rendered(ToggleState::Idle, false);
        let on = rendered(ToggleState::Idle, true);
        assert_ne!(off.paint_hash(), on.paint_hash());
    }

    #[test]
    fn r55_g20_view_carries_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<PdfExportView>(
            (ToggleState::Idle, false),
            &Frame::new(),
        );
    }

    #[test]
    fn repaint_chip_reports_switch_role() {
        let nodes = <PdfExportView as WidgetA11y>::access_node(&(ToggleState::Idle, true), None);
        assert_eq!(nodes[0].role, AriaRole::Switch);
        assert_eq!(nodes[0].tag, REPAINT_TAG);
    }
}
