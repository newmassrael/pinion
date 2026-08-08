//! `hello-tile-dashboard` — R1608 §5.21 — a **Grafana-class tile dashboard**
//! over the R1560 CSS Grid, driven by a press-drag and by the wire.
//!
//! R1607 built the model ([`TileGrid`]) and measured that the existing grid
//! layout holds a dashboard, and then deliberately stopped: it had no consumer,
//! so it could not answer whether the reflow's *locality* feels right under a
//! real cursor. This is that consumer, and it is what closes the gesture axis of
//! the tile-dashboard gap.
//!
//! ## The three layers, and which one is new
//!
//! * **Layout** — `Display::Grid` with twelve `Fr(1.0)` columns and fixed-height
//!   rows. Not new: R1560 added it for a text table's rowspans.
//! * **Arrangement** — [`TileGrid`], which holds cards that do not overlap and
//!   reports what a move displaced. Not new: R1607.
//! * **Gesture** — this file. A press latches the card under the cursor *and
//!   where inside it the grab was*; each move snaps the cursor to a grid cell
//!   and asks the model to move the card there; the release lets go.
//!
//! ## Why the grab offset is the whole trick
//!
//! Snapping the cursor's cell and moving the card's top-left there makes a card
//! jump under the pointer the moment you grab it anywhere but its corner. The
//! latch therefore stores the cell the grab happened at *relative to the card*,
//! and the move subtracts it — so the card stays under the finger, which is the
//! difference between a drag and a teleport. Grafana does the same thing; what
//! it does not do is tell you which other cards it pushed.
//!
//! ## The AI-first witness (§2 #7)
//!
//! The arrangement is a value, so the whole dashboard is one JSON read:
//! `scene/query /external/layout`. Every gesture is also a verb —
//! `scene/invoke /external/move_to "cpu,0,0"` / `resize "cpu,6,2"` / `compact` —
//! so an agent rearranges the board with no pixel, and `last_reflow` reports
//! what the last edit displaced. See `tools/demos/r1608_tile_dashboard.py`.

use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, FlexDirection, GridTrack, LayoutStyle, TextStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::tile_grid::{Reflow, Tile, TileGrid, TileId};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloTileDashboardRenderer, HelloTileDashboardRendererError);

const WIN_W: u32 = 760;
const WIN_H: u32 = 420;
const ROOT_TAG: &str = "dashboard";
const THEME_TAG: &str = "app";

/// The dashboard's columns — the one number a Grafana layout declares.
const COLUMNS: u32 = 12;
/// A grid row's pixel height. Rows are fixed so a card's height is a count.
const ROW_H: u32 = 64;
/// How many rows the grid always offers, so a card can be dragged into empty
/// space below the ones that exist.
const MIN_ROWS: u32 = 5;

const TITLE_FONT_PX: u32 = 15;
const CARD_FONT_PX: u32 = 12;

/// The board a fresh window opens with: the shape the R1607 measurement laid
/// out, so the example and the layout test describe the same dashboard.
fn seed() -> TileGrid {
    let mut grid = TileGrid::new(COLUMNS);
    for tile in [
        Tile::new("throughput", 0, 0, 12, 1),
        Tile::new("latency", 0, 1, 6, 1),
        Tile::new("loss", 6, 1, 6, 1),
        Tile::new("topology", 0, 2, 4, 2),
        Tile::new("alarms", 4, 2, 8, 1),
    ] {
        grid.place(tile).expect("the seed board is legal");
    }
    grid
}

/// What a press latched: which card, and where inside it the grab happened.
///
/// The offset is the reason a drag is a drag — see the module docs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Grab {
    id: TileId,
    /// Columns between the card's left edge and the grabbed cell.
    dx: u32,
    /// Rows between the card's top edge and the grabbed cell.
    dy: u32,
}

