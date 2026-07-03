//! `hello-richtext` — R713 §5.36 consumer of the styled-run text
//! substrate (the first `RichText` / `Text.rich` widget).
//!
//! ## What this demonstrates
//!
//! A single [`TextNode`] whose `content` is one string — `"The quick
//! brown fox"` — painted with **inline multi-style runs** via
//! [`StyleRun`]. The node carries a base [`TextStyle`] (theme
//! `OnSurface`, 22px) plus three styled-run spans over UTF-8 byte
//! ranges:
//!
//! * `"quick"` (`4..9`) — **bold**, purple emphasis;
//! * `"brown"` (`10..15`) — *italic*, a literal saddle-brown;
//! * `"fox"` (`16..19`) — larger (30px); the Toggle switches it
//!   between a teal regular weight (Off) and a bold red (On).
//!
//! The base style still owns the uncovered bytes (`"The "` and the
//! inter-word spaces). One `TextNode`, four visually distinct styles —
//! the paint adapter already emits one Vello glyph run per parley run,
//! so no extra paint plumbing was needed (R713 only added the
//! styled-run *build* path to `pinion-text::LayoutCache`).
//!
//! ## Why a Toggle
//!
//! Rich text is intrinsically a display widget, but `AppShell` drives a
//! `WidgetView` with a statechart `External`. Rather than invent a
//! one-off control ([[abstraction-needs-second-consumer]]), the binding
//! reuses the §5.38 [`ToggleExternal`] purely as the *fox-emphasis bit*:
//! Off/On flips the `"fox"` run's colour + weight. The Toggle is the
//! RPC-drivable, AT-exposed control; the styled runs are the substrate
//! under test. Toggling re-keys the §5.16 R682 paint-cache because the
//! manual `Hash for Scene::Text` folds the runs in (R713).
//!
//! ## Verification (substrate-first)
//!
//! * `scene/snapshot` exposes the paragraph's `runs` array as
//!   structured data — `tools/demos/r713_richtext.py` reads back each
//!   span's byte range + resolved style (colour / weight / italic /
//!   size) without OCR (§2 #7 scene-as-data,
//!   [[ai-first-rpc-introspection-obligation]]).
//! * the same demo's Phase 2 captures the live window and scans the
//!   paragraph band, asserting the three literal run inks (purple /
//!   saddle-brown / teal) each render (a single-style regression would
//!   show one) — the live-pixel guard a structural query cannot replace
//!   ([[introspection-from-paint-not-screen]], R706/R707.3 precedent).

