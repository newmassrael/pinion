//! R1851 §5.16 §5.27 §5.40 — a **sortable column header over a virtualised
//! feed** whose rows the caller draws.
//!
//! # The hole this fills, measured
//!
//! Both halves of this composition already existed and nothing composed them.
//! Measured at R1851 over the whole tree, by file:
//!
//! ```text
//! grep -rl 'view_virtual_list\|view_variable_virtual_list\|view_flex_virtual_list\|view_measured_list' crates/ examples/ --include=*.rs
//! grep -rl 'view_header_cell\|header_label_node' crates/ examples/ --include=*.rs
//! ```
//!
//! The first answered **27** files. The second answered **three**, of which one
//! is a screenshot harness and one a shell test — so exactly ONE screen in this
//! workspace had ever drawn a column header. And the only surface holding both
//! is [`crate::table`], a full data grid, where every row is a row of CELLS. A
//! feed is not that: its row is a shape (a severity swatch beside a graded word,
//! a clock reading and a message, in the reference this framework reproduces),
//! and a grid cannot draw one. So a screen wanting a sortable header over
//! caller-drawn rows had to wire the header to the list by hand, and the three
//! places that wiring goes wrong are the three this module removes.
//!
//! ⚠ The counts are written as the COMMANDS above rather than as prose, because
//! this project has measured repeatedly that a number in a comment starts rotting
//! the moment it is written. The figures beside them are what those commands
//! answered on the day.
//!
//! # ★★★★★ The three facts that must not be read twice
//!
//! **1. The sort indicator is DERIVED from the sort state.** The glyph comes out
//! of [`sort_glyph`] applied to
//! [`col_sort_dir`] over the same
//! `sort` the caller ordered its rows by, so the arrow cannot point one way
//! while the rows run the other. On the toolkit floor at 6.11.1 the two are
//! separate properties of separate objects — probed at R1851, the indicator sits
//! on the header and the order on the view — and they stay in step only because
//! enabling sorting connects one to the other. Wire them by hand and nothing
//! does.
//!
//! **2. The body's viewport is the given rect MINUS the header.** A feed whose
//! rows are laid out over the whole rect draws its first row under the header
//! strip, and the row that is hidden is the one the sort just brought to the
//! top — the failure is invisible precisely when the feature is working.
//! [`HeaderFeed::body_viewport`] is the one place that subtraction happens.
//!
//! **3. The window an assistive reader is told about is the window that was
//! built.** A virtualised feed announces the rows it constructed, so the a11y
//! builder needs the same range the painter used. [`HeaderFeed::window`] is that
//! range, asked once and answered to both — the alternative is two computations
//! that agree until one of them is edited, which is this project's most
//! frequently measured defect shape.
//!
//! # What stays the caller's
//!
//! The row, and the ORDER. `build_row` is handed a rect and the column
//! placements the header used, so a cell and its heading share one arithmetic
//! ([`HeaderFeed::placements`]) instead of two. What the row puts in those
//! columns is the caller's, which is what keeps this a feed rather than a grid.
//! Filtering and ordering stay
//! [`compute_order`](pinion_core::widgets::view_order::compute_order)'s, which
//! has been the permutation SSOT since R747; this assembly takes a count and an
//! index and never sorts anything itself.

use std::rc::Rc;

use pinion_core::scene::{ContainerNode, Rect, Scene};
use pinion_core::style::{LayoutStyle, Size, TextAlign};
use pinion_core::theme::Theme;
use pinion_core::voice::Silence;
use pinion_core::widgets::column_layout::{SectionPlacement, SectionSelection};
use pinion_core::widgets::grid_sort::col_sort_dir;
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::virtual_list::{VisibleWindow, compute_visible_range};

use crate::column_header::{ColumnHeaderStyle, HeaderSection, view_header_cell};
use crate::glyph::sort_glyph;
use crate::virtual_list::view_virtual_list;