/// The board's state, held in signals the oracle and the view SHARE.
///
/// `WidgetCore::State` is `Copy`, and an arrangement is a `Vec` — so the model
/// cannot travel through it. The canonical shape for a model this size is the
/// one `hello-node-groups` uses: the state lives in an `Owner::cache`d signal,
/// the external mutates it, and the view reads it. That also means the painter
/// and the wire read **one** arrangement rather than two copies that could
/// disagree.
#[derive(Debug)]
struct BoardState {
    grid: Signal<TileGrid>,
    /// Which card a press latched, and where inside it the grab happened.
    grab: Signal<Option<Grab>>,
    /// Set on a press, cleared once the following move has resolved the latch.
    pending: Signal<bool>,
    /// What the last edit displaced, as text the wire can read.
    last_reflow: Signal<String>,
    /// Why the last edit was refused, empty when it was not.
    refusal: Signal<String>,
}

impl BoardState {
    fn new() -> Self {
        Self {
            grid: Signal::new(seed()),
            grab: Signal::new(None),
            pending: Signal::new(false),
            last_reflow: Signal::new("clean".to_owned()),
            refusal: Signal::new(String::new()),
        }
    }
}

const STATE_KEY: &str = "tile-dashboard-state";

fn use_board_state() -> std::rc::Rc<BoardState> {
    Owner::current()
        .expect("use_board_state requires an active Owner scope")
        .cache(STATE_KEY, BoardState::new)
}

/// The dashboard's root external. It owns **no model** — the arrangement lives
/// in [`BoardState`]'s signals, which the view reads too, so the painter and the
/// wire cannot hold two arrangements that disagree.
#[derive(Debug)]
struct DashboardOracle {
    state: Option<std::rc::Rc<BoardState>>,
}

impl DashboardOracle {
    fn new() -> Self {
        Self { state: None }
    }

    fn attach(&mut self, state: std::rc::Rc<BoardState>) {
        self.state = Some(state);
    }

    fn bound(&self) -> Option<&BoardState> {
        self.state.as_deref()
    }

    /// How many rows the painted grid offers — the cards' own height, never less
    /// than [`MIN_ROWS`] so there is empty board to drag into.
    fn painted_rows(grid: &TileGrid) -> u32 {
        grid.rows().max(MIN_ROWS)
    }

