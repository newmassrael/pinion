// R989 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the architectural narrative carries many proper-noun
// identifiers (ToolbarExternal, WAI-ARIA, aria-disabled, …).
#![allow(clippy::doc_markdown)]

//! `hello-selection-toolbar` — R989 §5.16 §5.38 §5.40 **contextual
//! selection-action toolbar**.
//!
//! ## Why this binding exists
//!
//! Phase B common-catalog axis: the **bulk-action bar** every file manager,
//! mail client, asset browser, and scene outliner ships — select rows, and a
//! toolbar of actions (Delete / Clear / Select all) reflects what you can do
//! to the selection, greying out actions that do not apply (Gmail's trash icon
//! is dimmed until you tick a message; a DCC outliner's "Group" needs ≥1 node).
//! This is *not* a re-do of the static format [`hello-toolbar`](../hello-toolbar)
//! (R692) — it is the toolbar in its **contextual** role, and it lands the
//! toolbar substrate's documented-but-unimplemented **disabled axis** at its
//! first real consumer.
//!
//! ## Composition
//!
//! * **the list** — a [`SelRowExternal`] (the *primary* external) over a shared
//!   [`Signal<Vec<SelItem>>`] holder ([`use_items`]). A row click toggles its
//!   membership; arrows rove and `Space` / `Enter` toggle the focused row. The
//!   selection is *model* state (it survives, and shrinks under, deletes), so it
//!   lives in the holder, not in a fixed-`N` coordinator.
//! * **the action toolbar** — the R692 [`ToolbarExternal`] (an *extra* external)
//!   of three [`ToolItem::Command`]s, drawn by `view_toolbar`. Its disabled mask
//!   is **reflective** (R989): the view recomputes it each frame from the
//!   selection ([`action_disabled`]) and passes it to the paint and the a11y
//!   tree, exactly as `hello-textarea` reflects `aria-pressed` from the document.
//!   The toolbar owns no disabled bit, so the mask can never drift.
//! * **the bulk action** — the toolbar's `"command"` intent reaches
//!   [`update`](SelectionToolbarView::update), which applies the action to the
//!   **same** holder. The reducer is also the operable barrier: it gates on the
//!   *same* [`action_disabled`] mask, so a disabled action is a no-op even when
//!   driven by keyboard or RPC (the toolbar emits regardless — it cannot see the
//!   reflective mask).
//!
//! ## AI clients
//!
//! `query("/sel_list/external/selected_count")` and `.../ids_selected` expose
//! the selection; `scene/click` on a `sel_list#<id>` row (or
//! `invoke("send", "<id>:PointerUp")`) toggles it; a left click on `actions#<i>`
//! (or `invoke("send", "<i>:PointerUp")` on `/actions/external`) runs an action.
//! `scene/access` dumps the toolbar with `aria-disabled` on the unavailable
//! commands and the list with `aria-selected` per row (§2 invariant #7).

