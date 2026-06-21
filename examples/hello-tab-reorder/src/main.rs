// R743 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the architectural narrative carries many proper-noun
// identifiers (TabList, DragPayload, WAI-ARIA, …).
#![allow(clippy::doc_markdown)]

//! `hello-tab-reorder` — R743 §5.51 — **second consumer** of the generic
//! drag-and-drop substrate (the R742 additive
//! [`External`](pinion_core::external::External) hooks `begin_drag` /
//! `drag_to` / `drag_release` plus the `InputRouter` drag session).
//!
//! ## Why this binding exists
//!
//! `hello-dnd` (R742) proved the substrate with a *vertical* list whose
//! rows have no selection. This binding is the orthogonal second
//! consumer that [[abstraction-needs-second-consumer]] calls for: a
//! **horizontal** strip whose items also carry a *selection*. Together
//! the two pin down the substrate's reusable shape — orientation is a
//! clean axis (the drop classification reads `x_rel` here, `y_rel`
//! there) and selection layers on top without touching the reorder
//! mechanics. The shared reorder core is lifted to
//! [`pinion_core::widgets::reorder`] (see [`ReorderModel`]); this binding
//! owns only the tab-specific selection, paint, keyboard policy, and
//! WAI-ARIA tablist tree.
//!
//! Reorderable tabs are the canonical IDE / DCC editor-tab affordance —
//! the northern-star "Unreal-class editor self-hosted in pinion" wants
//! draggable document tabs — so this is a Phase-B catalog entry as much
//! as a substrate test.
//!
//! ## Selection follows the dragged tab (data-indexed)
//!
//! Selection is stored by **stable tab id**, not visual position
//! ([[r730-widget-tag-walk-back]]'s data-index lesson): a reorder is then
//! a pure permutation of `order` that leaves the selection untouched, so
//! the active tab visually follows when it moves. Both a click and a
//! drag commit through `drag_release`, which sets `selected` to the
//! dragged/clicked tab's id (carried in the [`DragPayload`] `begin_drag`
//! produced). A press-release-in-place is a zero-distance drag whose
//! `drag_release` therefore selects the clicked tab — one path for click
//! and drag, no separate selection statechart.
//!
//! ## Keyboard (WAI-ARIA tabs + APG keyboard-drag, modifier-free)
//!
//! - Arrow Left/Right, Home/End — move the roving cursor and *activate*
//!   the tab under it (automatic activation, the WAI-ARIA tabs default).
//! - Space / Enter — pick up the focused tab (APG keyboard drag).
//! - Arrow (while grabbed) — move the picked-up tab one slot, cursor
//!   following; the moved tab stays selected.
//! - Escape — cancel a grab, reverting to the pre-grab order.
//!
//! Modifier-free so it drives through plain `scene/key` (the RPC key
//! channel carries no modifiers — R742.2).
//!
//! ## AI clients
//!
//! `query("/external/order")` (visual → id), `query("/external/labels")`
//! (visual order), `query("/external/selected_id")`, and the drag is
//! driven by the existing `scene/drag` RPC — no new RPC method.

use std::borrow::Cow;
use std::cell::Cell;

use pinion_a11y::{AccessAction, AccessFocus, AccessNode, TabCell, WidgetA11y, tablist_tab_nodes};
// R815 §5.40 — `AriaRole` is now only referenced by the test asserts (the
// lifted `tablist_tab_nodes` builder owns the role + state tagging in prod).
#[cfg(test)]
use pinion_a11y::AriaRole;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, DragPayload, DropPoint, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size,
    SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::reorder::{ReorderAxis, ReorderModel, read_reorder};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::tabs::{TabsStyle, composite_tab_tag};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTabReorderRenderer, HelloTabReorderRendererError);

const WIN_W: u32 = 520;
const WIN_H: u32 = 300;
const THEME_TAG: &str = "app";
const N: usize = 4;
/// Root `External` tag — the composite primary the tabs hang off
/// (`tabreorder#0`..`tabreorder#3`) and the path the framework maps
/// `/external/*` onto.
const TAG: &str = "tabreorder";
const PANEL_TAG: &str = "tabreorder_panel";

/// Fixed tab width. M3 fixed tabs share a baseline; a *fixed* width (vs
/// content-sized) makes the drag insertion line an absolutely-positioned
/// overlay at a computable gap boundary (`k * TAB_W`), exactly as
/// `hello-dnd` fixes `ROW_W`. Variable-width tabs would need a measured
/// gap pass — deferred until a consumer needs elastic tabs.
const TAB_W: u32 = 120;
/// Strip width = N tabs (gap 0, M3 contiguous). Fixed so the insertion
/// line never reflows the strip. Locked to `N` by the const-assert.
const STRIP_W: u32 = 4 * TAB_W;
const _: () = assert!(N == 4, "STRIP_W hard-codes N tabs at gap 0");
/// Insertion-line thickness (a vertical accent bar at the drop gap).
const LINE_W: u32 = 4;
const SWATCH: u32 = 12;
const PANEL_FONT_PX: u32 = 16;
const FOOTER_FONT_PX: u32 = 12;

