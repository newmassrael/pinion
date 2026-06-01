//! `hello-dnd` — R742 §5.51 — first consumer of the generic
//! **drag-and-drop substrate**: the three additive
//! [`External`](pinion_core::external::External) hooks `begin_drag` /
//! `drag_to` / `drag_release` plus the `InputRouter` drag session that
//! resolves the drop location under the *absolute* cursor (the
//! pointer-driven generalisation of the invoke-driven dock
//! `resolve_dock_drop`, whose own doc deferred exactly this "shell grows
//! a drag-session" round).
//!
//! The canonical sortable-list demo: a vertical list of four colour-coded
//! rows. Each row is **both** a drag source and a drop target — pressing
//! a row arms a drag (`begin_drag`), moving the cursor over another row
//! shows a live insertion line (`drag_to` updates a preview the view
//! reads), and releasing reorders the list (`drag_release`). Because
//! every drop candidate belongs to one coordinator
//! ([`ReorderListExternal`]), the source *is* the resolver — no
//! cross-widget hook is needed, matching the dock model.
//!
//! Driven by the existing `scene/drag` RPC (press → N interpolated moves
//! → release): no new RPC method. The reorder is observable as
//! scene-as-data through `query("labels")` / `query("order")`, and the
//! row colours are a live-pixel witness that the visual order changed.
//!
//! The list owns no statechart (the drag *session* lives in the router),
//! so this is a plain `External` value/coordinator holder like
//! `hello-progress` / `hello-tooltip`. Keyboard reorder (APG
//! `Ctrl+Arrow` move) and free-floating drag ghost are deferred axes
//! (named in the round notes) — v1 reorder is reachable via pointer +
//! `scene/drag`, and the list is AT-readable as an `AriaRole::List`.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};

use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, DragPayload, DropPoint, External, ExternalIntrospect,
    IntrospectSchema, IntrospectValue, InterveneError, InvokeError, RepaintOwner, ThreadOwnership,
};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloDndRenderer, HelloDndRendererError);

/// Number of rows — fixed so [`ListState`] stays `Copy`
/// (`WidgetCore::State: Copy`, the `hello-table` `[RadioState; NROWS]`
/// precedent). The reorder substrate itself is order-count agnostic; the
/// fixed array is purely the state-projection shape.
const N: usize = 4;

/// The four list items: `(label, witness colour)`. The colour is a
/// deterministic per-item fill so a reorder is visible at the pixel level
/// (sampling a row centre yields that item's colour wherever it now
/// sits). Strong, well-separated hues so the live-pixel guard is robust.
const ITEMS: [(&str, Color); N] = [
    ("Alpha", Color::rgb(0xE5, 0x39, 0x35)),   // red
    ("Bravo", Color::rgb(0x43, 0xA0, 0x47)),   // green
    ("Charlie", Color::rgb(0x1E, 0x88, 0xE5)), // blue
    ("Delta", Color::rgb(0xFB, 0xC0, 0x2D)),   // amber
];

const WIN_W: u32 = 300;
const WIN_H: u32 = 260;
const THEME_TAG: &str = "app";
/// Root `External` tag — the composite primary the rows hang off
/// (`dnd#0`..`dnd#3`) and the path the framework maps `/external/*` onto.
const TAG: &str = "dnd";
const ROW_W: u32 = 240;
const ROW_H: u32 = 44;
const ROW_RADIUS: u32 = 8;
const GAP: u32 = 6;
/// Insertion-line thickness (drawn at the drop gap during a drag).
const LINE_H: u32 = 4;

/// A live drag preview: which visual row is being dragged and the gap
/// index (`0..=N`) the cursor currently targets. `Copy` so it rides
/// inside the `Copy` [`ListState`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DragPreview {
    /// Visual index of the row being dragged (dimmed in the view).
    from_visual: usize,
    /// Gap index the drop would insert at (`0` = above row 0, `N` =
    /// below the last row), drawn as the insertion line.
    insert_at: usize,
}

/// The projection `read_state` builds and `view` renders: the current
/// visual order (`order[visual] = item id`) plus an optional in-flight
/// drag preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ListState {
    order: [usize; N],
    preview: Option<DragPreview>,
}

impl Default for ListState {
    fn default() -> Self {
        Self {
            order: IDENTITY,
            preview: None,
        }
    }
}

/// Boot order — items in declaration order.
const IDENTITY: [usize; N] = [0, 1, 2, 3];