use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, ListOption, ToolbarControl, WidgetA11y,
    listbox_option_nodes, toolbar_button_nodes,
};
use pinion_core::composite_tag::parse_send_payload;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaArg, SchemaField,
    ThreadOwnership,
};
use pinion_core::intent::Intent;
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_core::widgets::toolbar::{ToolItem, ToolbarExternal, read_roving_focus};
use pinion_core::widgets::virtual_select::clamp_nav;
use pinion_core::{Color, Command, Frame, Modifiers, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::toolbar::{ToolbarStyle, composite_item_tag as action_tag, view_toolbar};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(
    HelloSelectionToolbarRenderer,
    HelloSelectionToolbarRendererError
);

/// Window size: fixed, tall enough that the toolbar + count + all rows + the
/// status bar are visible without scrolling.
const WIN_W: u32 = 460;
const WIN_H: u32 = 520;
const THEME_TAG: &str = "app";

/// The [`SelRowExternal`] anchor + paint root: rows paint as the R51.42
/// composite `sel_list#<id>`, routing a click to this primary external.
const LIST_TAG: &str = "sel_list";
/// The [`ToolbarExternal`] scope tag (an *extra* external): action buttons
/// paint as `actions#<i>` and `actions.command` is the activation intent.
const ACTIONS_TAG: &str = "actions";
/// The scene-as-data count readout tag.
const COUNT_TAG: &str = "sel_count";
/// The scene-as-data status-bar tag (full selection state for AI clients).
const STATUS_TAG: &str = "sel_status";

/// Action indices — the order the toolbar paints, the a11y tree builds, and
/// the reducer reads. One source so labels and effects cannot desync.
const ACTION_DELETE: usize = 0;
const ACTION_CLEAR: usize = 1;
const ACTION_SELECT_ALL: usize = 2;
/// Action labels in [`ACTION_DELETE`] / [`ACTION_CLEAR`] / [`ACTION_SELECT_ALL`]
/// order.
const ACTION_LABELS: [&str; 3] = ["Delete", "Clear", "Select all"];
/// Number of toolbar actions.
const ACTION_COUNT: usize = ACTION_LABELS.len();
/// The `"command"` intent the [`ToolbarExternal`] emits, dotted with its scope
/// tag (R51.173 §5.23); the literal must match [`ACTIONS_TAG`].
const COMMAND_INTENT_TAG_FULL: &str = pinion_core::intent_tag!("actions", "command");

/// The [`use_items`] holder cache key.
const ITEMS_KEY: &str = "selection_toolbar.items";
/// Per-row height.
const ROW_H: u32 = 36;

/// Unchecked / checked row glyphs (Unicode `BALLOT BOX` / `… WITH CHECK`),
/// named consts per the non-ASCII-literal rule.
const CHECK_EMPTY: &str = "\u{2610}";
const CHECK_MARK: &str = "\u{2611}";
/// Middle dot separating status-bar fields.
const DOT: &str = "\u{00B7}";

/// One selectable row. `id` is a stable identity (the paint/wire tag), so a
/// delete that removes earlier rows leaves the survivors' tags unchanged.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
struct SelItem {
    id: u64,
    label: String,
    selected: bool,
}

/// The seed dataset — eight rows, none selected.
fn seed_items() -> Vec<SelItem> {
    const LABELS: [&str; 8] = [
        "Mesh: Crate",
        "Mesh: Barrel",
        "Light: Sun",
        "Light: Fill",
        "Camera: Main",
        "Sprite: Hero",
        "Audio: Theme",
        "Script: AI",
    ];
    LABELS
        .iter()
        .enumerate()
        .map(|(i, &label)| SelItem {
            id: u64::try_from(i + 1).expect("seed index fits in u64"),
            label: label.to_string(),
            selected: false,
        })
        .collect()
}

/// The shared selection holder — the single source of truth for the rows and
/// their membership. The view (subscribes via `.get()`), the [`SelRowExternal`]
/// (toggles), and the reducer (bulk actions) all reach the same `Rc`.
fn use_items() -> Rc<Signal<Vec<SelItem>>> {
    Owner::current()
        .expect("use_items requires an active Owner scope")
        .cache(ITEMS_KEY, || Signal::new(seed_items()))
}

/// Count of selected rows in a snapshot.
fn selected_count(items: &[SelItem]) -> usize {
    items.iter().filter(|i| i.selected).count()
}

/// R989 — the reflective disabled mask for the action toolbar, derived from the
/// selection. This is the single source the view paints *and* the reducer gates
/// on, so a greyed action is genuinely inoperable. Delete / Clear need a
/// non-empty selection; Select all is pointless when everything (or nothing) is
/// there to select.
fn action_disabled(selected: usize, total: usize) -> [bool; ACTION_COUNT] {
    let mut d = [false; ACTION_COUNT];
    d[ACTION_DELETE] = selected == 0;
    d[ACTION_CLEAR] = selected == 0;
    d[ACTION_SELECT_ALL] = total == 0 || selected == total;
    d
}

/// Apply a bulk action to the holder (the reducer side effect). Delete drops the
/// selected rows (the collection shrinks); Clear deselects all; Select all
/// selects all. The caller has already confirmed the action is operable.
fn apply_action(action: usize, items: &Signal<Vec<SelItem>>) {
    match action {
        ACTION_DELETE => {
            items.set_with(|prev| prev.iter().filter(|i| !i.selected).cloned().collect());
        }
        ACTION_CLEAR => items.set_with(|prev| {
            prev.iter()
                .map(|i| SelItem {
                    selected: false,
                    ..i.clone()
                })
                .collect()
        }),
        ACTION_SELECT_ALL => items.set_with(|prev| {
            prev.iter()
                .map(|i| SelItem {
                    selected: true,
                    ..i.clone()
                })
                .collect()
        }),
        _ => {}
    }
}

// ----------------------------------------------------------------------------
// SelRowExternal — the primary: a roving multi-select list over the holder.
// ----------------------------------------------------------------------------

/// The list interaction owner. Holds the shared selection holder plus a roving
/// keyboard cursor (`focus`, a position into the *current* rows) and the group
/// focus posture. Mirrors `todomvc`'s `TodoToggleExternal` (composite-tag toggle
/// over a shared `Signal<Vec<…>>`), with the listbox roving keyboard model added.
struct SelRowExternal {
    items: Rc<Signal<Vec<SelItem>>>,
    focus: usize,
    group_focused: bool,
}

impl SelRowExternal {
    fn new(items: Rc<Signal<Vec<SelItem>>>) -> Self {
        Self {
            items,
            focus: 0,
            group_focused: false,
        }
    }

    fn count(&self) -> usize {
        self.items.get().len()
    }

    /// The roving cursor clamped into the current row range (rows can vanish
    /// under a delete the reducer applied to the shared holder).
    fn effective_focus(&self) -> usize {
        self.focus.min(self.count().saturating_sub(1))
    }

    /// Toggle the row with stable id `target_id`. Returns whether such a row
    /// exists (so the wire can tell a real flip from a stale id).
    fn toggle_by_id(&self, target_id: u64) -> bool {
        let present = self.items.get().iter().any(|i| i.id == target_id);
        if present {
            self.items.set_with(|prev| {
                prev.iter()
                    .map(|i| {
                        if i.id == target_id {
                            SelItem {
                                selected: !i.selected,
                                ..i.clone()
                            }
                        } else {
                            i.clone()
                        }
                    })
                    .collect()
            });
        }
        present
    }

    /// The WAI-ARIA listbox roving keyboard model. `Space` / `Enter` toggle the
    /// focused row; the arrow / Home / End / Page navigation reuses the shared
    /// [`clamp_nav`] policy SSOT (R777 — finite-list clamp, no wrap), which is
    /// `pub` precisely so a binding with its own mechanism (here a holder-backed
    /// list, not a `VirtualSelectExternal`) can borrow the key→index mapping
    /// instead of re-deriving it ([[use-substrate-not-hand-rolled-equivalent]]).
    /// Returns whether the key was handled (the swallow verdict).
    fn apply_key(&mut self, key: &str) -> bool {
        let n = self.count();
        if n == 0 {
            return false;
        }
        let cur = self.effective_focus();
        if matches!(key, "Enter" | " " | "Space") {
            if let Some(item) = self.items.get().get(cur) {
                self.toggle_by_id(item.id);
            }
            return true;
        }
        // The whole list is one "page" — PageUp/Down clamp to the ends.
        if let Some(next) = clamp_nav(Some(cur), key, n, n) {
            self.focus = next;
            return true;
        }
        false
    }
}

impl External for SelRowExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn on_focus_change(&mut self, focused: bool) {
        self.group_focused = focused;
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl core::fmt::Debug for SelRowExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SelRowExternal")
            .field("count", &self.count())
            .field("selected", &selected_count(&self.items.get()))
            .field("focus", &self.effective_focus())
            .field("group_focused", &self.group_focused)
            .finish_non_exhaustive()
    }
}

