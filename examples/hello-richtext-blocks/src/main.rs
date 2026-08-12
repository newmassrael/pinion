//! `hello-richtext-blocks` — R1551 §5.36 consumer of the **block format**: a
//! document whose paragraphs each declare their own [`BlockFormat`]
//! (the toolkit text block format).
//!
//! ## What this demonstrates
//!
//! Five paragraphs, built through
//! [`view_document`], each
//! stating something about itself that no `TextStyle` could say:
//!
//! * a **heading** (`heading_level: 1`) — which also becomes a WAI-ARIA
//!   `heading` with `aria-level`, so the document has an outline a screen
//!   reader can navigate;
//! * a **body paragraph** with a first-line `text-indent` — the classic prose
//!   indent, which narrows the FIRST line's break budget and no other;
//! * a **block quote** indented on both edges with space above and below;
//! * a **bibliography entry** with a *hanging* indent — the mirror case, where
//!   every line but the first moves in;
//! * a second-level heading.
//!
//! ## Why a Toggle
//!
//! A document is a display surface, but `AppShell` drives a `WidgetView` with a
//! statechart `External`. Rather than invent a one-off control
//! ([[abstraction-needs-second-consumer]]), the binding reuses the §5.38
//! [`ToggleExternal`] as the *indent-mode bit*: Off leaves the body paragraph's
//! indent on its first line, On makes it hanging. Both are the same declared
//! amount — only which lines it selects changes — so the published line boxes
//! move for exactly one reason.
//!
//! ## Verification (substrate-first)
//!
//! * `scene/snapshot` exposes each paragraph's declared `block` and its
//!   `style.text_indent` as structured data (§2 #7 scene-as-data).
//! * `scene/text_blocks` exposes the same declaration BESIDE the shaped line
//!   boxes, which is the only form in which "did my indent reach the layout"
//!   has an answer — `tools/demos/r1551_block_format.py` reads both and checks
//!   the first line's x against the amount declared.
//! * `scene/access` exposes the heading outline, which the toolkit's own text
//!   accessibility interface cannot express at all.
//! * this crate's own tests paint the SAME scene through the terminal backend,
//!   so the §2 #6 claim is made by the consumer and not only by each backend.
//!
//! [`BlockFormat`]: pinion_core::style::BlockFormat
//! [`view_document`]: pinion_widget_paint::document::view_document

#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_core::external::IntrospectValue;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BlockFormat, BoxStyle, Color, FlexDirection, FontWeight, JustifyContent,
    LayoutStyle, Size, TextIndent, TextStyle,
};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{ColorRole, Frame, Scene, WidgetStateName, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;
use pinion_widget_paint::document::{TextBlock, view_document};

// pinion-forge codegen output: `pub struct HelloRichTextBlocksRenderer` +
// async `new<...>` + sync `render` / `resize`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(
    HelloRichTextBlocksRenderer,
    HelloRichTextBlocksRendererError
);

const WIN_W: u32 = 560;
const WIN_H: u32 = 460;

const THEME_TAG: &str = "app";

/// The document's introspection handle. Every paragraph is
/// `DocumentTag::block(DOC_TAG, i)`.
const DOC_TAG: &str = "essay";

/// The width the document is laid out in. Fixed so the paragraphs wrap at a
/// known column and the published line boxes are a stable fact.
const DOC_W: u32 = 460;

const BODY_FONT_PX: u32 = 16;
const H1_FONT_PX: u32 = 24;
const H2_FONT_PX: u32 = 19;
const STATUS_FONT_PX: u32 = 12;
const ROW_GAP: u32 = 14;

/// The paragraph indent, in px — one amount, two ways of selecting which lines
/// it applies to.
const INDENT_PX: i32 = 28;
/// The block quote's indent on both edges.
const QUOTE_INDENT_PX: u32 = 32;
/// Space above and below a heading, and around the quote.
const BLOCK_SPACE_PX: u32 = 10;

/// The index of the body paragraph the Toggle re-indents.
#[cfg(test)]
const BODY_BLOCK: usize = 1;
/// The index of the bibliography entry, which is hanging whatever the Toggle
/// says — the fixed control beside the moving one.
#[cfg(test)]
const BIB_BLOCK: usize = 3;

const H1_TEXT: &str = "Chapter One";
const BODY_TEXT: &str = "A paragraph states its own block format: how far it is indented, how much \
     air it wants above and below, and where its first line begins.";
const QUOTE_TEXT: &str = "Every length here is one unit. The toolkit's block indent is not.";
const BIB_TEXT: &str =
    "Bringhurst, Robert. The Elements of Typographic Style. Hartley & Marks, 2004.";
