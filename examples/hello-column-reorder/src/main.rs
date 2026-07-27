// R1450 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the architectural narrative carries many proper-noun
// identifiers (QHeaderView, ReorderModel, WAI-ARIA, …).
#![allow(clippy::doc_markdown)]

//! `hello-column-reorder` — R1450 §5.27 §5.40 §5.51 **a column moves where its
//! header is dragged**: Qt's `QHeaderView` movable sections.
//!
//! ## The gap this closes
//!
//! Every other column axis was already in tree — width (R785/R786), visibility
//! (R990), sort (R778), filter (R783/R997), frozen panes (R859) — and the one
//! Qt has that pinion did not was **section order**: `setSectionsMovable`,
//! `moveSection`, and the `visualIndex` <-> `logicalIndex` mapping every other
//! axis has to compose with. This binding adds it as the **4th consumer** of
//! the lifted [`ReorderModel`] (R743: `hello-dnd` vertical, `hello-tab-reorder`
//! horizontal, `hello-data-grid` rows), so the drag session, the APG keyboard
//! grab, and the permutation come from the proven model rather than a fourth
//! hand-rolled copy.
//!
//! ## Why the header strip is its own external (and matches the reference)
//!
//! In Qt a `QHeaderView` is a **separate widget** the view owns, not a band
//! inside it — so modelling the header as its own external is the faithful
//! shape, not a shortcut. It is also the shape the substrate needs:
//! [`ReorderModel`]'s drop classification reads the composite `#<visual>`
//! subindex off the hovered tag, and the eager table's header cells are tagged
//! `{tag}_ch{col}` (no subindex) because their click routes to the table's own
//! sort. A strip whose cells ARE `colhdr#<visual>` gives the drag session real
//! per-section hit nodes, and the body below simply paints through the order.
//!
//! ## visualIndex / logicalIndex are the mapping, not a convenience
//!
//! `order[visual] = logical` is the whole model. The body's cell text, the
//! header labels, and the a11y column headers are all projected through it in
//! one place ([`visual_columns`]), so a reorder cannot move the header without
//! moving its column — the failure mode a second projection would invite.
//!
//! ## AI clients (§2 #7 + §2 #2 — where Qt cannot follow)
//!
//! Qt persists a header layout as `QHeaderView::saveState()`, an **opaque
//! versioned `QByteArray`**: an agent cannot read "which column is third now"
//! out of it, and cannot write one either without a live `QHeaderView`. Here the
//! permutation is typed data both ways — `query("order")` /
//! `query("visual_index.<logical>")` / `query("logical_index.<visual>")` read
//! it, `invoke("move_section", "<from>:<to>")` performs Qt's move, and
//! `intervene("order", [..])` restores a whole saved layout (R1450 made the
//! model's `order` writable, so `hello-tab-reorder` got session-order restore
//! from the same change).

use pinion_a11y::{AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_core::command::Command;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, DragPayload, DropPoint, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaArg,
    SchemaField, ThreadOwnership,
};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::reorder::{ReorderAxis, ReorderModel};
use pinion_core::{Frame, Intent, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use std::borrow::Cow;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloColumnReorderRenderer, HelloColumnReorderRendererError);

const WIN_W: u32 = 700;
const WIN_H: u32 = 420;
const THEME_TAG: &str = "app";

/// The header strip (primary external) — the `QHeaderView`. Section cells paint
/// as `colhdr#<visual>`, which is what gives the drag session per-section hit
/// nodes.
const HDR_TAG: &str = "colhdr";
/// The body container tag; data cells paint as `colbody#<row>_<visual>`.
const BODY_TAG: &str = "colbody";
/// Scene-as-data readout of the current section order.
const ORDER_TAG: &str = "colreorder_order";
/// The `DragPayload::kind` discriminator for a section drag.
const DRAG_KIND: &str = "column-section";