impl ExternalIntrospect for SelRowExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("count", "int"),
                    SchemaField::new("selected_count", "int"),
                    SchemaField::new("focus", "int"),
                    SchemaField::new("focused", "bool"),
                    SchemaField::new("ids_selected", "json"),
                    SchemaField::parametric(
                        "selected.<i>",
                        "bool",
                        const { &[SchemaArg::index("i", "count")] },
                    ),
                    SchemaField::parametric(
                        "label.<i>",
                        "string",
                        const { &[SchemaArg::index("i", "count")] },
                    ),
                    SchemaField::parametric(
                        "id.<i>",
                        "int",
                        const { &[SchemaArg::index("i", "count")] },
                    ),
                    SchemaField::new("send", "string"),
                    SchemaField::new("key", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let snap = self.items.get();
        match path {
            "count" => Some(IntrospectValue::Int(i64::try_from(snap.len()).ok()?)),
            "selected_count" => Some(IntrospectValue::Int(
                i64::try_from(selected_count(&snap)).ok()?,
            )),
            "focus" => Some(IntrospectValue::Int(
                i64::try_from(self.effective_focus()).ok()?,
            )),
            "focused" => Some(IntrospectValue::Bool(self.group_focused)),
            "ids_selected" => {
                let arr: Vec<serde_json::Value> = snap
                    .iter()
                    .filter(|i| i.selected)
                    .map(|i| serde_json::Value::from(i.id))
                    .collect();
                Some(IntrospectValue::Json(serde_json::Value::Array(arr)))
            }
            _ => {
                if let Some(s) = path.strip_prefix("selected.") {
                    return Some(IntrospectValue::Bool(
                        snap.get(s.parse::<usize>().ok()?)?.selected,
                    ));
                }
                if let Some(s) = path.strip_prefix("label.") {
                    return Some(IntrospectValue::Text(
                        snap.get(s.parse::<usize>().ok()?)?.label.clone(),
                    ));
                }
                if let Some(s) = path.strip_prefix("id.") {
                    return Some(IntrospectValue::Int(
                        i64::try_from(snap.get(s.parse::<usize>().ok()?)?.id).ok()?,
                    ));
                }
                None
            }
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "count" | "selected_count" | "ids_selected" => Err(InterveneError::ReadOnly),
            // The roving cursor is settable (RPC / AT focus restore).
            "focus" => match value {
                IntrospectValue::Int(i) => {
                    let n = self.count();
                    let idx = usize::try_from(i).map_err(|_| InterveneError::OutOfRange)?;
                    if n == 0 || idx >= n {
                        return Err(InterveneError::OutOfRange);
                    }
                    self.focus = idx;
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // R990.1 — absolute per-row set, the write mirror of the
            // `selected.<i>` read ([[wire-form-read-write-symmetry]]): an AI
            // client sets a row's membership idempotently rather than having to
            // read-then-relative-toggle through the `send` wire (the
            // `ListBoxExternal` multi-select admin path).
            _ => {
                let Some(s) = path.strip_prefix("selected.") else {
                    return Err(InterveneError::UnknownPath);
                };
                let idx: usize = s.parse().map_err(|_| InterveneError::UnknownPath)?;
                let IntrospectValue::Bool(want) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                if idx >= self.count() {
                    return Err(InterveneError::OutOfRange);
                }
                self.items.set_with(|prev| {
                    prev.iter()
                        .enumerate()
                        .map(|(i, it)| {
                            if i == idx {
                                SelItem {
                                    selected: want,
                                    ..it.clone()
                                }
                            } else {
                                it.clone()
                            }
                        })
                        .collect()
                });
                Ok(())
            }
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Pointer: "<id>:<Event>" — a row release toggles its membership.
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    let (id, event_name, _): (u64, &str, _) =
                        parse_send_payload(payload).ok_or(InvokeError::Rejected)?;
                    if event_name == "PointerUp" {
                        Ok(IntrospectValue::Bool(self.toggle_by_id(id)))
                    } else {
                        Ok(IntrospectValue::Bool(false))
                    }
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Keyboard: a W3C key name through the listbox roving model.
            "key" => match args {
                IntrospectValue::Text(ref name) => Ok(IntrospectValue::Bool(self.apply_key(name))),
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// ----------------------------------------------------------------------------
// View
// ----------------------------------------------------------------------------

fn list_style_fill(theme: &Theme, selected: bool, focused: bool) -> (Color, Color, Option<Border>) {
    if selected {
        let fill = theme
            .resolve(ColorRole::Surface)
            .lerp(theme.resolve(ColorRole::Accent), 0.18);
        let border = focused.then(|| Border::new(theme.resolve(ColorRole::Accent), 2));
        (fill, theme.resolve(ColorRole::OnSurface), border)
    } else {
        let border = focused.then(|| Border::new(theme.resolve(ColorRole::Accent), 2));
        (
            Color::TRANSPARENT,
            theme.resolve(ColorRole::OnSurface),
            border,
        )
    }
}

/// One list row: a clickable composite-tagged container with a checkbox glyph +
/// label. Tag `sel_list#<id>` routes a click to [`SelRowExternal`].
fn list_row(theme: &Theme, item: &SelItem, focused: bool) -> Scene {
    let (fill, fg, border) = list_style_fill(theme, item.selected, focused);
    let glyph = if item.selected {
        CHECK_MARK
    } else {
        CHECK_EMPTY
    };
    let mut box_style = BoxStyle::filled(fill).with_corner_radius(6);
    if let Some(b) = border {
        box_style = box_style.with_border(b);
    }
    let check = Scene::Text(TextNode::styled(
        glyph,
        Rect::default(),
        TextStyle::new()
            .with_size_px(18)
            .with_fg(theme.resolve(ColorRole::Accent)),
    ));
    let label = Scene::Text(TextNode::styled(
        item.label.clone(),
        Rect::default(),
        TextStyle::new().with_size_px(14).with_fg(fg),
    ));
    Scene::Container(
        ContainerNode::new(vec![check, label])
            .with_tag(format!("{LIST_TAG}#{}", item.id))
            .with_style(box_style)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(10)
                    .with_size(Size::auto().with_height(SizeValue::Px(ROW_H)))
                    .with_padding(Rect::new(12, 0, 12, 0)),
            ),
    )
}

/// A scene-as-data text line, tagged for RPC introspection.
fn data_line(theme: &Theme, tag: &'static str, text: String, size: u32, role: ColorRole) -> Scene {
    Scene::Text(
        TextNode::styled(
            text,
            Rect::default(),
            TextStyle::new()
                .with_size_px(size)
                .with_fg(theme.resolve(role)),
        )
        .with_tag(tag),
    )
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: SelState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let items = use_items().get();
    let total = items.len();
    let selected = selected_count(&items);
    let disabled = action_disabled(selected, total);

    // The contextual action toolbar (always visible; greyed = inoperable).
    let toolbar = view_toolbar(
        ACTIONS_TAG,
        &ACTION_LABELS,
        &[], // Command controls carry no pressed bit.
        &disabled,
        state.actions_focus,
        state.actions_focused,
        &theme,
        &ToolbarStyle::m3_default(),
    );

    // The selection count readout.
    let count_text = if total == 0 {
        "No items".to_string()
    } else if selected == 0 {
        format!("Nothing selected {DOT} {total} items")
    } else {
        format!("{selected} of {total} selected")
    };
    let count = data_line(&theme, COUNT_TAG, count_text, 15, ColorRole::OnSurface);

    // The list: focus ring on the roving row only while the list owns focus.
    let focus_pos = state.row_focus.min(total.saturating_sub(1));
    let rows: Vec<Scene> = items
        .iter()
        .enumerate()
        .map(|(pos, item)| list_row(&theme, item, state.list_focused && pos == focus_pos))
        .collect();
    let list = Scene::Container(
        ContainerNode::new(rows)
            .with_tag(LIST_TAG)
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerLow))
                    .with_corner_radius(8),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(2)
                    .with_flex_grow(1.0)
                    // R1020 §5.39 — the list is a single Tab stop (rows rove
                    // internally); opt its tag-carrying Container into the
                    // scene-derived enumeration.
                    .with_focusable(true)
                    .with_padding(Rect::new(6, 6, 6, 6)),
            ),
    );

    // The full selection state as scene-as-data for AI clients.
    let ids: Vec<String> = items
        .iter()
        .filter(|i| i.selected)
        .map(|i| i.id.to_string())
        .collect();
    let status = data_line(
        &theme,
        STATUS_TAG,
        format!(
            "selected={selected}/{total} {DOT} ids=[{}] {DOT} disabled=[del:{} clr:{} all:{}] {DOT} list_focus={focus_pos}",
            ids.join(","),
            disabled[ACTION_DELETE],
            disabled[ACTION_CLEAR],
            disabled[ACTION_SELECT_ALL],
        ),
        12,
        ColorRole::OnSurfaceMuted,
    );

    Scene::Container(
        ContainerNode::new(vec![toolbar, count, list, status])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(10)
                    .with_padding(Rect::new(14, 14, 14, 14)),
            ),
    )
}