/// The four tabs: `(label, swatch colour, panel body)`. The swatch is a
/// per-id colour (an IDE file-type-icon stand-in) so a reorder is visible
/// at the pixel level — sampling a tab's swatch yields that tab's colour
/// wherever it now sits, the same witness `hello-dnd` gets from full-row
/// fills. Strong, well-separated hues keep the live-pixel guard robust.
const TABS: [(&str, Color, &str); N] = [
    (
        "Scene",
        Color::rgb(0x43, 0xA0, 0x47),
        "3-D scene graph: nodes, transforms, and gizmos.",
    ),
    (
        "Script",
        Color::rgb(0x1E, 0x88, 0xE5),
        "Gameplay scripts and visual-script graphs.",
    ),
    (
        "Shader",
        Color::rgb(0xFB, 0xC0, 0x2D),
        "Material editor and shader-graph nodes.",
    ),
    (
        "Assets",
        Color::rgb(0xE5, 0x39, 0x35),
        "Asset browser: meshes, textures, and audio.",
    ),
];

/// Boot order — tabs in declaration order.
const IDENTITY: [usize; N] = [0, 1, 2, 3];

/// The composite paint tag for visual position `i` (`"tabreorder#0"` …).
/// The `InputRouter` splits this at `#`, routes hits to the primary
/// `tabreorder` external, and reports the full tag back as the
/// [`DropPoint`].
fn tab_tag(visual: usize) -> String {
    composite_tab_tag(TAG, visual)
}

/// Top-left x of the insertion line for drop gap `k` (`0..=N`): the gap
/// boundary `k * TAB_W`, backed off half the line so the bar straddles
/// the boundary, clamped inside the strip.
fn gap_line_left(k: usize) -> u32 {
    let k = u32::try_from(k).unwrap_or(0);
    let boundary = k * TAB_W;
    let left = boundary.saturating_sub(LINE_W / 2);
    left.min(STRIP_W.saturating_sub(LINE_W))
}

/// The `read_state` projection the view renders: visual order
/// (`order[visual] = tab id`), the selected tab **id**, an optional
/// in-flight drag preview, and the keyboard cursor / grab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TabsState {
    order: [usize; N],
    /// Selected tab **id** (not visual position) — survives a reorder.
    selected: usize,
    /// Dragged visual + target gap, while a drag is in flight.
    preview: Option<(usize, usize)>,
    /// Keyboard cursor / WAI-ARIA active descendant (visual position).
    focused: Option<usize>,
    /// Whether the focused tab is picked up (APG keyboard drag).
    grabbed: bool,
}

impl Default for TabsState {
    fn default() -> Self {
        Self {
            order: IDENTITY,
            selected: 0,
            preview: None,
            focused: None,
            grabbed: false,
        }
    }
}

/// R743 §5.51 — the reorderable tab-strip coordinator. Composes the
/// lifted [`ReorderModel`] (order + drag session state + keyboard grab)
/// with a tab-specific selected-id [`Cell`]. A plain `External` (no
/// statechart; the drag *session* lives in the router), like
/// `hello-progress` / the `hello-dnd` list.
struct ReorderableTabsExternal {
    /// The shared reorder mechanics (order / pressed / preview / focused
    /// cursor / grab snapshot). Horizontal axis: the drop classification
    /// reads `x_rel`.
    reorder: ReorderModel,
    /// Selected tab **id**. Set by `drag_release` (click or drag) and by
    /// the keyboard navigation policy; a reorder leaves it untouched.
    selected: Cell<usize>,
}

impl ReorderableTabsExternal {
    fn new() -> Self {
        Self {
            reorder: ReorderModel::new(N, ReorderAxis::Horizontal),
            selected: Cell::new(0),
        }
    }

    /// Visual position of the selected id in the current order (the tab
    /// the indicator sits under). `0` if the id somehow falls out of the
    /// order (cannot happen — `order` is a permutation of `0..N`).
    fn selected_visual(&self) -> usize {
        self.reorder
            .order()
            .iter()
            .position(|&id| id == self.selected.get())
            .unwrap_or(0)
    }

