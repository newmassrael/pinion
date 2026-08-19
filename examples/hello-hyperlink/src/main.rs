// R1405 §5.41 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-hyperlink` — R1405 §5.41 §5.35 — **OSC-8 hyperlink hover / click
//! interaction** over a [`Scene::TextGrid`], the R-69.3 layer on the R1403
//! hyperlink data model (sprag PINION-PR71).
//!
//! A few lines of terminal-style output carry OSC-8 links (a doc URL, a
//! wrapped GitHub issue link, an anonymous `file://` path). Hovering a link
//! cell:
//!
//! * lights the link's **whole id-group** — every cell sharing the hovered
//!   cell's [`HyperlinkId`], so a link split across a soft wrap lights as ONE
//!   logical target (R-71.2, the grouping a position-based highlight cannot
//!   express);
//! * shows the **pointer / hand cursor** ([`CursorHint::Pointer`], the new
//!   R1405 variant) so the link reads as clickable (R-71.1);
//! * and the link cells are single-underlined (the R1399 underline axis, the
//!   conventional affordance).
//!
//! Clicking a link **activates** its URI (R-71.3): the [`HyperlinkOracle`]
//! records the activated `(id, uri)` and exposes it via introspection; a
//! consumer (sprag) reads the URI and opens it with the platform opener —
//! pinion owns the affordance + the hit/activation seam, not the opening.
//!
//! ## How the hover reaches the grid (the R1405 seam)
//!
//! A plain hover (no button) forwards only `Enter` / `Leave` to a widget, not
//! the intra-widget position — so before R1405 a TextGrid could not know
//! *which* cell the pointer was over on hover. The [`HyperlinkOracle`] opts in
//! via [`External::wants_hover_move`], and the router then forwards each hover
//! move as `pointer_move(x_rel, y_rel)`; the oracle reconstructs the cell
//! ([`CellMetric::px_to_cell`], R1008) and resolves its link
//! ([`GridBuffer::cell_hyperlink`], R1405). Clicking rides the R1401 capture
//! path (`invoke("send", "PointerDown")`).
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! The hover highlight is cell reverse-video, so `scene/snapshot` reports it in
//! `grid_rows` — a client verifies the SAME id-group lit across the wrap
//! without a pixel. The oracle exposes what a snapshot cannot: the hovered /
//! activated link's `uri` and `id`, driven no-pixel via `scene/intervene
//! /external/hover_index` and `scene/invoke /external/activate`. See
//! `tools/demos/r1405_hyperlink.py`.

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner, SchemaField,
    ThreadOwnership,
};
use pinion_core::input::PointerReading;
use pinion_core::scene::{ContainerNode, Rect, TextGridNode, TextNode};
use pinion_core::style::{BoxStyle, CursorHint, LayoutStyle, Size, TextStyle};
use pinion_core::term_grid::{CellAttrs, Hyperlink, HyperlinkId, UnderlineStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{CellMetric, Frame, GridBuffer, Scene, TermCell, TermColor, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloHyperlinkRenderer, HelloHyperlinkRendererError);

const WIN_W: u32 = 560;
const WIN_H: u32 = 200;
const THEME_TAG: &str = "app";

/// The grid's paint tag **and** the primary [`HyperlinkOracle`]'s registration
/// tag — addressed over RPC as `/external/<field>`.
const GRID_TAG: &str = "links";

const TITLE_FONT_PX: u32 = 16;
const STATUS_FONT_PX: u32 = 12;

// --- The link content ------------------------------------------------------

/// Grid geometry (at the `CellMetric::DEFAULT` 8x16 cell). `GRID_W/H` derive
/// from the col/row counts so `px_to_cell` is exact.
const COLS: u16 = 40;
const ROWS: u16 = 4;
const GRID_POS: (u32, u32) = (16, 44);
const GRID_W: u32 = COLS as u32 * 8;
const GRID_H: u32 = ROWS as u32 * 16;

/// The interned link table (R1403): each `(uri, id)` is one entry, referenced
/// by a cell's [`HyperlinkId`] index. Entry 2 (the GitHub issue) is placed on
/// TWO rows to prove the wrap-spanning id-group.
const LINKS: [(&str, Option<&str>); 3] = [
    ("https://doc.rust-lang.org/book", Some("doc")),
    ("file:///home/user/src/main.rs", None),
    ("https://github.com/org/repo/issues/42", Some("gh")),
];

/// One placed text run: `text` at `(row, col)`, optionally the link table
/// index it belongs to.
struct Seg {
    row: u16,
    col: u16,
    text: &'static str,
    link: Option<u32>,
}

/// The fixed layout. The GitHub link (index 2) spans `row 1` + `row 2`, so
/// hovering either segment lights BOTH (the id-group across the wrap).
const SEGMENTS: &[Seg] = &[
    Seg {
        row: 0,
        col: 0,
        text: "docs  ",
        link: None,
    },
    Seg {
        row: 0,
        col: 6,
        text: "rust-lang.org",
        link: Some(0),
    },
    Seg {
        row: 1,
        col: 0,
        text: "bug   ",
        link: None,
    },
    Seg {
        row: 1,
        col: 6,
        text: "github.com/org/repo/",
        link: Some(2),
    },
    Seg {
        row: 2,
        col: 6,
        text: "issues/42",
        link: Some(2),
    },
    Seg {
        row: 3,
        col: 0,
        text: "edit  ",
        link: None,
    },
    Seg {
        row: 3,
        col: 6,
        text: "src/main.rs",
        link: Some(1),
    },
];

/// The resolved cell colours the grid paints with.
#[derive(Debug, Clone, Copy)]
struct CellColors {
    /// Plain (non-link) foreground.
    fg: TermColor,
    /// A link's foreground (the conventional blue).
    link: TermColor,
}

/// Build the interning table from [`LINKS`].
fn link_table() -> Vec<Hyperlink> {
    LINKS
        .iter()
        .map(|(uri, id)| {
            id.map_or_else(|| Hyperlink::new(*uri), |i| Hyperlink::new(*uri).with_id(i))
        })
        .collect()
}

/// Build the content [`GridBuffer`]. Link cells carry their [`HyperlinkId`]
/// index + a single underline + the link colour; the cells whose index equals
/// `hovered` reverse-video (the R-71.2 id-group highlight).
fn content_buffer(colors: CellColors, hovered: Option<HyperlinkId>) -> GridBuffer {
    let blank = TermCell::new(" ", colors.fg, TermColor::Default);
    let mut rows: Vec<Vec<TermCell>> = (0..ROWS)
        .map(|_| vec![blank.clone(); COLS as usize])
        .collect();

    for seg in SEGMENTS {
        for (i, ch) in seg.text.chars().enumerate() {
            let col = seg.col as usize + i;
            if col >= COLS as usize {
                break;
            }
            let cell = match seg.link {
                Some(idx) => {
                    let lit = hovered == Some(HyperlinkId(idx));
                    let attrs = CellAttrs::empty()
                        .with_underline_style(UnderlineStyle::Single)
                        .with_reverse(lit);
                    TermCell::new(ch.to_string(), colors.link, TermColor::Default)
                        .with_hyperlink(HyperlinkId(idx))
                        .with_attrs(attrs)
                }
                None => TermCell::new(ch.to_string(), colors.fg, TermColor::Default),
            };
            rows[seg.row as usize][col] = cell;
        }
    }

    let mut buf = GridBuffer::new(COLS, ROWS).with_hyperlinks(link_table());
    for (r, cells) in rows.into_iter().enumerate() {
        buf = buf.with_row(u16::try_from(r).unwrap_or(0), cells);
    }
    buf
}

/// The URI a link table index addresses, from the [`LINKS`] SSOT (so the view
/// resolves a `Copy` [`HyperlinkId`] state to its string without carrying the
/// non-`Copy` URI in the widget state).
fn uri_for(idx: HyperlinkId) -> Option<&'static str> {
    LINKS.get(idx.0 as usize).map(|(uri, _)| *uri)
}