// ----------------------------------------------------------------------------
// Binding
// ----------------------------------------------------------------------------

/// Copy projection of the two roving externals' focus posture (the list + the
/// action toolbar). The selection itself is read from [`use_items`] in the view
/// and a11y, not projected here.
#[derive(Copy, Clone, PartialEq, Debug, Default)]
struct SelState {
    row_focus: usize,
    list_focused: bool,
    actions_focus: usize,
    actions_focused: bool,
}

struct SelectionToolbarView;

impl WidgetCore for SelectionToolbarView {
    type State = SelState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(SelRowExternal::new(use_items()))
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![ExtraExternal::new(
            ACTIONS_TAG,
            Box::new(ToolbarExternal::new(vec![ToolItem::Command; ACTION_COUNT])),
        )]
    }

    fn tag() -> &'static str {
        LIST_TAG
    }

    /// Project both roving externals' focus + group-focus posture off the state
    /// scene (the list is the primary, the toolbar an extra — both reached by
    /// tag because the scene is a `Container([sel_list, actions])`).
    fn read_state(scene: &Scene) -> SelState {
        let mut out = SelState::default();
        // R990.1 — both roving externals decode through the lifted
        // `read_roving_focus` reader (the list mirrors the same focus/focused
        // slots as the toolbar, so the one reader serves both).
        if let Some(intro) = scene
            .find_external_with_tag(LIST_TAG)
            .and_then(|n| n.handle.introspect())
        {
            (out.row_focus, out.list_focused) = read_roving_focus(intro);
        }
        if let Some(intro) = scene
            .find_external_with_tag(ACTIONS_TAG)
            .and_then(|n| n.handle.introspect())
        {
            (out.actions_focus, out.actions_focused) = read_roving_focus(intro);
        }
        out
    }

    fn view(state: SelState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-selection-toolbar (R989 §5.38 §5.40 contextual action toolbar)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// Forward the focused key to whichever roving external owns shell focus
    /// (R804/R834 `forward_key_to_field`): the boot scene is a `Container`, so
    /// the default `forward_key_to_external` (bare-`External` root only) would
    /// not reach either widget.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: Modifiers,
    ) -> bool {
        let Some(tag) = focused else {
            return false;
        };
        if tag != LIST_TAG && tag != ACTIONS_TAG {
            return false;
        }
        pinion_core::input::forward_key_to_field(scene, tag, key, modifiers)
    }

    /// R51.173 §5.23 — the toolbar's `"command"` applies its bulk action to the
    /// shared holder (a reactive side effect, no `Command` emitted). The reducer
    /// is the operable barrier: it gates on the *same* reflective
    /// [`action_disabled`] mask the view paints, so a disabled action is a no-op
    /// however it is driven (the toolbar emits regardless — it cannot see the
    /// mask).
    fn update(_state: SelState, intent: &Intent) -> Vec<Command> {
        if intent.tag.as_ref() == COMMAND_INTENT_TAG_FULL {
            if let IntrospectValue::Text(idx) = &intent.payload {
                if let Ok(action) = idx.parse::<usize>() {
                    let items = use_items();
                    let snap = items.get();
                    let mask = action_disabled(selected_count(&snap), snap.len());
                    if mask.get(action) == Some(&false) {
                        apply_action(action, &items);
                    }
                }
            }
        }
        Vec::new()
    }

    /// Boot/state-change log line. Uses *only* the projected `state` — it is
    /// called outside an `Owner` scope (the boot `eprintln`), so it must not
    /// reach a reactive hook like [`use_items`].
    fn fmt_state_log(state: &SelState) -> String {
        format!(
            "list_focus={} list_focused={} actions_focus={} actions_focused={}",
            state.row_focus, state.list_focused, state.actions_focus, state.actions_focused
        )
    }
}