    /// The cell a `[0, 1]` fraction over the grid's rect lands on.
    ///
    /// This is the snap: a pointer anywhere inside a cell means that cell, which
    /// is what makes a dashboard's drag land on a slot rather than a pixel.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a dashboard's column and row counts round-trip f32 exactly, and \
                  both ends are clamped"
    )]
    fn cell_at(rows: u32, x_rel: f32, y_rel: f32) -> (u32, u32) {
        let col = (x_rel.clamp(0.0, 0.999) * COLUMNS as f32) as u32;
        let row = (y_rel.clamp(0.0, 0.999) * rows as f32) as u32;
        (col.min(COLUMNS - 1), row.min(rows.saturating_sub(1)))
    }

    /// The card covering a cell, if any.
    fn tile_at(grid: &TileGrid, col: u32, row: u32) -> Option<&Tile> {
        grid.tiles()
            .iter()
            .find(|t| col >= t.col && col < t.right() && row >= t.row && row < t.bottom())
    }

    /// A reflow as `"id:from>to, .."`, or `clean`.
    fn reflow_text(reflow: &Reflow) -> String {
        if reflow.is_clean() {
            return "clean".to_owned();
        }
        reflow
            .displaced()
            .iter()
            .map(|d| format!("{}:{}>{}", d.id, d.from, d.to))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The arrangement as `"id@col,row+wxh"` rows — the compact peer of the JSON
    /// `layout`, for a reader that wants one line rather than a document.
    fn tiles_text(grid: &TileGrid) -> String {
        grid.tiles()
            .iter()
            .map(|t| format!("{}@{},{}+{}x{}", t.id, t.col, t.row, t.w, t.h))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// `"<id>,<a>,<b>"` — the wire form both gesture verbs take.
    fn parse_triple(args: &IntrospectValue) -> Option<(TileId, u32, u32)> {
        let IntrospectValue::Text(spec) = args else {
            return None;
        };
        let mut parts = spec.split(',');
        let id = TileId::new(parts.next()?.trim());
        let a = parts.next()?.trim().parse().ok()?;
        let b = parts.next()?.trim().parse().ok()?;
        Some((id, a, b))
    }

    /// Apply an edit to the shared grid and record its outcome.
    fn edit(
        state: &BoardState,
        apply: impl FnOnce(&mut TileGrid) -> Result<Reflow, pinion_core::widgets::tile_grid::TileError>,
    ) -> Result<String, String> {
        let mut grid = state.grid.get();
        match apply(&mut grid) {
            Ok(reflow) => {
                state.grid.set(grid);
                let text = Self::reflow_text(&reflow);
                state.last_reflow.set(text.clone());
                state.refusal.set(String::new());
                Ok(text)
            }
            Err(error) => {
                // A refused edit leaves the grid alone: the model checks before
                // it mutates, so the copy is simply dropped.
                let sentence = error.to_string();
                state.last_reflow.set("clean".to_owned());
                state.refusal.set(sentence.clone());
                Err(sentence)
            }
        }
    }
}

impl External for DashboardOracle {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// Take pointer capture so a press-drag keeps forwarding
    /// [`pointer_move`](Self::pointer_move) while held.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// Snap to a cell, then either latch (a fresh press) or move the latched
    /// card so the grabbed cell stays under the cursor.
    fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
        let Some(state) = self.bound() else {
            return;
        };
        let grid = state.grid.get();
        let (col, row) = Self::cell_at(Self::painted_rows(&grid), x_rel, y_rel);
        if state.pending.get() {
            state.pending.set(false);
            state
                .grab
                .set(Self::tile_at(&grid, col, row).map(|tile| Grab {
                    id: tile.id.clone(),
                    dx: col - tile.col,
                    dy: row - tile.row,
                }));
            return;
        }
        let Some(grab) = state.grab.get() else {
            return;
        };
        // ★ The grab offset is what keeps the card under the finger rather than
        // teleporting its corner to the cursor.
        let target_col = col.saturating_sub(grab.dx);
        let target_row = row.saturating_sub(grab.dy);
        let _ = Self::edit(state, |grid| grid.move_to(&grab.id, target_col, target_row));
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for DashboardOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("columns", "int"),
                    SchemaField::new("row_count", "int"),
                    SchemaField::new("tile_count", "int"),
                    // The whole arrangement, as the value it is.
                    SchemaField::new("layout", "json"),
                    SchemaField::new("tiles", "string"),
                    SchemaField::new("last_reflow", "string"),
                    SchemaField::new("last_refusal", "string"),
                    SchemaField::new("violations", "int"),
                    SchemaField::new("dragging", "string"),
                    // The gestures, as verbs.
                    SchemaField::new("move_to", "string"),
                    SchemaField::new("resize", "string"),
                    SchemaField::new("compact", "string"),
                    SchemaField::new("remove", "string"),
                    SchemaField::new("send", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let state = self.bound()?;
        let grid = state.grid.get();
        let count = |n: usize| Some(IntrospectValue::Int(i64::try_from(n).unwrap_or(i64::MAX)));
        match path {
            "columns" => Some(IntrospectValue::Int(i64::from(grid.columns()))),
            "row_count" => Some(IntrospectValue::Int(i64::from(Self::painted_rows(&grid)))),
            "tile_count" => count(grid.tiles().len()),
            "layout" => serde_json::to_value(&grid).ok().map(IntrospectValue::Json),
            "tiles" => Some(IntrospectValue::Text(Self::tiles_text(&grid))),
            "last_reflow" => Some(IntrospectValue::Text(state.last_reflow.get())),
            "last_refusal" => Some(IntrospectValue::Text(state.refusal.get())),
            "violations" => count(grid.violations().len()),
            "dragging" => Some(IntrospectValue::Text(
                state
                    .grab
                    .get()
                    .map_or_else(String::new, |g| g.id.to_string()),
            )),
            _ => None,
        }
    }

    /// Read-only over every slot: a gesture is a **verb**, so nothing here is a
    /// writable field — and a read-only refusal is told apart from an unknown
    /// path, which is R1566's rule rather than this binding's invention.
    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        if self.query(path).is_some() || self.schema().field_for(path).is_some() {
            return Err(InterveneError::ReadOnly);
        }
        Err(InterveneError::UnknownPath)
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let state = self
            .bound()
            .ok_or_else(|| InvokeError::Rejected("the board is not attached yet".into()))?;
        match path {
            "move_to" | "resize" => {
                let (id, a, b) = Self::parse_triple(&args).ok_or_else(|| {
                    InvokeError::Rejected(
                        if path == "move_to" {
                            "expected \"<id>,<col>,<row>\""
                        } else {
                            "expected \"<id>,<w>,<h>\""
                        }
                        .into(),
                    )
                })?;
                let moving = path == "move_to";
                Self::edit(state, |grid| {
                    if moving {
                        grid.move_to(&id, a, b)
                    } else {
                        grid.resize(&id, a, b)
                    }
                })
                .map(IntrospectValue::Text)
                .map_err(|sentence| InvokeError::Rejected(sentence.into()))
            }
            "compact" => {
                let mut grid = state.grid.get();
                let reflow = grid.compact();
                state.grid.set(grid);
                let text = Self::reflow_text(&reflow);
                state.last_reflow.set(text.clone());
                state.refusal.set(String::new());
                Ok(IntrospectValue::Text(text))
            }
            "remove" => {
                let IntrospectValue::Text(id) = &args else {
                    return Err(InvokeError::Rejected("expected \"<id>\"".into()));
                };
                let mut grid = state.grid.get();
                match grid.remove(&TileId::new(id.trim())) {
                    Ok(tile) => {
                        state.grid.set(grid);
                        state.last_reflow.set("clean".to_owned());
                        state.refusal.set(String::new());
                        Ok(IntrospectValue::Text(tile.id.to_string()))
                    }
                    Err(error) => {
                        let sentence = error.to_string();
                        state.refusal.set(sentence.clone());
                        Err(InvokeError::Rejected(sentence.into()))
                    }
                }
            }
            "send" => {
                if let IntrospectValue::Text(event) = &args {
                    match event.as_str() {
                        "PointerDown" => state.pending.set(true),
                        "PointerUp" | "PointerLeave" | "PointerCancel" => {
                            state.pending.set(false);
                            state.grab.set(None);
                        }
                        _ => {}
                    }
                }
                Ok(IntrospectValue::Null)
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// The board's paint: one grid container, one child per card.
///
/// Every card's placement comes from [`TileGrid::placement`] — the one place the
/// zero-based model becomes CSS's one-based lines — so the painter cannot add
/// one separately and disagree with the model, the wire and the AT tree.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the view-fn signature `WidgetCore::view` hands down is `&Frame`; \
              taking it by value here would put a copy at the one call site"
)]
fn view(grid: &TileGrid, rows: u32, dragging: Option<&TileId>, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let surface = theme.resolve(ColorRole::Surface);
    let card = theme.resolve(ColorRole::Outline);
    let accent = theme.resolve(ColorRole::Accent);
    let ink = theme.resolve(ColorRole::OnSurface);

    let mut children = Vec::with_capacity(grid.tiles().len());
    for tile in grid.tiles() {
        let Some((col, row)) = grid.placement(&tile.id) else {
            continue;
        };
        let held = dragging == Some(&tile.id);
        let label = Scene::Text(
            TextNode::styled(
                format!("{}  {}x{}", tile.id, tile.w, tile.h),
                Rect::default(),
                TextStyle::new().with_size_px(CARD_FONT_PX).with_fg(ink),
            )
            .with_tag(format!("card.{}.label", tile.id)),
        );
        children.push(Scene::Container(
            ContainerNode::new(vec![label])
                .with_tag(format!("card.{}", tile.id))
                .with_style(
                    BoxStyle::filled(if held { accent } else { card }).with_corner_radius(6),
                )
                .with_layout(
                    LayoutStyle::new()
                        .with_grid_column(col)
                        .with_grid_row(row)
                        .with_padding(Rect::new(8, 8, 8, 8)),
                ),
        ));
    }

    Scene::Container(
        ContainerNode::new(vec![
            Scene::Text(
                TextNode::styled(
                    "pinion tile dashboard",
                    Rect::default(),
                    TextStyle::new().with_size_px(TITLE_FONT_PX).with_fg(ink),
                )
                .with_tag("title"),
            ),
            Scene::Container(
                ContainerNode::new(children).with_tag(ROOT_TAG).with_layout(
                    LayoutStyle::new()
                        .grid_columns(vec![GridTrack::Fr(1.0); COLUMNS as usize])
                        .with_grid_rows(vec![GridTrack::Px(ROW_H); rows as usize]),
                ),
            ),
        ])
        .with_style(BoxStyle::filled(surface))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_gap(8)
                .with_padding(Rect::new(12, 12, 12, 12)),
        ),
    )
}

struct DashboardView;

impl WidgetCore for DashboardView {
    /// `()` — the arrangement is a `Vec`, and `State` must be `Copy`. The model
    /// lives in [`use_board_state`], which the oracle and the view share, so
    /// there is one arrangement rather than a copy per reader
    /// (`hello-node-groups`' shape).
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut oracle = DashboardOracle::new();
        oracle.attach(use_board_state());
        Box::new(oracle)
    }

    fn tag() -> &'static str {
        ROOT_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(_state: (), frame: &Frame) -> Scene {
        let state = use_board_state();
        let grid = state.grid.get();
        let rows = DashboardOracle::painted_rows(&grid);
        let dragging = state.grab.get().map(|g| g.id);
        view(&grid, rows, dragging.as_ref(), frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "none"
    }

    fn title() -> &'static str {
        "pinion hello-tile-dashboard (R1608 §5.21 drag-and-snap tile grid)"
    }
}