/// Logical columns — the source schema. `order[visual] = logical` indexes this.
const HEADERS: [&str; 5] = ["Name", "Type", "Size", "Modified", "Owner"];
const NCOLS: usize = HEADERS.len();
const NROWS: usize = 6;
/// The un-reordered section order, and the fallback when a read comes back
/// malformed.
const IDENTITY_ORDER: [usize; NCOLS] = [0, 1, 2, 3, 4];

const NAMES: [&str; NROWS] = [
    "report.pdf",
    "photo.png",
    "notes.txt",
    "build.rs",
    "data.csv",
    "movie.mp4",
];
const TYPES: [&str; NROWS] = ["PDF", "Image", "Text", "Rust", "CSV", "Video"];
const SIZES: [&str; NROWS] = ["2.1 MB", "880 KB", "4 KB", "1 KB", "32 KB", "1.4 GB"];
const MODIFIED: [&str; NROWS] = [
    "2026-06-01",
    "2026-05-30",
    "2026-06-10",
    "2026-06-18",
    "2026-04-22",
    "2026-03-09",
];
const OWNERS: [&str; NROWS] = ["coin", "coin", "alex", "coin", "alex", "guest"];

/// Cell text for **logical** `(row, col)` — the source model, which a reorder
/// never touches.
fn cell_text(row: usize, logical: usize) -> &'static str {
    [NAMES, TYPES, SIZES, MODIFIED, OWNERS][logical][row]
}

// Geometry: uniform sections so a section's rect is derivable, and the demo can
// aim a drag at an exact half.
const GRID_X: u32 = 30;
const GRID_Y: u32 = 90;
const COL_W: u32 = 124;
const HDR_H: u32 = 40;
const ROW_H: u32 = 34;

/// The section paint / hit tag for visual position `i` (`"colhdr#0"` …).
fn section_tag(visual: usize) -> String {
    format!("{HDR_TAG}#{visual}")
}

/// The header strip external: the `QHeaderView`. It owns the section
/// permutation through the lifted [`ReorderModel`] and adds the Qt index
/// mapping on top; it holds no column *data* — the view projects the schema
/// through [`ReorderModel::order`].
#[derive(Debug)]
struct ColumnHeaderExternal {
    reorder: ReorderModel,
}

impl ColumnHeaderExternal {
    fn new() -> Self {
        Self {
            reorder: ReorderModel::new(NCOLS, ReorderAxis::Horizontal),
        }
    }

    /// Qt `QHeaderView::visualIndex(logical)` — where a logical column is now
    /// displayed. `None` when `logical` is not a column.
    fn visual_index(&self, logical: usize) -> Option<usize> {
        self.reorder.order().iter().position(|&l| l == logical)
    }

    /// Qt `QHeaderView::logicalIndex(visual)` — which column is displayed at a
    /// position.
    fn logical_index(&self, visual: usize) -> Option<usize> {
        self.reorder.order().get(visual).copied()
    }
}

impl External for ColumnHeaderExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }
    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }
    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }
    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }
    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    /// The three R742 drag hooks delegate straight to the model — a section
    /// drag is mechanically the tab-strip drag with a different paint.
    fn begin_drag(&self) -> Option<DragPayload> {
        self.reorder.begin_drag_payload(Cow::Borrowed(DRAG_KIND))
    }

    fn drag_to(&mut self, payload: &DragPayload, over: Option<DropPoint>) {
        self.reorder.drag_to(payload, over.as_ref());
    }

    fn drag_release(&mut self, payload: &DragPayload, over: Option<DropPoint>) {
        self.reorder.drag_release(payload, over.as_ref());
    }
}