impl WidgetA11y for SelectionToolbarView {
    /// The forest: the action `toolbar` (each command carrying `aria-disabled`
    /// from the reflective mask) + the multi-selectable `listbox` (each row
    /// carrying `aria-selected`). Two independent roots.
    fn access_node(state: &SelState, focused: Option<&str>) -> Vec<AccessNode> {
        let items = use_items().get();
        let total = items.len();
        let disabled = action_disabled(selected_count(&items), total);

        // Toolbar nodes — reflective disabled lowered to aria-disabled.
        let action_tags: Vec<String> = (0..ACTION_COUNT)
            .map(|i| action_tag(ACTIONS_TAG, i))
            .collect();
        let controls: Vec<ToolbarControl<'_>> = (0..ACTION_COUNT)
            .map(|i| ToolbarControl {
                tag: &action_tags[i],
                name: Some(ACTION_LABELS[i]),
                checked: None,
                disabled: disabled[i],
            })
            .collect();
        let actions_focus = state.actions_focused.then_some(state.actions_focus);
        let mut nodes =
            toolbar_button_nodes(ACTIONS_TAG, "Selection actions", &controls, actions_focus);

        // Listbox nodes — multiselectable, each row aria-selected.
        let row_tags: Vec<String> = items
            .iter()
            .map(|i| format!("{LIST_TAG}#{}", i.id))
            .collect();
        let focus_pos = state.row_focus.min(total.saturating_sub(1));
        let list_focused = focused == Some(LIST_TAG);
        let options: Vec<ListOption<'_>> = items
            .iter()
            .enumerate()
            .map(|(pos, item)| ListOption {
                tag: &row_tags[pos],
                label: Some(item.label.as_str()),
                state: ListboxItemState::Idle,
                selected: item.selected,
                focused: list_focused && pos == focus_pos,
            })
            .collect();
        nodes.extend(listbox_option_nodes(
            LIST_TAG,
            "Selectable items",
            true,
            &options,
        ));
        nodes
    }