const H2_TEXT: &str = "On indentation";

/// M3 state-layer overlay weights for the switch chrome.
const HOVER_OVERLAY_T: f32 = 0.08;
const PRESSED_OVERLAY_T: f32 = 0.12;
const DISABLED_OVERLAY_T: f32 = 0.50;

/// The body paragraph's indent for the current mode.
///
/// One amount either way — `hanging` inverts which lines it selects, it does
/// not negate it. That is CSS's own definition, and keeping the amount fixed is
/// what makes the two modes comparable over the wire.
#[must_use]
pub fn body_indent(hanging: bool) -> TextIndent {
    if hanging {
        TextIndent::hanging(INDENT_PX)
    } else {
        TextIndent::first_line(INDENT_PX)
    }
}

/// The document's five paragraphs.
///
/// `hanging` selects the body paragraph's indent mode; every other block is
/// fixed, so a diff of two frames isolates one declaration.
#[must_use]
pub fn blocks(base: &TextStyle, on_surface: Color, muted: Color, hanging: bool) -> Vec<TextBlock> {
    vec![
        TextBlock::new(H1_TEXT)
            .with_format(
                BlockFormat::new()
                    .with_heading_level(1)
                    .with_spacing(0, BLOCK_SPACE_PX),
            )
            .with_style(
                base.clone()
                    .with_size_px(H1_FONT_PX)
                    .with_weight(FontWeight::BOLD)
                    .with_fg(on_surface),
            ),
        TextBlock::new(BODY_TEXT).with_style(base.clone().with_text_indent(body_indent(hanging))),
        TextBlock::new(QUOTE_TEXT)
            .with_format(
                BlockFormat::new()
                    .with_indent(QUOTE_INDENT_PX)
                    .with_spacing(BLOCK_SPACE_PX, BLOCK_SPACE_PX),
            )
            .with_style(base.clone().with_fg(muted)),
        // A bibliography entry: the hanging indent's canonical use, held fixed
        // so the Toggle's effect on the body paragraph is legible against it.
        TextBlock::new(BIB_TEXT).with_style(
            base.clone()
                .with_size_px(BODY_FONT_PX - 1)
                .with_text_indent(TextIndent::hanging(INDENT_PX)),
        ),
        TextBlock::new(H2_TEXT)
            .with_format(
                BlockFormat::new()
                    .with_heading_level(2)
                    .with_spacing(BLOCK_SPACE_PX, 0),
            )
            .with_style(
                base.clone()
                    .with_size_px(H2_FONT_PX)
                    .with_weight(FontWeight::BOLD)
                    .with_fg(on_surface),
            ),
    ]
}

/// view-fn (§6.3): pure sync mapping `(ToggleState, bool) -> Scene`.
/// `hanging` selects the body paragraph's indent mode.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ToggleState, hanging: bool, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let accent = theme.resolve(ColorRole::Accent);

    let base = TextStyle::new()
        .with_size_px(BODY_FONT_PX)
        .with_fg(on_surface);
    let document = Scene::Container(
        view_document(DOC_TAG, &base, &blocks(&base, on_surface, muted, hanging))
            .map_layout(|l| l.with_size(Size::px(DOC_W, WIN_H - 120))),
    );

    let switch_base = if hanging {
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
    let switch_fg = if hanging {
        theme.resolve(ColorRole::OnAccent)
    } else {
        on_surface
    };
    let switch_label = Scene::Text(TextNode::styled(
        if hanging {
            "Body indent: hanging"
        } else {
            "Body indent: first line"
        },
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX + 2)
            .with_fg(switch_fg),
    ));
    let mode_chip = Scene::Container(
        ContainerNode::new(vec![switch_label])
            .with_tag("main_toggle")
            .with_aria_label("Body indent mode")
            .with_style(BoxStyle::filled(switch_fill).with_corner_radius(18))
            .with_layout(
                LayoutStyle::new()
                    .with_focusable(true)
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(190, 36)),
            ),
    );

    let status = Scene::Text(TextNode::styled(
        format!("{} | 5 blocks, 2 headings", state.as_name()),
        Rect::default(),
        TextStyle::new().with_size_px(STATUS_FONT_PX).with_fg(muted),
    ));

    Scene::Container(
        ContainerNode::new(vec![document, mode_chip, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Center)
                    .with_gap(ROW_GAP)
                    .with_padding(Rect::new(20, 20, 20, 20)),
            ),
    )
}