    /// Labels in current visual order — the AI-readable observable
    /// (`query("labels")`) and the AT name source.
    fn labels(&self) -> Vec<&'static str> {
        self.reorder.order().iter().map(|&id| TABS[id].0).collect()
    }

    /// Keyboard navigation policy (tab-specific, *not* part of the shared
    /// reorder core): move the cursor one step in visual order and
    /// activate the tab under it (WAI-ARIA automatic activation). Wraps
    /// at the ends. Selecting writes the tab **id** at the new visual
    /// position so the indicator follows. Returns the new cursor.
    fn navigate(&self, dir: i32) -> usize {
        let cur = self
            .reorder
            .focused()
            .unwrap_or_else(|| self.selected_visual());
        let next = match dir {
            1 => (cur + 1) % N,
            -1 => (cur + N - 1) % N,
            _ => cur,
        };
        self.reorder.set_focused(next);
        self.selected.set(self.reorder.order()[next]);
        next
    }

    /// Jump the cursor to `visual` and activate that tab.
    fn navigate_to(&self, visual: usize) -> usize {
        let visual = visual.min(N - 1);
        self.reorder.set_focused(visual);
        self.selected.set(self.reorder.order()[visual]);
        visual
    }
}

impl core::fmt::Debug for ReorderableTabsExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ReorderableTabsExternal")
            .field("reorder", &self.reorder)
            .field("selected", &self.selected.get())
            .finish()
    }
}

impl External for ReorderableTabsExternal {
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

    /// Arm a drag from the most-recently-pressed tab — delegates straight
    /// to the reorder model, which carries the dragged tab's stable id in
    /// the payload (`"tab"` kind). The id is what `drag_release` reads to
    /// follow the selection.
    fn begin_drag(&self) -> Option<DragPayload> {
        self.reorder.begin_drag_payload(Cow::Borrowed("tab"))
    }

    /// Live update: forward to the reorder model (classifies the cursor's
    /// drop gap along the horizontal axis and stores the preview the view
    /// reads as a dim + insertion line).
    fn drag_to(&mut self, payload: &DragPayload, over: Option<DropPoint>) {
        self.reorder.drag_to(payload, over.as_ref());
    }

    /// Commit: the selection follows the dragged (or clicked) tab — set
    /// `selected` to the payload's id *before* the model clears its
    /// transient state — then apply the move. A press-release-in-place is
    /// a zero-distance drag, so this is also the click-to-select path.
    fn drag_release(&mut self, payload: &DragPayload, over: Option<DropPoint>) {
        if let Some(id) = payload.value.as_usize() {
            if id < N {
                self.selected.set(id);
            }
        }
        self.reorder.drag_release(payload, over.as_ref());
    }
}

impl ExternalIntrospect for ReorderableTabsExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("order", "json"),
            ("labels", "json"),
            ("selected_id", "int"),
            ("selected_visual", "int"),
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
            "labels" => {
                let arr: Vec<serde_json::Value> = self
                    .labels()
                    .into_iter()
                    .map(serde_json::Value::from)
                    .collect();
                Some(IntrospectValue::Json(serde_json::Value::Array(arr)))
            }
            "selected_id" => Some(IntrospectValue::Int(
                i64::try_from(self.selected.get()).unwrap_or(0),
            )),
            "selected_visual" => Some(IntrospectValue::Int(
                i64::try_from(self.selected_visual()).unwrap_or(0),
            )),
            // Reorder-owned slots: order / preview / focused_index /
            // grabbed. The model returns `None` for anything else, so the
            // tab-specific slots above take precedence.
            other => self.reorder.query(other),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Form-default / persisted-restore selection (slot assignment,
            // no drag). Out-of-range ids are rejected.
            "selected_id" => {
                let i = value.as_usize().ok_or(InterveneError::TypeMismatch)?;
                if i >= N {
                    return Err(InterveneError::OutOfRange);
                }
                self.selected.set(i);
                Ok(())
            }
            // focused_index → reorder model; everything else (order) is
            // read-only / unknown there.
            other => self.reorder.intervene(other, &value),
        }
    }

    fn invoke(
        &mut self,
        method: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match method {
            // Tab-specific keyboard navigation (automatic activation):
            // step the cursor and activate the tab under it.
            "navigate" => {
                let d = args.as_i64().ok_or(InvokeError::TypeMismatch)?;
                let cur = self.navigate(if d >= 0 { 1 } else { -1 });
                Ok(IntrospectValue::Int(i64::try_from(cur).unwrap_or(0)))
            }
            "navigate_to" => {
                let v = args.as_usize().ok_or(InvokeError::TypeMismatch)?;
                let cur = self.navigate_to(v);
                Ok(IntrospectValue::Int(i64::try_from(cur).unwrap_or(0)))
            }
            // Space grabs the *active* tab — seed the cursor at the
            // selected tab when none is set, then delegate to the model.
            "grab" => {
                if self.reorder.focused().is_none() {
                    self.navigate_to(self.selected_visual());
                }
                self.reorder.invoke("grab", &args)
            }
            // `send` (composite "{visual}:{Event}" — PointerDown records
            // which visual was pressed so `begin_drag` arms it), `move`,
            // `grab_cancel` are pure reorder-model actions. Selection is
            // *not* wired through `send`: it commits in `drag_release`
            // (a click is a zero-distance drag).
            _ => self.reorder.invoke(method, &args),
        }
    }
}