    /// Roving active descendant for whichever widget owns focus.
    fn access_focus_target(state: &SelState, focused: Option<&str>) -> Option<AccessFocus> {
        match focused {
            Some(LIST_TAG) => {
                let items = use_items().get();
                if items.is_empty() {
                    return Some(AccessFocus::atomic(LIST_TAG));
                }
                let pos = state.row_focus.min(items.len() - 1);
                Some(AccessFocus::composite(
                    LIST_TAG,
                    format!("{LIST_TAG}#{}", items[pos].id),
                ))
            }
            Some(ACTIONS_TAG) => Some(AccessFocus::composite(
                ACTIONS_TAG,
                action_tag(ACTIONS_TAG, state.actions_focus.min(ACTION_COUNT - 1)),
            )),
            other => other.map(AccessFocus::atomic),
        }
    }

    /// AT child activation: a row `Click` toggles it; a toolbar action `Click`
    /// runs it. `Focus` records the AT-side active descendant on either widget.
    fn access_child_invoke(
        scene: &mut Scene,
        parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        let dispatch_tag = match parent_tag {
            LIST_TAG => LIST_TAG,
            ACTIONS_TAG => ACTIONS_TAG,
            _ => return false,
        };
        let Some(intro) = scene
            .find_external_with_tag_mut(dispatch_tag)
            .and_then(|n| n.handle.introspect_mut())
        else {
            return false;
        };
        match action {
            AccessAction::Click | AccessAction::Default => {
                let _ = intro.invoke(
                    "send",
                    IntrospectValue::Text(format!("{sub_tag}:PointerUp")),
                );
                true
            }
            AccessAction::Focus => {
                if let Ok(i) = sub_tag.parse::<i64>() {
                    let _ = intro.intervene("focus", IntrospectValue::Int(i));
                }
                true
            }
            AccessAction::Increment | AccessAction::Decrement | AccessAction::Other => false,
        }
    }
}

impl WidgetView for SelectionToolbarView {
    type Renderer = HelloSelectionToolbarRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<SelectionToolbarView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;