/// `WidgetView` binding. The §5.38 Toggle is the indent-mode bit.
///
/// [`WidgetCore`]: pinion_core::WidgetCore
/// [`WidgetA11y`]: pinion_a11y::WidgetA11y
/// [`WidgetView`]: pinion_shell::WidgetView
#[widget(
    tag = "main_toggle",
    state = (ToggleState, bool),
    event = ToggleEvent,
    title = "pinion hello-richtext-blocks (R1551 §5.36 block format)",
    renderer = HelloRichTextBlocksRenderer,
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
    event_name_derive,
    apply_key = aria_activate,
    keybinding,
)]
struct BlockFormatView;

impl BlockFormatView {
    /// Tuple-state introspect: SCXML state name via `query("state")` + the
    /// indent-mode bit via `query("value")`. Defaults to `(Idle, false)`.
    fn read_state(scene: &Scene) -> (ToggleState, bool) {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                let state = if let Ok(IntrospectValue::Text(name)) = intro.query("state") {
                    ToggleState::from_name_or_default(&name)
                } else {
                    ToggleState::Idle
                };
                let on = matches!(intro.query("value"), Ok(IntrospectValue::Bool(true)));
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

    // R1570 §5.16 — the functions this binding's `#[widget(...)]` had always
    // declared and never had. It was copied from `hello-richtext` without
    // them, and the macro's forward for a declared-but-absent name resolved
    // back to the trait method it defines, so `event_name`, `apply_key` and
    // `keybinding` were each an unconditional self-call — a tail-call loop in
    // release. `pinion_core::widget_forward` now makes that a compile error.
    //
    // `event_name` is answered by `event_name_derive` above rather than
    // written out, because the body would be character-for-character what that
    // flag emits, and hand-rolling a substrate that already exists is the very
    // shape this round is about.

    /// The `d` / `e` accelerators the sibling binding maps, so this demo's
    /// Switch can be driven to either palette from the keyboard.
    fn keybinding(key: &str) -> Option<ToggleEvent> {
        match key {
            "d" => Some(ToggleEvent::Disable),
            "e" => Some(ToggleEvent::Enable),
            _ => None,
        }
    }
}

fn main() {
    pinion_shell::run::<BlockFormatView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::composite_tag::DocumentTag;
    use pinion_text::LayoutCache;

    fn with_owner<R>(f: impl FnOnce() -> R) -> R {
        let owner = pinion_core::reactive::Owner::new();
        owner.run(f)
    }

    fn scene_for(hanging: bool) -> Scene {
        with_owner(|| view(ToggleState::Idle, hanging, &Frame::new()))
    }

    fn block_node(scene: &Scene, i: usize) -> &TextNode {
        fn find<'a>(s: &'a Scene, tag: &str) -> Option<&'a TextNode> {
            match s {
                Scene::Text(t) if t.tag.as_deref() == Some(tag) => Some(t),
                Scene::Container(c) => c.children.iter().find_map(|c| find(c, tag)),
                Scene::Scroll(n) => find(&n.content, tag),
                _ => None,
            }
        }
        find(scene, &DocumentTag::block(DOC_TAG, i)).expect("the paragraph is in the view")
    }

    /// Where each shaped line starts, for a paragraph laid out at `DOC_W`.
    fn line_starts(node: &TextNode) -> Vec<f32> {
        let mut cache = LayoutCache::new();
        let layout = cache.layout_with_runs(&node.content, &node.style, &node.runs, Some(DOC_W));
        pinion_text::line_boxes(layout)
            .into_iter()
            .map(|l| l.x)
            .collect()
    }

    /// Each paragraph declares its own format, and the declaration survives
    /// into the painted scene — which is what `scene/text_blocks` reads back.
    #[test]
    fn r1551_each_paragraph_carries_its_own_declaration() {
        let scene = scene_for(false);
        assert_eq!(block_node(&scene, 0).block.expect("h1").heading_level, 1);
        assert_eq!(block_node(&scene, 4).block.expect("h2").heading_level, 2);
        let quote = block_node(&scene, 2);
        let fmt = quote.block.expect("quote declares a format");
        assert_eq!(fmt.left_indent_px, QUOTE_INDENT_PX);
        assert_eq!(fmt.right_indent_px, QUOTE_INDENT_PX);
        assert_eq!(fmt.space_above_px, BLOCK_SPACE_PX);
    }

    /// The declaration lowers to the ordinary layout box, which is what lets
    /// the flex pass indent a paragraph with no document-specific layout code
    /// — the thing text document layout cannot do.
    #[test]
    fn r1551_the_quote_indent_is_the_nodes_margin() {
        let quote = block_node(&scene_for(false), 2).clone();
        assert_eq!(quote.layout.margin.x, QUOTE_INDENT_PX, "left");
        assert_eq!(quote.layout.margin.w, QUOTE_INDENT_PX, "right");
        assert_eq!(quote.layout.margin.y, BLOCK_SPACE_PX, "above");
        assert_eq!(quote.layout.margin.h, BLOCK_SPACE_PX, "below");
    }

    /// The declared first-line indent reaches the SHAPED layout: line 0 starts
    /// at the declared amount and no other line does.
    #[test]
    fn r1551_a_first_line_indent_moves_only_the_first_line() {
        let body = block_node(&scene_for(false), BODY_BLOCK).clone();
        let starts = line_starts(&body);
        assert!(starts.len() > 1, "the body paragraph wraps: {starts:?}");
        #[allow(clippy::cast_precision_loss, reason = "a 28px indent is exact in f32")]
        let want = INDENT_PX as f32;
        assert!(
            (starts[0] - want).abs() < 0.5,
            "the first line is indented: {starts:?}",
        );
        assert!(
            starts[1..].iter().all(|x| x.abs() < 0.5),
            "and no other line is: {starts:?}",
        );
    }

    /// `hanging` inverts the SELECTION, not the amount — the mirror image, from
    /// the same declared number.
    #[test]
    fn r1551_a_hanging_indent_moves_every_line_but_the_first() {
        let body = block_node(&scene_for(true), BODY_BLOCK).clone();
        assert_eq!(body.style.text_indent.amount_px, INDENT_PX, "same amount");
        assert!(body.style.text_indent.hanging);
        let starts = line_starts(&body);
        assert!(starts.len() > 1, "the body paragraph wraps: {starts:?}");
        assert!(starts[0].abs() < 0.5, "the first line is not: {starts:?}");
        #[allow(clippy::cast_precision_loss, reason = "a 28px indent is exact in f32")]
        let want = INDENT_PX as f32;
        assert!(
            starts[1..].iter().all(|x| (x - want).abs() < 0.5),
            "every continuation is indented: {starts:?}",
        );
    }

    /// The bibliography entry is hanging whatever the Toggle says — the fixed
    /// control that keeps the moving one honest.
    #[test]
    fn r1551_the_bibliography_entry_hangs_in_both_modes() {
        for hanging in [false, true] {
            let bib = block_node(&scene_for(hanging), BIB_BLOCK).clone();
            assert!(bib.style.text_indent.hanging, "mode={hanging}");
        }
    }

    /// R1551 §5.40 — the heading levels become an outline an assistive
    /// technology can navigate, named by the PAINTED text. The toolkit's
    /// accessible text interface has no method that reports block structure at
    /// all, so this is the half of `headingLevel` the toolkit does not have.
    #[test]
    fn r1551_the_headings_become_an_at_outline() {
        let scene = scene_for(false);
        let mut nodes = Vec::new();
        assert_eq!(pinion_a11y::attach_block_headings(&mut nodes, &scene), 2);
        assert_eq!(nodes[0].role, AriaRole::Heading);
        assert_eq!(nodes[0].level, Some(1));
        assert_eq!(nodes[0].name.as_deref(), Some(H1_TEXT));
        assert_eq!(nodes[1].level, Some(2));
        assert_eq!(nodes[1].name.as_deref(), Some(H2_TEXT));
    }

    /// R1551 §2#6 — the SAME declaration, painted through the terminal
    /// backend, indents the same line.
    ///
    /// Asserted here rather than only in `pinion-tui`'s own tests because what
    /// is claimed is a property of the DECLARATION: one `text_indent` on one
    /// paragraph reaches a GPU rasteriser and a cell grid from a view function
    /// that names neither.
    #[test]
    fn r1551_the_same_declaration_reaches_the_terminal_backend() {
        let mut node = block_node(&scene_for(false), BODY_BLOCK).clone();
        // 8px cells: a 28px indent is 3 cells, and a 320px box is 40 columns.
        node.rect = Rect::new(0, 0, 320, 64);
        let mut buf = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 60, 6));
        pinion_tui::paint::to_buffer(&Scene::Text(node.clone()), &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), " ", "the first line is indented");
        assert_eq!(buf[(3, 0)].symbol(), "A", "by three cells");
        assert_ne!(
            buf[(0, 1)].symbol(),
            " ",
            "and the second line is flush left",
        );
    }

    #[test]
    fn r1551_a11y_node_is_a_switch() {
        let nodes = <BlockFormatView as WidgetA11y>::access_node(&(ToggleState::Idle, false), None);
        assert_eq!(nodes[0].role, AriaRole::Switch);
        assert_eq!(nodes[0].tag, "main_toggle");
    }
}