/// The composite paint tag for visual row `i` (`"dnd#0"` …). The
/// `InputRouter` splits this at `#`, routes hits to the primary `dnd`
/// external, and reports the full tag back as the [`DropPoint`].
fn row_tag(visual: usize) -> String {
    format!("{TAG}#{visual}")
}

/// Parse the visual index out of a (possibly composite) drop tag
/// (`"dnd#2"` → `Some(2)`). `None` for the bare root tag `"dnd"` or any
/// non-row tag — a drop there resolves to "no target".
fn visual_of(tag: &str) -> Option<usize> {
    tag.split_once('#')?.1.parse::<usize>().ok()
}

/// Classify a [`DropPoint`] into the gap index (`0..=N`) the drop targets.
/// Top half of row `j` inserts *above* `j` (gap `j`); bottom half inserts
/// *below* (gap `j + 1`) — the standard sortable-list rule. `None` when
/// the cursor is over no row (a gap / the background), so the source
/// leaves the order unchanged.
fn drop_gap(over: Option<&DropPoint>) -> Option<usize> {
    let p = over?;
    let j = visual_of(&p.tag)?;
    if j >= N {
        return Some(N);
    }
    Some(if p.y_rel < 0.5 { j } else { j + 1 })
}

/// Move the item at visual index `from` to gap index `insert_at`,
/// accounting for the shift the removal introduces (inserting *after* the
/// original slot lands one index earlier once the source is removed). A
/// no-op move (drop onto the source's own gap) leaves the order
/// unchanged, so a press-release-in-place never reorders.
fn apply_move(order: &mut [usize; N], from: usize, insert_at: usize) {
    if from >= N {
        return;
    }
    let mut v: Vec<usize> = order.to_vec();
    let item = v.remove(from);
    let dest = if insert_at > from { insert_at - 1 } else { insert_at };
    let dest = dest.min(v.len());
    v.insert(dest, item);
    order.copy_from_slice(&v);
}

/// R742 §5.51 — the reorderable list coordinator. A plain `External`
/// (no statechart; the drag session lives in the router) that owns the
/// visual order and the in-flight drag preview. Every row's drag source
/// and drop target funnel back here, so it both produces the payload
/// (`begin_drag`) and resolves the drop (`drag_to` / `drag_release`).
struct ReorderListExternal {
    /// `order[visual] = item id`. Mutated on a committed drop.
    order: RefCell<[usize; N]>,
    /// Visual index of the row whose `PointerDown` last landed — read by
    /// `begin_drag` to arm a session for the pressed row.
    pressed: Cell<Option<usize>>,
    /// Live drag preview (dragged row + target gap), set by `drag_to`,
    /// cleared by `drag_release`. Drives the view's dim + insertion line.
    preview: RefCell<Option<DragPreview>>,
}

impl ReorderListExternal {
    fn new() -> Self {
        Self {
            order: RefCell::new(IDENTITY),
            pressed: Cell::new(None),
            preview: RefCell::new(None),
        }
    }

    /// Labels in current visual order — the AI-readable observable
    /// (`query("labels")`) and the AT name source.
    fn labels(&self) -> Vec<&'static str> {
        self.order
            .borrow()
            .iter()
            .map(|&id| ITEMS[id].0)
            .collect()
    }
}

impl core::fmt::Debug for ReorderListExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ReorderListExternal")
            .field("order", &self.order.borrow())
            .field("pressed", &self.pressed.get())
            .field("preview", &self.preview.borrow())
            .finish()
    }
}

impl External for ReorderListExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
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

    /// Arm a drag from the most-recently-pressed row. The payload carries
    /// the dragged item's stable id (not its visual index) under the
    /// `"dnd-row"` kind, so the in-flight drag is introspectable and a
    /// future cross-widget target could match on it. The visual index the
    /// reorder needs is recovered from `pressed` on commit.
    fn begin_drag(&self) -> Option<DragPayload> {
        let visual = self.pressed.get()?;
        let item = self.order.borrow().get(visual).copied()?;
        Some(DragPayload {
            kind: Cow::Borrowed("dnd-row"),
            value: IntrospectValue::Int(i64::try_from(item).unwrap_or(0)),
        })
    }

    /// Live update: classify the cursor's drop gap and store it as the
    /// preview the view reads (dragged row dims, insertion line at the
    /// gap). `pressed` is the source row; the gap defaults to the source
    /// itself when the cursor is over no row, so the line never vanishes
    /// mid-drag.
    fn drag_to(&mut self, _payload: &DragPayload, over: Option<DropPoint>) {
        let Some(from_visual) = self.pressed.get() else {
            return;
        };
        let insert_at = drop_gap(over.as_ref()).unwrap_or(from_visual);
        *self.preview.borrow_mut() = Some(DragPreview {
            from_visual,
            insert_at,
        });
    }

    /// Commit: move the source row to the final gap, then clear the
    /// transient drag state. A drop over no row (or the source's own gap)
    /// leaves the order unchanged.
    fn drag_release(&mut self, _payload: &DragPayload, over: Option<DropPoint>) {
        if let Some(from_visual) = self.pressed.get() {
            let insert_at = drop_gap(over.as_ref()).unwrap_or(from_visual);
            apply_move(&mut self.order.borrow_mut(), from_visual, insert_at);
        }
        *self.preview.borrow_mut() = None;
        self.pressed.set(None);
    }
}