impl WidgetA11y for DashboardView {
    /// The board as an `application` whose value counts the cards, plus one
    /// `group` per card **naming its slot** — so a screen-reader user can read
    /// the arrangement, which a Grafana board does not expose at all.
    fn access_node(_state: &(), focused: Option<&str>) -> Vec<AccessNode> {
        let state = use_board_state();
        let grid = state.grid.get();
        let mut nodes = vec![
            AccessNode::new(ROOT_TAG, AriaRole::Group)
                .with_name("Tile dashboard")
                .with_value(AccessValue::Text(format!(
                    "{} card(s) on {} columns",
                    grid.tiles().len(),
                    grid.columns()
                )))
                .with_state(AccessState {
                    focused: focused == Some(ROOT_TAG),
                    ..AccessState::default()
                }),
        ];
        for tile in grid.tiles() {
            nodes.push(
                AccessNode::new(format!("card.{}", tile.id), AriaRole::Group)
                    .with_name(tile.id.to_string())
                    .with_value(AccessValue::Text(format!(
                        "column {}, row {}, {} by {}",
                        tile.col + 1,
                        tile.row + 1,
                        tile.w,
                        tile.h
                    ))),
            );
        }
        nodes
    }
}

impl WidgetView for DashboardView {
    type Renderer = HelloTileDashboardRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<DashboardView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    /// An attached oracle inside a live `Owner` scope — the same binding the
    /// shell builds, so a test drives the real thing rather than a stand-in.
    fn drive<T>(body: impl FnOnce(&mut DashboardOracle) -> T) -> T {
        Owner::new().run(|| {
            let mut oracle = DashboardOracle::new();
            oracle.attach(use_board_state());
            body(&mut oracle)
        })
    }