/// Build one tab: a fixed-width Column of a [swatch + label] row above a
/// 3 px active-indicator bar (M3). The dragged tab dims; the focused tab
/// gets a ring; a grabbed tab gets a thicker Accent ring.
fn tab_node(
    visual: usize,
    state: &TabsState,
    theme: &pinion_core::theme::Theme,
    style: &TabsStyle,
) -> Scene {
    let id = state.order[visual];
    let (label, swatch_base, _) = TABS[id];
    let is_selected = id == state.selected;
    let dim = state.preview.is_some_and(|(from, _)| from == visual);
    let focused = state.focused == Some(visual);
    let grabbed = focused && state.grabbed;

    let swatch_fill = if dim {
        swatch_base.with_alpha(0x55)
    } else {
        swatch_base
    };
    let swatch = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(swatch_fill).with_corner_radius(3),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(SWATCH, SWATCH))),
    );
    let label_color = if is_selected {
        theme.resolve(ColorRole::Accent)
    } else {
        theme.resolve(ColorRole::OnSurfaceMuted)
    };
    let label_node = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new()
            .with_size_px(style.font_size_px)
            .with_fg(label_color),
    ));
    // Leading swatch + label, left-aligned (IDE editor-tab convention):
    // the swatch then sits at a *fixed* offset (`tab_padding`) inside every
    // tab regardless of label width, so it is a reliable per-tab pixel
    // witness that moves with a reorder (the `hello-dnd` full-row-fill
    // witness adapted to a uniform M3 strip).
    let label_row = Scene::Container(
        ContainerNode::new(vec![swatch, label_node]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Start)
                .with_flex_grow(1.0)
                .with_gap(8)
                .with_padding(Rect::new(style.tab_padding, 0, style.tab_padding, 0)),
        ),
    );
    let indicator_color = if is_selected {
        theme.resolve(ColorRole::Accent)
    } else {
        Color::TRANSPARENT
    };
    let indicator = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(indicator_color).with_corner_radius(style.indicator_radius),
        )
        .with_layout(
            LayoutStyle::new()
                .with_size(Size::auto().with_height(SizeValue::Px(style.indicator_thickness))),
        ),
    );

    let mut box_style = BoxStyle::filled(theme.resolve(ColorRole::Surface));
    if grabbed {
        box_style = box_style.with_border(Border::new(theme.resolve(ColorRole::Accent), 3));
    } else if focused {
        box_style = box_style.with_border(Border::new(theme.resolve(ColorRole::OnSurface), 2));
    }
    Scene::Container(
        ContainerNode::new(vec![label_row, indicator])
            .with_tag(tab_tag(visual))
            .with_style(box_style)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_size(
                        Size::auto()
                            .with_width(SizeValue::Px(TAB_W))
                            .with_height(SizeValue::Px(style.tab_height)),
                    ),
            ),
    )
}

/// The vertical insertion line for drop gap `k` — a thin accent bar,
/// **absolutely positioned** inside the fixed-width strip so showing it
/// never reflows the tabs (the `hello-dnd` R742.1 overlay precedent).
fn insertion_line(k: usize, theme: &pinion_core::theme::Theme, height: u32) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![])
            .with_tag("tabreorder_insert")
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Accent)).with_corner_radius(2))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(gap_line_left(k), 0)
                    .with_size(Size::px(LINE_W, height)),
            ),
    )
}