impl ExternalIntrospect for ReorderListExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("order", "json"),
            ("labels", "json"),
            ("preview", "json"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "order" => {
                let arr: Vec<serde_json::Value> = self
                    .order
                    .borrow()
                    .iter()
                    .map(|&id| serde_json::Value::from(id))
                    .collect();
                Some(IntrospectValue::Json(serde_json::Value::Array(arr)))
            }
            "labels" => {
                let arr: Vec<serde_json::Value> = self
                    .labels()
                    .into_iter()
                    .map(serde_json::Value::from)
                    .collect();
                Some(IntrospectValue::Json(serde_json::Value::Array(arr)))
            }
            "preview" => Some(match *self.preview.borrow() {
                Some(p) => IntrospectValue::Json(serde_json::json!({
                    "from_visual": p.from_visual,
                    "insert_at": p.insert_at,
                })),
                None => IntrospectValue::Null,
            }),
            _ => None,
        }
    }

    fn intervene(&mut self, _path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        // Order mutates only through the drag session (the AI-first path
        // is `scene/drag`); there are no directly-writable slots.
        Err(InterveneError::UnknownPath)
    }

    fn invoke(
        &mut self,
        method: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        if method != "send" {
            return Err(InvokeError::UnknownPath);
        }
        // Composite "{visual}:{Event}" wire form — parsed through the
        // shared `parse_send_payload` SSOT (R51.42 / R734.1), not an
        // inline split. A PointerDown records which row was pressed so
        // `begin_drag` can arm it.
        let IntrospectValue::Text(ref payload) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let (visual, event): (usize, &str) =
            pinion_core::composite_tag::parse_send_payload(payload)
                .ok_or(InvokeError::Rejected)?;
        if event == "PointerDown" && visual < N {
            self.pressed.set(Some(visual));
        }
        Ok(IntrospectValue::Null)
    }
}

/// Build one row: a colour-filled rounded container tagged `dnd#{visual}`
/// holding its centred label. The dragged row renders dimmed (translucent
/// fill over the surface) so the user sees what is being moved.
fn row(visual: usize, item: usize, dim: bool, theme: &pinion_core::theme::Theme) -> Scene {
    let (label, base) = ITEMS[item];
    let fill = if dim {
        // Translucent so the dragged row reads as "lifted"; the witness
        // colour still shows through but muted (non-dragged rows stay
        // opaque, so the live-pixel order check is unaffected).
        Color::rgba(base.r, base.g, base.b, 0x55)
    } else {
        base
    };
    let text = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new()
            .with_size_px(16)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![text])
            .with_tag(row_tag(visual))
            .with_style(BoxStyle::filled(fill).with_corner_radius(ROW_RADIUS))
            .with_layout(
                LayoutStyle::new()
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(ROW_W, ROW_H)),
            ),
    )
}

/// The insertion line drawn at the drop gap during a drag — a thin accent
/// bar full row width.
fn insertion_line(theme: &pinion_core::theme::Theme) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![])
            .with_tag("dnd_insert")
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Accent)).with_corner_radius(2))
            .with_layout(LayoutStyle::new().with_size(Size::px(ROW_W, LINE_H))),
    )
}

/// view-fn (§6.3): pure sync `ListState -> Scene`. Renders rows in visual
/// order; during a drag the dragged row dims and an insertion line sits
/// at the target gap.
fn view(state: ListState) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let mut children = Vec::with_capacity(N + 2);
    for k in 0..=N {
        if state.preview.is_some_and(|p| p.insert_at == k) {
            children.push(insertion_line(&theme));
        }
        if k < N {
            let dim = state.preview.is_some_and(|p| p.from_visual == k);
            children.push(row(k, state.order[k], dim, &theme));
        }
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(TAG)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(GAP),
            ),
    )
}

