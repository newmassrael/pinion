//! `hello-richtext-list` — R1559 §5.36 consumer of the **list**: a document
//! whose items are numbered by their place among their siblings (Qt
//! `QTextList`).
//!
//! ## What this demonstrates
//!
//! Everything else a list has can be written by hand — the indent is a margin,
//! the bullet is a glyph. The **number** cannot, because it is not a property
//! of the item: it is a property of the item's position. So the binding is
//! built around exactly that, and the Toggle is what proves it:
//!
//! * an **ordered procedure** (`decimal`), with a **nested unordered list**
//!   (`disc`) under its first step. The nested list restarts at its own first
//!   marker and does NOT interrupt the outer numbering, which resumes
//!   underneath it;
//! * the Toggle **inserts a step into the middle** of that procedure. Nothing
//!   the author wrote changes for the steps that follow, and every one of them
//!   renumbers. That is the whole feature, visible in one click;
//! * a second list in `upper-roman` starting at 3999, where the second item is
//!   4000 — a value Roman numerals have **no standard form for**. Qt's
//!   `QTextList::itemText()` answers `"?"` there and the number is gone; CSS
//!   Counter Styles Level 3 says render through the fallback style, so it
//!   reads `4000.` and `scene/text_lists` names `Decimal` as the notation that
//!   wrote it.
//!
//! ## Verification (substrate-first)
//!
//! * `scene/text_lists` publishes each list with its items in order, their
//!   markers, their ordinals and where each marker was painted — a census Qt
//!   has no accessor for at all (finding a `QTextDocument`'s lists means
//!   walking every block and de-duplicating `textList()` pointers, in-process);
//! * `scene/snapshot` carries the same derivation on each paragraph node, so
//!   the two introspection channels check each other rather than restating one
//!   derivation;
//! * `scene/access` carries the WAI-ARIA `list` / `listitem` structure with
//!   `aria-posinset` / `aria-setsize` / `aria-level`. Qt's
//!   `QAccessibleTextInterface` has no method that reports block structure at
//!   all, so a Qt document's lists are invisible to a screen reader — and its
//!   bullets are painted geometry, so an unordered item does not even begin
//!   with a character an AT could read;
//! * this crate's own tests lay out and paint the SAME scene through the
//!   terminal backend. A marker is ordinary text, so the cell backend needed no
//!   list code — which is the §2 #6 claim made by a consumer.
//!
//! [`view_document`]: pinion_widget_paint::document::view_document