/// One column of a feed's header.
#[derive(Debug, Clone, Copy)]
pub struct FeedColumn<'a> {
    /// The heading a reader sees.
    pub label: &'a str,
    /// The column's width in logical pixels, header and rows alike.
    pub size: u32,
    /// Where the heading sits inside its box.
    pub align: TextAlign,
    /// Whether this column can be sorted on.
    ///
    /// A column that cannot sort never shows a glyph even when `sort` names it,
    /// so a stale or out-of-range sort state reads as *no indicator* rather than
    /// as an arrow over a column that does not respond.
    pub sortable: bool,
}

impl<'a> FeedColumn<'a> {
    /// A sortable, leading-aligned column — what a feed's columns usually are.
    #[must_use]
    pub const fn new(label: &'a str, size: u32) -> Self {
        Self {
            label,
            size,
            align: TextAlign::Start,
            sortable: true,
        }
    }

    /// The same column, but not sortable.
    #[must_use]
    pub const fn fixed(mut self) -> Self {
        self.sortable = false;
        self
    }

    /// The same column, aligned differently.
    #[must_use]
    pub const fn aligned(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }
}

/// Geometry of a header-plus-feed assembly.
#[derive(Debug, Clone, Copy)]
pub struct HeaderFeedStyle {
    /// The header strip's own style — its height is what the body gives up.
    pub header: ColumnHeaderStyle,
    /// One row's vertical slot, in logical pixels.
    pub row_pitch: u32,
    /// Rows built beyond the strict window on each side.
    pub overscan: usize,
    /// Whether the body's height comes DOWN to a whole number of rows.
    ///
    /// ★★★★★ Off by default, because a scrolling list showing a half row at its
    /// edge is what scrolling looks like. On for a feed that sits in a fixed box
    /// and is read as a table, where a half-drawn row is worse than an empty
    /// strip: its words are clipped, so it is a row that LOOKS present and
    /// cannot be read — and every word census over the paint then reports a row
    /// painting nothing, which is a true report of a real defect.
    pub whole_rows: bool,
}

impl HeaderFeedStyle {
    /// A feed of `row_pitch`-tall rows under the header's measured defaults.
    #[must_use]
    pub const fn new(row_pitch: u32) -> Self {
        Self {
            header: ColumnHeaderStyle::new(),
            row_pitch,
            overscan: 1,
            whole_rows: false,
        }
    }

    /// The same geometry with a shorter header strip.
    ///
    /// ★★★★★ The label's vertical inset comes down with it, and the number it
    /// comes down to is the LINE BOX of the label's own face
    /// ([`pinion_core::containment::line_box`]) rather than the type size. The
    /// header's default insets are measured for a 40px strip and leave a label
    /// box of `height - 2 * label_y`; at any shorter height that box is under
    /// the line its own face needs, so the glyph does not fit in the box it was
    /// given — a defect a caller lowering the strip alone would get with no
    /// complaint, and one this workspace has a census counting.
    /// ⚠ And the strip is never SHORTER than the line its face needs. A caller
    /// asking for twenty pixels with a thirteen-pixel face is asking for a box
    /// no glyph of that face fits in: `label_y` would saturate to zero and leave
    /// the label one pixel short whatever it did. Growing to the line is the
    /// graceful answer and it is stated rather than silent — the alternative,
    /// honouring the request, produces a header that draws clipped text and says
    /// nothing, which is the defect this whole builder exists to remove.
    #[must_use]
    pub const fn with_header_height(mut self, height: u32) -> Self {
        let line = pinion_core::containment::line_box(self.header.text_px);
        self.header.height = if height > line { height } else { line };
        self.header.label_y = self.header.height.saturating_sub(line) / 2;
        self
    }

    /// The same geometry with a different label type size.
    ///
    /// Re-derives the label inset, because the inset is a function of the face.
    #[must_use]
    pub const fn with_header_text_px(mut self, text_px: u32) -> Self {
        self.header.text_px = text_px;
        self.with_header_height(self.header.height)
    }