/// Parse a `query("order")` JSON array back into the fixed `[usize; N]`
/// projection; falls back to identity on any shape mismatch.
fn order_from_json(v: &serde_json::Value) -> [usize; N] {
    let mut out = IDENTITY;
    if let serde_json::Value::Array(a) = v {
        if a.len() == N {
            for (slot, item) in out.iter_mut().zip(a) {
                if let Some(id) = item.as_u64().and_then(|n| usize::try_from(n).ok()) {
                    *slot = id;
                }
            }
        }
    }
    out
}

/// Manual [`WidgetCore`] binding (descriptive-coordinator pattern shared
/// with `hello-progress`): the reorder list has no keyboard-channel event
/// enum — order changes flow through the drag session, observed via
/// introspection.
struct DndView;

impl WidgetCore for DndView {
    type State = ListState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ReorderListExternal::new())
    }

    fn tag() -> &'static str {
        TAG
    }

    fn read_state(scene: &Scene) -> ListState {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                let order = match intro.query("order") {
                    Some(IntrospectValue::Json(v)) => order_from_json(&v),
                    _ => IDENTITY,
                };
                let preview = match intro.query("preview") {
                    Some(IntrospectValue::Json(v)) => {
                        let from_visual = v
                            .get("from_visual")
                            .and_then(serde_json::Value::as_u64)
                            .and_then(|n| usize::try_from(n).ok());
                        let insert_at = v
                            .get("insert_at")
                            .and_then(serde_json::Value::as_u64)
                            .and_then(|n| usize::try_from(n).ok());
                        match (from_visual, insert_at) {
                            (Some(from_visual), Some(insert_at)) => Some(DragPreview {
                                from_visual,
                                insert_at,
                            }),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                return ListState { order, preview };
            }
        }
        ListState::default()
    }

    fn view(state: ListState, _frame: &Frame) -> Scene {
        view(state)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-dnd (R742 §5.51)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// v1 has no keyboard reorder (APG `Ctrl+Arrow` move is a deferred
    /// axis), so no row takes focus — reorder is reached via pointer +
    /// `scene/drag`. The list stays AT-readable through `access_node`.
    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
    }

    fn fmt_state_log(state: &ListState) -> String {
        let order: Vec<String> = state.order.iter().map(usize::to_string).collect();
        format!("[{}]", order.join(","))
    }
}

impl WidgetA11y for DndView {
    /// An [`AriaRole::List`] container with one [`AriaRole::ListItem`] per
    /// row in *visual* order, each named by its label and carrying its
    /// 1-based position — so AT (and `scene/snapshot`) read the live order
    /// without a pixel round-trip.
    fn access_node(state: &ListState, _focused: Option<&str>) -> Vec<AccessNode> {
        let mut nodes = vec![AccessNode::new(TAG, AriaRole::List).with_name("Reorderable list")];
        for (visual, &item) in state.order.iter().enumerate() {
            let pos = u32::try_from(visual + 1).unwrap_or(1);
            nodes.push(
                AccessNode::new(row_tag(visual), AriaRole::ListItem)
                    .with_name(ITEMS[item].0)
                    .with_position_in_set(pos),
            );
        }
        nodes
    }
}

impl WidgetView for DndView {
    type Renderer = HelloDndRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<DndView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    fn fresh() -> ReorderListExternal {
        ReorderListExternal::new()
    }

    fn payload(item: usize) -> DragPayload {
        DragPayload {
            kind: Cow::Borrowed("dnd-row"),
            value: IntrospectValue::Int(i64::try_from(item).unwrap_or(0)),
        }
    }

    fn drop_at(visual: usize, y_rel: f32) -> DropPoint {
        DropPoint {
            tag: row_tag(visual),
            x_rel: 0.5,
            y_rel,
        }
    }

    #[test]
    fn drop_gap_classifies_top_and_bottom_halves() {
        assert_eq!(drop_gap(Some(&drop_at(1, 0.2))), Some(1)); // above row 1
        assert_eq!(drop_gap(Some(&drop_at(1, 0.8))), Some(2)); // below row 1
        assert_eq!(drop_gap(None), None);
        // A drop on the bare root tag (a gap / background) targets no row.
        assert_eq!(
            drop_gap(Some(&DropPoint {
                tag: TAG.to_string(),
                x_rel: 0.5,
                y_rel: 0.5,
            })),
            None
        );
    }

