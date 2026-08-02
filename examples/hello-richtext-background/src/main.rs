//! `hello-richtext-background` — R1546 §5.36 consumer of the run-background
//! substrate ([`TextStyle::bg_color`], Qt `QTextCharFormat::setBackground`).
//!
//! ## What this demonstrates
//!
//! A search-results line — the canonical reason a character format needs a
//! background. One [`TextNode`] carries the sentence; the search term is a
//! [`StyleRun`] declaring a highlight colour, the way a syntax highlighter, a
//! diff view or an LSP semantic-token layer declares one.
//!
//! Three properties of the substrate are visible on screen at once:
//!
//! * **the highlight** — `"fox"` sits on a marker-pen background;
//! * **a base background with a hole in it** — the trailing clause has a
//!   base-style background, and one word inside it declares *no* background.
//!   A [`StyleRun`] carries a FULLY RESOLVED style, so that word states its
//!   bytes have none, and the band splits rather than being painted over;
//! * **contrast that moves** — the Toggle switches the highlight between a
//!   readable pairing and an unreadable one. Nothing about the *declaration*
//!   changes shape; what changes is the WCAG ratio `scene/text_backgrounds`
//!   publishes, from ~11:1 down to ~1.3:1.
//!
//! ## Why the Toggle
//!
//! A background is a display property, but `AppShell` drives a `WidgetView`
//! with a statechart `External`. Rather than invent a one-off control
//! ([[abstraction-needs-second-consumer]]), this reuses the §5.38
//! [`ToggleExternal`] as the *palette bit* — the same choice `hello-richtext`
//! made for its emphasis bit. Off is the readable pairing, On the unreadable
//! one, so an agent can drive the contrast across the WCAG 4.5 body-text bar
//! in one call and assert both sides.
//!
//! ## Verification (substrate-first)
//!
//! * `scene/snapshot` exposes each run's `bg_color` — the DECLARATION, as
//!   scene data (§2 #7).
//! * `scene/text_backgrounds` exposes where each band was PAINTED, plus the
//!   WCAG contrast of the run's ink against it. That is the half Qt has no
//!   accessor for: `QTextCharFormat` knows the brush and nothing about the
//!   rect, which `QTextLayout::draw` computes privately and discards.
//! * `tools/demos/r1546_run_background.py` reads both, drives the Toggle
//!   across the readability boundary, and checks the painted band against the
//!   node it belongs to.
//! * this crate's own tests paint the SAME view scene through the terminal
//!   backend and assert the highlight reaches `ratatui`'s cell background —
//!   one declaration, two backends (§2 #6).

#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_core::external::IntrospectValue;
use pinion_core::scene::{ContainerNode, Rect, StyleRun, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, FontWeight, JustifyContent, LayoutStyle, Size,
    TextStyle,
};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{ColorRole, Frame, Scene, WidgetStateName, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

// pinion-forge codegen output: `pub struct HelloRichTextBackgroundRenderer` +
// async `new<...>` + sync `render` / `resize`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(
    HelloRichTextBackgroundRenderer,
    HelloRichTextBackgroundRendererError
);

const WIN_W: u32 = 520;
const WIN_H: u32 = 260;

const THEME_TAG: &str = "app";

const TITLE_FONT_PX: u32 = 18;
const HIT_FONT_PX: u32 = 22;
const CLAUSE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 12;
const ROW_GAP: u32 = 16;

/// The search-hit line's introspection handle.
const HIT_TAG: &str = "search_hit";
/// The base-background clause's handle — the hole-punch demonstration.
const CLAUSE_TAG: &str = "clause";

// ── The search-hit line ────────────────────────────────────────────
// content is ONE logical string; the highlight is a byte range over it, the
// way a search / highlighter / diff layer emits ranges.
const HIT_TEXT: &str = "The quick brown fox jumps";
/// `"fox"` — the matched term.
const HIT_START: u32 = 16;
/// One past `"fox"`.
const HIT_END: u32 = 19;