    /// The same geometry with a different overscan.
    #[must_use]
    pub const fn with_overscan(mut self, overscan: usize) -> Self {
        self.overscan = overscan;
        self
    }

    /// The same geometry, showing only whole rows. See
    /// [`whole_rows`](Self::whole_rows).
    #[must_use]
    pub const fn with_whole_rows(mut self) -> Self {
        self.whole_rows = true;
        self
    }
}

/// A header strip over a virtualised body, assembled as one scene.
///
/// A builder rather than a free function because the assembly has three
/// independent optional facts (the sort, the keyboard cursor, the viewport's
/// declared silence) and four derived quantities a caller needs *before* it can
/// build a row — the placements, the body rect, how many rows fit, and the
/// window that will actually be constructed. A free function taking all of them
/// would be nine parameters and would still not let a caller ask for the window
/// without building the scene.
pub struct HeaderFeed<'a> {
    tag_prefix: &'a str,
    rect: Rect,
    columns: &'a [FeedColumn<'a>],
    style: HeaderFeedStyle,
    sort: Option<(usize, bool)>,
    focused: Option<usize>,
    row_count: usize,
}

impl<'a> HeaderFeed<'a> {
    /// A feed of `row_count` rows in `rect`, headed by `columns`.
    #[must_use]
    pub const fn new(
        tag_prefix: &'a str,
        rect: Rect,
        columns: &'a [FeedColumn<'a>],
        style: HeaderFeedStyle,
        row_count: usize,
    ) -> Self {
        Self {
            tag_prefix,
            rect,
            columns,
            style,
            sort: None,
            focused: None,
            row_count,
        }
    }

    /// The order the rows are in, as `(column, ascending)`.
    ///
    /// The SAME value the caller ordered by. Nothing here sorts, so passing a
    /// sort this feed's rows are not actually in would make the indicator lie —
    /// which is why the parameter is the order rather than a request to order.
    #[must_use]
    pub const fn with_sort(mut self, sort: Option<(usize, bool)>) -> Self {
        self.sort = sort;
        self
    }

    /// Which header section the keyboard cursor rests on.
    #[must_use]
    pub const fn with_focus(mut self, focused: Option<usize>) -> Self {
        self.focused = focused;
        self
    }

    /// Why the scrolling viewport and the body frame say nothing to a reader.
    ///
    /// ★★★★★ R1856 — **derived, and there is no way to omit it.**
    ///
    /// A tagged [`ScrollState`] makes the viewport an addressable region, and an
    /// addressable region that neither speaks nor declares why is what a voice
    /// census calls *undecided*. R1851 shipped this as an opt-in
    /// (`with_viewport_silence`) and the only screen that assembled a feed never
    /// called it, so the clip and the frame around it went out undecided — the
    /// measured lesson being that **an optional declaration is a declaration
    /// somebody forgets**, and the state was better made unrepresentable.
    ///
    /// The reason is the same every time this assembly is used, which is what
    /// makes deriving it honest rather than convenient: a clip is not a thing on
    /// the screen — what a reader walks is the rows inside it — so it arranges
    /// and does not speak. That is
    /// [`SilenceKind::Layout`](pinion_core::voice::SilenceKind::Layout), the one
    /// arm that deliberately does **not** reach the subtree, so declaring it
    /// says nothing about the rows and cannot silence them by accident.
    fn frame_silence(&self, what: &str) -> Silence {
        Silence::layout(format!("{}: {what}", self.tag_prefix))
    }

    /// The columns as header placements: cumulative x offsets over the sizes.
    ///
    /// The caller's row painter must use these very offsets. A row computing its
    /// own would agree with the header until one of the two was edited.
    #[must_use]
    pub fn placements(&self) -> Vec<SectionPlacement> {
        let mut x = 0;
        self.columns
            .iter()
            .enumerate()
            .map(|(n, column)| {
                let placement = SectionPlacement {
                    visual: n,
                    logical: n,
                    x,
                    size: column.size,
                };
                x += column.size;
                placement
            })
            .collect()
    }

    /// The rect the rows live in: the feed's rect less the header strip.
    ///
    /// Saturating, so a rect shorter than its own header yields a zero-height
    /// viewport rather than wrapping to an enormous one — a feed with no room
    /// shows no rows, and the windowing downstream then builds none.
    #[must_use]
    pub const fn body_viewport(&self) -> Rect {
        let left = self.rect.h.saturating_sub(self.style.header.height);
        let h = if self.style.whole_rows && self.style.row_pitch > 0 {
            (left / self.style.row_pitch) * self.style.row_pitch
        } else {
            left
        };
        Rect::new(
            self.rect.x,
            self.rect.y + self.style.header.height,
            self.rect.w,
            h,
        )
    }

    /// How many whole rows fit in the body.
    ///
    /// The number a caller needs to say *this feed shows three of nine* — and
    /// the number a virtualised list otherwise keeps to itself.
    #[must_use]
    pub const fn rows_in_view(&self) -> usize {
        if self.style.row_pitch == 0 {
            return 0;
        }
        (self.body_viewport().h / self.style.row_pitch) as usize
    }

    /// The rows that will be CONSTRUCTED at `offset_y`, overscan included.
    ///
    /// Asked by the a11y builder as well as answered to the painter, so what a
    /// reader is told about is what was built. Nothing else in this assembly is
    /// allowed to decide that range.
    #[must_use]
    pub fn window(&self, offset_y: i32) -> VisibleWindow {
        compute_visible_range(
            offset_y,
            self.body_viewport().h,
            self.row_count,
            self.style.row_pitch,
            self.style.overscan,
        )
    }

    /// The header sections under the current sort, glyph included.
    ///
    /// Separate from the paint because the derivation in it is the claim worth
    /// testing: a section shows an indicator when and only when the sort names
    /// its column AND that column can be sorted.
    #[must_use]
    pub fn sections(&self) -> Vec<HeaderSection<'a>> {
        self.columns
            .iter()
            .enumerate()
            .map(|(n, column)| HeaderSection {
                label: column.label,
                align: column.align,
                sort_glyph: if column.sortable {
                    sort_glyph(col_sort_dir(self.sort, n))
                } else {
                    None
                },
                dragged: false,
                focused: self.focused == Some(n),
                selection: SectionSelection::Unselected,
            })
            .collect()
    }

    /// Assemble the header strip and the virtualised body.
    ///
    /// Tagged `<tag_prefix>`; the strip is `<tag_prefix>.head` with its sections
    /// under `<tag_prefix>.head.col#<n>`, and the rows sit inside
    /// `<tag_prefix>.body`.
    ///
    /// `build_row` is called with the row's index, the rect it may paint in (the
    /// body's width at `row_pitch` tall, row-local) and the column placements —
    /// the same ones the header used. It is invoked **only** for the rows in
    /// [`window`](Self::window), which is the property a caller asserts by
    /// counting the calls.
    ///
    /// # ★★★★★ Which regions this assembly decides, and which are the caller's
    ///
    /// Every region painted here has a voice answer, and the split is by *who
    /// can know it*. The assembly declares the ones whose reason is structural
    /// and identical every time — the body frame and the scrolling clip (this
    /// type's private `frame_silence`), the heading's label leaf and its sort
    /// arrow (declared inside [`view_header_cell`]). The
    /// caller announces the three SEMANTIC ones, because only it knows what they
    /// are called: `<tag_prefix>` (the feed), `<tag_prefix>.head` (the heading
    /// row) and `<tag_prefix>.head.col#<n>` (each heading, with its sort
    /// direction).
    ///
    /// That is the whole contract, and `a_built_feed_leaves_no_region_undecided`
    /// is the gate on it: it announces exactly those three shapes and asserts
    /// the census comes back with **no undecided region and no defect**. A
    /// region added here without a decision fails that test in the round that
    /// adds it, rather than in whichever screen happens to assert a count.
    pub fn build(
        &self,
        scroll: &Rc<ScrollState>,
        theme: &Theme,
        mut build_row: impl FnMut(usize, Rect, &[SectionPlacement]) -> Scene,
    ) -> Scene {
        let placements = self.placements();
        let head_tag = format!("{}.head", self.tag_prefix);
        let cells: Vec<Scene> = placements
            .iter()
            .zip(self.sections())
            .map(|(placement, section)| {
                view_header_cell(
                    &format!("{head_tag}.col"),
                    placement,
                    &section,
                    &self.style.header,
                    theme,
                )
            })
            .collect();
        let head = Scene::Container(
            ContainerNode::new(cells).with_tag(head_tag).with_layout(
                LayoutStyle::new()
                    .with_absolute_position(0, 0)
                    .with_size(Size::px(self.rect.w, self.style.header.height)),
            ),
        );

        let body_rect = self.body_viewport();
        let row_pitch = self.style.row_pitch;
        let rows = view_virtual_list(
            scroll,
            Rect::new(0, 0, body_rect.w, body_rect.h),
            self.row_count,
            row_pitch,
            self.style.overscan,
            |index| build_row(index, Rect::new(0, 0, body_rect.w, row_pitch), &placements),
        )
        .silenced(self.frame_silence("the clip the rows scroll inside"));
        let body = Scene::Container(
            ContainerNode::new(vec![rows])
                .with_tag(format!("{}.body", self.tag_prefix))
                .with_layout(
                    LayoutStyle::new()
                        .with_absolute_position(0, self.style.header.height)
                        .with_size(Size::px(body_rect.w, body_rect.h)),
                ),
        )
        .silenced(self.frame_silence("the frame the body's viewport sits in"));

        Scene::Container(
            ContainerNode::new(vec![head, body])
                .with_tag(self.tag_prefix.to_string())
                .with_layout(
                    LayoutStyle::new()
                        .with_absolute_position(self.rect.x, self.rect.y)
                        .with_size(Size::px(self.rect.w, self.rect.h)),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{FeedColumn, HeaderFeed, HeaderFeedStyle};
    use pinion_core::scene::{ContainerNode, Rect, Scene};
    use pinion_core::theme::Theme;
    use pinion_core::widgets::scroll::ScrollState;
    use std::cell::RefCell;
    use std::rc::Rc;

    const COLUMNS: &[FeedColumn<'static>] = &[
        FeedColumn::new("Severity", 90),
        FeedColumn::new("Time", 80),
        FeedColumn::new("Event", 170).fixed(),
    ];

    fn feed(rect: Rect, rows: usize) -> HeaderFeed<'static> {
        HeaderFeed::new("feed", rect, COLUMNS, HeaderFeedStyle::new(44), rows)
    }

    #[test]
    fn placements_are_cumulative_over_the_sizes() {
        let places = feed(Rect::new(0, 0, 340, 298), 6).placements();
        assert_eq!(
            places.iter().map(|p| (p.x, p.size)).collect::<Vec<_>>(),
            vec![(0, 90), (90, 80), (170, 170)]
        );
        assert_eq!(
            places.iter().map(|p| p.visual).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    /// ★★★★★ Fact one: the indicator IS the sort state, so it cannot disagree
    /// with the order the rows are in.
    #[test]
    fn the_indicator_is_derived_from_the_sort_and_nothing_else() {
        let glyphs = |sort| {
            feed(Rect::new(0, 0, 340, 298), 6)
                .with_sort(sort)
                .sections()
                .into_iter()
                .map(|s| s.sort_glyph)
                .collect::<Vec<_>>()
        };
        assert_eq!(glyphs(None), vec![None, None, None], "unsorted: no arrow");

        let asc = glyphs(Some((0, true)));
        let desc = glyphs(Some((0, false)));
        assert!(asc[0].is_some() && desc[0].is_some());
        assert_ne!(asc[0], desc[0], "the two directions draw different glyphs");
        assert_eq!(
            (asc[1], asc[2]),
            (None, None),
            "and only the sorted column carries one"
        );
        // A column declared unsortable shows nothing even when named, so a
        // stale sort reads as an absent indicator rather than as an arrow over
        // a column that does not respond. Same for a column out of range.
        assert_eq!(glyphs(Some((2, true)))[2], None);
        assert_eq!(glyphs(Some((9, true))), vec![None, None, None]);
    }

    /// Fact two: the rows never get the header's band, and a shorter header
    /// brings its label inset down with it.
    #[test]
    fn the_body_gives_up_exactly_the_header_strip() {
        let rect = Rect::new(10, 20, 340, 298);
        let f = feed(rect, 6);
        let body = f.body_viewport();
        assert_eq!(body.y, rect.y + 40);
        assert_eq!(body.h, rect.h - 40);
        assert_eq!((body.x, body.w), (rect.x, rect.w));
        assert_eq!(f.rows_in_view(), (298 - 40) / 44);

        // A rect shorter than its own header yields no room rather than
        // wrapping to an enormous one.
        let tiny = feed(Rect::new(0, 0, 340, 10), 6);
        assert_eq!(tiny.body_viewport().h, 0);
        assert_eq!(tiny.rows_in_view(), 0);

        // ★ Lowering the strip lowers the label box with it, and the box must
        // clear the LINE its own face needs — not merely the type size. At the
        // default insets a 26px strip leaves `26 - 2 * 12 = 2` for a face whose
        // line box is 21.
        for height in [20, 24, 26, 30, 40] {
            for text_px in [10, 11, 13] {
                let s = HeaderFeedStyle::new(44)
                    .with_header_text_px(text_px)
                    .with_header_height(height);
                let line = pinion_core::containment::line_box(text_px);
                assert!(
                    s.header.height - 2 * s.header.label_y >= line,
                    "a {height}px strip with label_y {} leaves {} for a {text_px}px \
                     face whose line box is {line}",
                    s.header.label_y,
                    s.header.height - 2 * s.header.label_y,
                );
                // And a request under the line grew to it rather than being
                // honoured into a box no glyph fits.
                assert!(s.header.height >= line, "{height} -> {}", s.header.height);
            }
        }
    }

    /// ★★★★★ A feed asked for whole rows never shows a half one — because a half
    /// row's words are clipped, so it is a row that looks present and cannot be
    /// read.
    #[test]
    fn whole_rows_take_the_body_down_to_a_multiple_of_the_pitch() {
        let style = HeaderFeedStyle::new(24)
            .with_header_height(26)
            .with_whole_rows();
        // 148 - 26 = 122, and 122 is not a multiple of 24: a sixth row would be
        // two pixels tall, so the body comes down to five whole ones.
        let f = HeaderFeed::new("feed", Rect::new(0, 0, 340, 148), COLUMNS, style, 18);
        assert_eq!(f.body_viewport().h, 120, "five whole rows, not 122");
        assert_eq!(f.rows_in_view(), 5);
        assert_eq!(f.body_viewport().h % style.row_pitch, 0);

        // Without the flag the body keeps every pixel, which is what a long
        // scrolling list wants.
        let loose = HeaderFeedStyle::new(24).with_header_height(26);
        let g = HeaderFeed::new("feed", Rect::new(0, 0, 340, 148), COLUMNS, loose, 18);
        assert_eq!(g.body_viewport().h, 122);

        // And with no overscan the constructed set IS the visible set.
        let exact = HeaderFeed::new(
            "feed",
            Rect::new(0, 0, 340, 148),
            COLUMNS,
            style.with_overscan(0),
            18,
        );
        let window = exact.window(0);
        assert_eq!((window.first, window.count), (0, 5));
    }

    /// ★★★★★ The virtualisation, asserted the only way it can be: by counting
    /// the rows the assembly asked the caller to BUILD. A test over the painted
    /// scene can say a row is absent; only this can say it was never built.
    ///
    /// And fact three: the window the caller can ASK for is the window that was
    /// built, so an a11y walk cannot announce a row nobody constructed.
    #[test]
    fn only_the_visible_window_is_constructed_and_it_is_askable() {
        let f = feed(Rect::new(0, 0, 340, 298), 10_000).with_sort(Some((1, false)));
        let scroll = Rc::new(ScrollState::new());
        let theme = Theme::default();
        let built: RefCell<Vec<usize>> = RefCell::new(Vec::new());
        let scene = f.build(&scroll, &theme, |index, rect, places| {
            built.borrow_mut().push(index);
            assert_eq!(rect.h, 44, "a row is handed its own pitch");
            assert_eq!(places.len(), 3, "and the header's own placements");
            Scene::Container(ContainerNode::new(Vec::new()).with_tag(format!("feed.row#{index}")))
        });
        let built = built.into_inner();
        assert!(
            built.len() <= 8,
            "10 000 rows must not become 10 000 constructions; built {}",
            built.len()
        );
        assert!(!built.is_empty(), "and the visible ones must be built");

        let window = f.window(scroll.offset_y());
        assert_eq!(
            built,
            (window.first..window.first + window.count).collect::<Vec<_>>(),
            "the askable window IS the built one"
        );

        let tags = walk(&scene);
        for want in ["feed", "feed.head", "feed.body", "feed.head.col#0"] {
            assert!(
                tags.iter().any(|t| t == want),
                "{want} missing from {tags:?}"
            );
        }
        assert_eq!(
            tags.iter().filter(|t| t.starts_with("feed.row#")).count(),
            built.len(),
            "every constructed row is in the scene: {tags:?}"
        );
    }

    /// ★★★★★ R1856 — the assembly's own ZERO gate: nothing it paints is left
    /// undecided.
    ///
    /// This is the check whose absence let R1851 publish six undecided regions
    /// on the shipped shell. The workspace's voice gate REPORTS `unvoiced` and
    /// deliberately does not judge it, so whether an undecided region is refused
    /// depended on which screen happened to assert the number itself — and the
    /// two demos that do were not the ones R1851 ran. A gate on the assembly
    /// does not have that dependence: it fails in the round that adds the
    /// region.
    ///
    /// ⚠ That last clause is READ, not inferred: R1851's own ledger entry has a
    /// section headed "WHAT WAS NOT RUN, and why", which narrows its demo
    /// population by capability and names `hello-column-reorder` as the demo to
    /// watch. The screen that broke was the other one — the very screen the
    /// round was building on. ⇒ **narrowing by "which capability changed" still
    /// misses the capability a NEW consumer changes about ITSELF**, and the
    /// remedy is not a wider sweep but a gate that travels with the composite.
    ///
    /// ```text
    /// mnemosyne-cli query --changelog-entry R1851   # the section is verbatim
    /// ```
    ///
    /// The caller's half is modelled exactly as the contract states it — the
    /// feed, the heading row and each heading are announced here and NOTHING
    /// else is — so a region this assembly stops declaring cannot be hidden by
    /// the fixture announcing it. ⚠ That is the property to preserve when
    /// editing this test: announcing a fourth shape would make it pass
    /// vacuously.
    #[test]
    fn a_built_feed_leaves_no_region_undecided() {
        use pinion_core::voice::{Announcement, Voice, voice_census};
        use std::collections::{BTreeMap, BTreeSet};

        let f = feed(Rect::new(0, 0, 340, 298), 40).with_sort(Some((1, false)));
        let scroll = Rc::new(ScrollState::with_tag("feed.scroll"));
        let theme = Theme::default();
        let mut rows: Vec<String> = Vec::new();
        let scene = f.build(&scroll, &theme, |index, _rect, _places| {
            rows.push(format!("feed.row.{index}"));
            Scene::Container(ContainerNode::new(Vec::new()).with_tag(format!("feed.row.{index}")))
        });

        let mut announced = BTreeMap::new();
        announced.insert("feed".to_string(), Announcement::named("Alarms"));
        announced.insert(
            "feed.head".to_string(),
            Announcement::named("Alarm columns"),
        );
        for (n, column) in COLUMNS.iter().enumerate() {
            announced.insert(
                format!("feed.head.col#{n}"),
                Announcement::named(column.label),
            );
        }
        for tag in &rows {
            announced.insert(tag.clone(), Announcement::named("a row"));
        }

        let census = voice_census(&scene, &announced, &BTreeSet::new());
        let undecided = census
            .defects()
            .map(|n| format!("{} is {}", n.tag, n.voice.name()))
            .collect::<Vec<_>>();
        assert!(
            undecided.is_empty(),
            "the assembly must decide every region it paints, and left: {undecided:?}"
        );
        assert_eq!(
            census.count(Voice::Unvoiced),
            0,
            "no region may be painted with neither a node nor a reason"
        );

        // ★ And the two frames are decided as ARRANGING, not as ornament — the
        // one arm that does not reach the subtree. A `decorative` clip would
        // silence every row inside it and this census would still come back
        // clean, which is why the KIND is asserted and not merely the absence
        // of a defect.
        let framed = |tag: &str| {
            let mut kind = None;
            scene.for_each_node(&mut |visit| {
                if visit.node.tag() == Some(tag) {
                    kind = visit
                        .node
                        .layout_style()
                        .and_then(|l| l.silence.as_ref())
                        .map(pinion_core::voice::Silence::kind);
                }
            });
            kind
        };
        for tag in ["feed.body", "feed.scroll"] {
            assert_eq!(
                framed(tag),
                Some(pinion_core::voice::SilenceKind::Layout),
                "{tag} must arrange rather than decorate"
            );
        }
    }

    /// ★★★★★ R1856 — the sort arrow may be called ornament only because the
    /// heading says the direction in words.
    ///
    /// [`view_header_cell`](crate::column_header::view_header_cell) declares the
    /// arrow `decorative`, which is a claim that a reader loses nothing by never
    /// reaching it. That claim rests entirely on the heading carrying the
    /// direction, and nothing in this crate builds the heading's node — so the
    /// pairing is asserted here, at the assembly that owns both halves, rather
    /// than left as prose beside the declaration it justifies.
    #[test]
    fn a_sorted_column_is_the_only_one_that_paints_an_arrow() {
        let arrows = |sort| {
            feed(Rect::new(0, 0, 340, 298), 6)
                .with_sort(sort)
                .sections()
                .into_iter()
                .enumerate()
                .filter(|(_, s)| s.sort_glyph.is_some())
                .map(|(n, _)| n)
                .collect::<Vec<_>>()
        };
        assert_eq!(arrows(None), Vec::<usize>::new());
        assert_eq!(arrows(Some((1, true))), vec![1]);
        // The third column is `fixed()`, so a sort naming it paints no arrow —
        // and a reader is told nothing that is not also true of the heading.
        assert_eq!(arrows(Some((2, true))), Vec::<usize>::new());
    }

    fn walk(scene: &Scene) -> Vec<String> {
        let mut out = Vec::new();
        scene.for_each_node(&mut |visit| {
            if let Some(tag) = visit.node.tag() {
                out.push(tag.to_owned());
            }
        });
        out
    }
}