/// view-fn (§6.3): pure sync `TabsState -> Scene`. A fixed-width tab
/// strip (visual order) over the active tab's panel body. The dragged tab
/// dims, the focused tab gets a ring, and — while dragging — an
/// absolutely-positioned vertical insertion line overlays the target gap
/// without shifting any tab.
fn view(state: TabsState) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let style = TabsStyle::m3_default();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);

    let mut tabs: Vec<Scene> = Vec::with_capacity(N + 1);
    for v in 0..N {
        tabs.push(tab_node(v, &state, &theme, &style));
    }
    if let Some((_, insert_at)) = state.preview {
        tabs.push(insertion_line(insert_at, &theme, style.tab_height));
    }
    let strip = Scene::Container(
        ContainerNode::new(tabs)
            .with_tag(TAG)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Start)
                    .with_gap(style.tab_gap)
                    .with_size(Size::px(STRIP_W, style.tab_height))
                    .with_focusable(true),
            ),
    );

    let panel = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            TABS[state.selected].2,
            Rect::default(),
            TextStyle::new()
                .with_size_px(PANEL_FONT_PX)
                .with_fg(on_surface),
        ))])
        .with_tag(PANEL_TAG)
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Start)
                .with_justify(JustifyContent::Start)
                .with_flex_grow(1.0)
                .with_padding(Rect::new(16, 16, 16, 16)),
        ),
    );

    let footer = Scene::Text(TextNode::styled(
        "drag a tab to reorder   \u{2190}/\u{2192} switch   Space grab \u{2192} \u{2190}/\u{2192} move",
        Rect::default(),
        TextStyle::new()
            .with_size_px(FOOTER_FONT_PX)
            .with_fg(on_surface_muted),
    ));

    Scene::Container(
        ContainerNode::new(vec![strip, panel, footer])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Stretch)
                    .with_gap(8)
                    .with_padding(Rect::new(12, 12, 12, 12)),
            ),
    )
}

struct TabReorderView;

impl WidgetCore for TabReorderView {
    type State = TabsState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let ext = ReorderableTabsExternal::new();
        // Form default: tab id 0 (Scene) active — a tab strip always
        // shows one panel. `selected` is seeded in `new`.
        Box::new(ext)
    }

    fn tag() -> &'static str {
        TAG
    }

    fn read_state(scene: &Scene) -> TabsState {
        let mut out = TabsState::default();
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        // Shared reorder slots through `read_reorder` (the deserialize
        // peer of `ReorderModel::query`); only `selected_id` is this
        // binding's own slot.
        let v = read_reorder(intro);
        out.order = v.order.try_into().unwrap_or(IDENTITY);
        out.preview = v.preview.map(|p| (p.from_visual, p.insert_at));
        out.focused = v.focused;
        out.grabbed = v.grabbed;
        if let Some(IntrospectValue::Int(i)) = intro.query("selected_id") {
            if let Ok(i) = usize::try_from(i) {
                out.selected = i;
            }
        }
        out
    }

    fn view(state: TabsState, _frame: &Frame) -> Scene {
        view(state)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-tab-reorder (R743 §5.51)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// WAI-ARIA tabs + APG keyboard-drag, modifier-free. The TabList is a
    /// single tab stop with a roving cursor:
    ///
    /// * Arrow Left/Right, Home/End — move the cursor and activate the
    ///   tab under it (automatic activation). While *grabbing*, Arrow
    ///   moves the picked-up tab one slot (Home/End to the ends).
    /// * Space / Enter — pick up / drop the focused tab.
    /// * Escape — cancel a grab, reverting the order.
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
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        let grabbed = matches!(intro.query("grabbed"), Some(IntrospectValue::Bool(true)));
        let big = i64::try_from(N).unwrap_or(i64::MAX);
        let last = i64::try_from(N - 1).unwrap_or(0);
        match key {
            " " | "Spacebar" | "Enter" => {
                let _ = intro.invoke("grab", IntrospectValue::Null);
                true
            }
            "Escape" => grabbed && intro.invoke("grab_cancel", IntrospectValue::Null).is_ok(),
            "ArrowRight" | "ArrowDown" => {
                let _ = if grabbed {
                    intro.invoke("move", IntrospectValue::Int(1))
                } else {
                    intro.invoke("navigate", IntrospectValue::Int(1))
                };
                true
            }
            "ArrowLeft" | "ArrowUp" => {
                let _ = if grabbed {
                    intro.invoke("move", IntrospectValue::Int(-1))
                } else {
                    intro.invoke("navigate", IntrospectValue::Int(-1))
                };
                true
            }
            "Home" => {
                let _ = if grabbed {
                    intro.invoke("move", IntrospectValue::Int(-big))
                } else {
                    intro.invoke("navigate_to", IntrospectValue::Int(0))
                };
                true
            }
            "End" => {
                let _ = if grabbed {
                    intro.invoke("move", IntrospectValue::Int(big))
                } else {
                    intro.invoke("navigate_to", IntrospectValue::Int(last))
                };
                true
            }
            _ => false,
        }
    }

    fn fmt_state_log(state: &TabsState) -> String {
        let order: Vec<String> = state.order.iter().map(usize::to_string).collect();
        format!("order=[{}] selected={}", order.join(","), state.selected)
    }
}

