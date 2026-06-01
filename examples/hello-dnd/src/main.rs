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
    AlignItems, Border, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size,
    TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_a11y::{AccessAction, AccessFocus, AccessNode, AccessState, AriaRole, WidgetA11y};
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
    /// Keyboard cursor / AT active descendant (visual index). `Arrow`
    /// keys move it. Set via `intervene("focused_index")`, read via
    /// `query("focused_index")`.
    focused: Cell<Option<usize>>,
    /// APG "keyboard drag" grab state. `Some(order)` while the focused
    /// row is *picked up* — `Arrow` then moves the row (cursor following)
    /// and the snapshot is the pre-grab order an `Escape` reverts to.
    /// `None` when not grabbing. Grab is toggled by `Space` / `Enter`.
    grab_snapshot: Cell<Option<[usize; N]>>,
}

impl ReorderListExternal {
    fn new() -> Self {
        Self {
            order: RefCell::new(IDENTITY),
            pressed: Cell::new(None),
            preview: RefCell::new(None),
            focused: Cell::new(None),
            grab_snapshot: Cell::new(None),
        }
    }

    /// Whether the focused row is currently picked up (APG keyboard drag).
    fn grabbed(&self) -> bool {
        self.grab_snapshot.get().is_some()
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

    /// Keyboard reorder: move the focused row to `target` (clamped to a
    /// valid index) and keep the cursor on the moved row. No-op when no
    /// row is focused or the position is unchanged. Returns the new
    /// focused index, if any — the value `invoke("move")` reports back.
    fn move_focused_to(&self, target: usize) -> Option<usize> {
        let from = self.focused.get()?;
        let target = target.min(N - 1);
        if target != from {
            apply_move(&mut self.order.borrow_mut(), from, gap_for_target(from, target));
        }
        self.focused.set(Some(target));
        Some(target)
    }
}

/// Map a keyboard move (`from` row → `target` row) to the gap index
/// [`apply_move`] expects. Moving *down* (`target > from`) inserts after
/// `target` (gap `target+1`); moving *up* inserts before it (gap
/// `target`). This makes a one-step `Ctrl+Arrow` land exactly one slot
/// over, matching the displayed motion.
fn gap_for_target(from: usize, target: usize) -> usize {
    if target > from {
        target + 1
    } else {
        target
    }
}

impl core::fmt::Debug for ReorderListExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ReorderListExternal")
            .field("order", &self.order.borrow())
            .field("pressed", &self.pressed.get())
            .field("preview", &self.preview.borrow())
            .field("focused", &self.focused.get())
            .field("grab_snapshot", &self.grab_snapshot.get())
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
        // `drop_gap` is `None` when the cursor sits between rows (over the
        // list container, not a row). **Hold** the last resolved gap there
        // rather than snapping the indicator back to the dragged row —
        // otherwise the line jumps up to the dragged row's own gap every
        // time the cursor crosses a 6 px inter-row gap, then drops back on
        // the next row ("up then down" flicker). Falls back to the dragged
        // row only on the first frame, before any gap has resolved.
        let last = (*self.preview.borrow()).map(|p| p.insert_at);
        let insert_at = drop_gap(over.as_ref()).or(last).unwrap_or(from_visual);
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
            // Release in a gap honours the gap the indicator was showing
            // (the held preview), not a snap-back to the dragged row.
            let last = (*self.preview.borrow()).map(|p| p.insert_at);
            let insert_at = drop_gap(over.as_ref()).or(last).unwrap_or(from_visual);
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
            ("focused_index", "int"),
            ("grabbed", "bool"),
            ("move", "int"),
            ("grab", "bool"),
            ("grab_cancel", "null"),
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
            "focused_index" => Some(match self.focused.get() {
                Some(i) => IntrospectValue::Int(i64::try_from(i).unwrap_or(0)),
                None => IntrospectValue::Null,
            }),
            "grabbed" => Some(IntrospectValue::Bool(self.grabbed())),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // The keyboard cursor / AT active descendant. Out-of-range
            // indices are rejected (composite-index convention); the
            // shell's apply_key always writes a clamped value.
            "focused_index" => {
                let i = value.as_usize().ok_or(InterveneError::TypeMismatch)?;
                if i >= N {
                    return Err(InterveneError::OutOfRange);
                }
                self.focused.set(Some(i));
                Ok(())
            }
            // The order mutates through the drag session or the `move`
            // action, never a direct slot write.
            "order" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        method: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match method {
            // Composite "{visual}:{Event}" wire form — parsed through the
            // shared `parse_send_payload` SSOT (R51.42 / R734.1), not an
            // inline split. A PointerDown records which row was pressed so
            // `begin_drag` can arm it.
            "send" => {
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
            // Keyboard reorder: move the focused row by `delta` slots
            // (`+1` down / `-1` up), clamped, cursor following. Returns
            // the new focused index, or `Null` when no row is focused.
            // This is the `Ctrl+Arrow` path and the AT increment/decrement
            // funnel — one channel for shell, RPC, and assistive tech.
            "move" => {
                let delta = args.as_i64().ok_or(InvokeError::TypeMismatch)?;
                let Some(from) = self.focused.get() else {
                    return Ok(IntrospectValue::Null);
                };
                let target = (i64::try_from(from).unwrap_or(0) + delta)
                    .clamp(0, i64::try_from(N - 1).unwrap_or(0));
                let target = usize::try_from(target).unwrap_or(0);
                match self.move_focused_to(target) {
                    Some(i) => Ok(IntrospectValue::Int(i64::try_from(i).unwrap_or(0))),
                    None => Ok(IntrospectValue::Null),
                }
            }
            // APG keyboard drag — toggle pick-up of the focused row.
            // Entering grab snapshots the order (for `grab_cancel` to
            // revert); dropping keeps the live order. Returns the new
            // grabbed state. No-op without a focused row.
            "grab" => {
                if self.focused.get().is_none() {
                    return Ok(IntrospectValue::Bool(false));
                }
                // `Bool(b)` sets the grab explicitly (deterministic RPC /
                // AT); `Null` or anything else toggles (the keyboard
                // `Space` path).
                let want = match args {
                    IntrospectValue::Bool(b) => b,
                    _ => !self.grabbed(),
                };
                if want && !self.grabbed() {
                    self.grab_snapshot.set(Some(*self.order.borrow()));
                } else if !want && self.grabbed() {
                    self.grab_snapshot.set(None);
                }
                Ok(IntrospectValue::Bool(self.grabbed()))
            }
            // Cancel a grab: revert to the snapshotted order and drop.
            "grab_cancel" => {
                if let Some(snap) = self.grab_snapshot.get() {
                    *self.order.borrow_mut() = snap;
                    self.grab_snapshot.set(None);
                }
                Ok(IntrospectValue::Null)
            }
            _ => Err(InvokeError::UnknownPath),
        }
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
        kids.push(row(k, state.order[k], dim, focused, focused && state.grabbed, &theme));
    }
    if let Some(p) = state.preview {
        kids.push(insertion_line(p.insert_at, &theme));
    }
    let list = Scene::Container(
        ContainerNode::new(kids)
            .with_tag(TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    .with_gap(GAP)
                    .with_size(Size::px(ROW_W, LIST_H)),
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
        let _ = intro.intervene("focused_index", IntrospectValue::Int(i64::try_from(idx).unwrap_or(0)));
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
                let focused = match intro.query("focused_index") {
                    Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
                    _ => None,
                };
                let grabbed = matches!(intro.query("grabbed"), Some(IntrospectValue::Bool(true)));
                return ListState {
                    order,
                    preview,
                    focused,
                    grabbed,
                };
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

    /// The list is a single Tab stop with roving focus (the WAI-ARIA
    /// active-descendant model): `Arrow` keys move the cursor among rows,
    /// `Ctrl+Arrow` reorders the row under it. Keyboard, pointer
    /// (`scene/drag`), and AT all reach reorder.
    fn focusable_tags() -> Vec<&'static str> {
        vec![TAG]
    }

    /// Keyboard reorder — the APG "keyboard drag" model (one tab stop
    /// with a roving cursor and pick-up). Modifier-free, so it drives
    /// through plain `scene/key` (the RPC key channel carries no
    /// modifiers) and reads naturally to a screen reader:
    ///
    /// * `Arrow`(Down/Right / Up/Left) — when *not* grabbing, move the
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
                hovered: false,
                pressed: false,
                checked: None,
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
            Some(AccessFocus::composite(TAG, row_tag(state.focused.unwrap_or(0))))
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
            AccessAction::Focus | AccessAction::Click | AccessAction::Default => set_cursor(node, idx),
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
    use pinion_core::scene::ExternalNode;
    use pinion_core::Modifiers;

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
    fn drag_to_over_gap_holds_last_gap_no_snap_back() {
        let mut ext = fresh();
        ext.invoke("send", IntrospectValue::Text("1:PointerDown".into())).expect("grab Bravo");
        let pl = payload(1);
        // Over row 2's lower half → gap 3.
        ext.drag_to(&pl, Some(drop_at(2, 0.8)));
        assert_eq!(ext.preview.borrow().unwrap().insert_at, 3);
        // Cursor crosses an inter-row gap (over the list container `TAG`,
        // not a row): `drop_gap` is None, but the indicator HOLDS at gap 3
        // instead of snapping back to the dragged row's gap (1) — the
        // "line jumps up then down" fix.
        let gap_pt = DropPoint {
            tag: TAG.to_string(),
            x_rel: 0.5,
            y_rel: 0.5,
        };
        ext.drag_to(&pl, Some(gap_pt));
        assert_eq!(ext.preview.borrow().unwrap().insert_at, 3, "held last gap, no snap to 1");
        // Fully off the list (None) also holds, not snaps.
        ext.drag_to(&pl, None);
        assert_eq!(ext.preview.borrow().unwrap().insert_at, 3);
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

    // ----- R742.2 keyboard reorder + overlay -----

    #[test]
    fn move_action_reorders_clamps_and_follows_cursor() {
        let mut ext = fresh();
        ext.intervene("focused_index", IntrospectValue::Int(0)).expect("focus 0");
        // Ctrl+Down: Alpha (vis 0) moves below Bravo; cursor follows to 1.
        let r = ext.invoke("move", IntrospectValue::Int(1)).expect("move");
        assert_eq!(r, IntrospectValue::Int(1));
        assert_eq!(*ext.order.borrow(), [1, 0, 2, 3]);
        // Over-range delta clamps to the last / first slot.
        ext.intervene("focused_index", IntrospectValue::Int(1)).expect("focus 1");
        ext.invoke("move", IntrospectValue::Int(10)).expect("move");
        assert_eq!(ext.focused.get(), Some(3));
        ext.invoke("move", IntrospectValue::Int(-10)).expect("move");
        assert_eq!(ext.focused.get(), Some(0));
    }

    #[test]
    fn move_without_focus_is_noop() {
        let mut ext = fresh();
        assert_eq!(ext.invoke("move", IntrospectValue::Int(1)).expect("move"), IntrospectValue::Null);
        assert_eq!(*ext.order.borrow(), IDENTITY);
    }

    #[test]
    fn intervene_focused_index_clamps_and_rejects() {
        let mut ext = fresh();
        ext.intervene("focused_index", IntrospectValue::Int(2)).expect("in range");
        assert_eq!(ext.focused.get(), Some(2));
        assert!(matches!(
            ext.intervene("focused_index", IntrospectValue::Int(99)),
            Err(InterveneError::OutOfRange)
        ));
        assert!(matches!(
            ext.intervene("order", IntrospectValue::Int(0)),
            Err(InterveneError::ReadOnly)
        ));
    }

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
        assert_eq!(DndView::read_state(&scene).order, IDENTITY, "navigation does not reorder");
        // Space picks up the focused (last) row.
        assert!(DndView::apply_key(&mut scene, Some(TAG), " ", NONE));
        assert!(DndView::read_state(&scene).grabbed, "Space grabs the focused row");
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
    fn grab_action_snapshots_and_cancel_reverts() {
        let mut ext = fresh();
        ext.intervene("focused_index", IntrospectValue::Int(0)).expect("focus");
        // Explicit grab via Bool(true) (the deterministic RPC / AT path).
        assert_eq!(
            ext.invoke("grab", IntrospectValue::Bool(true)).expect("grab"),
            IntrospectValue::Bool(true)
        );
        assert!(ext.grabbed());
        ext.invoke("move", IntrospectValue::Int(1)).expect("move");
        assert_ne!(*ext.order.borrow(), IDENTITY);
        ext.invoke("grab_cancel", IntrospectValue::Null).expect("cancel");
        assert!(!ext.grabbed());
        assert_eq!(*ext.order.borrow(), IDENTITY, "cancel reverts to the snapshot");
        // Grab without a focused row is a no-op (false).
        let mut ext2 = fresh();
        assert_eq!(
            ext2.invoke("grab", IntrospectValue::Null).expect("grab"),
            IntrospectValue::Bool(false)
        );
    }

    #[test]
    fn gap_line_top_is_monotonic_and_clamped() {
        let tops: Vec<u32> = (0..=N).map(gap_line_top).collect();
        assert_eq!(tops[0], 0, "gap 0 sits at the top edge");
        for w in tops.windows(2) {
            assert!(w[1] >= w[0], "insertion line moves down as the gap index grows");
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
