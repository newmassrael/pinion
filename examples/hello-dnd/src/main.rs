//! `hello-dnd` — R742 §5.51 — first consumer of the generic
//! **drag-and-drop substrate**: the three additive
//! [`External`] hooks `begin_drag` /
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

use pinion_a11y::{AccessAction, AccessFocus, AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, DragPayload, DropPoint, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField,
    ThreadOwnership,
};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size,
    TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::reorder::{DragPreview, ReorderAxis, ReorderModel, read_reorder};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};

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
/// Inner list container height = N rows + (N-1) gaps. Fixed so the
/// insertion line is an **absolute overlay** (the rows never reflow when
/// it appears). Locked to `N` by the const-assert below.
const LIST_H: u32 = 4 * ROW_H + 3 * GAP;
const _: () = assert!(N == 4, "LIST_H hard-codes N rows + (N-1) gaps");

/// Top edge (in the list container) of the insertion line for drop gap
/// `k` (`0..=N`): the centre of the gap before row `k`, clamped inside
/// the container. The gap before row `k` sits at `k*(ROW_H+GAP)`.
fn gap_line_top(k: usize) -> u32 {
    let k = u32::try_from(k).unwrap_or(0);
    let centre = k * (ROW_H + GAP);
    // centre is the boundary between row k-1 and row k; back off half a
    // gap + half the line so the bar is centred in the gap.
    let top = centre.saturating_sub(GAP / 2 + LINE_H / 2);
    top.min(LIST_H.saturating_sub(LINE_H))
}

/// The projection `read_state` builds and `view` renders: the current
/// visual order (`order[visual] = item id`) plus an optional in-flight
/// drag preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ListState {
    order: [usize; N],
    preview: Option<DragPreview>,
    /// Keyboard cursor / WAI-ARIA active descendant — the visual row the
    /// arrow keys move. `None` until the list is focused and a navigation
    /// key lands.
    focused: Option<usize>,
    /// Whether the focused row is *picked up* (APG keyboard drag) — drawn
    /// with a "lifted" ring so the keyboard reorder reads clearly.
    grabbed: bool,
}