impl ExternalIntrospect for ColumnHeaderExternal {
    fn schema(&self) -> IntrospectSchema {
        // `order` / `preview` / `focused_index` / `grabbed` come from the model;
        // `labels`, the Qt index mapping, and `count` are this binding's.
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("order", "json"),
                    SchemaField::new("labels", "json"),
                    SchemaField::new("count", "int"),
                    SchemaField::new("preview", "json"),
                    SchemaField::new("focused_index", "int"),
                    SchemaField::new("grabbed", "boolean"),
                    SchemaField::parametric(
                        "visual_index.<logical>",
                        "int",
                        const { &[SchemaArg::index("logical", "count")] },
                    ),
                    SchemaField::parametric(
                        "logical_index.<visual>",
                        "int",
                        const { &[SchemaArg::index("visual", "count")] },
                    ),
                    SchemaField::new("send", "string"),
                    SchemaField::new("move", "int"),
                    SchemaField::new("move_section", "string"),
                    SchemaField::new("grab", "boolean"),
                    SchemaField::new("grab_cancel", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        // The Qt mapping, both directions. Out of range reports Null
        // (present-but-empty), never absence — the shared edge contract.
        if let Some(rest) = path.strip_prefix("visual_index.") {
            let v = rest
                .parse::<usize>()
                .ok()
                .and_then(|l| self.visual_index(l));
            return Some(
                v.and_then(|i| i64::try_from(i).ok())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int),
            );
        }
        if let Some(rest) = path.strip_prefix("logical_index.") {
            let v = rest
                .parse::<usize>()
                .ok()
                .and_then(|p| self.logical_index(p));
            return Some(
                v.and_then(|i| i64::try_from(i).ok())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int),
            );
        }
        match path {
            // Header labels in VISUAL order — what the strip reads left to
            // right, which is the thing a human compares a snapshot against.
            "labels" => {
                let arr: Vec<serde_json::Value> = self
                    .reorder
                    .order()
                    .iter()
                    .map(|&l| serde_json::Value::from(HEADERS[l]))
                    .collect();
                Some(IntrospectValue::Json(serde_json::Value::Array(arr)))
            }
            "count" => Some(IntrospectValue::Int(
                i64::try_from(NCOLS).unwrap_or(i64::MAX),
            )),
            // Reorder-owned slots (order / preview / focused_index / grabbed);
            // the model returns None for anything else, so the slots above win.
            other => self.reorder.query(other),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "labels" | "count" => Err(InterveneError::ReadOnly),
            // `focused_index` and (R1450) the whole `order` permutation — Qt's
            // restoreState, as typed data rather than an opaque blob.
            other => self.reorder.intervene(other, &value),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        // `send` / `move` / `move_section` / `grab` / `grab_cancel` are all the
        // model's; this binding adds no action of its own, which is the point of
        // the lift.
        self.reorder.invoke(path, &args)
    }
}

/// The logical columns in display order — the single projection the header
/// paint, the body paint, and the a11y tree all read, so a section cannot move
/// its label without moving its data.
fn visual_columns(scene: &Scene) -> [usize; NCOLS] {
    let read: Option<Vec<usize>> = scene
        .find_external_with_tag(HDR_TAG)
        .and_then(|n| n.handle.introspect())
        .and_then(|i| i.query("order"))
        .and_then(|v| match v {
            IntrospectValue::Json(serde_json::Value::Array(items)) => items
                .iter()
                .map(|x| x.as_u64().and_then(|n| usize::try_from(n).ok()))
                .collect(),
            _ => None,
        });
    // A short / absent read falls back to the identity order rather than
    // painting a partial grid: the strip is always NCOLS sections wide.
    read.and_then(|v| <[usize; NCOLS]>::try_from(v).ok())
        .unwrap_or(IDENTITY_ORDER)
}

/// Copy posture the view paints from — the section order plus the live drag
/// preview, both read off the primary external.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct HeaderState {
    /// `order[visual] = logical`.
    order: [usize; NCOLS],
    /// The dragged visual position, while a drag is in flight.
    dragging: Option<usize>,
    /// The insertion gap the drop would target (`0..=NCOLS`).
    insert_at: Option<usize>,
    /// Keyboard cursor / active descendant.
    focused: Option<usize>,
    /// Whether an APG keyboard grab is in flight.
    grabbed: bool,
}