/// The number of cells that belong to link index `idx` (the id-group size,
/// including a wrap's far segment) — the SSOT the hover highlight paints and
/// the oracle reports.
fn group_size(idx: u32) -> usize {
    SEGMENTS
        .iter()
        .filter(|s| s.link == Some(idx))
        .map(|s| s.text.chars().count())
        .sum()
}

// --- The view --------------------------------------------------------------

/// view-fn (§6.3): the link grid with the hovered link's id-group reversed and
/// the pointer cursor when a link is hovered, plus a status line.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "mirrors the WidgetCore::view(&Frame) signature the caller forwards"
)]
fn view(hovered: Option<HyperlinkId>, activated: Option<HyperlinkId>, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let surface = theme.resolve(ColorRole::Surface);

    let colors = CellColors {
        fg: TermColor::Rgb(on_surface),
        // The conventional link blue (bright, distinct from body text).
        link: TermColor::Rgb(theme.resolve(ColorRole::Accent)),
    };
    let cells = content_buffer(colors, hovered);

    // The grid declares the pointer cursor while a link is hovered (R-71.1) —
    // the whole grid rect resolves to the hand exactly when the current hover
    // is a link, because the oracle only sets `hovered` over a link cell.
    let mut grid_layout = LayoutStyle::new()
        .with_absolute_position(GRID_POS.0, GRID_POS.1)
        .with_size(Size::px(GRID_W, GRID_H))
        .with_focusable(true);
    if hovered.is_some() {
        grid_layout = grid_layout.with_cursor(CursorHint::Pointer);
    }
    let grid = Scene::TextGrid(
        TextGridNode::new(CellMetric::DEFAULT)
            .with_tag(GRID_TAG)
            .with_cells(cells)
            .with_layout(grid_layout),
    );

    let title = Scene::Text(
        TextNode::styled(
            "Hyperlinks — hover a link (id-group lights across the wrap), click to open",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(16, 14)),
    );

    let status_text = activated.and_then(uri_for).map_or_else(
        || "click a link to activate its URI".to_owned(),
        |uri| format!("activated: {uri}"),
    );
    let status = Scene::Text(
        TextNode::styled(
            status_text,
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_FONT_PX)
                .with_fg(on_surface_muted),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(16, WIN_H - 24)),
    );

    Scene::Container(
        ContainerNode::new(vec![grid, title, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

/// Read `(hovered, activated)` link indices from the primary
/// [`HyperlinkOracle`] in the state scene; the boot default (nothing hovered /
/// activated) when absent. Both are `Copy` [`HyperlinkId`] indices — the view
/// resolves them to URIs via [`uri_for`] so the widget state stays `Copy`.
fn read_oracle(scene: &Scene) -> (Option<HyperlinkId>, Option<HyperlinkId>) {
    let Some(intro) = scene
        .find_external_with_tag(GRID_TAG)
        .and_then(|n| n.handle.introspect())
    else {
        return (None, None);
    };
    let index = |name: &str| match intro.query(name) {
        Ok(IntrospectValue::Int(i)) => u32::try_from(i).ok().map(HyperlinkId),
        _ => None,
    };
    (index("hover_index"), index("activated_index"))
}

// --- The interaction oracle (primary external) -----------------------------

/// The hover / click state, as the interactive primary external. Tracks the
/// hovered link (via [`External::wants_hover_move`]) and the last activated
/// link (via the R1401 press channel), and answers the byte↔cell↔link
/// mapping so an AI client drives it with no pixel.
#[derive(Debug, Clone)]
struct HyperlinkOracle {
    /// A structure-only content buffer (default colours, no hover) used to
    /// resolve a `(col, row)` cell to its link index + URI.
    buffer: GridBuffer,
    /// The link index the pointer is currently over, or `None`.
    hovered: Option<HyperlinkId>,
    /// The last activated `(id_index, uri)`, or `None`.
    activated: Option<(HyperlinkId, String)>,
}

impl HyperlinkOracle {
    fn new() -> Self {
        let colors = CellColors {
            fg: TermColor::Default,
            link: TermColor::Default,
        };
        Self {
            buffer: content_buffer(colors, None),
            hovered: None,
            activated: None,
        }
    }

    /// The link index at cell `(col, row)`, or `None` for a non-link cell.
    fn link_at(&self, col: u16, row: u16) -> Option<HyperlinkId> {
        self.buffer.cell(col, row)?.hyperlink
    }

    /// The URI for a link index, via the buffer's table.
    fn uri_of(&self, id: HyperlinkId) -> Option<String> {
        self.buffer.hyperlink(id).map(|h| h.uri.clone())
    }
}

impl External for HyperlinkOracle {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// Opt into hover-move so the pointer's position is forwarded on a plain
    /// hover, not only under a press (R1405) — the whole point of hover
    /// affordance.
    fn wants_hover_move(&self) -> bool {
        true
    }

    /// Capture the press so `PointerDown` is dispatched (the R1401 click
    /// channel) — a click activates the hovered link.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// Each hover move (or drag move) delivers a `[0, 1]` rect fraction:
    /// reconstruct the cell and set the hovered link index (or `None` off a
    /// link).
    fn pointer_move(&mut self, at: PointerReading) {
        // R1408 — the router's rect fraction → cell in one call (the lifted
        // `frac_to_px` + `px_to_cell` composite).
        let (col, row) = CellMetric::DEFAULT.frac_to_cell(at.u(), at.v(), GRID_W, GRID_H);
        self.hovered = self.link_at(col, row);
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for HyperlinkOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("link_count", "int"),
                    // The hovered link: its table index, URI, OSC-8 id, and the
                    // number of cells in its id-group (the wrap-spanning size).
                    SchemaField::new("hover_index", "int"),
                    SchemaField::new("hover_uri", "string"),
                    SchemaField::new("hover_id", "string"),
                    SchemaField::new("hover_group_size", "int"),
                    // The last activated link.
                    SchemaField::new("activated_index", "int"),
                    SchemaField::new("activated_uri", "string"),
                    SchemaField::action("activate", "int"),
                    // The router's pointer press / release symbolic events.
                    SchemaField::action("send", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        let int = |n: usize| IntrospectValue::Int(i64::try_from(n).unwrap_or(0));
        match path {
            "link_count" => Ok(int(LINKS.len())),
            "hover_index" => Ok(self
                .hovered
                .map_or(IntrospectValue::Null, |h| int(h.0 as usize))),
            "hover_uri" => Ok(self
                .hovered
                .and_then(|h| self.uri_of(h))
                .map_or(IntrospectValue::Null, IntrospectValue::Text)),
            "hover_id" => Ok(self
                .hovered
                .and_then(|h| self.buffer.hyperlink(h))
                .and_then(|h| h.id.clone())
                .map_or(IntrospectValue::Null, IntrospectValue::Text)),
            "hover_group_size" => Ok(self
                .hovered
                .map_or(IntrospectValue::Null, |h| int(group_size(h.0)))),
            "activated_index" => Ok(self
                .activated
                .as_ref()
                .map_or(IntrospectValue::Null, |(h, _)| int(h.0 as usize))),
            "activated_uri" => Ok(self
                .activated
                .as_ref()
                .map_or(IntrospectValue::Null, |(_, uri)| {
                    IntrospectValue::Text(uri.clone())
                })),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // AI-first, no-pixel hover: set the hovered link index (or Null to
            // clear). Rejects an out-of-range index.
            "hover_index" => match value {
                IntrospectValue::Null => {
                    self.hovered = None;
                    Ok(())
                }
                IntrospectValue::Int(i) => {
                    let idx = u32::try_from(i).map_err(|_| InterveneError::TypeMismatch)?;
                    if (idx as usize) >= LINKS.len() {
                        return Err(InterveneError::out_of_range(format!(
                            "no hyperlink {idx} in this document (it has {})",
                            LINKS.len()
                        )));
                    }
                    self.hovered = Some(HyperlinkId(idx));
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "link_count" | "hover_uri" | "hover_id" | "hover_group_size" | "activated_index"
            | "activated_uri" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Activate a link by index (the AI-first, no-pixel click). Returns
            // the activated URI, or errors on a bad / out-of-range index.
            "activate" => {
                let i = match args {
                    IntrospectValue::Int(i) => i,
                    IntrospectValue::Text(ref s) => {
                        s.trim().parse().map_err(|_| InvokeError::TypeMismatch)?
                    }
                    _ => return Err(InvokeError::TypeMismatch),
                };
                let idx = u32::try_from(i).map_err(|_| InvokeError::TypeMismatch)?;
                let uri = self.uri_of(HyperlinkId(idx)).ok_or_else(|| {
                    InvokeError::rejected(format!("{path}: no hyperlink {idx} in this document"))
                })?;
                self.activated = Some((HyperlinkId(idx), uri.clone()));
                Ok(IntrospectValue::Text(uri))
            }
            // The router press / release. A `PointerDown` over a link activates
            // it (the click); a leave clears the hover.
            "send" => {
                if let IntrospectValue::Text(ref name) = args {
                    match name.as_str() {
                        "PointerDown" => {
                            if let Some(h) = self.hovered {
                                if let Some(uri) = self.uri_of(h) {
                                    self.activated = Some((h, uri));
                                }
                            }
                        }
                        "PointerLeave" | "PointerCancel" => self.hovered = None,
                        _ => {}
                    }
                }
                Ok(IntrospectValue::Null)
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// --- The binding -----------------------------------------------------------

struct HyperlinkView;

impl WidgetCore for HyperlinkView {
    type State = (Option<HyperlinkId>, Option<HyperlinkId>);
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(HyperlinkOracle::new())
    }

    fn tag() -> &'static str {
        GRID_TAG
    }

    fn read_state(scene: &Scene) -> (Option<HyperlinkId>, Option<HyperlinkId>) {
        read_oracle(scene)
    }

    fn view(state: (Option<HyperlinkId>, Option<HyperlinkId>), frame: &Frame) -> Scene {
        view(state.0, state.1, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-hyperlink (R1405 §5.41 OSC-8 hover + click)"
    }

    fn apply_key(
        _scene: &mut Scene,
        _focused: Option<&str>,
        _key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        false
    }

    fn fmt_state_log(state: &(Option<HyperlinkId>, Option<HyperlinkId>)) -> String {
        format!("hovered {:?} / activated {:?}", state.0, state.1)
    }
}

impl WidgetA11y for HyperlinkView {
    fn access_node(
        state: &(Option<HyperlinkId>, Option<HyperlinkId>),
        _focused: Option<&str>,
    ) -> Vec<AccessNode> {
        let value = state.1.and_then(uri_for).map_or_else(
            || "no link activated".to_owned(),
            |uri| format!("activated {uri}"),
        );
        vec![
            AccessNode::new(GRID_TAG, AriaRole::Group)
                .with_name("Hyperlinks")
                .with_value(AccessValue::Text(value)),
        ]
    }
}

impl WidgetView for HyperlinkView {
    type Renderer = HelloHyperlinkRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<HyperlinkView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::test_fixtures::assert_out_of_range_saying;
    use pinion_core::test_fixtures::assert_refused_saying;

    fn colors() -> CellColors {
        CellColors {
            fg: TermColor::Default,
            link: TermColor::Rgb(pinion_core::style::Color::rgb(0x21, 0x5b, 0xd0)),
        }
    }

    #[test]
    fn link_cells_carry_their_index_and_are_underlined() {
        let buf = content_buffer(colors(), None);
        // "rust-lang.org" is link 0 at row 0, col 6.
        let c = buf.cell(6, 0).unwrap();
        assert_eq!(c.hyperlink, Some(HyperlinkId(0)));
        assert_eq!(c.attrs.underline, UnderlineStyle::Single);
        // A plain cell has no link and no underline.
        let plain = buf.cell(0, 0).unwrap();
        assert_eq!(plain.hyperlink, None);
        assert_eq!(plain.attrs.underline, UnderlineStyle::None);
        // The buffer resolves the index to the URI.
        assert_eq!(
            buf.cell_hyperlink(6, 0).unwrap().uri,
            "https://doc.rust-lang.org/book"
        );
    }

    #[test]
    fn hovering_lights_the_whole_id_group_across_the_wrap() {
        // Link 2 (github) spans row 1 (github.com/org/repo/) + row 2
        // (issues/42) — the same index, so hovering lights BOTH rows.
        let buf = content_buffer(colors(), Some(HyperlinkId(2)));
        // A row-1 cell of the link is reversed...
        assert!(buf.cell(6, 1).unwrap().attrs.reverse, "row1 link cell lit");
        // ...and a row-2 cell of the SAME link is reversed (the wrap group).
        assert!(buf.cell(6, 2).unwrap().attrs.reverse, "row2 link cell lit");
        // A different link (row 0) is NOT lit.
        assert!(!buf.cell(6, 0).unwrap().attrs.reverse, "other link not lit");
        // The id-group size counts both segments.
        assert_eq!(
            group_size(2),
            "github.com/org/repo/".len() + "issues/42".len()
        );
    }

    #[test]
    fn oracle_resolves_a_cell_to_its_link_and_reports_it() {
        let mut o = HyperlinkOracle::new();
        assert_eq!(o.query("link_count"), Ok(IntrospectValue::Int(3)));
        assert_eq!(o.query("hover_index"), Ok(IntrospectValue::Null));
        // Hover the doc link (row 0 col 6 = link 0).
        assert_eq!(o.link_at(6, 0), Some(HyperlinkId(0)));
        o.hovered = o.link_at(6, 0);
        assert_eq!(o.query("hover_index"), Ok(IntrospectValue::Int(0)));
        assert_eq!(
            o.query("hover_uri"),
            Ok(IntrospectValue::Text(
                "https://doc.rust-lang.org/book".into()
            ))
        );
        assert_eq!(o.query("hover_id"), Ok(IntrospectValue::Text("doc".into())));
        // A non-link cell resolves to nothing.
        assert_eq!(o.link_at(0, 0), None);
    }

    #[test]
    fn activate_records_the_uri_and_send_pointerdown_activates_the_hover() {
        let mut o = HyperlinkOracle::new();
        // Direct activate by index.
        assert_eq!(
            o.invoke("activate", IntrospectValue::Int(1)),
            Ok(IntrospectValue::Text(
                "file:///home/user/src/main.rs".into()
            ))
        );
        assert_eq!(o.query("activated_index"), Ok(IntrospectValue::Int(1)));
        // A press over a hovered link activates it.
        o.hovered = Some(HyperlinkId(2));
        o.invoke("send", IntrospectValue::Text("PointerDown".into()))
            .unwrap();
        assert_eq!(
            o.query("activated_uri"),
            Ok(IntrospectValue::Text(
                "https://github.com/org/repo/issues/42".into()
            ))
        );
        // A leave clears the hover.
        o.invoke("send", IntrospectValue::Text("PointerLeave".into()))
            .unwrap();
        assert_eq!(o.query("hover_index"), Ok(IntrospectValue::Null));
        // Guards.
        assert_refused_saying(
            &o.invoke("activate", IntrospectValue::Int(9)),
            "no hyperlink 9 in this document",
        );
        assert_eq!(
            o.invoke("bogus", IntrospectValue::Null),
            Err(InvokeError::UnknownPath)
        );
    }

    #[test]
    fn intervene_hover_index_sets_and_clears_and_guards() {
        let mut o = HyperlinkOracle::new();
        o.intervene("hover_index", IntrospectValue::Int(0)).unwrap();
        assert_eq!(o.hovered, Some(HyperlinkId(0)));
        o.intervene("hover_index", IntrospectValue::Null).unwrap();
        assert_eq!(o.hovered, None);
        assert_out_of_range_saying(
            &o.intervene("hover_index", IntrospectValue::Int(9)),
            "no hyperlink 9 in this document",
        );
        assert_eq!(
            o.intervene("link_count", IntrospectValue::Int(1)),
            Err(InterveneError::ReadOnly)
        );
    }

    /// The cursor hint the grid node declares in a built scene (before the
    /// layout pass, so read the `LayoutStyle` directly rather than
    /// `cursor_hint_at`, which needs resolved rects).
    fn grid_cursor_hint(scene: &Scene) -> Option<CursorHint> {
        let Scene::Container(root) = scene else {
            panic!("view root is a Container");
        };
        root.children.iter().find_map(|c| match c {
            Scene::TextGrid(n) if n.tag.as_deref() == Some(GRID_TAG) => Some(n.layout.cursor),
            _ => None,
        })?
    }

    #[test]
    fn view_sets_the_pointer_cursor_only_while_a_link_is_hovered() {
        // No hover -> the grid declares no cursor hint.
        let plain = pinion_core::Owner::new().run(|| view(None, None, &Frame::new()));
        assert_eq!(grid_cursor_hint(&plain), None);
        // Hovering a link -> the grid declares the pointer/hand cursor, so the
        // shell commands it while the pointer is over the (link) grid.
        let hot = pinion_core::Owner::new().run(|| view(Some(HyperlinkId(0)), None, &Frame::new()));
        assert_eq!(grid_cursor_hint(&hot), Some(CursorHint::Pointer));
    }
}
