//! `hello-richtext-cells` — R1560 §5.36 consumer of the **table**: a document
//! whose cells are addressed by their place in the flow (Qt `QTextTable`).
//!
//! ## What this demonstrates
//!
//! Everything else a table has can be written by hand — a border is a stroke, a
//! padding is an inset, a column width is a length. A cell's **address** cannot,
//! because it is not a property of the cell: it is where the cell lands once
//! every earlier cell's spans have taken their slots. So the binding is built
//! around exactly that, and the Toggle is what proves it:
//!
//! * a **timetable** with a header row and a header column, one cell that spans
//!   two rows (the room booked for two days), and a last row that simply stops;
//! * the Toggle **inserts a note into the middle** of the table. Nothing the
//!   author wrote changes for the cells that follow, and every one of them
//!   re-addresses — a whole row further down. That is the feature, in one
//!   click;
//! * the note declares a span of **nine columns** in a three-column table. Qt's
//!   `QTextTable::mergeCells` returns `void` and silently does nothing when a
//!   merge does not fit; here the span is clamped to the free run and the ask
//!   survives beside the result, so `scene/text_tables` reports
//!   `column_span: 3, declared_column_span: 9, clamped: true`;
//! * Thursday asks for the table's full width and gets ONE column, because the
//!   second room booking reaches down into its row. The clamp is against the
//!   free run, not the row's remaining width — two different numbers, and the
//!   only pair that tells the two rules apart;
//! * that leaves the topic slot of the last row with no cell at all — a state
//!   `QTextTable` cannot be in, because `insertRows` fills its grid.
//!
//! ## Verification (substrate-first)
//!
//! * `scene/text_tables` publishes each table with its shape, its slack and
//!   every cell's address and painted box — a census Qt has no accessor for at
//!   all (finding a `QTextDocument`'s tables means walking the frame tree
//!   `qobject_cast`-ing each child, in-process);
//! * `scene/snapshot` carries the same derivation on each paragraph node, so
//!   the two introspection channels check each other rather than restating one
//!   derivation;
//! * `scene/access` carries the WAI-ARIA `table` / `row` / `cell` structure
//!   with `aria-rowindex` / `aria-colindex` / `aria-rowspan` and the
//!   `columnheader` / `rowheader` bands. A `QTextTable` reaches no
//!   accessibility interface at all — `QAccessibleTextInterface` has no method
//!   that reports block structure — so to a screen reader a Qt document's table
//!   is an undifferentiated run of paragraphs;
//! * this crate's own tests lay out and paint the SAME scene through the
//!   terminal backend. A table is an ordinary grid container, so the cell
//!   backend needed no table code — which is the §2 #6 claim made by a
//!   consumer.
//!
//! [`view_document`]: pinion_widget_paint::document::view_document