impl Default for HeaderState {
    fn default() -> Self {
        Self {
            order: IDENTITY_ORDER,
            dragging: None,
            insert_at: None,
            focused: None,
            grabbed: false,
        }
    }
}

fn read_header_state(scene: &Scene) -> HeaderState {
    let mut out = HeaderState {
        order: visual_columns(scene),
        ..HeaderState::default()
    };
    let Some(intro) = scene
        .find_external_with_tag(HDR_TAG)
        .and_then(|n| n.handle.introspect())
    else {
        return out;
    };
    if let Some(IntrospectValue::Json(p)) = intro.query("preview") {
        out.dragging = p
            .get("from_visual")
            .and_then(serde_json::Value::as_u64)
            .and_then(|n| usize::try_from(n).ok());
        out.insert_at = p
            .get("insert_at")
            .and_then(serde_json::Value::as_u64)
            .and_then(|n| usize::try_from(n).ok());
    }
    if let Some(IntrospectValue::Int(i)) = intro.query("focused_index") {
        out.focused = usize::try_from(i).ok();
    }
    if let Some(IntrospectValue::Bool(g)) = intro.query("grabbed") {
        out.grabbed = g;
    }
    out
}

/// One header section cell, tagged `colhdr#<visual>` so the router's `'#'`
/// split reaches the composite external and the model's drop classification
/// sees a real subindex.
fn section_cell(visual: usize, logical: usize, state: &HeaderState, theme: &Theme) -> Scene {
    let is_dragged = state.dragging == Some(visual);
    let fill = if is_dragged {
        theme.resolve(ColorRole::SurfaceContainerLow)
    } else if state.focused == Some(visual) {
        theme.resolve(ColorRole::SurfaceContainerHighest)
    } else {
        theme.resolve(ColorRole::SurfaceContainerHigh)
    };
    let label = Scene::Text(
        TextNode::styled(
            HEADERS[logical],
            Rect::default(),
            TextStyle::new().with_size_px(14).with_fg(if is_dragged {
                theme.resolve(ColorRole::OnSurfaceMuted)
            } else {
                theme.resolve(ColorRole::OnSurface)
            }),
        )
        .with_tag(format!("colhdr_label#{visual}"))
        .with_layout(LayoutStyle::new().with_absolute_position(12, 12)),
    );
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(section_tag(visual))
            .with_style(BoxStyle::filled(fill))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(u32::try_from(visual).unwrap_or(0) * COL_W, 0)
                    .with_size(Size::px(COL_W - 2, HDR_H)),
            ),
    )
}

/// The strip that owns the sections. It carries the external's own tag and is
/// the §5.39 Tab stop, so the keyboard model has something to focus — Qt's
/// `QHeaderView` is one focusable widget whose sections are its parts, not five
/// separate tab stops.
fn header_strip(sections: Vec<Scene>, theme: &Theme) -> Scene {
    Scene::Container(
        ContainerNode::new(sections)
            .with_tag(HDR_TAG)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainer)))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(GRID_X, GRID_Y)
                    .with_size(Size::px(u32::try_from(NCOLS).unwrap_or(0) * COL_W, HDR_H))
                    .with_focusable(true),
            ),
    )
}

/// The insertion line the live drag draws at gap `insert_at`.
fn insertion_line(insert_at: usize, theme: &Theme) -> Scene {
    let x = GRID_X + u32::try_from(insert_at).unwrap_or(0) * COL_W;
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag("colhdr_dropline")
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Accent)))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(x.saturating_sub(1), GRID_Y)
                    .with_size(Size::px(
                        3,
                        HDR_H + u32::try_from(NROWS).unwrap_or(0) * ROW_H,
                    )),
            ),
    )
}