    fn text(oracle: &DashboardOracle, path: &str) -> String {
        match oracle.query(path) {
            Some(IntrospectValue::Text(s)) => s,
            other => panic!("{path} answered {other:?}"),
        }
    }

    fn press(oracle: &mut DashboardOracle) {
        oracle
            .invoke("send", IntrospectValue::Text("PointerDown".to_owned()))
            .unwrap();
    }

    #[test]
    fn the_seed_board_is_the_arrangement_the_layout_measurement_lays_out() {
        drive(|oracle| {
            assert_eq!(oracle.query("columns"), Some(IntrospectValue::Int(12)));
            assert_eq!(oracle.query("tile_count"), Some(IntrospectValue::Int(5)));
            assert_eq!(oracle.query("violations"), Some(IntrospectValue::Int(0)));
            assert_eq!(
                text(oracle, "tiles"),
                "throughput@0,0+12x1 latency@0,1+6x1 loss@6,1+6x1 topology@0,2+4x2 alarms@4,2+8x1"
            );
            assert_eq!(
                oracle.query("row_count"),
                Some(IntrospectValue::Int(5)),
                "four rows of cards, floored at MIN_ROWS so there is board to drag into"
            );
        });
    }

    #[test]
    fn the_whole_arrangement_is_one_json_read() {
        drive(|oracle| {
            let Some(IntrospectValue::Json(value)) = oracle.query("layout") else {
                panic!("layout is a document")
            };
            let back: TileGrid = serde_json::from_value(value).unwrap();
            assert_eq!(back.tiles().len(), 5);
            assert!(back.violations().is_empty());
        });
    }