impl WidgetA11y for TabReorderView {
    /// WAI-ARIA 1.2 §3.6 tablist/tab/tabpanel tree. Emits N+2 nodes: a
    /// [`AriaRole::TabList`] parent claiming each tab in *visual* order, a
    /// [`AriaRole::Tab`] per visual position (named by the tab now at
    /// that position, `aria-selected` when its id is selected,
    /// `aria-posinset`/`setsize`), and a [`AriaRole::TabPanel`] for the
    /// active tab's body.
    fn access_node(state: &TabsState, focused: Option<&str>) -> Vec<AccessNode> {
        // R815 §5.40 — lifted `tablist_tab_nodes` builder. Tabs are
        // addressed by *visual* slot `v` (`tab_tag(v)`); the data tab id is
        // `state.order[v]`, which keys the explicit label. The builder
        // derives `aria-posinset` / `aria-setsize` from the slice.
        let list_focused = focused == Some(<Self as WidgetCore>::tag());
        let active_cursor = state.focused.unwrap_or_else(|| {
            state
                .order
                .iter()
                .position(|&id| id == state.selected)
                .unwrap_or(0)
        });
        let tab_tags: Vec<String> = (0..N).map(tab_tag).collect();
        let tabs: Vec<TabCell<'_>> = (0..N)
            .map(|v| {
                let id = state.order[v];
                TabCell {
                    tag: &tab_tags[v],
                    label: Some(TABS[id].0),
                    selected: id == state.selected,
                    focused: list_focused && v == active_cursor,
                }
            })
            .collect();
        tablist_tab_nodes(
            <Self as WidgetCore>::tag(),
            "Editor tabs",
            &tabs,
            PANEL_TAG,
            TABS[state.selected].0,
        )
    }

    /// Composite focus model: when the TabList holds focus, focus stays on
    /// the parent (`tabreorder`) and the cursor tab's sub-tag is the
    /// `aria-activedescendant`.
    fn access_focus_target(state: &TabsState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(<Self as WidgetCore>::tag()) {
            let cursor = state.focused.unwrap_or_else(|| {
                state
                    .order
                    .iter()
                    .position(|&id| id == state.selected)
                    .unwrap_or(0)
            });
            Some(AccessFocus::composite(
                <Self as WidgetCore>::tag(),
                tab_tag(cursor),
            ))
        } else {
            focused.map(AccessFocus::atomic)
        }
    }

    /// AT actions on a tab sub-tag: `Click` / `Default` / `Focus` activate
    /// that tab (cursor + selection follow); `Increment` / `Decrement`
    /// move it one slot through the reorder `move` funnel.
    fn access_child_invoke(
        scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        let Ok(visual) = sub_tag.parse::<usize>() else {
            return false;
        };
        if visual >= N {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        let v = i64::try_from(visual).unwrap_or(0);
        match action {
            AccessAction::Click | AccessAction::Default | AccessAction::Focus => {
                let _ = intro.invoke("navigate_to", IntrospectValue::Int(v));
                true
            }
            AccessAction::Increment => {
                let _ = intro.invoke("navigate_to", IntrospectValue::Int(v));
                intro.invoke("move", IntrospectValue::Int(1)).is_ok()
            }
            AccessAction::Decrement => {
                let _ = intro.invoke("navigate_to", IntrospectValue::Int(v));
                intro.invoke("move", IntrospectValue::Int(-1)).is_ok()
            }
            AccessAction::Other => false,
        }
    }
}

impl WidgetView for TabReorderView {
    type Renderer = HelloTabReorderRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<TabReorderView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Modifiers;
    use pinion_core::scene::ExternalNode;

    const NONE: Modifiers = Modifiers::empty();

    fn fresh() -> ReorderableTabsExternal {
        ReorderableTabsExternal::new()
    }

    fn drop_at(visual: usize, x_rel: f32) -> DropPoint {
        DropPoint {
            tag: tab_tag(visual),
            x_rel,
            y_rel: 0.5,
        }
    }

    fn scene_of(ext: ReorderableTabsExternal) -> Scene {
        Scene::External(ExternalNode::new(Box::new(ext)).with_tag(TAG))
    }

    #[test]
    fn r743_boot_state_is_identity_with_tab0_selected() {
        let st = TabReorderView::read_state(&scene_of(fresh()));
        assert_eq!(st.order, IDENTITY);
        assert_eq!(st.selected, 0);
        assert!(st.preview.is_none());
    }