#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_core::external::IntrospectValue;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BlockFormat, BoxStyle, Color, FlexDirection, FontWeight, GridTrack, JustifyContent,
    LayoutStyle, Size, TextStyle,
};
use pinion_core::text_table::{CellSpec, TableFormat};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{ColorRole, Frame, Scene, WidgetCore, WidgetStateName, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;
use pinion_widget_paint::document::{TextBlock, view_document};

// pinion-forge codegen output: `pub struct HelloRichTextCellsRenderer` +
// async `new<...>` + sync `render` / `resize`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloRichTextCellsRenderer, HelloRichTextCellsRendererError);

const WIN_W: u32 = 620;
const WIN_H: u32 = 520;

const THEME_TAG: &str = "app";

/// The document's introspection handle. Every paragraph is
/// `DocumentTag::block(DOC_TAG, i)`, every table `DocumentTag::table(DOC_TAG, k)`,
/// every cell `DocumentTag::cell(DOC_TAG, i)`.
const DOC_TAG: &str = "week";

/// The width the document is laid out in. Fixed so the columns land in known
/// bands and the published boxes are a stable fact.
const DOC_W: u32 = 540;

const BODY_FONT_PX: u32 = 16;
const H1_FONT_PX: u32 = 23;
const STATUS_FONT_PX: u32 = 12;
const ROW_GAP: u32 = 14;

/// The timetable's column count. Three, so a nine-column span has something to
/// be clamped against.
const COLUMNS: u16 = 3;
/// The day column's width. Fixed, because a row label reads better in a band
/// that does not move when a topic gets longer — Qt's
/// `QTextLength::FixedLength`.
const DAY_COL_PX: u32 = 90;
/// The room column's width, as a fraction of what is left over — CSS `fr`,
/// which Qt's column constraints have no equivalent for.
const ROOM_COL_FR: f32 = 1.0;
/// The topic column's share. Twice the room's, so the wide text gets the space.
const TOPIC_COL_FR: f32 = 2.0;

const CELL_PADDING_PX: u32 = 6;
const CELL_SPACING_PX: u32 = 0;
const BORDER_PX: u32 = 1;

/// The span the note asks for — far more columns than the table has, so the
/// clamp against the row's WIDTH has something to report.
const NOTE_DECLARED_SPAN: u16 = 9;
/// The span Thursday asks for. Exactly the table's width, so what stops it is
/// not the row's width at all: it is the room booking that reaches down into
/// that row. Clamping against the row's remainder would leave it at three, so
/// this is the cell that tells the two clamps apart.
const THU_DECLARED_SPAN: u16 = 3;

/// Space above and below a heading.
const BLOCK_SPACE_PX: u32 = 10;

const H1_TEXT: &str = "This week";
const INTRO_TEXT: &str = "Rooms are held for the whole booking.";
const HEAD_DAY: &str = "Day";
const HEAD_ROOM: &str = "Room";
const HEAD_TOPIC: &str = "Topic";
const MON: &str = "Mon";
const ROOM_A: &str = "A1";
const TOPIC_KICKOFF: &str = "Kickoff";
const KICKOFF_NOTE: &str = "bring the printed agenda";
const TUE: &str = "Tue";
const TOPIC_DESIGN: &str = "Design review";
const NOTE_TEXT: &str = "Thursday is provisional.";
const WED: &str = "Wed";
const ROOM_B: &str = "B2";
const TOPIC_WRAP: &str = "Wrap-up";
const THU: &str = "Thu";

/// M3 state-layer overlay weights for the switch chrome.
const HOVER_OVERLAY_T: f32 = 0.08;
const PRESSED_OVERLAY_T: f32 = 0.12;
const DISABLED_OVERLAY_T: f32 = 0.50;

/// The timetable's format — one declaration shared by every cell, which is what
/// makes them ONE table (the format is a table's identity, exactly as a
/// `ListFormat` is a list's).
///
/// The header bands are declared here and nowhere else: which cells are headers
/// is then a function of where they land, so inserting the note does not need a
/// single `<th>` to be restated.
#[must_use]
pub fn week_format(rule: Color) -> TableFormat {
    TableFormat::new(COLUMNS)
        .with_column_widths(vec![
            GridTrack::Px(DAY_COL_PX),
            GridTrack::Fr(ROOM_COL_FR),
            GridTrack::Fr(TOPIC_COL_FR),
        ])
        .with_header_rows(1)
        .with_header_columns(1)
        .with_metrics(CELL_PADDING_PX, CELL_SPACING_PX)
        .with_border(BORDER_PX, rule)
}

/// A one-slot cell of the timetable, ruled in `rule`.
fn cell(text: &str, rule: Color) -> TextBlock {
    TextBlock::new(text).in_cell(CellSpec::new(week_format(rule)))
}

/// The document's blocks.
///
/// `noting` inserts one cell into the MIDDLE of the table. Nothing else in this
/// function changes, which is the point: every later cell's address moves
/// because its position moved, not because anything restated it.
#[must_use]
pub fn blocks(base: &TextStyle, on_surface: Color, muted: Color, noting: bool) -> Vec<TextBlock> {
    let heading = base
        .clone()
        .with_size_px(H1_FONT_PX)
        .with_weight(FontWeight::BOLD)
        .with_fg(on_surface);
    let header = base
        .clone()
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
        // The header row. Nothing marks these as headers — the format's
        // `header_rows` does, and only because they land in row 0.
        cell(HEAD_DAY, muted).with_style(header.clone()),
        cell(HEAD_ROOM, muted).with_style(header.clone()),
        cell(HEAD_TOPIC, muted).with_style(header),
        cell(MON, muted),
        // The room is booked for two days, so its cell reaches down into a row
        // that has not been written yet — and Tuesday's cells step around it.
        TextBlock::new(ROOM_A).in_cell(CellSpec::new(week_format(muted)).spanning_rows(2)),
        cell(TOPIC_KICKOFF, muted),
        // A second paragraph in the SAME cell — Qt's cell is a frame of blocks,
        // and this is the flat-sequence spelling of that.
        TextBlock::new(KICKOFF_NOTE)
            .with_style(base.clone().with_fg(muted))
            .in_cell(CellSpec::new(week_format(muted)).continued()),
        cell(TUE, muted),
        cell(TOPIC_DESIGN, muted),
    ];
    if noting {
        out.push(
            TextBlock::new(NOTE_TEXT)
                .with_style(base.clone().with_fg(muted))
                .in_cell(CellSpec::new(week_format(muted)).spanning_columns(NOTE_DECLARED_SPAN)),
        );
    }
    out.push(cell(WED, muted));
    // The second room is booked into Thursday as well, which is what makes
    // Thursday's own cell unable to widen below.
    out.push(TextBlock::new(ROOM_B).in_cell(CellSpec::new(week_format(muted)).spanning_rows(2)));
    out.push(cell(TOPIC_WRAP, muted));
    // Thursday asks to run the width of the table and cannot: the room booking
    // above holds the slot beside it, so the span is clamped to the FREE RUN
    // rather than to the row's remaining width — two different numbers here,
    // which is the only way to tell the two clamps apart. The topic slot is
    // left empty, so the table is ragged as well.
    out.push(
        TextBlock::new(THU)
            .in_cell(CellSpec::new(week_format(muted)).spanning_columns(THU_DECLARED_SPAN)),
    );
    out
}