    #[test]
    fn a_move_verb_reports_what_it_displaced() {
        drive(|oracle| {
            let reply = oracle
                .invoke("move_to", IntrospectValue::Text("topology,0,0".to_owned()))
                .unwrap();
            assert_eq!(
                reply,
                IntrospectValue::Text("throughput:0>2, latency:1>3, alarms:2>4".to_owned()),
                "the displaced cards are NAMED, which Grafana cannot answer"
            );
            assert_eq!(
                text(oracle, "last_reflow"),
                "throughput:0>2, latency:1>3, alarms:2>4",
                "the reflow is TRANSITIVE: alarms was pushed by the throughput \
                 that topology pushed, and then again by the latency below it"
            );
            assert_eq!(oracle.query("violations"), Some(IntrospectValue::Int(0)));
        });
    }

    #[test]
    fn a_move_that_fits_is_clean() {
        drive(|oracle| {
            oracle
                .invoke("move_to", IntrospectValue::Text("topology,8,3".to_owned()))
                .unwrap();
            assert_eq!(text(oracle, "last_reflow"), "clean");
        });
    }

    #[test]
    fn a_refused_gesture_says_why_and_changes_nothing() {
        drive(|oracle| {
            let before = text(oracle, "tiles");
            let wide = oracle.invoke("resize", IntrospectValue::Text("latency,20,1".to_owned()));
            assert!(matches!(wide, Err(InvokeError::Rejected(_))));
            assert_eq!(
                text(oracle, "last_refusal"),
                "a tile 20 columns wide does not fit a grid of 12"
            );
            let ghost = oracle.invoke("move_to", IntrospectValue::Text("ghost,0,0".to_owned()));
            assert!(matches!(ghost, Err(InvokeError::Rejected(_))));
            assert_eq!(text(oracle, "last_refusal"), "no tile ghost in this grid");
            assert_eq!(text(oracle, "tiles"), before, "no refusal moved a card");
        });
    }