    fn by_tag<'a>(nodes: &'a [AccessNode], tag: &str) -> &'a AccessNode {
        nodes.iter().find(|n| n.tag == tag).expect("node present")
    }

    // ----- the reflective disabled mask -----

    #[test]
    fn disabled_mask_tracks_selection() {
        // Nothing selected: Delete + Clear disabled, Select all operable.
        assert_eq!(action_disabled(0, 8), [true, true, false]);
        // Some selected: every action operable.
        assert_eq!(action_disabled(3, 8), [false, false, false]);
        // All selected: Delete + Clear operable, Select all disabled.
        assert_eq!(action_disabled(8, 8), [false, false, true]);
        // Empty dataset: nothing is operable.
        assert_eq!(action_disabled(0, 0), [true, true, true]);
    }

    // ----- the reducer gate + bulk effects -----

    fn cmd(action: usize) -> Intent {
        Intent::new_static("actions.command", IntrospectValue::Text(action.to_string()))
    }

    #[test]
    fn delete_removes_only_selected_rows() {
        Owner::new().run(|| {
            let items = use_items();
            items.set_with(|prev| {
                prev.iter()
                    .map(|i| SelItem {
                        selected: i.id == 2 || i.id == 5,
                        ..i.clone()
                    })
                    .collect()
            });
            let _ = SelectionToolbarView::update(SelState::default(), &cmd(ACTION_DELETE));
            let after = items.get();
            assert_eq!(after.len(), 6, "two selected rows deleted");
            assert!(
                !after.iter().any(|i| i.id == 2 || i.id == 5),
                "the selected ids are gone"
            );
            assert!(
                after.iter().any(|i| i.id == 1),
                "survivors keep their stable ids"
            );
        });
    }

    #[test]
    fn select_all_then_clear_round_trips() {
        Owner::new().run(|| {
            let items = use_items();
            let _ = SelectionToolbarView::update(SelState::default(), &cmd(ACTION_SELECT_ALL));
            assert_eq!(
                selected_count(&items.get()),
                8,
                "select all selects every row"
            );
            let _ = SelectionToolbarView::update(SelState::default(), &cmd(ACTION_CLEAR));
            assert_eq!(selected_count(&items.get()), 0, "clear deselects every row");
        });
    }

    #[test]
    fn disabled_action_is_a_no_op_even_via_intent() {
        Owner::new().run(|| {
            let items = use_items();
            // Nothing selected -> Delete is disabled; the reducer must gate it.
            let before = items.get().len();
            let _ = SelectionToolbarView::update(SelState::default(), &cmd(ACTION_DELETE));
            assert_eq!(items.get().len(), before, "disabled Delete deletes nothing");
            // ... and Clear is a no-op too (already empty selection).
            let _ = SelectionToolbarView::update(SelState::default(), &cmd(ACTION_CLEAR));
            assert_eq!(selected_count(&items.get()), 0);
            // All selected -> Select all is disabled and must not error / change.
            let _ = SelectionToolbarView::update(SelState::default(), &cmd(ACTION_SELECT_ALL));
            let _ = SelectionToolbarView::update(SelState::default(), &cmd(ACTION_SELECT_ALL));
            assert_eq!(
                selected_count(&items.get()),
                8,
                "select all is idempotent + then gated"
            );
        });
    }

    // ----- the row external -----

    #[test]
    fn row_send_toggles_membership() {
        Owner::new().run(|| {
            let mut ext = SelRowExternal::new(use_items());
            assert_eq!(
                ext.invoke("send", IntrospectValue::Text("3:PointerUp".into())),
                Ok(IntrospectValue::Bool(true))
            );
            assert_eq!(ext.query("selected_count"), Some(IntrospectValue::Int(1)));
            // A second release toggles it back off.
            let _ = ext.invoke("send", IntrospectValue::Text("3:PointerUp".into()));
            assert_eq!(ext.query("selected_count"), Some(IntrospectValue::Int(0)));
            // A stale id is a no-op reported as Bool(false).
            assert_eq!(
                ext.invoke("send", IntrospectValue::Text("999:PointerUp".into())),
                Ok(IntrospectValue::Bool(false))
            );
        });
    }

    #[test]
    fn r990_1_selected_intervene_is_an_absolute_idempotent_set() {
        Owner::new().run(|| {
            let mut ext = SelRowExternal::new(use_items());
            // Absolute set (the write mirror of the `selected.<i>` read).
            ext.intervene("selected.1", IntrospectValue::Bool(true))
                .unwrap();
            assert_eq!(
                ext.query("selected.1"),
                Some(IntrospectValue::Bool(true)),
                "read mirrors write"
            );
            // Idempotent: setting true again leaves it set (not a toggle).
            ext.intervene("selected.1", IntrospectValue::Bool(true))
                .unwrap();
            assert_eq!(
                ext.query("selected_count"),
                Some(IntrospectValue::Int(1)),
                "set is not a toggle"
            );
            ext.intervene("selected.1", IntrospectValue::Bool(false))
                .unwrap();
            assert_eq!(ext.query("selected_count"), Some(IntrospectValue::Int(0)));
            // Out-of-range / type errors are reported, not silently dropped.
            assert_eq!(
                ext.intervene("selected.99", IntrospectValue::Bool(true)),
                Err(InterveneError::OutOfRange)
            );
            assert_eq!(
                ext.intervene("selected.0", IntrospectValue::Int(1)),
                Err(InterveneError::TypeMismatch)
            );
        });
    }

    #[test]
    fn keyboard_roves_and_toggles() {
        Owner::new().run(|| {
            let mut ext = SelRowExternal::new(use_items());
            assert!(ext.apply_key("ArrowDown"));
            assert!(ext.apply_key("ArrowDown"));
            assert_eq!(ext.query("focus"), Some(IntrospectValue::Int(2)));
            assert!(ext.apply_key("Space"), "Space toggles the focused row");
            assert_eq!(ext.query("selected_count"), Some(IntrospectValue::Int(1)));
            assert_eq!(ext.query("selected.2"), Some(IntrospectValue::Bool(true)));
            // Home / End clamp, no wrap.
            assert!(ext.apply_key("End"));
            assert_eq!(ext.query("focus"), Some(IntrospectValue::Int(7)));
            assert!(ext.apply_key("ArrowDown"));
            assert_eq!(
                ext.query("focus"),
                Some(IntrospectValue::Int(7)),
                "no wrap past the last row"
            );
        });
    }

    #[test]
    fn focus_clamps_after_delete() {
        Owner::new().run(|| {
            let mut ext = SelRowExternal::new(use_items());
            let _ = ext.apply_key("End"); // focus the last row (7)
            // Select + delete several rows so the list shrinks under the cursor.
            ext.items
                .set_with(|prev| prev.iter().filter(|i| i.id <= 3).cloned().collect());
            assert_eq!(ext.query("count"), Some(IntrospectValue::Int(3)));
            assert_eq!(
                ext.query("focus"),
                Some(IntrospectValue::Int(2)),
                "stale focus clamps to the new last row"
            );
        });
    }

    // ----- a11y forest: toolbar (aria-disabled) + listbox (aria-selected) -----

    #[test]
    fn a11y_toolbar_disabled_reflects_empty_selection() {
        let nodes = Owner::new()
            .run(|| SelectionToolbarView::access_node(&SelState::default(), Some(ACTIONS_TAG)));
        let toolbar = by_tag(&nodes, ACTIONS_TAG);
        assert_eq!(toolbar.role, AriaRole::Toolbar);
        assert!(
            by_tag(&nodes, &action_tag(ACTIONS_TAG, ACTION_DELETE))
                .state
                .disabled,
            "Delete disabled when empty"
        );
        assert!(
            by_tag(&nodes, &action_tag(ACTIONS_TAG, ACTION_CLEAR))
                .state
                .disabled,
            "Clear disabled when empty"
        );
        assert!(
            !by_tag(&nodes, &action_tag(ACTIONS_TAG, ACTION_SELECT_ALL))
                .state
                .disabled,
            "Select all operable"
        );
    }

    #[test]
    fn a11y_listbox_marks_selected_rows() {
        let nodes = Owner::new().run(|| {
            let items = use_items();
            items.set_with(|prev| {
                prev.iter()
                    .map(|i| SelItem {
                        selected: i.id == 4,
                        ..i.clone()
                    })
                    .collect()
            });
            SelectionToolbarView::access_node(&SelState::default(), Some(LIST_TAG))
        });
        let list = by_tag(&nodes, LIST_TAG);
        assert_eq!(list.role, AriaRole::Listbox, "the list is a listbox root");
        assert_eq!(list.children.len(), 8, "one option per row");
        let selected: Vec<&str> = nodes
            .iter()
            .filter(|n| n.role == AriaRole::ListBoxOption && n.selected == Some(true))
            .map(|n| n.tag.as_str())
            .collect();
        assert_eq!(
            selected,
            vec!["sel_list#4"],
            "exactly the selected row is aria-selected"
        );
    }

    #[test]
    fn a11y_disabled_clears_once_a_row_is_selected() {
        let nodes = Owner::new().run(|| {
            let items = use_items();
            items.set_with(|prev| {
                prev.iter()
                    .map(|i| SelItem {
                        selected: i.id == 1,
                        ..i.clone()
                    })
                    .collect()
            });
            SelectionToolbarView::access_node(&SelState::default(), Some(ACTIONS_TAG))
        });
        assert!(
            !by_tag(&nodes, &action_tag(ACTIONS_TAG, ACTION_DELETE))
                .state
                .disabled,
            "Delete operable with a selection"
        );
    }

    #[test]
    fn a11y_focus_target_follows_the_focused_widget() {
        Owner::new().run(|| {
            let list = SelectionToolbarView::access_focus_target(
                &SelState {
                    row_focus: 2,
                    list_focused: true,
                    ..SelState::default()
                },
                Some(LIST_TAG),
            )
            .expect("list focused");
            assert_eq!(list.active_descendant.as_deref(), Some("sel_list#3"));
            let actions = SelectionToolbarView::access_focus_target(
                &SelState {
                    actions_focus: 1,
                    actions_focused: true,
                    ..SelState::default()
                },
                Some(ACTIONS_TAG),
            )
            .expect("toolbar focused");
            assert_eq!(actions.active_descendant.as_deref(), Some("actions#1"));
        });
    }
}