/// One body cell, tagged `colbody#<row>_<visual>` — the data at the logical
/// column now displayed at `visual`.
fn body_cell(row: usize, visual: usize, logical: usize, theme: &Theme) -> Scene {
    let label = Scene::Text(
        TextNode::styled(
            cell_text(row, logical),
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_tag(format!("{BODY_TAG}#{row}_{visual}"))
        .with_layout(LayoutStyle::new().with_absolute_position(12, 9)),
    );
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_style(BoxStyle::filled(if row % 2 == 0 {
                theme.resolve(ColorRole::Surface)
            } else {
                theme.resolve(ColorRole::SurfaceContainerLow)
            }))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(
                        GRID_X + u32::try_from(visual).unwrap_or(0) * COL_W,
                        GRID_Y + HDR_H + u32::try_from(row).unwrap_or(0) * ROW_H,
                    )
                    .with_size(Size::px(COL_W - 2, ROW_H - 2)),
            ),
    )
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &HeaderState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let mut children: Vec<Scene> = Vec::with_capacity(NCOLS * (NROWS + 1) + 3);

    let caption = Scene::Text(
        TextNode::styled(
            "Drag a header to move its column",
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(GRID_X, 30)),
    );
    let order_row = Scene::Text(
        TextNode::styled(
            format!(
                "order {} | grabbed {}",
                state
                    .order
                    .iter()
                    .map(|&l| HEADERS[l])
                    .collect::<Vec<_>>()
                    .join(" "),
                state.grabbed,
            ),
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_tag(ORDER_TAG)
        .with_layout(LayoutStyle::new().with_absolute_position(GRID_X, 55)),
    );
    children.push(caption);
    children.push(order_row);

    let mut sections: Vec<Scene> = Vec::with_capacity(NCOLS);
    for (visual, &logical) in state.order.iter().enumerate() {
        sections.push(section_cell(visual, logical, state, &theme));
        for row in 0..NROWS {
            children.push(body_cell(row, visual, logical, &theme));
        }
    }
    children.push(header_strip(sections, &theme));
    if let Some(gap) = state.insert_at {
        children.push(insertion_line(gap, &theme));
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Start),
            ),
    )
}

struct ColumnReorderView;

impl WidgetCore for ColumnReorderView {
    type State = HeaderState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ColumnHeaderExternal::new())
    }

    fn tag() -> &'static str {
        HDR_TAG
    }

    fn read_state(scene: &Scene) -> HeaderState {
        read_header_state(scene)
    }

    fn view(state: HeaderState, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-column-reorder (R1450 §5.51 QHeaderView movable sections)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// APG keyboard drag, modifier-free so it drives through plain `scene/key`:
    /// arrows move the cursor, or the grabbed section; Space / Enter picks up
    /// and drops; Escape cancels back to the pre-grab order. The policy is the
    /// binding's; every mutation is the model's.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(HDR_TAG) {
            return false;
        }
        let Some(intro) = scene
            .find_external_with_tag_mut(HDR_TAG)
            .and_then(|n| n.handle.introspect_mut())
        else {
            return false;
        };
        let cursor = match intro.query("focused_index") {
            Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
            _ => None,
        };
        let grabbed = matches!(intro.query("grabbed"), Some(IntrospectValue::Bool(true)));
        match key {
            "ArrowRight" | "ArrowLeft" => {
                let delta = if key == "ArrowRight" { 1 } else { -1 };
                if grabbed {
                    // Move the picked-up section, cursor following.
                    intro.invoke("move", IntrospectValue::Int(delta)).is_ok()
                } else {
                    let next = match cursor {
                        Some(c) => usize::try_from(
                            (i64::try_from(c).unwrap_or(0) + delta)
                                .clamp(0, i64::try_from(NCOLS - 1).unwrap_or(0)),
                        )
                        .unwrap_or(0),
                        None => 0,
                    };
                    intro
                        .intervene(
                            "focused_index",
                            IntrospectValue::Int(i64::try_from(next).unwrap_or(0)),
                        )
                        .is_ok()
                }
            }
            "Home" | "End" => {
                let target = if key == "Home" { 0 } else { NCOLS - 1 };
                if grabbed {
                    intro
                        .invoke(
                            "move_section",
                            IntrospectValue::Text(format!("{}:{target}", cursor.unwrap_or(0))),
                        )
                        .is_ok()
                } else {
                    intro
                        .intervene(
                            "focused_index",
                            IntrospectValue::Int(i64::try_from(target).unwrap_or(0)),
                        )
                        .is_ok()
                }
            }
            " " | "Enter" => {
                cursor.is_some() && intro.invoke("grab", IntrospectValue::Null).is_ok()
            }
            "Escape" => grabbed && intro.invoke("grab_cancel", IntrospectValue::Null).is_ok(),
            _ => false,
        }
    }

    fn update(_state: HeaderState, _intent: &Intent) -> Vec<Command> {
        Vec::new()
    }

    fn fmt_state_log(state: &HeaderState) -> String {
        format!("order={:?} focused={:?}", state.order, state.focused)
    }
}