    #[test]
    fn a_malformed_argument_is_refused_apart_from_a_wrong_one() {
        drive(|oracle| {
            match oracle.invoke("move_to", IntrospectValue::Text("latency,x".to_owned())) {
                Err(InvokeError::Rejected(reason)) => {
                    assert!(reason.to_string().contains("expected"), "{reason:?}");
                }
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn compaction_is_a_verb_here_too_and_removing_a_card_leaves_its_gap() {
        drive(|oracle| {
            oracle
                .invoke("remove", IntrospectValue::Text("throughput".to_owned()))
                .unwrap();
            assert!(
                text(oracle, "tiles").starts_with("latency@0,1"),
                "the gap stays until someone asks for it to close"
            );
            oracle.invoke("compact", IntrospectValue::Null).unwrap();
            assert!(text(oracle, "tiles").starts_with("latency@0,0"));
            assert_eq!(
                text(oracle, "last_reflow"),
                "latency:1>0, loss:1>0, topology:2>1, alarms:2>1"
            );
        });
    }

    #[test]
    fn a_press_drag_keeps_the_card_under_the_finger() {
        // ★ The gesture R1607 could not test without a consumer. Grab
        // `topology` (columns 0..4 on rows 2..4) TWO columns in from its left
        // edge, then drag the cursor to column 5: the card's left edge must
        // land at 3, not at the cursor's column 5.
        //
        // `alarms` was the first choice and it could not move at all — eight
        // columns wide in a grid of twelve pins it to column 4, which the
        // model's own clamp does correctly. The fixture had to be a card
        // narrow enough to have somewhere to go.
        drive(|oracle| {
            press(oracle);
            oracle.pointer_move(2.5 / 12.0, 2.5 / 5.0);
            assert_eq!(text(oracle, "dragging"), "topology");

            oracle.pointer_move(5.5 / 12.0, 2.5 / 5.0);
            assert!(
                text(oracle, "tiles").contains("topology@3,2+4x2"),
                "the grab offset moved with the card — a corner-snap would have \
                 put it at column 5. Got {}",
                text(oracle, "tiles")
            );

            oracle
                .invoke("send", IntrospectValue::Text("PointerUp".to_owned()))
                .unwrap();
            assert_eq!(text(oracle, "dragging"), "", "the release let go");
            let settled = text(oracle, "tiles");
            oracle.pointer_move(0.05, 0.05);
            assert_eq!(
                text(oracle, "tiles"),
                settled,
                "and a move after the release moves nothing"
            );
        });
    }

    #[test]
    fn a_press_on_empty_board_latches_nothing() {
        drive(|oracle| {
            press(oracle);
            // Row 4 is below every card.
            oracle.pointer_move(0.5, 4.5 / 5.0);
            assert_eq!(text(oracle, "dragging"), "");
            let before = text(oracle, "tiles");
            oracle.pointer_move(0.1, 0.1);
            assert_eq!(text(oracle, "tiles"), before);
        });
    }

    #[test]
    fn a_drag_displaces_and_the_board_stays_legal() {
        drive(|oracle| {
            press(oracle);
            oracle.pointer_move(0.02, 2.5 / 5.0); // grab `topology` at its corner
            assert_eq!(text(oracle, "dragging"), "topology");
            oracle.pointer_move(0.02, 0.02); // drag it to the top-left
            assert!(text(oracle, "tiles").contains("topology@0,0"));
            assert_ne!(text(oracle, "last_reflow"), "clean");
            assert_eq!(oracle.query("violations"), Some(IntrospectValue::Int(0)));
        });
    }

    #[test]
    fn every_slot_is_read_only_and_an_unknown_one_is_told_apart() {
        drive(|oracle| {
            assert_eq!(
                oracle.intervene("tiles", IntrospectValue::Text(String::new())),
                Err(InterveneError::ReadOnly),
                "a gesture is a verb, so no slot here is writable"
            );
            assert_eq!(
                oracle.intervene("nonesuch", IntrospectValue::Text(String::new())),
                Err(InterveneError::UnknownPath)
            );
            assert!(matches!(
                oracle.invoke("nonesuch", IntrospectValue::Null),
                Err(InvokeError::UnknownPath)
            ));
        });
    }

    #[test]
    fn the_at_tree_reads_the_same_slots_the_painter_places_from() {
        Owner::new().run(|| {
            let nodes = DashboardView::access_node(&(), Some(ROOT_TAG));
            assert_eq!(nodes.len(), 6, "the board plus one group per card");
            let alarms = nodes
                .iter()
                .find(|n| n.tag == "card.alarms")
                .expect("a node per card");
            assert_eq!(
                alarms.value,
                Some(AccessValue::Text("column 5, row 3, 8 by 1".to_owned())),
                "the AT tree states CSS's one-based lines, derived from the same \
                 placement the painter uses"
            );
        });
    }

    #[test]
    fn the_painted_board_is_a_grid_with_one_child_per_card() {
        fn board(scene: &Scene) -> Option<&ContainerNode> {
            match scene {
                Scene::Container(c) if c.tag.as_deref() == Some(ROOT_TAG) => Some(c),
                Scene::Container(c) => c.children.iter().find_map(board),
                _ => None,
            }
        }
        Owner::new().run(|| {
            let scene = DashboardView::view((), &Frame::new());
            let container = board(&scene).expect("the board container is tagged");
            assert_eq!(
                container.layout.display,
                pinion_core::style::Display::Grid,
                "twelve Fr tracks put it in grid mode"
            );
            assert_eq!(container.children.len(), 5);
            assert_eq!(container.layout.grid_template_columns.len(), 12);
        });
    }
}