// ── The clause line ────────────────────────────────────────────────
const CLAUSE_TEXT: &str = "over the lazy dog";
/// `"lazy"` — declares NO background inside a base style that has one.
const HOLE_START: u32 = 9;
/// One past `"lazy"`.
const HOLE_END: u32 = 13;

// ── Palettes ───────────────────────────────────────────────────────
// Literal (theme-independent) so the published contrast is a fixed number the
// demo can assert rather than a function of the host's theme.
/// Marker-pen yellow. Against black ink this clears the WCAG 4.5 body-text
/// bar with room to spare.
const HIGHLIGHT_READABLE: Color = Color::rgb(0xFF, 0xF1, 0x76);
/// A saturated indigo somebody might reach for because it "looks like a
/// selection". Against the same black ink it is around 1.3:1 — text on it is
/// effectively invisible, and NOTHING in Qt would tell you so.
const HIGHLIGHT_UNREADABLE: Color = Color::rgb(0x2A, 0x1E, 0x6E);
/// The ink the highlighted run draws in. Held fixed across the Toggle so the
/// published ratio moves for exactly one reason.
const HIT_INK: Color = Color::rgb(0x11, 0x11, 0x11);
/// The clause's base background — a faint theme-independent wash.
const CLAUSE_WASH: Color = Color::rgb(0xDC, 0xEB, 0xFA);
/// The clause's ink.
const CLAUSE_INK: Color = Color::rgb(0x1A, 0x1A, 0x2E);

/// M3 state-layer overlay weights for the switch chrome.
const HOVER_OVERLAY_T: f32 = 0.08;
const PRESSED_OVERLAY_T: f32 = 0.12;
const DISABLED_OVERLAY_T: f32 = 0.50;

/// The highlight colour the palette bit selects.
#[must_use]
pub fn highlight_for(unreadable: bool) -> Color {
    if unreadable {
        HIGHLIGHT_UNREADABLE
    } else {
        HIGHLIGHT_READABLE
    }
}

/// The search-hit line's runs: one highlighted term.
///
/// Built from `base` so the run inherits every paragraph-level field (the
/// authoring convention [`StyleRun`] documents) and overrides only the ink and
/// the background.
fn hit_runs(base: &TextStyle, unreadable: bool) -> Vec<StyleRun> {
    vec![StyleRun::new(
        HIT_START,
        HIT_END,
        base.clone()
            .with_fg(HIT_INK)
            .with_weight(FontWeight::BOLD)
            .with_bg_color(highlight_for(unreadable)),
    )]
}

/// The clause line's runs: one word that declares NO background, inside a base
/// style that declares one.
///
/// `without_bg_color` is not "inherit" — it states that these bytes have no
/// background, and the band splits around them. That falls out of a run
/// carrying a fully-resolved style, and it is the behaviour a caller wants:
/// there is exactly one background per byte and no ordering question.
fn clause_runs(base: &TextStyle) -> Vec<StyleRun> {
    vec![StyleRun::new(
        HOLE_START,
        HOLE_END,
        base.clone().without_bg_color(),
    )]
}