#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_core::external::IntrospectValue;
use pinion_core::scene::{ContainerNode, Rect, StyleRun, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, FontStyle, FontWeight, JustifyContent, LayoutStyle,
    Size, TextStyle,
};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{ColorRole, Frame, Scene, WidgetCore, WidgetStateName, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

// pinion-forge codegen output: `pub struct HelloRichTextRenderer` +
// async `new<...>` + sync `render` / `resize`. Same emit template as
// hello-gradient; only the struct name diverges.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so `AppShell<V>` can build /
// render / resize it. Identical pattern to hello-gradient.
vello_renderer_impl!(HelloRichTextRenderer, HelloRichTextRendererError);

const WIN_W: u32 = 480;
const WIN_H: u32 = 240;

const THEME_TAG: &str = "app";

const TITLE_FONT_PX: u32 = 18;
const PARA_FONT_PX: u32 = 22;
const FOX_FONT_PX: u32 = 30;
const STATUS_FONT_PX: u32 = 12;
const ROW_GAP: u32 = 18;

// The rich paragraph hit-test / introspection handle.
const PARA_TAG: &str = "rich_para";

// ── Paragraph content + styled-run byte ranges ─────────────────────
// content = "The quick brown fox" (one logical string). The byte
// offsets are the SSOT the demo + pixel guard assert against — each
// span fully restyles its [start, end) range on top of the base style.
const PARA_TEXT: &str = "The quick brown fox";
// "quick" — bold purple emphasis.
const QUICK_START: u32 = 4;
const QUICK_END: u32 = 9;
// "brown" — italic, a literal saddle-brown.
const BROWN_START: u32 = 10;
const BROWN_END: u32 = 15;
// "fox" — larger; teal (Off) ↔ bold red (On).
const FOX_START: u32 = 16;
const FOX_END: u32 = 19;

// ── Span colours ───────────────────────────────────────────────────
// Literal (theme-independent) so the live-pixel guard predicts the
// rendered ink exactly. Only the base "The " / spaces follow the theme.
const EMPH_PURPLE: Color = Color::rgb(0x7c, 0x3a, 0xed);
const SADDLE_BROWN: Color = Color::rgb(0x8b, 0x45, 0x13);
const FOX_TEAL: Color = Color::rgb(0x0d, 0x94, 0x88);
const FOX_RED: Color = Color::rgb(0xd1, 0x1d, 0x2a);

/// Material 3 state-layer overlay weights (linear-sRGB lerp toward
/// [`ColorRole::OnSurface`]) for the switch chrome — 8 % hover / 12 %
/// pressed, matching the rest of the widget gallery.
const HOVER_OVERLAY_T: f32 = 0.08;
const PRESSED_OVERLAY_T: f32 = 0.12;
const DISABLED_OVERLAY_T: f32 = 0.50;

/// Build the styled-run spans for the paragraph. Each run carries a
/// *fully-resolved* [`TextStyle`] derived from `base` (the R713 model:
/// a run is unambiguous, not a partial override), then overrides the
/// span's colour / weight / italic / size. `on` flips the `"fox"` run
/// between a teal regular weight and a bold red.
fn rich_runs(base: &TextStyle, on: bool) -> Vec<StyleRun> {
    let quick = base
        .clone()
        .with_fg(EMPH_PURPLE)
        .with_weight(FontWeight::BOLD);
    let brown = base
        .clone()
        .with_fg(SADDLE_BROWN)
        .with_style(FontStyle::Italic);
    let fox = base
        .clone()
        .with_size_px(FOX_FONT_PX)
        .with_fg(if on { FOX_RED } else { FOX_TEAL })
        .with_weight(if on {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        });
    vec![
        StyleRun::new(QUICK_START, QUICK_END, quick),
        StyleRun::new(BROWN_START, BROWN_END, brown),
        StyleRun::new(FOX_START, FOX_END, fox),
    ]
}

/// view-fn (§6.3): pure sync mapping `(ToggleState, bool) -> Scene`.
/// `on` selects the fox-emphasis styling. `&Frame` is the §6.3 ZST
/// hedge. The shell calls `compute_layout` before paint, so the view
/// need not resolve rects.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ToggleState, on: bool, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let accent = theme.resolve(ColorRole::Accent);

    let title = Scene::Text(TextNode::styled(
        "RichText (Text.rich)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_fg(on_surface),
    ));

    // The rich paragraph — ONE TextNode, four inline styles. The base
    // style owns "The " + the spaces; the three runs restyle their
    // word spans. `with_layout` gives it a fixed width so the single
    // line lays out predictably for the live-pixel scan.
    let para_base = TextStyle::new()
        .with_size_px(PARA_FONT_PX)
        .with_fg(on_surface);
    let paragraph = Scene::Text(
        TextNode::styled(PARA_TEXT, Rect::default(), para_base.clone())
            .with_runs(rich_runs(&para_base, on))
            .with_tag(PARA_TAG)
            .with_layout(LayoutStyle::new().with_size(Size::px(420, 44))),
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
        if on { "Fox: bold red" } else { "Fox: teal" },
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX + 2)
            .with_fg(switch_fg),
    ));
    let mode_chip = Scene::Container(
        ContainerNode::new(vec![switch_label])
            .with_tag("main_toggle")
            .with_aria_label("Fox emphasis")
            .with_style(BoxStyle::filled(switch_fill).with_corner_radius(18))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(150, 36)),
            ),
    );

    let status = Scene::Text(TextNode::styled(
        format!("{} | {} runs", state.as_name(), 3),
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    Scene::Container(
        ContainerNode::new(vec![title, paragraph, mode_chip, status])
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

/// `WidgetView` binding. The §5.38 Toggle is reused as the fox-emphasis
/// mode bit (see the module doc); the `#[widget]` attribute derives the
/// mechanical [`WidgetCore`] / [`WidgetA11y`] / [`WidgetView`] trio. No
/// `update` reducer — the Off/On bit is read straight in [`view`].
///
/// [`WidgetCore`]: pinion_core::WidgetCore
/// [`WidgetA11y`]: pinion_a11y::WidgetA11y
/// [`WidgetView`]: pinion_shell::WidgetView
#[widget(
    tag = "main_toggle",
    state = (ToggleState, bool),
    event = ToggleEvent,
    title = "pinion hello-richtext (R713 §5.36 styled-run text substrate)",
    renderer = HelloRichTextRenderer,
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
struct RichTextView;

impl RichTextView {
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
    pinion_shell::run::<RichTextView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    /// Walk the scene for the first `Scene::Text` carrying `tag`.
    fn find_text<'a>(scene: &'a Scene, tag: &str) -> Option<&'a TextNode> {
        match scene {
            Scene::Text(t) if t.tag.as_deref() == Some(tag) => Some(t),
            Scene::Container(c) => c.children.iter().find_map(|ch| find_text(ch, tag)),
            _ => None,
        }
    }

    fn rendered(state: ToggleState, on: bool) -> Scene {
        let owner = Owner::new();
        owner.run(|| view(state, on, &Frame::new()))
    }

    #[test]
    fn paragraph_is_one_node_with_three_runs() {
        let scene = rendered(ToggleState::Idle, false);
        let para = find_text(&scene, PARA_TAG).expect("paragraph present");
        assert_eq!(para.content, PARA_TEXT, "one logical string");
        assert_eq!(para.runs.len(), 3, "quick / brown / fox spans");
        assert_eq!(
            (para.runs[0].start, para.runs[0].end),
            (QUICK_START, QUICK_END)
        );
        assert_eq!(
            (para.runs[1].start, para.runs[1].end),
            (BROWN_START, BROWN_END)
        );
        assert_eq!((para.runs[2].start, para.runs[2].end), (FOX_START, FOX_END));
    }

    #[test]
    fn runs_carry_distinct_resolved_styles() {
        let scene = rendered(ToggleState::Idle, false);
        let para = find_text(&scene, PARA_TAG).expect("paragraph present");
        // quick: bold purple.
        assert_eq!(para.runs[0].style.fg_color, EMPH_PURPLE);
        assert_eq!(para.runs[0].style.font_weight, FontWeight::BOLD);
        // brown: italic saddle-brown.
        assert_eq!(para.runs[1].style.fg_color, SADDLE_BROWN);
        assert_eq!(para.runs[1].style.font_style, FontStyle::Italic);
        // fox: larger.
        assert_eq!(para.runs[2].style.font_size_px, FOX_FONT_PX);
        // base owns "The " — paragraph base size is the para font.
        assert_eq!(para.style.font_size_px, PARA_FONT_PX);
    }

    #[test]
    fn fox_run_flips_with_toggle() {
        let off = rendered(ToggleState::Idle, false);
        let off_para = find_text(&off, PARA_TAG).unwrap();
        assert_eq!(off_para.runs[2].style.fg_color, FOX_TEAL);
        assert_eq!(off_para.runs[2].style.font_weight, FontWeight::NORMAL);

        let on = rendered(ToggleState::Idle, true);
        let on_para = find_text(&on, PARA_TAG).unwrap();
        assert_eq!(on_para.runs[2].style.fg_color, FOX_RED);
        assert_eq!(on_para.runs[2].style.font_weight, FontWeight::BOLD);
    }

    #[test]
    fn run_change_re_keys_the_paint_hash() {
        // The §5.16 R682 paint-cache keys off `Scene::paint_hash`, which
        // (R713) folds `TextNode.runs` into the manual `Hash for
        // Scene::Text`. Off vs On paragraphs must hash differently or
        // the cache would replay a stale fragment.
        let off = rendered(ToggleState::Idle, false);
        let on = rendered(ToggleState::Idle, true);
        assert_ne!(off.paint_hash(), on.paint_hash());
    }

    #[test]
    fn r55_g20_view_carries_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<RichTextView>(
            (ToggleState::Idle, false),
            &Frame::new(),
        );
    }

    #[test]
    fn switch_reports_switch_role() {
        let nodes = <RichTextView as WidgetA11y>::access_node(&(ToggleState::Idle, true), None);
        assert_eq!(nodes[0].role, AriaRole::Switch);
        assert_eq!(nodes[0].tag, "main_toggle");
    }
}