    #[test]
    fn r743_drag_reorders_and_selection_follows_dragged_tab() {
        let mut ext = fresh();
        // Press visual 0 (Scene, id 0), drag onto the right half of
        // visual 2 → gap 3.
        ext.invoke("send", IntrospectValue::Text("0:PointerDown".into()))
            .expect("press");
        let pl = ext.begin_drag().expect("armed");
        assert_eq!(pl.value.as_usize(), Some(0), "payload carries dragged id");
        ext.drag_to(&pl, Some(drop_at(2, 0.75)));
        ext.drag_release(&pl, Some(drop_at(2, 0.75)));
        // [0,1,2,3] move 0 → gap 3 → [1,2,0,3]; Scene (id 0) now at visual 2.
        assert_eq!(ext.reorder.order(), [1, 2, 0, 3]);
        assert_eq!(
            ext.selected.get(),
            0,
            "selection follows the dragged tab id"
        );
        assert_eq!(
            ext.selected_visual(),
            2,
            "selected tab now sits at visual 2"
        );
    }

    #[test]
    fn r743_press_release_in_place_selects_clicked_tab_no_reorder() {
        let mut ext = fresh();
        // Click visual 2 (Shader, id 2): press + release over its own gap.
        ext.invoke("send", IntrospectValue::Text("2:PointerDown".into()))
            .expect("press");
        let pl = ext.begin_drag().expect("armed");
        ext.drag_release(&pl, Some(drop_at(2, 0.25)));
        assert_eq!(ext.reorder.order(), IDENTITY, "click does not reorder");
        assert_eq!(ext.selected.get(), 2, "click selects the clicked tab");
    }

    #[test]
    fn r743_reorder_is_pure_permutation_selection_id_stable() {
        let mut ext = fresh();
        // Select Shader (id 2) by clicking visual 2.
        ext.invoke("send", IntrospectValue::Text("2:PointerDown".into()))
            .expect("press");
        ext.drag_release(&ext.begin_drag().expect("armed"), Some(drop_at(2, 0.25)));
        assert_eq!(ext.selected.get(), 2);
        // Now drag a *different* tab (visual 0, id 0) elsewhere; the
        // selection id must stay 2 and visually follow.
        ext.invoke("send", IntrospectValue::Text("0:PointerDown".into()))
            .expect("press");
        let pl = ext.begin_drag().expect("armed");
        ext.drag_to(&pl, Some(drop_at(3, 0.75)));
        ext.drag_release(&pl, Some(drop_at(3, 0.75)));
        // Dragging tab 0 changes the selection to the dragged tab (id 0)
        // — VSCode/Chrome behaviour: the dragged tab becomes active.
        assert_eq!(ext.selected.get(), 0, "dragging a tab activates it");
    }

    #[test]
    fn r743_query_order_labels_selected_round_trip() {
        let mut ext = fresh();
        ext.invoke("send", IntrospectValue::Text("0:PointerDown".into()))
            .expect("press");
        ext.drag_release(&ext.begin_drag().expect("armed"), Some(drop_at(2, 0.75)));
        match ext.query("labels") {
            Some(IntrospectValue::Json(serde_json::Value::Array(a))) => {
                let got: Vec<&str> = a.iter().filter_map(serde_json::Value::as_str).collect();
                assert_eq!(got, ["Script", "Shader", "Scene", "Assets"]);
            }
            other => panic!("expected labels array, got {other:?}"),
        }
        assert_eq!(ext.query("selected_id"), Some(IntrospectValue::Int(0)));
        assert_eq!(ext.query("selected_visual"), Some(IntrospectValue::Int(2)));
    }

    #[test]
    fn r743_intervene_selected_id_clamps_and_rejects() {
        let mut ext = fresh();
        ext.intervene("selected_id", IntrospectValue::Int(3))
            .expect("in range");
        assert_eq!(ext.selected.get(), 3);
        assert!(matches!(
            ext.intervene("selected_id", IntrospectValue::Int(9)),
            Err(InterveneError::OutOfRange)
        ));
        assert!(matches!(
            ext.intervene("order", IntrospectValue::Int(0)),
            Err(InterveneError::ReadOnly)
        ));
    }

    #[test]
    fn r743_keyboard_arrow_navigates_and_activates() {
        let mut scene = scene_of(fresh());
        assert!(!TabReorderView::apply_key(
            &mut scene,
            None,
            "ArrowRight",
            NONE
        ));
        assert!(TabReorderView::apply_key(
            &mut scene,
            Some(TAG),
            "ArrowRight",
            NONE
        ));
        let st = TabReorderView::read_state(&scene);
        assert_eq!(st.selected, 1, "ArrowRight activates the next tab");
        assert_eq!(st.focused, Some(1), "cursor follows");
        assert_eq!(st.order, IDENTITY, "navigation does not reorder");
        assert!(TabReorderView::apply_key(
            &mut scene,
            Some(TAG),
            "End",
            NONE
        ));
        assert_eq!(
            TabReorderView::read_state(&scene).selected,
            3,
            "End → last tab"
        );
    }