/// view-fn (§6.3): pure sync mapping `(ToggleState, bool) -> Scene`.
/// `unreadable` selects the highlight palette.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ToggleState, unreadable: bool, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let accent = theme.resolve(ColorRole::Accent);

    let title = Scene::Text(TextNode::styled(
        "Run background (QTextCharFormat::setBackground)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_fg(on_surface),
    ));

    // The search hit — ONE TextNode, one highlighted byte range.
    let hit_base = TextStyle::new()
        .with_size_px(HIT_FONT_PX)
        .with_fg(on_surface);
    let hit = Scene::Text(
        TextNode::styled(HIT_TEXT, Rect::default(), hit_base.clone())
            .with_runs(hit_runs(&hit_base, unreadable))
            .with_tag(HIT_TAG)
            .with_layout(LayoutStyle::new().with_size(Size::px(440, 34))),
    );

    // The clause — a base background with a hole punched in it by a run that
    // declares none.
    let clause_base = TextStyle::new()
        .with_size_px(CLAUSE_FONT_PX)
        .with_fg(CLAUSE_INK)
        .with_bg_color(CLAUSE_WASH);
    let clause = Scene::Text(
        TextNode::styled(CLAUSE_TEXT, Rect::default(), clause_base.clone())
            .with_runs(clause_runs(&clause_base))
            .with_tag(CLAUSE_TAG)
            .with_layout(LayoutStyle::new().with_size(Size::px(440, 28))),
    );

    let switch_base = if unreadable {
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
    let switch_fg = if unreadable {
        theme.resolve(ColorRole::OnAccent)
    } else {
        on_surface
    };
    let switch_label = Scene::Text(TextNode::styled(
        if unreadable {
            "Highlight: indigo"
        } else {
            "Highlight: marker"
        },
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX + 2)
            .with_fg(switch_fg),
    ));
    let mode_chip = Scene::Container(
        ContainerNode::new(vec![switch_label])
            .with_tag("main_toggle")
            .with_aria_label("Highlight palette")
            .with_style(BoxStyle::filled(switch_fill).with_corner_radius(18))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(170, 36)),
            ),
    );

    let status = Scene::Text(TextNode::styled(
        format!("{} | 2 backgrounds declared", state.as_name()),
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    Scene::Container(
        ContainerNode::new(vec![title, hit, clause, mode_chip, status])
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

/// `WidgetView` binding. The §5.38 Toggle is the highlight-palette bit.
///
/// [`WidgetCore`]: pinion_core::WidgetCore
/// [`WidgetA11y`]: pinion_a11y::WidgetA11y
/// [`WidgetView`]: pinion_shell::WidgetView
#[widget(
    tag = "main_toggle",
    state = (ToggleState, bool),
    event = ToggleEvent,
    title = "pinion hello-richtext-background (R1546 §5.36 run background)",
    renderer = HelloRichTextBackgroundRenderer,
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
struct RunBackgroundView;

impl RunBackgroundView {
    /// Tuple-state introspect: SCXML state name via `query("state")` + the
    /// palette bit via `query("value")`. Defaults to `(Idle, false)`.
    fn read_state(scene: &Scene) -> (ToggleState, bool) {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                let state = if let Some(IntrospectValue::Text(name)) = intro.query("state") {
                    ToggleState::from_name_or_default(&name)
                } else {
                    ToggleState::Idle
                };
                let on = matches!(intro.query("value"), Some(IntrospectValue::Bool(true)));
                return (state, on);
            }
        }
        (ToggleState::Idle, false)
    }

    /// R641 §5.16 inherent view shim — unpacks the tuple and forwards to the
    /// free [`view`].
    fn view(state: (ToggleState, bool), frame: Frame) -> Scene {
        view(state.0, state.1, &frame)
    }
}

fn main() {
    pinion_shell::run::<RunBackgroundView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::contrast::contrast_ratio;

    fn with_owner<R>(f: impl FnOnce() -> R) -> R {
        let owner = pinion_core::reactive::Owner::new();
        owner.run(f)
    }

    fn scene_for(unreadable: bool) -> Scene {
        with_owner(|| view(ToggleState::Idle, unreadable, &Frame::new()))
    }

    fn text_node<'a>(scene: &'a Scene, tag: &str) -> &'a TextNode {
        fn find<'a>(s: &'a Scene, tag: &str) -> Option<&'a TextNode> {
            match s {
                Scene::Text(t) if t.tag.as_deref() == Some(tag) => Some(t),
                Scene::Container(c) => c.children.iter().find_map(|c| find(c, tag)),
                _ => None,
            }
        }
        find(scene, tag).expect("the tagged text node is in the view")
    }

    /// The highlight is DECLARED on the run, not painted by the binding as a
    /// box behind the text. That is the whole point: a highlight the binding
    /// draws itself cannot follow the glyphs when the font, the size or the
    /// wrap width changes, and cannot be published as a text property.
    #[test]
    fn r1546_the_hit_declares_its_background_on_the_run() {
        let scene = scene_for(false);
        let hit = text_node(&scene, HIT_TAG);
        assert_eq!(hit.runs.len(), 1);
        assert_eq!((hit.runs[0].start, hit.runs[0].end), (HIT_START, HIT_END));
        assert_eq!(hit.runs[0].style.bg_color, Some(HIGHLIGHT_READABLE));
        assert_eq!(
            hit.style.bg_color, None,
            "only the matched term is highlighted",
        );
    }

    /// The palette bit moves the declaration, and moves it across the WCAG
    /// body-text bar in both directions — which is what makes the published
    /// `contrast` a number worth reading rather than a constant.
    #[test]
    fn r1546_the_palette_bit_crosses_the_wcag_body_text_bar() {
        let readable = contrast_ratio(HIT_INK, HIGHLIGHT_READABLE);
        let unreadable = contrast_ratio(HIT_INK, HIGHLIGHT_UNREADABLE);
        assert!(readable >= 4.5, "marker pen reads: {readable}");
        assert!(unreadable < 4.5, "indigo does not: {unreadable}");
        assert_eq!(
            text_node(&scene_for(true), HIT_TAG).runs[0].style.bg_color,
            Some(HIGHLIGHT_UNREADABLE),
        );
    }

    /// A run declaring no background inside a base style that has one states
    /// that its bytes have none — so the shaped bands split around it.
    #[test]
    fn r1546_the_clause_band_is_split_by_a_run_with_no_background() {
        let scene = scene_for(false);
        let clause = text_node(&scene, CLAUSE_TAG);
        assert_eq!(clause.style.bg_color, Some(CLAUSE_WASH));
        assert_eq!(clause.runs[0].style.bg_color, None);

        let mut cache = pinion_text::LayoutCache::new();
        let bands = cache
            .backgrounds(&clause.content, &clause.style, &clause.runs, Some(440))
            .to_vec();
        assert_eq!(bands.len(), 2, "the hole splits the wash: {bands:?}");
        assert_eq!((bands[0].start, bands[0].end), (0, HOLE_START));
        assert_eq!(
            (bands[1].start, bands[1].end),
            (HOLE_END, u32::try_from(CLAUSE_TEXT.len()).expect("fits")),
        );
        assert!(
            bands[0].x + bands[0].width <= bands[1].x,
            "and leaves a gap where the word is: {bands:?}",
        );
    }

    /// R1546 §2#6 — the SAME view scene, painted through the terminal backend,
    /// puts the highlight on the cell background.
    ///
    /// Asserted here rather than only in `pinion-tui`'s own tests because what
    /// is being claimed is a property of the DECLARATION: one `bg_color` on one
    /// run reaches a GPU rasteriser and a terminal cell grid, from a view
    /// function that names neither.
    #[test]
    fn r1546_the_same_declaration_reaches_the_terminal_backend() {
        let scene = scene_for(false);
        let hit = text_node(&scene, HIT_TAG).clone();
        // Place the node on the cell grid; the TUI walker reads `rect`.
        let mut node = hit;
        node.rect = Rect::new(0, 0, 400, 20);
        let mut buf = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 60, 4));
        pinion_tui::paint::to_buffer(&Scene::Text(node), &mut buf);

        let painted = ratatui::style::Color::Rgb(
            HIGHLIGHT_READABLE.r,
            HIGHLIGHT_READABLE.g,
            HIGHLIGHT_READABLE.b,
        );
        // "The quick brown fox jumps" — bytes 16..19 are the 17th..19th cells
        // for this all-ASCII line, so column 16 is `f`.
        assert_eq!(buf[(16, 0)].symbol(), "f");
        assert_eq!(buf[(16, 0)].bg, painted);
        assert_eq!(buf[(18, 0)].bg, painted);
        assert_eq!(
            buf[(15, 0)].bg,
            ratatui::style::Color::Reset,
            "the space before the term is not highlighted",
        );
        assert_eq!(
            buf[(19, 0)].bg,
            ratatui::style::Color::Reset,
            "nor the space after it",
        );
    }

    #[test]
    fn r1546_a11y_node_is_a_switch() {
        let nodes =
            <RunBackgroundView as WidgetA11y>::access_node(&(ToggleState::Idle, false), None);
        assert_eq!(nodes[0].role, AriaRole::Switch);
        assert_eq!(nodes[0].tag, "main_toggle");
    }
}