#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_core::external::IntrospectValue;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BlockFormat, BoxStyle, Color, FlexDirection, FontWeight, JustifyContent,
    LayoutStyle, Size, TextStyle,
};
use pinion_core::text_list::{ListFormat, ListSpec, ListStyle};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{ColorRole, Frame, Scene, WidgetCore, WidgetStateName, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;
use pinion_widget_paint::document::{TextBlock, view_document};

// pinion-forge codegen output: `pub struct HelloRichTextListRenderer` +
// async `new<...>` + sync `render` / `resize`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloRichTextListRenderer, HelloRichTextListRendererError);

const WIN_W: u32 = 600;
const WIN_H: u32 = 520;

const THEME_TAG: &str = "app";

/// The document's introspection handle. Every paragraph is
/// `DocumentTag::block(DOC_TAG, i)`, every list `DocumentTag::list(DOC_TAG, k)`.
const DOC_TAG: &str = "guide";

/// The width the document is laid out in. Fixed so the markers land in a known
/// column and the published boxes are a stable fact.
const DOC_W: u32 = 500;

const BODY_FONT_PX: u32 = 16;
const H1_FONT_PX: u32 = 23;
const STATUS_FONT_PX: u32 = 12;
const ROW_GAP: u32 = 14;

/// The procedure's marker gutter. Wide enough for `4.` at the body size.
const STEP_INDENT_PX: u32 = 34;
/// The nested list's gutter — narrower, because a bullet is narrower than a
/// number, and per-list because [`ListFormat::indent_px`] is per-list.
const BULLET_INDENT_PX: u32 = 22;
/// The roman list's gutter. `MMMCMXCIX.` is a wide marker, and a list that
/// declares its own indent is how a document makes room for one.
const ROMAN_INDENT_PX: u32 = 116;
/// Where the roman list's counter starts — one below the last value Roman
/// numerals have a standard form for, so its two items straddle the boundary.
const ROMAN_START: i32 = 3999;

/// Space above and below a heading.
const BLOCK_SPACE_PX: u32 = 10;

const H1_TEXT: &str = "Assembly";
const INTRO_TEXT: &str = "Work through the steps in order.";
const STEP_1: &str = "Unpack the parts and lay them out.";
const PART_A: &str = "two long bolts";
const PART_B: &str = "one hex key";
const STEP_INSERTED: &str = "Check them against the packing list.";
const STEP_BOLT: &str = "Bolt the frame together, finger tight.";
const STEP_TIGHTEN: &str = "Tighten in a star pattern.";
const NOTE_TEXT: &str = "Roman numerals stop where the notation stops.";
const ROMAN_A: &str = "the last numeral there is a form for";
const ROMAN_B: &str = "one past it, written the way CSS says";

/// M3 state-layer overlay weights for the switch chrome.
const HOVER_OVERLAY_T: f32 = 0.08;
const PRESSED_OVERLAY_T: f32 = 0.12;
const DISABLED_OVERLAY_T: f32 = 0.50;

/// The procedure's list format — one declaration shared by every step, which
/// is what makes them ONE list (the format is a list's identity).
#[must_use]
pub fn step_format() -> ListFormat {
    ListFormat::numbered().with_indent_px(STEP_INDENT_PX)
}

/// The nested parts list's format.
#[must_use]
pub fn part_format() -> ListFormat {
    ListFormat::bulleted().with_indent_px(BULLET_INDENT_PX)
}

/// The roman list's format, started at the edge of the notation's range.
#[must_use]
pub fn roman_format() -> ListFormat {
    ListFormat::new(ListStyle::UpperRoman)
        .with_start(ROMAN_START)
        .with_indent_px(ROMAN_INDENT_PX)
}

/// The document's blocks.
///
/// `checking` inserts one step into the MIDDLE of the procedure. Nothing else
/// in this function changes, which is the point: the markers of every later
/// step move because their position moved, not because anything restated them.
#[must_use]
pub fn blocks(base: &TextStyle, on_surface: Color, muted: Color, checking: bool) -> Vec<TextBlock> {
    let heading = base
        .clone()
        .with_size_px(H1_FONT_PX)
        .with_weight(FontWeight::BOLD)
        .with_fg(on_surface);
    let mut out = vec![
        TextBlock::new(H1_TEXT)
            .with_format(
                BlockFormat::new()
                    .with_heading_level(1)
                    .with_spacing(0, BLOCK_SPACE_PX),
            )
            .with_style(heading),
        TextBlock::new(INTRO_TEXT).with_style(base.clone().with_fg(muted)),
        TextBlock::new(STEP_1).in_list(ListSpec::new(step_format())),
        TextBlock::new(PART_A).in_list(ListSpec::new(part_format()).at_level(1)),
        TextBlock::new(PART_B).in_list(ListSpec::new(part_format()).at_level(1)),
    ];
    if checking {
        out.push(TextBlock::new(STEP_INSERTED).in_list(ListSpec::new(step_format())));
    }
    out.push(TextBlock::new(STEP_BOLT).in_list(ListSpec::new(step_format())));
    out.push(TextBlock::new(STEP_TIGHTEN).in_list(ListSpec::new(step_format())));
    out.push(TextBlock::new(NOTE_TEXT).with_style(base.clone().with_fg(muted)));
    out.push(TextBlock::new(ROMAN_A).in_list(ListSpec::new(roman_format())));
    out.push(TextBlock::new(ROMAN_B).in_list(ListSpec::new(roman_format())));
    out
}

/// view-fn (§6.3): pure sync mapping `(ToggleState, bool) -> Scene`.
/// `checking` selects whether the extra step is in the procedure.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ToggleState, checking: bool, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let accent = theme.resolve(ColorRole::Accent);

    let base = TextStyle::new()
        .with_size_px(BODY_FONT_PX)
        .with_fg(on_surface);
    let document = Scene::Container(
        view_document(DOC_TAG, &base, &blocks(&base, on_surface, muted, checking))
            .map_layout(|l| l.with_size(Size::width_px(DOC_W))),
    );

    let switch_base = if checking {
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
    let switch_fg = if checking {
        theme.resolve(ColorRole::OnAccent)
    } else {
        on_surface
    };
    let switch_label = Scene::Text(TextNode::styled(
        if checking {
            "Checking step: in"
        } else {
            "Checking step: out"
        },
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX + 2)
            .with_fg(switch_fg),
    ));
    let mode_chip = Scene::Container(
        ContainerNode::new(vec![switch_label])
            .with_tag("main_toggle")
            .with_aria_label("Insert the checking step")
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

    let steps = if checking { 4 } else { 3 };
    let status = Scene::Text(TextNode::styled(
        format!("{} | {steps} steps, 3 lists", state.as_name()),
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

/// `WidgetView` binding. The §5.38 Toggle is the "extra step present" bit.
///
/// [`WidgetCore`]: pinion_core::WidgetCore
/// [`WidgetA11y`]: pinion_a11y::WidgetA11y
/// [`WidgetView`]: pinion_shell::WidgetView
#[widget(
    tag = "main_toggle",
    state = (ToggleState, bool),
    event = ToggleEvent,
    title = "pinion hello-richtext-list (R1559 §5.36 list numbering)",
    renderer = HelloRichTextListRenderer,
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
    apply_key,
    keybinding,
)]
struct ListDocumentView;

impl ListDocumentView {
    /// Tuple-state introspect: SCXML state name via `query("state")` + the
    /// extra-step bit via `query("value")`. Defaults to `(Idle, false)`.
    fn read_state(scene: &Scene) -> (ToggleState, bool) {
        if let Scene::External(node) = scene
            && let Some(intro) = node.handle.introspect()
        {
            let state = if let Some(IntrospectValue::Text(name)) = intro.query("state") {
                ToggleState::from_name_or_default(&name)
            } else {
                ToggleState::Idle
            };
            let on = matches!(intro.query("value"), Some(IntrospectValue::Bool(true)));
            return (state, on);
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

    /// ARIA toggle-button keyboard activation (Space / Enter flips the
    /// Off / On sidecar in parity with a pointer click) — required of a
    /// `role = Switch`, and absent here until R1570.
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
    pinion_shell::run::<ListDocumentView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::composite_tag::DocumentTag;
    use pinion_runtime::{LayoutCache, compute_layout};

    fn with_owner<R>(f: impl FnOnce() -> R) -> R {
        let owner = pinion_core::reactive::Owner::new();
        owner.run(f)
    }

    fn scene_for(checking: bool) -> Scene {
        with_owner(|| view(ToggleState::Idle, checking, &Frame::new()))
    }

    /// The same scene the window paints, measured — so every assertion about a
    /// box is about a box the layout engine produced.
    fn laid_out(checking: bool) -> Scene {
        let mut scene = scene_for(checking);
        let mut cache = LayoutCache::new();
        compute_layout(&mut scene, &mut cache, WIN_W, WIN_H);
        scene
    }

    fn find_text<'a>(scene: &'a Scene, tag: &str) -> Option<&'a TextNode> {
        match scene {
            Scene::Text(t) if t.tag.as_deref() == Some(tag) => Some(t),
            Scene::Container(c) => c.children.iter().find_map(|c| find_text(c, tag)),
            Scene::Scroll(n) => find_text(&n.content, tag),
            _ => None,
        }
    }

    /// The painted marker of the paragraph whose text is `content`.
    ///
    /// Addressed by CONTENT rather than by block index, because inserting a
    /// step shifts every later index — which is the behaviour under test, so a
    /// test keyed on indices would be asserting against its own subject.
    fn marker_of(scene: &Scene, content: &str) -> String {
        let ink = Color::rgb(0, 0, 0);
        let count = blocks(&TextStyle::new(), ink, ink, true).len();
        for i in 0..count {
            if let Some(text) = find_text(scene, &DocumentTag::block(DOC_TAG, i))
                && text.content == content
            {
                return find_text(scene, &DocumentTag::marker(DOC_TAG, i))
                    .map(|m| m.content.clone())
                    .unwrap_or_default();
            }
        }
        panic!("no paragraph reads {content:?}");
    }

    /// The defining property, end to end: one click inserts a step and every
    /// step after it renumbers, while the steps before it do not move. Nothing
    /// in `blocks` states a number.
    #[test]
    fn r1559_inserting_a_step_renumbers_the_ones_after_it() {
        let before = scene_for(false);
        assert_eq!(marker_of(&before, STEP_1), "1.");
        assert_eq!(marker_of(&before, STEP_BOLT), "2.");
        assert_eq!(marker_of(&before, STEP_TIGHTEN), "3.");
        let after = scene_for(true);
        assert_eq!(
            marker_of(&after, STEP_1),
            "1.",
            "unchanged before the insert"
        );
        assert_eq!(marker_of(&after, STEP_INSERTED), "2.");
        assert_eq!(marker_of(&after, STEP_BOLT), "3.", "renumbered");
        assert_eq!(marker_of(&after, STEP_TIGHTEN), "4.", "renumbered");
    }

    /// The nested list restarts and does not interrupt its parent — the
    /// property that makes nesting usable, and the one an author maintaining
    /// numbers by hand gets wrong first.
    #[test]
    fn r1559_the_nested_list_restarts_without_interrupting_the_steps() {
        let scene = scene_for(false);
        assert_eq!(marker_of(&scene, PART_A), "\u{2022}");
        assert_eq!(marker_of(&scene, PART_B), "\u{2022}");
        let part = find_text(&scene, &DocumentTag::block(DOC_TAG, 3))
            .expect("the first part")
            .list
            .clone()
            .expect("placed");
        assert_eq!(part.level, 1);
        assert_eq!(part.count, 2, "the parts are their own set");
        let step = find_text(&scene, &DocumentTag::block(DOC_TAG, 2))
            .expect("the first step")
            .list
            .clone()
            .expect("placed");
        assert_eq!(step.count, 3, "and the steps are theirs");
        assert_eq!(
            part.parent_list_tag.as_deref(),
            Some(step.list_tag.as_str())
        );
    }

    /// The CSS range fallback, in the painted document: 3999 is a Roman
    /// numeral and 4000 is not, so 4000 is written in the fallback notation and
    /// the placement names it. Qt answers `"?"` and loses the value.
    #[test]
    fn r1559_a_value_past_the_roman_range_falls_back_to_decimal() {
        let scene = scene_for(false);
        assert_eq!(marker_of(&scene, ROMAN_A), "MMMCMXCIX.");
        assert_eq!(marker_of(&scene, ROMAN_B), "4000.");
        let last = find_text(&scene, &DocumentTag::block(DOC_TAG, 9))
            .expect("the second roman item")
            .list
            .clone()
            .expect("placed");
        assert_eq!(last.ordinal, ROMAN_START + 1);
        assert_eq!(last.position, 2, "position is unaffected by the fallback");
        assert_eq!(last.format.style, ListStyle::UpperRoman, "as DECLARED");
        assert_eq!(
            last.rendered_as,
            ListStyle::Decimal,
            "but written in the notation that could hold it",
        );
    }

    /// A marker lands in its own gutter, to the LEFT of the text it belongs
    /// to, and the two together occupy the declared indent. Measured, not
    /// assumed.
    #[test]
    fn r1559_a_marker_is_laid_out_in_the_gutter_before_its_text() {
        let scene = laid_out(false);
        let marker = find_text(&scene, &DocumentTag::marker(DOC_TAG, 2)).expect("a marker");
        let text = find_text(&scene, &DocumentTag::block(DOC_TAG, 2)).expect("a paragraph");
        assert!(
            marker.rect.x + marker.rect.w <= text.rect.x,
            "marker {:?} then text {:?}",
            marker.rect,
            text.rect,
        );
        assert_eq!(
            text.rect.x - marker.rect.x,
            STEP_INDENT_PX,
            "and the whole distance is what the list declared",
        );
        let bullet = find_text(&scene, &DocumentTag::marker(DOC_TAG, 3)).expect("a bullet");
        assert!(
            bullet.rect.x > marker.rect.x,
            "the nested list is inset past its parent's gutter: \
             bullet={:?} step={:?}",
            bullet.rect,
            marker.rect,
        );
    }

    /// R1559 §5.12 — the census reads the same derivation the paint did.
    #[test]
    fn r1559_the_wire_census_reports_the_three_lists() {
        let scene = laid_out(true);
        let lists = pinion_rpc::text_lists::collect_lists(&scene);
        assert_eq!(lists.len(), 3, "steps, parts, roman");
        assert_eq!(lists[0].style, "Decimal");
        assert_eq!(lists[0].count, 4, "with the inserted step");
        assert_eq!(lists[1].level, 1);
        assert_eq!(lists[1].parent_tag.as_deref(), Some(lists[0].tag.as_str()));
        assert_eq!(lists[2].style, "UpperRoman");
        assert_eq!(lists[2].start, ROMAN_START);
        assert!(lists[2].items[1].fell_back, "and the fall is reported");
        let marker_width = lists[0].items[0].marker_width.expect("laid out");
        assert!(marker_width > 0, "the marker's painted box is published");
    }

    /// R1559 §5.40 — the structure reaches assistive technology: `list`,
    /// `listitem`, and "N of M at level L". Qt's text a11y interface reports no
    /// block structure at all, so this half simply does not exist there.
    #[test]
    fn r1559_the_lists_reach_assistive_technology() {
        let scene = scene_for(false);
        let mut nodes = Vec::new();
        pinion_a11y::attach_block_lists(&mut nodes, &scene);
        let by_tag = |tag: &str| {
            nodes
                .iter()
                .find(|n| n.tag == tag)
                .unwrap_or_else(|| panic!("no node for {tag}"))
                .clone()
        };
        let first_step = by_tag(&DocumentTag::block(DOC_TAG, 2));
        assert_eq!(first_step.role, AriaRole::ListItem);
        assert_eq!(first_step.position_in_set, Some(1));
        assert_eq!(first_step.size_of_set, Some(3));
        assert_eq!(first_step.level, Some(1));
        let nested = by_tag(&DocumentTag::block(DOC_TAG, 3));
        assert_eq!(nested.level, Some(2), "one level in");
        assert_eq!(nested.size_of_set, Some(2));
        let list = by_tag(&DocumentTag::list(DOC_TAG, 0));
        assert_eq!(list.role, AriaRole::List);
        assert_eq!(list.size_of_set, Some(3));
        // The heading is not an item, and the intro paragraph is not either.
        assert!(
            !nodes
                .iter()
                .any(|n| n.tag == DocumentTag::block(DOC_TAG, 0)),
            "the heading is outside every list",
        );
    }

    /// R1559 §2#6 — the SAME scene, painted through the terminal backend, puts
    /// the same markers on screen.
    ///
    /// Asserted here rather than only in `pinion-tui`'s own tests because what
    /// is claimed is a property of the DERIVATION: a marker is ordinary text,
    /// so the cell backend needed no list code to draw one. If a bullet were
    /// painted geometry — as Qt's is — there would be nothing for a terminal
    /// to put in the cell.
    #[test]
    fn r1559_the_same_markers_reach_the_terminal_backend() {
        let scene = laid_out(false);
        let mut buf = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 90, 40));
        pinion_tui::paint::to_buffer(&scene, &mut buf);
        let painted: String = buf
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        for marker in ["1.", "2.", "3.", "\u{2022}", "MMMCMXCIX.", "4000."] {
            assert!(
                painted.contains(marker),
                "the terminal painted {marker:?}: {painted:?}",
            );
        }
    }

    #[test]
    fn r1559_a11y_node_is_a_switch() {
        let nodes =
            <ListDocumentView as WidgetA11y>::access_node(&(ToggleState::Idle, false), None);
        assert_eq!(nodes[0].role, AriaRole::Switch);
        assert_eq!(nodes[0].tag, "main_toggle");
    }
}