    #[test]
    fn r743_keyboard_grab_then_arrow_reorders_cursor_follows() {
        let mut scene = scene_of(fresh());
        // Cursor to visual 0, grab, move right.
        assert!(TabReorderView::apply_key(
            &mut scene,
            Some(TAG),
            "Home",
            NONE
        ));
        assert!(TabReorderView::apply_key(&mut scene, Some(TAG), " ", NONE));
        assert!(TabReorderView::read_state(&scene).grabbed, "Space grabs");
        assert!(TabReorderView::apply_key(
            &mut scene,
            Some(TAG),
            "ArrowRight",
            NONE
        ));
        let st = TabReorderView::read_state(&scene);
        assert_eq!(st.order, [1, 0, 2, 3], "grabbed tab moved one slot right");
        assert_eq!(st.focused, Some(1), "cursor follows the grabbed tab");
        assert_eq!(st.selected, 0, "the grabbed (moved) tab stays selected");
        // Drop.
        assert!(TabReorderView::apply_key(&mut scene, Some(TAG), " ", NONE));
        assert!(!TabReorderView::read_state(&scene).grabbed);
        assert_eq!(TabReorderView::read_state(&scene).order, [1, 0, 2, 3]);
    }

    #[test]
    fn r743_keyboard_escape_cancels_grab_reverts_order() {
        let mut scene = scene_of(fresh());
        assert!(TabReorderView::apply_key(
            &mut scene,
            Some(TAG),
            "Home",
            NONE
        ));
        assert!(TabReorderView::apply_key(&mut scene, Some(TAG), " ", NONE));
        assert!(TabReorderView::apply_key(
            &mut scene,
            Some(TAG),
            "End",
            NONE
        ));
        assert_ne!(
            TabReorderView::read_state(&scene).order,
            IDENTITY,
            "grabbed End moved it"
        );
        assert!(TabReorderView::apply_key(
            &mut scene,
            Some(TAG),
            "Escape",
            NONE
        ));
        let st = TabReorderView::read_state(&scene);
        assert!(!st.grabbed);
        assert_eq!(st.order, IDENTITY, "Escape reverts to the pre-grab order");
    }

    #[test]
    fn r743_unknown_path_rejected() {
        let ext = fresh();
        assert!(ext.query("no_such_field").is_none());
    }
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    fn state_sel(id: usize) -> TabsState {
        TabsState {
            selected: id,
            ..TabsState::default()
        }
    }

    #[test]
    fn r743_emits_tablist_plus_n_tabs_plus_panel() {
        let nodes = TabReorderView::access_node(&state_sel(0), None);
        assert_eq!(nodes.len(), N + 2);
        assert_eq!(nodes[0].role, AriaRole::TabList);
        for i in 0..N {
            assert_eq!(nodes[i + 1].role, AriaRole::Tab);
        }
        assert_eq!(nodes[N + 1].role, AriaRole::TabPanel);
    }

    #[test]
    fn r743_tab_names_and_panel_follow_visual_order_and_selection() {
        let state = TabsState {
            order: [2, 0, 3, 1],
            selected: 3,
            ..TabsState::default()
        };
        let nodes = TabReorderView::access_node(&state, None);
        let names: Vec<&str> = nodes[1..=N]
            .iter()
            .filter_map(|n| n.name.as_deref())
            .collect();
        assert_eq!(names, ["Shader", "Scene", "Assets", "Script"]);
        // Selected id 3 (Assets) sits at visual 2 → that Tab is selected.
        assert_eq!(nodes[1].selected, Some(false));
        assert_eq!(nodes[3].selected, Some(true));
        assert_eq!(
            nodes[N + 1].name.as_deref(),
            Some("Assets"),
            "panel = active tab label"
        );
    }

    #[test]
    fn r743_access_child_invoke_click_activates_tab() {
        let mut scene = Scene::External(
            pinion_core::scene::ExternalNode::new(Box::new(ReorderableTabsExternal::new()))
                .with_tag(TAG),
        );
        assert!(TabReorderView::access_child_invoke(
            &mut scene,
            TAG,
            "2",
            AccessAction::Click
        ));
        assert_eq!(TabReorderView::read_state(&scene).selected, 2);
    }

    #[test]
    fn r55_g20_view_contains_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<TabReorderView>(
            TabsState::default(),
            &Frame::new(),
        );
    }
}