impl Default for ListState {
    fn default() -> Self {
        Self {
            order: IDENTITY,
            preview: None,
            focused: None,
            grabbed: false,
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

/// R742 §5.51 — the reorderable list coordinator. A plain `External`
/// (no statechart; the drag session lives in the router) that owns the
/// reorder mechanics. Since R743 the reorder state + drag/keyboard logic
/// lives in the shared [`ReorderModel`] (lifted when `hello-tab-reorder`
/// became the substrate's second consumer); this binding embeds one and
/// layers on only the list-specific `labels` observable. Every row's drag
/// source and drop target funnel back here, so the model both produces
/// the payload (`begin_drag`) and resolves the drop (`drag_to` /
/// `drag_release`).
#[derive(Debug)]
struct ReorderListExternal {
    /// The shared reorder mechanics — vertical axis (the drop
    /// classification reads `y_rel`).
    model: ReorderModel,
}

impl ReorderListExternal {
    fn new() -> Self {
        Self {
            model: ReorderModel::new(N, ReorderAxis::Vertical),
        }
    }

    /// Labels in current visual order — the list-specific AI-readable
    /// observable (`query("labels")`) and the AT name source. Maps the
    /// model's `order` through the binding-owned `ITEMS` table.
    fn labels(&self) -> Vec<&'static str> {
        self.model.order().iter().map(|&id| ITEMS[id].0).collect()
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

    /// Arm a drag from the most-recently-pressed row, under the
    /// `"dnd-row"` kind. Delegates to the model, which carries the dragged
    /// item's stable id in the payload.
    fn begin_drag(&self) -> Option<DragPayload> {
        self.model.begin_drag_payload(Cow::Borrowed("dnd-row"))
    }

    fn drag_to(&mut self, payload: &DragPayload, over: Option<DropPoint>) {
        self.model.drag_to(payload, over.as_ref());
    }

    fn drag_release(&mut self, payload: &DragPayload, over: Option<DropPoint>) {
        self.model.drag_release(payload, over.as_ref());
    }
}

impl ExternalIntrospect for ReorderListExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("order", "json"),
                    SchemaField::new("labels", "json"),
                    SchemaField::new("preview", "json"),
                    SchemaField::new("focused_index", "int"),
                    SchemaField::new("grabbed", "bool"),
                    SchemaField::new("move", "int"),
                    SchemaField::new("grab", "bool"),
                    SchemaField::new("grab_cancel", "null"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            // List-specific observable; everything else is a reorder slot.
            "labels" => {
                let arr: Vec<serde_json::Value> = self
                    .labels()
                    .into_iter()
                    .map(serde_json::Value::from)
                    .collect();
                Some(IntrospectValue::Json(serde_json::Value::Array(arr)))
            }
            other => self.model.query(other),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        self.model.intervene(path, &value)
    }

    fn invoke(
        &mut self,
        method: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        self.model.invoke(method, &args)
    }
}

/// Build one row: a colour-filled rounded container tagged `dnd#{visual}`
/// holding its centred label. The dragged row renders dimmed (translucent
/// fill over the surface) so the user sees what is being moved.
fn row(
    visual: usize,
    item: usize,
    dim: bool,
    focused: bool,
    grabbed: bool,
    theme: &pinion_core::theme::Theme,
) -> Scene {
    let (label, base) = ITEMS[item];
    let fill = if dim {
        // Translucent so the dragged row reads as "lifted"; the witness
        // colour still shows through but muted (non-dragged rows stay
        // opaque, so the live-pixel order check is unaffected).
        base.with_alpha(0x55)
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
    let mut style = BoxStyle::filled(fill).with_corner_radius(ROW_RADIUS);
    if grabbed {
        // Picked-up (keyboard-dragged) row: a thicker Accent ring reads as
        // "lifted", distinct from the plain focus cue.
        style = style.with_border(Border::new(theme.resolve(ColorRole::Accent), 3));
    } else if focused {
        // Keyboard focus ring / AT active-descendant cue — a 2-px
        // high-contrast outline (OnSurface reads against every witness
        // hue). Drawn inside the row rect so it never reflows siblings.
        style = style.with_border(Border::new(theme.resolve(ColorRole::OnSurface), 2));
    }
    Scene::Container(
        ContainerNode::new(vec![text])
            .with_tag(row_tag(visual))
            .with_style(style)
            .with_layout(
                LayoutStyle::new()
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(ROW_W, ROW_H)),
            ),
    )
}

/// The insertion line for drop gap `k` — a thin accent bar, **absolutely
/// positioned** inside the list container so showing it never reflows the
/// rows (the R742.1-review polish fix: previously a flex child that
/// shifted every row by its height).
fn insertion_line(k: usize, theme: &pinion_core::theme::Theme) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![])
            .with_tag("dnd_insert")
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Accent)).with_corner_radius(2))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(0, gap_line_top(k))
                    .with_size(Size::px(ROW_W, LINE_H)),
            ),
    )
}