impl WidgetA11y for ColumnReorderView {
    /// The header strip as a WAI-ARIA `row` of `columnheader`s, announced in
    /// **visual** order — an AT reading the strip left to right hears what the
    /// screen shows, which is the whole point of a movable section.
    fn access_node(state: &HeaderState, focused: Option<&str>) -> Vec<AccessNode> {
        let strip_focused = focused == Some(HDR_TAG);
        let mut nodes = vec![
            AccessNode::new(HDR_TAG, AriaRole::Row)
                .with_name("Columns")
                .with_state(AccessState {
                    focused: strip_focused,
                    ..AccessState::default()
                }),
        ];
        for (visual, &logical) in state.order.iter().enumerate() {
            nodes.push(
                AccessNode::new(section_tag(visual), AriaRole::ColumnHeader)
                    .with_name(HEADERS[logical])
                    .with_state(AccessState {
                        focused: strip_focused && state.focused == Some(visual),
                        ..AccessState::default()
                    }),
            );
        }
        nodes
    }
}

impl WidgetView for ColumnReorderView {
    type Renderer = HelloColumnReorderRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<ColumnReorderView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    fn fresh() -> ColumnHeaderExternal {
        ColumnHeaderExternal::new()
    }

    fn boot_scene() -> Scene {
        Scene::Container(ContainerNode::new(vec![Scene::External(
            ExternalNode::new(ColumnReorderView::create_external()).with_tag(HDR_TAG),
        )]))
    }

    fn press(scene: &mut Scene, key: &str) -> bool {
        ColumnReorderView::apply_key(scene, Some(HDR_TAG), key, pinion_core::Modifiers::empty())
    }

    fn order_of(scene: &Scene) -> [usize; NCOLS] {
        visual_columns(scene)
    }

    #[test]
    fn r1450_the_index_mapping_is_the_inverse_of_the_order() {
        let mut ext = fresh();
        ext.invoke("move_section", IntrospectValue::Text("0:2".into()))
            .expect("move_section is a known action");
        // order = [Type, Size, Name, Modified, Owner] = [1, 2, 0, 3, 4]
        assert_eq!(ext.logical_index(2), Some(0), "Name is displayed third");
        assert_eq!(ext.visual_index(0), Some(2), "and Name's visual index is 2");
        for logical in 0..NCOLS {
            let v = ext.visual_index(logical).expect("every column is placed");
            assert_eq!(
                ext.logical_index(v),
                Some(logical),
                "the two directions must invert each other"
            );
        }
        assert_eq!(ext.query("visual_index.9"), Some(IntrospectValue::Null));
        assert_eq!(ext.query("logical_index.9"), Some(IntrospectValue::Null));
    }