    #[test]
    fn apply_move_relocates_with_removal_shift() {
        // Move row 0 to gap 2: [0,1,2,3] -> remove 0 -> [1,2,3], dest =
        // 2-1 = 1 -> [1,0,2,3].
        let mut order = IDENTITY;
        apply_move(&mut order, 0, 2);
        assert_eq!(order, [1, 0, 2, 3]);
        // Move row 3 to gap 1 (top): [0,1,2,3] -> [0,3,1,2].
        let mut order = IDENTITY;
        apply_move(&mut order, 3, 1);
        assert_eq!(order, [0, 3, 1, 2]);
    }

    #[test]
    fn apply_move_onto_own_gap_is_noop() {
        let mut order = IDENTITY;
        apply_move(&mut order, 2, 2); // gap above self
        assert_eq!(order, IDENTITY);
        let mut order = IDENTITY;
        apply_move(&mut order, 2, 3); // gap below self
        assert_eq!(order, IDENTITY);
    }

    #[test]
    fn begin_drag_arms_only_after_press() {
        let ext = fresh();
        assert!(ext.begin_drag().is_none(), "no press → no drag");
        // Press row 2 via the composite send wire.
        let mut ext = ext;
        ext.invoke("send", IntrospectValue::Text("2:PointerDown".into()))
            .expect("send accepted");
        let p = ext.begin_drag().expect("armed");
        assert_eq!(p.kind, Cow::Borrowed("dnd-row"));
        assert_eq!(p.value.as_usize(), Some(2)); // item id at visual 2
    }

    #[test]
    fn drag_to_then_release_reorders_and_clears_preview() {
        let mut ext = fresh();
        ext.invoke("send", IntrospectValue::Text("0:PointerDown".into()))
            .expect("press");
        let pl = payload(0);
        // Drag row 0 over the bottom half of row 2 → gap 3.
        ext.drag_to(&pl, Some(drop_at(2, 0.8)));
        assert_eq!(
            *ext.preview.borrow(),
            Some(DragPreview {
                from_visual: 0,
                insert_at: 3,
            }),
            "preview tracks the live gap"
        );
        ext.drag_release(&pl, Some(drop_at(2, 0.8)));
        // [0,1,2,3] move 0 -> gap 3 -> [1,2,0,3].
        assert_eq!(*ext.order.borrow(), [1, 2, 0, 3]);
        assert!(ext.preview.borrow().is_none(), "preview cleared on drop");
        assert!(ext.pressed.get().is_none(), "press cleared on drop");
    }

    #[test]
    fn query_order_and_labels_reflect_reorder() {
        let mut ext = fresh();
        ext.invoke("send", IntrospectValue::Text("0:PointerDown".into()))
            .expect("press");
        ext.drag_release(&payload(0), Some(drop_at(2, 0.8)));
        // labels follow the new order [1,2,0,3] = Bravo, Charlie, Alpha,
        // Delta.
        match ext.query("labels") {
            Some(IntrospectValue::Json(serde_json::Value::Array(a))) => {
                let got: Vec<&str> = a.iter().filter_map(serde_json::Value::as_str).collect();
                assert_eq!(got, ["Bravo", "Charlie", "Alpha", "Delta"]);
            }
            other => panic!("expected labels array, got {other:?}"),
        }
    }

    #[test]
    fn read_state_round_trips_through_introspection() {
        let mut ext = fresh();
        ext.invoke("send", IntrospectValue::Text("0:PointerDown".into()))
            .expect("press");
        ext.drag_release(&payload(0), Some(drop_at(2, 0.8)));
        let scene = Scene::External(ExternalNode::new(Box::new(ext)).with_tag(TAG));
        let state = DndView::read_state(&scene);
        assert_eq!(state.order, [1, 2, 0, 3]);
        assert!(state.preview.is_none());
    }
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    #[test]
    fn emits_list_with_one_item_per_row() {
        let nodes = DndView::access_node(&ListState::default(), None);
        assert_eq!(nodes.len(), N + 1);
        assert_eq!(nodes[0].role, AriaRole::List);
        for (i, node) in nodes[1..].iter().enumerate() {
            assert_eq!(node.role, AriaRole::ListItem);
            assert_eq!(node.name.as_deref(), Some(ITEMS[i].0));
        }
    }

    #[test]
    fn list_item_names_follow_visual_order() {
        let state = ListState {
            order: [2, 0, 3, 1],
            preview: None,
        };
        let nodes = DndView::access_node(&state, None);
        let names: Vec<&str> = nodes[1..].iter().filter_map(|n| n.name.as_deref()).collect();
        assert_eq!(names, ["Charlie", "Alpha", "Delta", "Bravo"]);
    }

    #[test]
    fn r55_g20_view_contains_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<DndView>(
            ListState::default(),
            &Frame::new(),
        );
    }
}