/// view-fn (§6.3): pure sync `ListState -> Scene`. Rows flow in a
/// fixed-size list container (visual order); the dragged row dims, the
/// focused row gets a ring, and — while dragging — an absolutely-
/// positioned insertion line overlays the target gap without shifting
/// any row.
fn view(state: ListState) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let mut kids = Vec::with_capacity(N + 1);
    for k in 0..N {
        let dim = state.preview.is_some_and(|p| p.from_visual == k);
        let focused = state.focused == Some(k);
        kids.push(row(
            k,
            state.order[k],
            dim,
            focused,
            focused && state.grabbed,
            &theme,
        ));
    }
    if let Some(p) = state.preview {
        kids.push(insertion_line(p.insert_at, &theme));
    }
    let list = Scene::Container(
        ContainerNode::new(kids).with_tag(TAG).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Center)
                .with_gap(GAP)
                .with_size(Size::px(ROW_W, LIST_H))
                .with_focusable(true),
        ),
    );
    Scene::Container(
        ContainerNode::new(vec![list])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// Read the list's current keyboard cursor (`focused_index`), if any.
fn cursor(node: &pinion_core::scene::ExternalNode) -> Option<usize> {
    node.handle
        .introspect()
        .and_then(|i| i.query("focused_index"))
        .and_then(|v| v.as_usize())
}

/// Set the keyboard cursor to `idx` via the `focused_index` slot.
fn set_cursor(node: &mut pinion_core::scene::ExternalNode, idx: usize) -> bool {
    if let Some(intro) = node.handle.introspect_mut() {
        let _ = intro.intervene(
            "focused_index",
            IntrospectValue::Int(i64::try_from(idx).unwrap_or(0)),
        );
    }
    true
}

/// Move the cursor one row in `dir` (`+1` down, `-1` up), clamped at the
/// ends. A first Arrow with no cursor lands on `0` (down) or `N-1` (up).
fn move_cursor(node: &mut pinion_core::scene::ExternalNode, dir: i32) -> bool {
    let next = match (cursor(node), dir) {
        (Some(c), 1) => (c + 1).min(N - 1),
        (Some(c), -1) => c.saturating_sub(1),
        (None, 1) => 0,
        (None, -1) => N - 1,
        _ => 0,
    };
    set_cursor(node, next)
}

/// Move the focused row by `delta` slots (cursor following) through the
/// `move` action — the shared funnel the AT increment/decrement path also
/// uses. No-op (returns `false`) when no row is focused.
fn move_item(node: &mut pinion_core::scene::ExternalNode, delta: i64) -> bool {
    if cursor(node).is_none() {
        return false;
    }
    if let Some(intro) = node.handle.introspect_mut() {
        let _ = intro.invoke("move", IntrospectValue::Int(delta));
    }
    true
}

/// Whether the focused row is currently picked up (APG keyboard drag).
fn is_grabbed(node: &pinion_core::scene::ExternalNode) -> bool {
    node.handle
        .introspect()
        .and_then(|i| i.query("grabbed"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Toggle the grab on the focused row (`Space` / `Enter`).
fn toggle_grab(node: &mut pinion_core::scene::ExternalNode) -> bool {
    if let Some(intro) = node.handle.introspect_mut() {
        let _ = intro.invoke("grab", IntrospectValue::Null);
    }
    true
}

/// Cancel a grab, reverting to the pre-grab order (`Escape`).
fn cancel_grab(node: &mut pinion_core::scene::ExternalNode) -> bool {
    if let Some(intro) = node.handle.introspect_mut() {
        let _ = intro.invoke("grab_cancel", IntrospectValue::Null);
    }
    true
}

/// Manual [`WidgetCore`] binding (descriptive-coordinator pattern shared
/// with `hello-progress`): the reorder list has no keyboard-channel event
/// enum — order changes flow through the drag session or the keyboard
/// `move` action, observed via introspection.
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
        let Scene::External(node) = scene else {
            return ListState::default();
        };
        let Some(intro) = node.handle.introspect() else {
            return ListState::default();
        };
        // Decode the reorder slots through the shared `read_reorder`
        // (the deserialize peer of `ReorderModel::query`), then project
        // the count-agnostic `Vec` onto this binding's fixed `[usize; N]`
        // `Copy` state.
        let v = read_reorder(intro);
        ListState {
            order: v.order.try_into().unwrap_or(IDENTITY),
            preview: v.preview,
            focused: v.focused,
            grabbed: v.grabbed,
        }
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

    /// Keyboard reorder — the APG "keyboard drag" model (one tab stop
    /// with a roving cursor and pick-up). Modifier-free, so it drives
    /// through plain `scene/key` (the RPC key channel carries no
    /// modifiers) and reads naturally to a screen reader:
    ///
    /// * `Arrow` (Down/Right / Up/Left) — when *not* grabbing, move the
    ///   cursor one row (clamped at the ends). When grabbing, move the
    ///   picked-up row one slot, the cursor following.
    /// * `Home` / `End` — cursor to first / last (or, while grabbing,
    ///   move the row there — the `move` action clamps an over-range
    ///   delta).
    /// * `Space` / `Enter` — pick up the focused row, or drop it.
    /// * `Escape` — cancel a grab, reverting to the pre-grab order.
    ///
    /// Keys route only when the list itself holds focus (no sibling
    /// aliasing). Unrecognised keys return `false` for the shell's
    /// swallow contract. `modifiers` is unused — the model is
    /// deliberately modifier-free.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(TAG) {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let big = i64::try_from(N).unwrap_or(i64::MAX);
        let grabbed = is_grabbed(node);
        match key {
            " " | "Spacebar" | "Enter" => toggle_grab(node),
            "Escape" => grabbed && cancel_grab(node),
            "ArrowDown" | "ArrowRight" => {
                if grabbed {
                    move_item(node, 1)
                } else {
                    move_cursor(node, 1)
                }
            }
            "ArrowUp" | "ArrowLeft" => {
                if grabbed {
                    move_item(node, -1)
                } else {
                    move_cursor(node, -1)
                }
            }
            "Home" => {
                if grabbed {
                    move_item(node, -big)
                } else {
                    set_cursor(node, 0)
                }
            }
            "End" => {
                if grabbed {
                    move_item(node, big)
                } else {
                    set_cursor(node, N - 1)
                }
            }
            _ => false,
        }
    }

    fn fmt_state_log(state: &ListState) -> String {
        let order: Vec<String> = state.order.iter().map(usize::to_string).collect();
        format!("[{}]", order.join(","))
    }
}

impl WidgetA11y for DndView {
    /// An [`AriaRole::List`] container claiming one [`AriaRole::ListItem`]
    /// per row in *visual* order, each named by its label and carrying its
    /// 1-based position — so AT (and `scene/snapshot`) read the live order
    /// without a pixel round-trip. When the list holds focus, the active
    /// descendant (the keyboard cursor row) is marked `focused` so AT
    /// announces the move target.
    fn access_node(state: &ListState, focused: Option<&str>) -> Vec<AccessNode> {
        let list_focused = focused == Some(TAG);
        let active = state.focused.unwrap_or(0);
        let mut list = AccessNode::new(TAG, AriaRole::List).with_name("Reorderable list");
        for visual in 0..N {
            list = list.with_child(row_tag(visual));
        }
        let mut nodes = vec![list];
        for (visual, &item) in state.order.iter().enumerate() {
            let pos = u32::try_from(visual + 1).unwrap_or(1);
            let access_state = AccessState {
                focused: list_focused && visual == active,
                disabled: false,
                ..AccessState::default()
            };
            nodes.push(
                AccessNode::new(row_tag(visual), AriaRole::ListItem)
                    .with_name(ITEMS[item].0)
                    .with_position_in_set(pos)
                    .with_state(access_state),
            );
        }
        nodes
    }

    /// Composite focus model: when the list is focused, report the parent
    /// tag as the focus target and the cursor row as the
    /// `aria-activedescendant`.
    fn access_focus_target(state: &ListState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(TAG) {
            Some(AccessFocus::composite(
                TAG,
                row_tag(state.focused.unwrap_or(0)),
            ))
        } else {
            focused.map(AccessFocus::atomic)
        }
    }

    /// AT actions on a row sub-tag: `Focus` parks the cursor on it;
    /// `Increment` / `Decrement` move that row down / up (the same `move`
    /// funnel `Ctrl+Arrow` uses), giving assistive tech a reorder path.
    fn access_child_invoke(
        scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        let Ok(idx) = sub_tag.parse::<usize>() else {
            return false;
        };
        if idx >= N {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        match action {
            AccessAction::Focus | AccessAction::Click | AccessAction::Default => {
                set_cursor(node, idx)
            }
            AccessAction::Increment => {
                set_cursor(node, idx);
                move_item(node, 1)
            }
            AccessAction::Decrement => {
                set_cursor(node, idx);
                move_item(node, -1)
            }
            AccessAction::Other => false,
        }
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
    use pinion_core::Modifiers;
    use pinion_core::scene::ExternalNode;

    /// `apply_key` is deliberately modifier-free; `NONE` stands in for the
    /// (ignored) modifier argument.
    const NONE: Modifiers = Modifiers::empty();

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
    fn begin_drag_delegates_and_arms_only_after_press() {
        // The reorder *mechanics* (drop classification, apply_move, grab,
        // move, hold-last-gap) are unit-tested once in the shared model
        // (`pinion_core::widgets::reorder`); these binding tests cover the
        // delegation + the list-specific surface (labels / read_state /
        // keyboard policy / paint).
        let mut ext = fresh();
        assert!(ext.begin_drag().is_none(), "no press → no drag");
        ext.invoke("send", IntrospectValue::Text("2:PointerDown".into()))
            .expect("send accepted");
        let p = ext.begin_drag().expect("armed");
        assert_eq!(p.kind, Cow::Borrowed("dnd-row"), "list kind");
        assert_eq!(
            p.value.as_usize(),
            Some(2),
            "payload carries item id at visual 2"
        );
    }

    #[test]
    fn drag_to_then_release_reorders_through_model() {
        let mut ext = fresh();
        ext.invoke("send", IntrospectValue::Text("0:PointerDown".into()))
            .expect("press");
        let pl = payload(0);
        ext.drag_to(&pl, Some(drop_at(2, 0.8))); // → gap 3
        assert!(
            matches!(ext.query("preview"), Some(IntrospectValue::Json(_))),
            "preview is in flight (queryable as scene-as-data)"
        );
        ext.drag_release(&pl, Some(drop_at(2, 0.8)));
        assert_eq!(ext.model.order(), [1, 2, 0, 3], "row 0 dropped below row 2");
        assert!(
            matches!(ext.query("preview"), Some(IntrospectValue::Null)),
            "preview cleared"
        );
        assert!(ext.begin_drag().is_none(), "press cleared on drop");
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

    // ----- R742.2 keyboard reorder (binding policy over the model) -----

    #[test]
    fn apply_key_arrow_navigates_then_grab_reorders() {
        let mut scene = Scene::External(ExternalNode::new(Box::new(fresh())).with_tag(TAG));
        // Keys ignored unless the list holds focus.
        assert!(!DndView::apply_key(&mut scene, None, "ArrowDown", NONE));
        // Arrow navigates the cursor; clamps at the ends; no reorder.
        assert!(DndView::apply_key(&mut scene, Some(TAG), "ArrowDown", NONE));
        assert_eq!(DndView::read_state(&scene).focused, Some(0));
        assert!(DndView::apply_key(&mut scene, Some(TAG), "End", NONE));
        assert_eq!(DndView::read_state(&scene).focused, Some(N - 1));
        assert_eq!(
            DndView::read_state(&scene).order,
            IDENTITY,
            "navigation does not reorder"
        );
        // Space picks up the focused (last) row.
        assert!(DndView::apply_key(&mut scene, Some(TAG), " ", NONE));
        assert!(
            DndView::read_state(&scene).grabbed,
            "Space grabs the focused row"
        );
        // While grabbed, ArrowUp MOVES the row; the cursor follows.
        assert!(DndView::apply_key(&mut scene, Some(TAG), "ArrowUp", NONE));
        let st = DndView::read_state(&scene);
        assert_eq!(st.order, [0, 1, 3, 2], "Delta moved above Charlie");
        assert_eq!(st.focused, Some(2), "cursor followed the grabbed row");
        assert!(st.grabbed, "still grabbed mid-reorder");
        // Space drops; the new order is kept.
        assert!(DndView::apply_key(&mut scene, Some(TAG), " ", NONE));
        assert!(!DndView::read_state(&scene).grabbed);
        assert_eq!(DndView::read_state(&scene).order, [0, 1, 3, 2]);
    }

    #[test]
    fn escape_cancels_grab_and_reverts_order() {
        let mut scene = Scene::External(ExternalNode::new(Box::new(fresh())).with_tag(TAG));
        assert!(DndView::apply_key(&mut scene, Some(TAG), "Home", NONE)); // cursor -> 0
        assert!(DndView::apply_key(&mut scene, Some(TAG), " ", NONE)); // grab row 0
        assert!(DndView::apply_key(&mut scene, Some(TAG), "End", NONE)); // move it to the bottom
        assert_eq!(DndView::read_state(&scene).order, [1, 2, 3, 0]);
        // Escape reverts to the pre-grab order and drops.
        assert!(DndView::apply_key(&mut scene, Some(TAG), "Escape", NONE));
        let st = DndView::read_state(&scene);
        assert!(!st.grabbed);
        assert_eq!(st.order, IDENTITY, "Escape reverts to the pre-grab order");
    }

    #[test]
    fn gap_line_top_is_monotonic_and_clamped() {
        let tops: Vec<u32> = (0..=N).map(gap_line_top).collect();
        assert_eq!(tops[0], 0, "gap 0 sits at the top edge");
        for w in tops.windows(2) {
            assert!(
                w[1] >= w[0],
                "insertion line moves down as the gap index grows"
            );
        }
        for &t in &tops {
            assert!(t <= LIST_H - LINE_H, "line stays inside the list container");
        }
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
            focused: None,
            grabbed: false,
        };
        let nodes = DndView::access_node(&state, None);
        let names: Vec<&str> = nodes[1..]
            .iter()
            .filter_map(|n| n.name.as_deref())
            .collect();
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