    #[test]
    fn r1450_the_labels_readout_is_the_visual_order() {
        let mut ext = fresh();
        ext.invoke("move_section", IntrospectValue::Text("4:0".into()))
            .expect("move_section is a known action");
        assert_eq!(
            ext.query("labels"),
            Some(IntrospectValue::Json(serde_json::json!([
                "Owner", "Name", "Type", "Size", "Modified"
            ])))
        );
    }

    #[test]
    fn r1450_the_body_follows_its_header() {
        // The projection is one function, so a section move must carry its data.
        let mut scene = boot_scene();
        let before: Vec<&str> = (0..NCOLS).map(|c| cell_text(0, c)).collect();
        assert_eq!(before[0], "report.pdf", "Name is the first column's data");
        {
            let intro = scene
                .find_external_with_tag_mut(HDR_TAG)
                .and_then(|n| n.handle.introspect_mut())
                .expect("the header external is in the scene");
            intro
                .invoke("move_section", IntrospectValue::Text("0:4".into()))
                .expect("move_section is a known action");
        }
        let order = order_of(&scene);
        assert_eq!(order.last(), Some(&0), "Name is now displayed last");
        assert_eq!(
            cell_text(0, order[4]),
            "report.pdf",
            "and its data is what the last visual column paints"
        );
        assert_eq!(cell_text(0, order[0]), "PDF", "Type took the first slot");
    }

    #[test]
    fn r1450_a_saved_order_restores_over_the_wire() {
        let mut ext = fresh();
        ext.intervene(
            "order",
            IntrospectValue::Json(serde_json::json!([4, 3, 2, 1, 0])),
        )
        .expect("a permutation restores the layout");
        assert_eq!(
            ext.query("labels"),
            Some(IntrospectValue::Json(serde_json::json!([
                "Owner", "Modified", "Size", "Type", "Name"
            ])))
        );
        // Not a permutation: refused, and nothing moved.
        assert!(matches!(
            ext.intervene(
                "order",
                IntrospectValue::Json(serde_json::json!([0, 0, 1, 2, 3]))
            ),
            Err(InterveneError::OutOfRange)
        ));
        assert_eq!(
            ext.logical_index(0),
            Some(4),
            "the refused write changed nothing"
        );
        assert!(matches!(
            ext.intervene("labels", IntrospectValue::Int(0)),
            Err(InterveneError::ReadOnly)
        ));
    }

    #[test]
    fn r1450_the_keyboard_grab_moves_a_section_and_escape_reverts() {
        let mut scene = boot_scene();
        assert!(press(&mut scene, "ArrowRight"), "cursor lands on section 0");
        assert!(press(&mut scene, "ArrowRight"), "cursor moves to 1");
        assert!(press(&mut scene, " "), "Space picks the section up");
        assert!(press(&mut scene, "ArrowRight"), "the grabbed section moves");
        assert_eq!(order_of(&scene), [0, 2, 1, 3, 4], "Type and Size swapped");
        assert!(press(&mut scene, "Escape"), "Escape cancels the grab");
        assert_eq!(
            order_of(&scene),
            [0, 1, 2, 3, 4],
            "and reverts to the pre-grab order"
        );
    }

    #[test]
    fn r1450_the_a11y_strip_announces_the_visual_order() {
        let mut scene = boot_scene();
        {
            let intro = scene
                .find_external_with_tag_mut(HDR_TAG)
                .and_then(|n| n.handle.introspect_mut())
                .expect("the header external is in the scene");
            intro
                .invoke("move_section", IntrospectValue::Text("4:0".into()))
                .expect("move_section is a known action");
        }
        let state = read_header_state(&scene);
        let nodes = ColumnReorderView::access_node(&state, Some(HDR_TAG));
        let names: Vec<&str> = nodes
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader)
            .filter_map(|n| n.name.as_deref())
            .collect();
        assert_eq!(
            names,
            ["Owner", "Name", "Type", "Size", "Modified"],
            "an AT reads the strip in the order the screen shows"
        );
    }
}