/// view-fn (§6.3): pure sync mapping `(ToggleState, bool) -> Scene`.
/// `noting` selects whether the note cell is in the table.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ToggleState, noting: bool, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let accent = theme.resolve(ColorRole::Accent);

    let base = TextStyle::new()
        .with_size_px(BODY_FONT_PX)
        .with_fg(on_surface);
    let document = Scene::Container(
        view_document(DOC_TAG, &base, &blocks(&base, on_surface, muted, noting))
            .map_layout(|l| l.with_size(Size::width_px(DOC_W))),
    );

    let switch_base = if noting {
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
    let switch_fg = if noting {
        theme.resolve(ColorRole::OnAccent)
    } else {
        on_surface
    };
    let switch_label = Scene::Text(TextNode::styled(
        if noting {
            "Note row: in"
        } else {
            "Note row: out"
        },
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX + 2)
            .with_fg(switch_fg),
    ));
    let mode_chip = Scene::Container(
        ContainerNode::new(vec![switch_label])
            .with_tag("main_toggle")
            .with_aria_label("Insert the provisional note")
            .with_style(BoxStyle::filled(switch_fill).with_corner_radius(18))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(190, 36)),
            ),
    );

    let rows = if noting { 6 } else { 5 };
    let status = Scene::Text(TextNode::styled(
        format!("{} | {rows} rows, {COLUMNS} columns", state.as_name()),
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

/// `WidgetView` binding. The §5.38 Toggle is the "note cell present" bit.
///
/// [`WidgetCore`]: pinion_core::WidgetCore
/// [`WidgetA11y`]: pinion_a11y::WidgetA11y
/// [`WidgetView`]: pinion_shell::WidgetView
#[widget(
    tag = "main_toggle",
    state = (ToggleState, bool),
    event = ToggleEvent,
    title = "pinion hello-richtext-cells (R1560 §5.36 cell addressing)",
    renderer = HelloRichTextCellsRenderer,
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
struct TableDocumentView;

impl TableDocumentView {
    /// Tuple-state introspect: SCXML state name via `query("state")` + the
    /// note-cell bit via `query("value")`. Defaults to `(Idle, false)`.
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
    pinion_shell::run::<TableDocumentView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::composite_tag::DocumentTag;
    use pinion_core::text_table::CellPlacement;
    use pinion_runtime::{LayoutCache, compute_layout};

    fn with_owner<R>(f: impl FnOnce() -> R) -> R {
        let owner = pinion_core::reactive::Owner::new();
        owner.run(f)
    }

    fn scene_for(noting: bool) -> Scene {
        with_owner(|| view(ToggleState::Idle, noting, &Frame::new()))
    }

    /// The same scene the window paints, measured — so every assertion about a
    /// box is about a box taffy's CSS Grid produced.
    fn laid_out(noting: bool) -> Scene {
        let mut scene = scene_for(noting);
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

    /// The placement of the paragraph whose text is `content`.
    ///
    /// Addressed by CONTENT rather than by block index, because inserting the
    /// note shifts every later index — which is the behaviour under test, so a
    /// test keyed on indices would be asserting against its own subject.
    fn placement_of(scene: &Scene, content: &str) -> CellPlacement {
        let ink = Color::rgb(0, 0, 0);
        let count = blocks(&TextStyle::new(), ink, ink, true).len();
        for i in 0..count {
            if let Some(text) = find_text(scene, &DocumentTag::block(DOC_TAG, i))
                && text.content == content
            {
                return *text.cell.clone().expect("the paragraph is in a cell");
            }
        }
        panic!("no paragraph reads {content:?}");
    }

    fn address_of(scene: &Scene, content: &str) -> (u32, u32) {
        let placement = placement_of(scene, content);
        (placement.row, placement.column)
    }

    /// The defining property, end to end: one click inserts a cell and every
    /// cell after it re-addresses, while the cells before it do not move.
    /// Nothing in `blocks` states an address.
    #[test]
    fn r1560_inserting_a_cell_re_addresses_the_ones_after_it() {
        let before = scene_for(false);
        assert_eq!(address_of(&before, HEAD_DAY), (0, 0));
        assert_eq!(address_of(&before, WED), (3, 0));
        assert_eq!(address_of(&before, TOPIC_WRAP), (3, 2));
        assert_eq!(address_of(&before, THU), (4, 0));
        let after = scene_for(true);
        assert_eq!(
            address_of(&after, HEAD_DAY),
            (0, 0),
            "unchanged before the insert",
        );
        assert_eq!(address_of(&after, TOPIC_DESIGN), (2, 2), "also unchanged");
        assert_eq!(address_of(&after, NOTE_TEXT), (3, 0));
        assert_eq!(address_of(&after, WED), (4, 0), "re-addressed");
        assert_eq!(address_of(&after, TOPIC_WRAP), (4, 2), "re-addressed");
        assert_eq!(address_of(&after, THU), (5, 0), "re-addressed");
    }

    /// A cell that reaches down into a row nobody has written yet pushes the
    /// next row's cells aside — the case a nest of flex rows cannot express and
    /// Qt makes the caller `mergeCells` by hand.
    #[test]
    fn r1560_a_two_day_booking_pushes_tuesdays_cells_aside() {
        let scene = scene_for(false);
        let room = placement_of(&scene, ROOM_A);
        assert_eq!((room.row, room.column), (1, 1));
        assert_eq!(room.row_span, 2);
        assert_eq!(address_of(&scene, TUE), (2, 0));
        assert_eq!(
            address_of(&scene, TOPIC_DESIGN),
            (2, 2),
            "column 1 is held by the booking, so the topic lands in column 2",
        );
    }

    /// The span that does not fit is clamped, and the ask survives beside the
    /// result — the distinction Qt's `void mergeCells` throws away.
    #[test]
    fn r1560_the_notes_impossible_span_is_clamped_and_named() {
        let scene = scene_for(true);
        let note = placement_of(&scene, NOTE_TEXT);
        assert_eq!(note.declared_column_span, NOTE_DECLARED_SPAN);
        assert_eq!(note.column_span, COLUMNS);
        assert!(note.clamped());
        assert!(
            !placement_of(&scene, WED).clamped(),
            "and an ordinary cell is not reported as clamped",
        );
    }

    /// The other clamp, and the one that tells the rule apart: Thursday asks
    /// for exactly the table's width, so a clamp against the row's REMAINDER
    /// would grant it. What stops it is the room booked into its row, and the
    /// span it gets is the free run.
    #[test]
    fn r1560_thursday_is_clamped_by_the_booking_not_by_the_row_width() {
        let scene = scene_for(false);
        let thu = placement_of(&scene, THU);
        assert_eq!(thu.declared_column_span, THU_DECLARED_SPAN);
        assert_eq!(u32::from(thu.column_span), 1, "the free run beside it");
        assert!(thu.clamped());
        let booking = placement_of(&scene, ROOM_B);
        assert_eq!(
            booking.row_span, 2,
            "because the room is held into Thursday"
        );
        assert_eq!((booking.row, booking.column), (3, 1));
    }

    /// Header-ness is derived from the address: the corner cell, the column
    /// labels and the row labels are all headers because of where they landed,
    /// and no cell declares it.
    #[test]
    fn r1560_the_header_bands_are_derived_from_the_address() {
        use pinion_core::text_table::HeaderScope;
        let scene = scene_for(false);
        assert_eq!(placement_of(&scene, HEAD_DAY).header, HeaderScope::Corner);
        assert_eq!(placement_of(&scene, HEAD_ROOM).header, HeaderScope::Column);
        assert_eq!(placement_of(&scene, MON).header, HeaderScope::Row);
        assert_eq!(
            placement_of(&scene, TOPIC_KICKOFF).header,
            HeaderScope::None
        );
    }

    /// Two paragraphs, one cell: the continuation joins the box its opener
    /// made rather than taking a slot of its own.
    #[test]
    fn r1560_the_kickoff_note_shares_its_cells_box() {
        let scene = scene_for(false);
        let topic = placement_of(&scene, TOPIC_KICKOFF);
        let note = placement_of(&scene, KICKOFF_NOTE);
        assert_eq!(note.cell_tag, topic.cell_tag, "one cell, two paragraphs");
        assert_eq!((note.row, note.column), (topic.row, topic.column));
        assert!(topic.opens_cell);
        assert!(!note.opens_cell);
    }

    /// The columns line up across rows because the tracks are sized once for
    /// the whole grid — the property a column of flex rows cannot have. The day
    /// column is the width the format declared, and the topic column takes
    /// twice the room column's share of what is left.
    #[test]
    fn r1560_the_declared_tracks_reach_the_laid_out_columns() {
        let scene = laid_out(false);
        let head_day = find_text(&scene, &DocumentTag::block(DOC_TAG, 2)).expect("Day");
        let head_room = find_text(&scene, &DocumentTag::block(DOC_TAG, 3)).expect("Room");
        let head_topic = find_text(&scene, &DocumentTag::block(DOC_TAG, 4)).expect("Topic");
        let day_left = head_day.rect.x;
        assert_eq!(
            head_room.rect.x - day_left,
            DAY_COL_PX,
            "the fixed column is the width the format declared",
        );
        let room_w = head_topic.rect.x - head_room.rect.x;
        let topic_w = DOC_W - (head_topic.rect.x - day_left);
        assert!(
            topic_w > room_w,
            "the 2fr column is wider than the 1fr one ({topic_w} vs {room_w})",
        );
        // Every cell in a column starts at that column's left edge, which is
        // what "the columns line up" means and what makes it a table.
        let wed_room = find_text(&scene, &DocumentTag::block(DOC_TAG, 12)).expect("B2");
        assert_eq!(wed_room.rect.x, head_room.rect.x);
    }

    /// §5.12 — the census reports the same table the paint drew, including the
    /// slots nobody filled.
    #[test]
    fn r1560_the_wire_reports_the_shape_and_the_slack() {
        let scene = laid_out(false);
        let tables = pinion_rpc::text_tables::collect_tables(&scene);
        assert_eq!(tables.len(), 1, "one table");
        let table = &tables[0];
        assert_eq!(table.tag, DocumentTag::table(DOC_TAG, 0));
        assert_eq!((table.rows, table.columns), (5, u32::from(COLUMNS)));
        assert_eq!(
            table.column_widths,
            [
                format!("{DAY_COL_PX}px"),
                "1fr".to_string(),
                "2fr".to_string(),
            ]
        );
        assert_eq!(
            table
                .slack
                .iter()
                .map(|s| (s.row, s.column))
                .collect::<Vec<_>>(),
            [(4, 2)],
            "the week has no topic for Thursday",
        );
        let booking = table
            .cells
            .iter()
            .find(|c| c.row_span > 1)
            .expect("the two-day booking");
        assert_eq!((booking.row, booking.column, booking.row_span), (1, 1, 2));
        assert!(
            booking.height.is_some_and(|h| h > 0),
            "and the census carries the box the grid gave it",
        );
    }

    /// §5.40 — the structure reaches an assistive technology THROUGH THE
    /// ASSEMBLER.
    ///
    /// Called through `build_access_tree` rather than by calling the pass
    /// directly, which is R1559's lesson repaid: that round wired a new
    /// `attach_*` pass into nothing and six unit tests plus an example test all
    /// passed, because every one of them called the pass itself. A test that
    /// invokes the derivation cannot see that nobody invokes the derivation.
    #[test]
    fn r1560_the_assembler_announces_the_table() {
        let scene = laid_out(false);
        let (nodes, _) = with_owner(|| {
            let owner = pinion_core::reactive::Owner::new();
            pinion_a11y::build_access_tree(&owner, Some(&scene), Vec::new, || None)
        });
        let table = nodes
            .iter()
            .find(|n| n.tag == DocumentTag::table(DOC_TAG, 0))
            .expect("the table is announced");
        assert_eq!(table.role, AriaRole::Table);
        assert_eq!(table.row_count, Some(5));
        assert_eq!(table.column_count, Some(u32::from(COLUMNS)));
        let cells: Vec<_> = nodes
            .iter()
            .filter(|n| {
                matches!(
                    n.role,
                    AriaRole::Cell | AriaRole::ColumnHeader | AriaRole::RowHeader
                )
            })
            .collect();
        assert_eq!(cells.len(), 12, "every cell of the timetable");
        let booking = nodes
            .iter()
            .find(|n| n.row_span == Some(2))
            .expect("the two-day booking announces its span");
        assert_eq!(
            (booking.row_index, booking.column_index),
            (Some(2), Some(2))
        );
        assert_eq!(
            booking.name.as_deref(),
            Some(ROOM_A),
            "and it is named from the text that was painted in it",
        );
        assert!(
            nodes
                .iter()
                .any(|n| n.role == AriaRole::RowHeader && n.name.as_deref() == Some(MON)),
            "the day column is a header band",
        );
    }

    /// §2 #6 — the SAME scene paints through the terminal backend. A table is
    /// an ordinary grid container, so `pinion-tui` needed no table code; the
    /// cells' text lands in the terminal buffer with the columns still lined
    /// up.
    #[test]
    fn r1560_the_table_paints_through_the_cell_backend() {
        const COLS: u16 = 90;
        let scene = laid_out(false);
        let mut buf = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, COLS, 40));
        pinion_tui::paint::to_buffer(&scene, &mut buf);
        let cells: Vec<&str> = buf
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        let rows: Vec<String> = cells.chunks(COLS as usize).map(<[&str]>::concat).collect();
        let screen = rows.join("\n");
        for text in [HEAD_DAY, HEAD_ROOM, HEAD_TOPIC, MON, ROOM_A, THU] {
            assert!(screen.contains(text), "{text:?} is missing from\n{screen}");
        }
        let day_row = rows
            .iter()
            .find(|line| line.contains(MON))
            .expect("Monday's row");
        let head_row = rows
            .iter()
            .find(|line| line.contains(HEAD_DAY))
            .expect("the header row");
        assert_eq!(
            day_row.find(MON),
            head_row.find(HEAD_DAY),
            "the day column begins in one terminal column on both rows",
        );
    }

    #[test]
    fn r1560_a11y_node_is_a_switch() {
        let nodes =
            <TableDocumentView as WidgetA11y>::access_node(&(ToggleState::Idle, false), None);
        assert_eq!(nodes[0].role, AriaRole::Switch);
        assert_eq!(nodes[0].tag, "main_toggle");
    }
}
