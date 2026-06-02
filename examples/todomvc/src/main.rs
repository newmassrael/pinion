//! `todomvc` §5.16 — first composed multi-widget application
//! verifying pinion's AI-native composition primitives end-to-end.
//!
//! Phase-2 cascade ([[r652-substrate-roi-matrix]]):
//! R655 scaffold • R656 stable ids + delete + ARIA •
//! R657 `view_field` lift • R658 toggle + scroll • R659 filter +
//! scrollbar peer • R660 Option β walk-back (`RadioGroupExternal` +
//! M3 state-layers + scene/drag) • R661 doc compression •
//! R662 edit in-place • R663 persistence.
//!
//! State shape: `(TextFieldState, u32, FilterRadioStates, TextFieldState, u32)` — Copy
//! per R51.173. Non-Copy reactive stores reach the binding through
//! `Owner::cache` hooks ([`use_todos`] / [`use_filter`] /
//! [`use_text_edit_state`] / [`use_scroll_state`] /
//! [`use_scrollbar_interaction`]).
//!
//! Multi-External composition (R55.D.5) — primary `TextField` plus
//! four `ExtraExternal` siblings: `TodoDeleteExternal` (R656),
//! `TodoToggleExternal` (R658), `RadioGroupExternal` filter group
//! (R660), `ScrollBarExternal` (R659). Paint-side composite-tag wire
//! (`<primary>#<sub>`) routes per-item events to the matching
//! handler; the handlers share the [`parse_send_payload`] helper
//! (`composite_tag` 5-of-5 substrate, R660).
//!
//! `cargo run --release -p todomvc`. Tab cycles textfield ↔ filter
//! group. Type + Enter to submit; click × to delete; click toggle
//! glyph to flip completed; arrow / Home / End / Space to drive the
//! filter via keyboard; drag the scrollbar thumb to scroll. `d` / `e`
//! toggle the field's Disabled state.

use std::cell::Cell;
use std::rc::Rc;

use pinion_core::clipboard::{Clipboard, InMemoryClipboard};
use pinion_core::storage::{InMemoryStorage, Storage};
use pinion_platform_clipboard::ArboardClipboard;
use pinion_platform_storage::{open_app_storage, FileStorage};
// R659 §5.16 §5.35 — lifted composite-tag parser shared by every
// `<key>:<EventName>` send-payload consumer
// (TodoDeleteExternal / TodoToggleExternal / TodoFilterExternal —
// 3-of-3 Rule of Three fired per [[abstraction-needs-second-consumer]]).
use pinion_core::composite_tag::parse_send_payload;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, IntrospectSchema,
    IntrospectValue, InterveneError, InvokeError, RepaintOwner, ThreadOwnership,
};
use pinion_core::reactive::{Effect, Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, ScrollNode, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::scroll::use_scroll_state;
// R659 §5.45 — ScrollBarExternal sibling registered through
// `create_extra_externals` so the visible scrollbar peer becomes
// drag-able (4th ExtraExternal after delete + toggle + filter).
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::radio_group::RadioGroupExternal;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::{TextFieldEvent, TextFieldExternal, TextFieldState};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::{Color, Frame, Scene, WidgetCore, WidgetStateName};
use pinion_a11y::{AccessAction, AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_text::CaretRect;
// R657 §5.16 §5.38 — lifted TextField paint substrate shared with
// hello-textfield (2nd consumer reached per
// [[abstraction-needs-second-consumer]]).
use pinion_widget_paint::text_field as tf_paint;
// R659 §5.45 — lifted vertical scrollbar peer substrate shared with
// hello-listbox (2nd consumer reached). One M3 role mapping
// (SurfaceContainerHighest track + Outline thumb), one closed-form
// thumb geometry, one absolute-position composition across both
// bindings.
use pinion_widget_paint::scrollbar::{view_vertical_scrollbar, VerticalScrollbarStyle};

// pinion-forge codegen output. Defines `pub struct TodoMvcRenderer`
// + async `new<W: Into<wgpu::SurfaceTarget<'static>>>` + sync
// `render(&vello::Scene, peniko::Color)` + sync `resize(u32, u32)`.
use pinion_widget_paint::state_layer::{HOVER, PRESSED};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so the generic `AppShell<V>` can
// construct + render + resize it.
vello_renderer_impl!(TodoMvcRenderer, TodoMvcRendererError);

/// Tag for the textfield widget. Matches the `WidgetCore::tag`
/// (paint-root + input-router hit-test target) and the
/// [`use_text_edit_state`] / [`use_caret_blink`] cache keys, so the
/// `create_external` factory and the view fn both resolve to the same
/// reactive `Rc<TextEditState>` + `Rc<CaretBlink>` instances.
const TF_TAG: &str = "main_textfield";

/// (R655 §5.16) Tag for the todo list `Scene::Container` so RPC
/// `scene/query` can introspect the list shape independently of the
/// textfield via the existing `Scene::find_*` walkers.
const LIST_TAG: &str = "todo_list";

/// (R655 §5.16) [`Owner::cache`] key for the reactive
/// `Signal<Vec<TodoItem>>` carrying the todo entries. Symmetric with
/// the [`TF_TAG`] / [`use_text_edit_state`] convention — the
/// [`apply_key`] handler, the view fn, and [`TodoDeleteExternal`]
/// (R656) all resolve through this key, so the same
/// `Rc<Signal<Vec<TodoItem>>>` instance is shared and reactive
/// subscriptions land in the same store. R656 swapped the inner
/// element type from `String` to [`TodoItem`] (id + text) for stable
/// per-item identity under deletes.
const TODOS_KEY: &str = "todomvc.todos";

/// (R656 §5.16) [`Owner::cache`] key for the monotonic
/// `Rc<Cell<u64>>` counter that [`use_next_todo_id`] increments to
/// allocate the `id` field of each fresh [`TodoItem`]. Separate from
/// [`TODOS_KEY`] so the counter does not subscribe view-fn renders
/// to its mutations (a `Cell<u64>` is not reactive — only the
/// `Signal<Vec<TodoItem>>` it feeds is).
const NEXT_ID_KEY: &str = "todomvc.next_id";

/// (R656 §5.16) Singleton paint + state tag for the per-item delete
/// button handler. The paint scene emits one
/// `Scene::Container { tag: "todo_delete#<id>" }` per todo entry,
/// and the state scene holds exactly one [`TodoDeleteExternal`]
/// registered via [`create_extra_externals`] under the primary tag
/// `"todo_delete"`. The R51.42 §5.35 composite-tag wire splits the
/// paint tag on `#` so the [`InputRouter`] resolves the primary
/// against the state scene and forwards the sub-index `<id>` to
/// the External through the `invoke("send", "{id}:{Event}")`
/// channel — the canonical pinion-native pattern
/// [`RadioGroupExternal`] established at R51.43 and reused here at
/// the application tier without any new framework substrate.
const DELETE_TAG: &str = "todo_delete";

/// (R656 §5.16) Per-item paint tag prefix for the row container.
/// Full row tag is `"todo_item#<id>"` (stable u64 id, NOT array
/// index — R656 corrects the R655 index-based tagging that would
/// alias `todo_item#1` to a different logical row after a sibling
/// delete). Listed as a constant so RPC verify demos + tests
/// reference one source of truth.
const ITEM_TAG_PREFIX: &str = "todo_item";

/// (R656 §5.16) Per-item delete-button paint tag prefix. Full
/// per-item tag is `"todo_delete#<id>"`; the `#` separator triggers
/// the R51.42 composite-tag split so the [`InputRouter`] routes the
/// click into [`TodoDeleteExternal`] (registered under primary tag
/// [`DELETE_TAG`]).
const DELETE_TAG_PREFIX: &str = "todo_delete";

/// (R658 §5.16) Singleton paint + state tag for the per-item toggle
/// (completed flag) handler. Mirrors [`DELETE_TAG`] — registered via
/// [`WidgetCore::create_extra_externals`] as the 2nd sibling
/// [`ExtraExternal`] under primary tag `"todo_toggle"`. The
/// per-row paint tag is `"todo_toggle#<id>"`; the R51.42 §5.35
/// composite-tag wire splits on `#` so the [`InputRouter`] resolves
/// the primary against [`TodoToggleExternal`] and forwards the
/// sub-index `<id>` as part of `invoke("send", "{id}:{Event}")`.
/// 2nd consumer of the multi-External `create_extra_externals`
/// substrate ([[multi-external-substrate-extra-externals-pattern]],
/// R55.D.5 listbox + scrollbar was 1st, R658 todomvc toggle + delete
/// is 2nd).
const TOGGLE_TAG: &str = "todo_toggle";

/// (R658 §5.16) Per-item toggle-button paint tag prefix. Full
/// per-item tag is `"todo_toggle#<id>"`. Mirrors [`DELETE_TAG_PREFIX`].
const TOGGLE_TAG_PREFIX: &str = "todo_toggle";

/// (R658 §5.16) Width of the per-item toggle button. Matches the
/// 24×24 WCAG 2.5.5 AAA target-size recommendation [`DELETE_BUTTON_W`]
/// uses for the destructive affordance, so the two side buttons sit
/// symmetrical at the row edges.
const TOGGLE_BUTTON_W: u32 = 24;
/// (R658 §5.16) Height of the per-item toggle button. Square hit
/// target, symmetric with [`DELETE_BUTTON_H`].
const TOGGLE_BUTTON_H: u32 = 24;
/// (R658 §5.16) Font size for the `☐` / `☑` toggle glyphs. 18 px to
/// match the destructive [`DELETE_FONT_SIZE_PX`] glyph weight so the
/// two row affordances read with equal visual prominence.
const TOGGLE_FONT_SIZE_PX: u32 = 18;

/// (R658 §5.16) Unchecked toggle glyph (Unicode `BALLOT BOX`,
/// U+2610). Pulled into a named const + `\u{...}` escape per
/// `[[non-ascii-literal-named-const-escape]]` so the source file
/// stays ASCII-only.
const TOGGLE_GLYPH_UNCHECKED: &str = "\u{2610}";

/// (R658 §5.16) Checked toggle glyph (Unicode
/// `BALLOT BOX WITH CHECK`, U+2611). Single-codepoint canonical for
/// "completed todo" used by the `TasteJS` `TodoMVC` HTML reference,
/// by `macOS` Reminders, and by GitHub Markdown task lists — Unicode
/// parley/swash shaping draws this without needing a custom SVG
/// asset.
const TOGGLE_GLYPH_CHECKED: &str = "\u{2611}";

/// (R659/R660 §5.16) Filter group tags + sizing.
const FILTER_TAG: &str = "todo_filter";
const FILTER_TAG_PREFIX: &str = "todo_filter";
const SCROLLBAR_TAG: &str = "todo_scrollbar";
const FILTER_BUTTON_W: u32 = 92;
const FILTER_BUTTON_H: u32 = 32; // WCAG 2.5.5 AAA min 24, +8 desktop comfort
const FILTER_FONT_SIZE_PX: u32 = 13;
const FILTER_BUTTON_GAP: u32 = 6;
const FILTER_KEY: &str = "todomvc.filter";

/// (R660 §5.16) Dotted wire-form intent tag the R660 [`RadioGroupExternal`]
/// emits on every selection-change. [`TodoMvcView::update`] matches
/// this exact string; [[intent-tag-dotted-wire-form]].
const TODO_FILTER_SELECTED_INTENT_TAG: &str =
    pinion_core::intent_tag!("todo_filter", "selected");

/// (R664 §5.16 §5.38) Per-row in-place edit affordance — shared
/// `TextField` tag for the *single* row in edit mode. Only one row is
/// editable at a time ([`TasteJS`] `TodoMVC` canonical), so a single
/// [`Owner::cache`] slot keyed by this static tag carries the editor's
/// [`TextEditState`] / [`CaretBlink`] pair — the row swap moves the
/// editor instance instead of allocating one per row. Mirror of
/// [`TF_TAG`] for the main "add todo" field above; the editor is a
/// distinct `TextField` so the two fields' caret + blink + selection
/// state stay independent across the focus transitions
/// (mouse press on the main field while editing must not nuke the
/// in-progress edit — separate cache slots achieve that with the
/// existing `Owner::cache` per-`(TypeId, key)` slot model
/// per [[owner-cache-typed-key]]).
const EDIT_TF_TAG: &str = "todo_edit";

/// (R664 §5.16) Primary tag for the [`TodoEditExternal`] sibling
/// that handles double-click activations on `todo_item#<id>` rows.
/// The composite-tag wire splits incoming `"<id>:DoubleClick"`
/// payloads — mirror of [`TodoToggleExternal`]'s shape under
/// [`TOGGLE_TAG`].
const ITEM_TAG: &str = "todo_item";

/// (R664 §5.16) [`Owner::cache`] key for the
/// `Rc<Signal<Option<u64>>>` carrying the *id of the row currently
/// in edit mode* (`None` = no row editing). The
/// [`build_todos_list`] swap, the [`TodoEditExternal::invoke`]
/// activation arm, the [`apply_key_edit`] commit / cancel handlers,
/// and the [`access_node`] AT-side mode reporting all resolve
/// through this signal — single source of truth, equality-skip
/// suppresses no-op writes (a `set(Some(7))` while already editing
/// row 7 does not re-render).
const EDITING_ID_KEY: &str = "todomvc.editing_id";

/// (R665 §3 §5.15) Per-application directory name for
/// [`pinion_platform_storage::open_app_storage`]. Resolves to:
///
/// - Linux: `$XDG_DATA_HOME/pinion-todomvc/` (defaults to
///   `~/.local/share/pinion-todomvc/`);
/// - macOS: `~/Library/Application Support/pinion-todomvc/`;
/// - Windows: `%APPDATA%\pinion-todomvc\`.
///
/// The `PINION_STORAGE_DIR` env var overrides this resolution
/// wholesale — useful for the R665 RPC verify demo which points
/// persistence at a tempdir to keep the developer's real data
/// directory pristine across runs.
const STORAGE_APP_NAME: &str = "pinion-todomvc";

/// (R665 §3 §5.15) Storage key under which the dehydrated
/// [`PersistedState`] (todos + filter + next id) is written. Single
/// blob (vs three keys) so the atomic rename invariant covers the
/// entire persistence transaction — a partially-restored read where
/// `todos` survived but `next_id` got reset would re-allocate
/// retired ids, breaking the R656 stable-id contract.
const STORAGE_STATE_KEY: &str = "todomvc.state";

/// (R665 §3 §5.15) Env var override for the persistence root
/// directory. When set, [`build_app_storage`] uses the value as-is
/// (creating intermediate directories) instead of consulting
/// `dirs::data_dir()`. Used by the R665 RPC verify demo so the
/// host's real data dir stays out of the test cycle.
const STORAGE_DIR_ENV: &str = "PINION_STORAGE_DIR";

/// (R682.B §5.16) Env var that, when present + parseable as a
/// non-negative integer ≤ [`SEED_N_MAX`], seeds the freshly-booted
/// binding with N synthetic [`TodoItem`] rows during
/// [`use_persistence_boot`]. The seed only fires when the `todos`
/// signal is empty after hydration (so persisted user data is never
/// clobbered); subsequent boots from the same on-disk state skip the
/// seed because the hydrated list is non-empty.
///
/// Used by the R682.B `tools/demos/r682_dirty_subtree_cache.py` demo
/// to drive the 100-row stress consumer that exercises the §5.16
/// paint-fragment cache substrate (per-Container `paint_hash` keyed
/// fragment storage + mark-and-sweep eviction + damage region
/// propagation). The 2nd-consumer trigger per
/// [[abstraction-needs-second-consumer]] ratifies the
/// [`pinion_runtime::FragmentCacheStats`] observability surface
/// landed in R682.A.
const SEED_N_ENV: &str = "PINION_TODOMVC_SEED_N";

/// Hard upper bound on the seed env var so a typo (`100000000`) does
/// not lock the binding boot for minutes. Reasonable maximum for a
/// dirty-cache stress consumer; the demo uses 100 by default. R682.B
/// authoritative — bump only on the next round that genuinely needs a
/// higher stress size.
const SEED_N_MAX: u32 = 10_000;

/// Completion split for the seeded rows: every 3rd entry is born
/// completed (i % 3 == 0). Deterministic so the R682.B demo can
/// assert the filter-driven cache eviction count without enumerating
/// the seed list. With `N=100` the split is 34 completed / 66 active
/// (the 0-indexed positions 0, 3, 6, …, 99 are completed).
const SEED_COMPLETION_STRIDE: u64 = 3;

/// (R665 §3 §5.15) Schema version stamped onto every
/// [`PersistedState`] blob. Bump on every breaking field-shape
/// change; the load path treats mismatched versions as "start
/// fresh" (silent fall-through, no migration today — additive
/// schema changes can land as `serde(default)` on new fields
/// without bumping the version).
const PERSISTED_SCHEMA_VERSION: u32 = 1;

/// (R659 §5.16) Canonical `TasteJS` `TodoMVC` filter modes. Discriminants
/// 0/1/2 match the visual left-to-right order + composite-tag wire
/// (`todo_filter#<i>`). `#[non_exhaustive]` keeps future additions
/// (Archived / Starred) additive.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FilterMode {
    Active,
    Completed,
    All,
}

impl FilterMode {
    /// Boot default — show everything (`TasteJS` + `macOS` Reminders).
    #[must_use]
    pub const fn default_mode() -> Self {
        Self::All
    }

    #[must_use]
    pub const fn from_index(idx: usize) -> Option<Self> {
        match idx {
            0 => Some(Self::Active),
            1 => Some(Self::Completed),
            2 => Some(Self::All),
            _ => None,
        }
    }

    #[must_use]
    pub const fn to_index(self) -> usize {
        match self {
            Self::Active => 0,
            Self::Completed => 1,
            Self::All => 2,
        }
    }

    /// Predicate driving the visible-list derivation in [`build_todos_list`].
    #[must_use]
    pub const fn matches(self, item: &TodoItem) -> bool {
        match self {
            Self::Active => !item.completed,
            Self::Completed => item.completed,
            Self::All => true,
        }
    }

    /// Single-source label — view fn + a11y emitter both read this.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Completed => "Completed",
            Self::All => "All",
        }
    }
}

/// (R660 §5.16) Per-paint snapshot of the [`RadioGroupExternal`] driving
/// the filter row. [`TodoMvcView::read_state`] pulls it through
/// [`read_filter_radio_states`] so the view fn paints M3 state-layer
/// overlays without re-borrowing the framework-owned external.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FilterRadioStates {
    /// Per-radio interaction tag, indexed by [`FilterMode::to_index`].
    pub states: [RadioState; 3],
    /// AT-side active descendant (R51.87). `None` falls back to selected.
    pub focused: Option<usize>,
}

impl Default for FilterRadioStates {
    /// All-`Idle` seed before `create_extra_externals` lands. Forge-
    /// generated `RadioState` lacks `Default`, so the const-array
    /// literal carries the seed explicitly.
    fn default() -> Self {
        Self {
            states: [RadioState::Idle; 3],
            focused: None,
        }
    }
}

/// `apply_key` `TF_TAG` arm: R655 Enter → submit + clear, else delegate
/// to [`TextFieldExternal::invoke`]`("key", Text(key))`. Trim guard
/// per `TasteJS` — blank entries never land.
fn apply_key_textfield(scene: &mut Scene, key: &str) -> bool {
    if key == "Enter" {
        let text_state = use_text_edit_state(TF_TAG);
        let raw = text_state.text();
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let entry_text = trimmed.to_owned();
            let id = allocate_todo_id();
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id,
                    text: entry_text.clone(),
                    completed: false,
                });
                next
            });
            text_state.set_text(String::new());
        }
        return true;
    }
    let Some(node) = scene.find_external_with_tag_mut(TF_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    match intro.invoke("key", IntrospectValue::Text(key.to_owned())) {
        Ok(IntrospectValue::Bool(handled)) => handled,
        _ => false,
    }
}

/// `apply_key` `FILTER_TAG` arm — W3C ARIA Authoring Practices
/// "radiogroup" pattern. ArrowLeft/Right cycle (activate on every
/// step — no navigate-without-commit; AT-only `Focus` action lives
/// on `access_child_invoke`, R660+ carry); Home/End jump to first /
/// last; Space/Enter activate the addressed row (idempotent).
/// Activation runs the full PointerEnter/Down/Up/Leave composite-tag
/// wire so the framework's state machine + intent emission matches
/// what a paint-side mouse click produces.
fn apply_key_filter(scene: &mut Scene, key: &str) -> bool {
    let current_idx = filter_focused_index(scene).unwrap_or_else(|| {
        Owner::current().map_or(0, |_| use_filter().get().to_index())
    });
    let count: usize = 3;
    let target_idx: usize = match key {
        "ArrowLeft" => current_idx.checked_sub(1).unwrap_or(count - 1),
        "ArrowRight" => {
            if current_idx + 1 >= count {
                0
            } else {
                current_idx + 1
            }
        }
        "Home" => 0,
        "End" => count - 1,
        "Space" | "Enter" => current_idx,
        _ => return false,
    };
    let Some(node) = scene.find_external_with_tag_mut(FILTER_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
        let _ = intro.invoke(
            "send",
            IntrospectValue::Text(format!("{target_idx}:{ev}")),
        );
    }
    true
}

/// (R664 §5.16 §5.38) `apply_key` `EDIT_TF_TAG` arm — in-place edit
/// keymap mirror of [`apply_key_textfield`] over the per-row editor.
///
/// | key      | action                                                      |
/// |----------|-------------------------------------------------------------|
/// | `Enter`  | Commit the editor text to the matching `TodoItem` row, clear `editing_id`, restore focus to [`TF_TAG`]. Empty / blank text deletes the row (`TasteJS` convention — the user erased everything). |
/// | `Escape` | Cancel — clear `editing_id` without mutating the row, restore focus to [`TF_TAG`]. The editor's [`TextEditState`] is reset so the next edit starts blank. |
/// | other    | Delegate to [`TextFieldExternal::invoke`]`("key", …)` so the embedded `TextField` SCXML drives caret motion, character insertion, selection extension, clipboard, IME — the canonical R56.1 keymap reused unchanged. |
///
/// Focus restoration uses [`pinion_core::focus_request`] so the
/// substrate drains a single transition through `notify_focus_change`
/// (`TextField`'s `on_focus_change(false)` fires on the editor, then
/// the main field's `on_focus_change(true)` fires) — observers see
/// the same arc a mouse-driven blur would emit.
fn apply_key_edit(scene: &mut Scene, key: &str) -> bool {
    match key {
        "Enter" => {
            commit_edit(scene);
            true
        }
        "Escape" => {
            cancel_edit();
            true
        }
        _ => {
            let Some(node) = scene.find_external_with_tag_mut(EDIT_TF_TAG) else {
                return false;
            };
            let Some(intro) = node.handle.introspect_mut() else {
                return false;
            };
            match intro.invoke("key", IntrospectValue::Text(key.to_owned())) {
                Ok(IntrospectValue::Bool(handled)) => handled,
                _ => false,
            }
        }
    }
}

/// (R664 §5.16) Commit the in-flight edit. Reads the editor's current
/// text + the active `editing_id`, writes the trimmed text to the
/// matching [`TodoItem`] (or removes the row when the trimmed text is
/// empty — `TasteJS` "erase to delete" convention), then clears the
/// edit state + restores focus to the main input field.
///
/// Idempotent under the equality-skip `Signal::set` path: a commit
/// that does not change the text yields a no-op `set_with` (the
/// `Vec<TodoItem>` clone with the same text compares equal).
fn commit_edit(_scene: &mut Scene) {
    let Some(target_id) = use_editing_id().get() else {
        return;
    };
    let editor = use_text_edit_state(EDIT_TF_TAG);
    let raw = editor.text();
    let trimmed = raw.trim();
    let todos = use_todos();
    if trimmed.is_empty() {
        // R664 §5.16 — `TasteJS` `TodoMVC` "erase row to delete"
        // canonical: clearing the inline editor and committing
        // removes the row.
        todos.set_with(|prev| {
            let mut next = prev.clone();
            next.retain(|item| item.id != target_id);
            next
        });
    } else {
        let new_text = trimmed.to_owned();
        todos.set_with(|prev| {
            prev.iter()
                .map(|item| {
                    if item.id == target_id {
                        TodoItem {
                            text: new_text.clone(),
                            ..item.clone()
                        }
                    } else {
                        item.clone()
                    }
                })
                .collect()
        });
    }
    end_edit_mode();
}

/// (R664 §5.16) Cancel the in-flight edit. Drops the editor's
/// text + clears the edit state without writing back; restores focus
/// to the main input field so the user's next keystroke lands in
/// the "add todo" channel.
fn cancel_edit() {
    end_edit_mode();
}

/// (R664 §5.16) Shared finish-edit teardown — clears [`editing_id`],
/// wipes the editor's [`TextEditState`] so the next edit starts
/// blank, and queues a focus return to [`TF_TAG`] via
/// [`pinion_core::focus_request`]. Used by both the `Enter` commit
/// and `Escape` cancel paths.
fn end_edit_mode() {
    use_editing_id().set(None);
    use_text_edit_state(EDIT_TF_TAG).set_text(String::new());
    pinion_core::focus_request::request(TF_TAG);
}

/// Read the [`RadioGroupExternal`] `focused_index` slot. `None`
/// falls back to selected index in [`apply_key_filter`].
fn filter_focused_index(scene: &Scene) -> Option<usize> {
    let node = scene.find_external_with_tag(FILTER_TAG)?;
    let intro = node.handle.introspect()?;
    match intro.query("focused_index") {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
        _ => None,
    }
}

/// Walk the multi-External scene for the filter group + pull per-radio
/// `state.<i>` slots + `focused_index`. Default snapshot pre-registration.
fn read_filter_radio_states(scene: &Scene) -> FilterRadioStates {
    let Some(node) = scene.find_external_with_tag(FILTER_TAG) else {
        return FilterRadioStates::default();
    };
    let Some(intro) = node.handle.introspect() else {
        return FilterRadioStates::default();
    };
    let mut states = [RadioState::Idle; 3];
    for (i, slot) in states.iter_mut().enumerate() {
        if let Some(IntrospectValue::Text(name)) = intro.query(&format!("state.{i}")) {
            *slot = RadioState::from_name_or_default(&name);
        }
    }
    let focused = match intro.query("focused_index") {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
        _ => None,
    };
    FilterRadioStates { states, focused }
}

/// `Owner::cache`-keyed hook returning the shared
/// `Rc<Signal<FilterMode>>`. One Rc across view fn (auto-subscribe via
/// `.get()`), `access_node` (reads current mode), and the R660
/// `TodoMvcView::update` reducer (writes on `"selected"` intent).
///
/// # Panics
///
/// Panics outside an `Owner::run(...)` scope. Framework dispatch wraps
/// every runtime invocation; only test forgetfulness hits this.
#[must_use]
pub fn use_filter() -> Rc<Signal<FilterMode>> {
    Owner::current()
        .expect("use_filter requires an active Owner scope")
        .cache(FILTER_KEY, || Signal::new(FilterMode::default_mode()))
}

/// (R664 §5.16) `Owner::cache`-keyed hook returning the shared
/// `Rc<Signal<Option<u64>>>` carrying the *id of the row currently in
/// edit mode*. `None` = no row editing (steady state).
///
/// Single source of truth across:
/// - [`TodoEditExternal`] activation arm — sets `Some(id)` on
///   double-click;
/// - [`build_todos_list`] — swaps the matching row's text label for
///   an inline [`view_field`] editor when the signal equals the row's
///   id;
/// - [`apply_key_edit`] — commits (`Enter`), cancels (`Escape`), or
///   delegates to [`TextFieldExternal`] (typing / arrow keys) when
///   `Some(_)`;
/// - [`access_node`] — surfaces the edit-mode row through the W3C
///   ARIA `aria-multiline=false` text-input role so AT clients see
///   the editor announce as a labelled field.
///
/// # Panics
///
/// Panics outside an `Owner::run(...)` scope (mirrors [`use_todos`]).
#[must_use]
pub fn use_editing_id() -> Rc<Signal<Option<u64>>> {
    Owner::current()
        .expect("use_editing_id requires an active Owner scope")
        .cache(EDITING_ID_KEY, || Signal::new(None::<u64>))
}

/// (R658 §5.16) [`Owner::cache`] key for the reactive [`ScrollState`]
/// owning the todo list's vertical scroll offset + max-y bound.
/// Mirror of `hello-listbox`'s `SCROLL_KEY = "main_list_scroll"`
/// shape — the [`use_scroll_state`] hook resolves through this key
/// inside the view fn, and the runtime's [`compute_layout`] pass
/// writes the laid-out max-y back through the same `Rc<ScrollState>`
/// on the next frame.
const LIST_SCROLL_KEY: &str = "todomvc.list_scroll";

/// (R658 §5.16) Vertical viewport height for the todo list scroll
/// container. Sized so 5 fully-visible rows fit (each ~30 px tall at
/// 14 px font + 6 px gap → 36 px row total × 5 = 180 px) plus the
/// "Todos (N)" header (~24 px) plus a few pixels of breathing
/// room — the 6th-onward row scrolls beneath the visible window edge
/// via wheel / Arrow keys / future scrollbar drag (R55.D.4 substrate
/// inherited via [`use_scroll_state`], reused per
/// [[abstraction-needs-second-consumer]]).
const LIST_VIEWPORT_H: u32 = 220;
/// (R658 §5.16) Horizontal viewport width — same width as the text
/// input field above so the list and the field visually align (the
/// `tf_paint::view_field` substrate paints a 360-px-wide field, so
/// 360 px gives a flush column).
const LIST_VIEWPORT_W: u32 = 360;

/// (R656 §5.16) Width of the per-item delete button in logical
/// pixels. Sized to comfortably host the `MULTIPLICATION_SIGN` (×,
/// U+00D7) glyph at [`DELETE_FONT_SIZE_PX`] with a few pixels of
/// padding on each side; large enough to satisfy the WCAG 2.5.5
/// "target size (minimum)" 24×24 CSS px recommendation for AAA.
const DELETE_BUTTON_W: u32 = 24;
/// (R656 §5.16) Height of the per-item delete button. Matches
/// [`DELETE_BUTTON_W`] for a square hit target — the most
/// forgiving shape for touch and mouse alike.
const DELETE_BUTTON_H: u32 = 24;
/// (R656 §5.16) Font size for the `×` delete glyph. 18 px gives the
/// glyph the same visual weight as the entry text below it without
/// dominating the row.
const DELETE_FONT_SIZE_PX: u32 = 18;

/// (R656 §5.16) WAI-AA WCAG 1.4.6 contrast-compliant tinting of
/// the `×` delete glyph. Resolves through [`ColorRole::Error`]
/// (R590 §5.50) so the destructive affordance reads with the same
/// hue the rest of the M3 palette reserves for error state — the
/// glyph is visually identified as "danger / destructive" without
/// the row needing a separate "are you sure?" confirmation step
/// (the per-item delete is a single-click destructive action by
/// `TasteJS` `TodoMVC` convention).
fn delete_glyph_color(theme: &Theme) -> Color {
    theme.resolve(ColorRole::Error)
}

/// (R656 §5.16) The visible `×` glyph itself (Unicode
/// `MULTIPLICATION_SIGN`, U+00D7). Pulled into a named constant
/// + `\u{...}` escape per `[[non-ascii-literal-named-const-escape]]`
///   so the source file stays ASCII-only.
const DELETE_GLYPH: &str = "\u{00D7}";

const WIN_W: u32 = 480;
// R655 §5.16 — tall enough to host the textfield section (title +
// field + status ≈ 120 px) plus the [`LIST_VIEWPORT_H`] todo list
// region, mirroring the macOS / iOS Reminders-app vertical rhythm.
// R658 §5.16 — the list region is now a [`ScrollNode`] of fixed
// viewport height [`LIST_VIEWPORT_H`]; rows past the visible window
// scroll via wheel / Arrow / future scrollbar drag rather than
// causing the outer window flex to overflow. The WIN_H constant
// (R655 carry magic) is therefore now `title_section + gap +
// LIST_VIEWPORT_H + padding` rather than an unbounded growth budget.
const WIN_H: u32 = 480;

/// (R57.X.textfield §5.50) [`ThemeProvider`] cache key. Matches the
/// `"app"` convention shared with `hello-toggle` / `hello-theme` /
/// `hello-listbox` so the example gallery shares one provider when a
/// host binds them together.
const THEME_TAG: &str = "app";

/// (R656 §5.16) Single todo entry — stable monotonic `id` allocated
/// by [`use_next_todo_id`] plus the user-typed `text` from the
/// input field at submit time. `Clone` so [`Signal::set_with`]
/// closures can construct the next `Vec<TodoItem>` snapshot without
/// touching the original (the framework's reactive equality-skip
/// path needs the previous snapshot intact for the cheap-`Eq`
/// compare). `PartialEq` so the `set_with` equality check ever
/// fires; `Debug` for the `Vec<TodoItem>` `Debug` chain the
/// `fmt_state_log` path walks for stderr.
///
/// The `id` field is `u64` rather than `usize` so the wire-form
/// `"todo_item#<id>"` paint tag and the
/// `invoke("send", "{id}:PointerDown")` payload stay
/// architecture-independent — every desktop/wasm/embedded backend
/// agrees on the integer representation. `u64::MAX` is a
/// practically unbounded id space (an app that adds one todo per
/// nanosecond for ~580 years would still not exhaust it), so the
/// counter never needs reuse — every deleted item's id stays
/// retired, and the AI client / RPC verify scripts can safely
/// remember per-id state across the lifetime of the process.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TodoItem {
    /// Stable monotonic identifier — allocated once at submit time
    /// by [`use_next_todo_id`] and never reused. Survives sibling
    /// deletes (per R656 stable-id contract): if items
    /// `[#1, #2, #3]` exist and `#2` is deleted, the surviving
    /// items keep tags `todo_item#1` + `todo_item#3` (NOT
    /// resequenced to `#0` + `#1`).
    pub id: u64,
    /// User-typed entry text. Trimmed at submit time so
    /// leading/trailing whitespace doesn't leak into the rendered
    /// list (the trim guard in `apply_key` also drops blank-only
    /// submissions per the `TasteJS` `TodoMVC` spec).
    pub text: String,
    /// (R658 §5.16) Completion flag. Defaults to `false` on submit;
    /// flipped by [`TodoToggleExternal`] in response to a click on
    /// the per-row `todo_toggle#<id>` button (paint-side composite-
    /// tag wire) OR the direct `invoke("toggle", Int(id))` RPC
    /// route. When `true`, the per-row paint substitutes the
    /// `☐` (U+2610) glyph with `☑` (U+2611) and the entry text
    /// renders through [`ColorRole::OnSurfaceMuted`] to signal
    /// "done" without obstructing readability (no strikethrough
    /// today — text-decoration is a R663+ framework primitive
    /// candidate per [[abstraction-needs-second-consumer]]).
    pub completed: bool,
}

/// (R655/R656 §5.16) `Owner::cache`-keyed hook returning the shared
/// `Rc<Signal<Vec<TodoItem>>>` of submitted todo entries. Symmetric
/// with [`use_text_edit_state`] / [`use_caret_blink`] hook shape —
/// the [`Owner::cache`] dedup guarantees one `Rc` across the view fn
/// (subscribes via `.get()` to re-run paint on submit/delete),
/// [`apply_key`] (mutates via `.set_with(|v| v.push(...))` on
/// Enter), [`create_extra_externals`] (constructs the singleton
/// [`TodoDeleteExternal`] with a clone of this `Rc`), and the
/// `access_node` hook (walks the current snapshot to emit one
/// `AccessNode` per item under the list root). Single-source-of-
/// truth for the todo list state; the signal's equality-skip
/// suppresses the re-run when a no-op `set_with` would not change
/// the snapshot.
///
/// R656 §5.16 — element type swapped from `String` to [`TodoItem`]
/// (id + text) for stable per-item identity under deletes.
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope. Every
/// runtime invocation site (view fn, `apply_key`,
/// `create_extra_externals`, `access_node`) is wrapped by the
/// substrate's `root_owner.run`, so this only fires under unit
/// tests that forget the `with_owner` helper.
#[must_use]
pub fn use_todos() -> Rc<Signal<Vec<TodoItem>>> {
    Owner::current()
        .expect("use_todos requires an active Owner scope")
        .cache(TODOS_KEY, || Signal::new(Vec::<TodoItem>::new()))
}

/// (R656 §5.16) `Owner::cache`-keyed hook returning a monotonic
/// `Rc<Cell<u64>>` counter. The Enter-key submit handler invokes
/// [`Cell::get`] + [`Cell::set`] to allocate a fresh id for each
/// new [`TodoItem`]; the counter starts at `1` so deleted-then-
/// reused-tag-name confusion is impossible (id `0` is reserved as
/// a sentinel for "no item" by convention, though no current code
/// path relies on it — defensive carry).
///
/// Returns an `Rc<Cell<u64>>` (not just `u64`) so the call site
/// can both *read* the current counter (for tests, mostly) and
/// *advance* it through one shared instance. The hook is not
/// reactive — `Cell<u64>` does not implement the [`Signal`]
/// substrate, by design: id allocation is a one-way side effect
/// of the Enter handler, and view fns must not subscribe to it
/// (they re-render on `Signal<Vec<TodoItem>>` updates instead,
/// which already carries the freshly-allocated id inside the new
/// element).
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope (same
/// shape as [`use_todos`] — only test paths can trigger).
#[must_use]
pub fn use_next_todo_id() -> Rc<Cell<u64>> {
    Owner::current()
        .expect("use_next_todo_id requires an active Owner scope")
        .cache(NEXT_ID_KEY, || Cell::new(1_u64))
}

/// (R656 §5.16) Allocate and return the next fresh `id`. Convenience
/// wrapper over [`use_next_todo_id`] — fetches the counter, reads
/// the current value, advances by 1, and returns the pre-advance
/// snapshot so the caller can stamp it onto a fresh [`TodoItem`].
/// Saturating-add at `u64::MAX` is technically a defensive guard
/// (a single process would have to allocate ~580 years of
/// nanosecond-spaced ids to hit it) but explicit for textbook
/// integer-overflow hygiene.
#[must_use]
pub fn allocate_todo_id() -> u64 {
    let counter = use_next_todo_id();
    let current = counter.get();
    counter.set(current.saturating_add(1));
    current
}

/// (R655 §5.16) Title color for the todo list section header — same
/// `OnSurface` role the textfield title above resolves through, so
/// the two labels read with identical weight against the surface
/// fill. Lifted out of the view fn body so the symbol shows up in
/// tests + grep audits as a single source.
const LIST_TITLE_FONT_SIZE_PX: u32 = 14;

/// (R655 §5.16) Per-item font size for the rendered todo text —
/// matches the textfield status line (12 px) so the visual rhythm
/// reads as "compact secondary content under the active input".
const LIST_ITEM_FONT_SIZE_PX: u32 = 14;

/// (R655 §5.16) Vertical gap between list items, smaller than the
/// section-level [`ROW_GAP`] so items pack tighter than the
/// title/field/status rows. 6 px ≈ 0.5× the row gap.
const LIST_ITEM_GAP: u32 = 6;

// R657 §5.16 §5.38 — TextField paint sizing + alpha tuning live in
// `tf_paint::TextFieldStyle::m3_filled` in pinion-widget-paint.
// The binding uses the M3 default for the input field.

/// Title label font size (kept binding-local — it's part of the
/// surrounding chrome composition, not the field substrate).
const TITLE_FONT_SIZE_PX: u32 = 18;
const STATUS_FONT_SIZE_PX: u32 = 12;

// Gap between title / field / status line in the root column flex —
// matches the macOS / iOS settings-pane vertical rhythm (~16 px
// between related controls).
const ROW_GAP: u32 = 16;

// R657 §5.16 §5.38 — use_layout_cache lifted to
// `pinion_widget_paint::text_field::use_text_field_layout_cache`
// (private impl detail of `view_field` + `ime_caret_rect_for`).

/// R56.1.e §5.22 / R56.2.b §5.22 — `Owner::cache`-keyed clipboard
/// hook. Prefers [`ArboardClipboard`] (Wayland / X11 / macOS / Windows
/// canonical platform backend); falls back to [`InMemoryClipboard`]
/// on init failure (headless CI, sandboxed). [`AppClipboard`] is the
/// `Sized` wrapper the `Owner::cache<V>` slot stores.
fn use_clipboard(key: &'static str) -> Rc<dyn Clipboard> {
    let cb: Rc<AppClipboard> = Owner::current()
        .expect("use_clipboard requires an active Owner scope")
        .cache(key, || {
            AppClipboard(match ArboardClipboard::try_new() {
                Ok(arboard) => Box::new(arboard) as Box<dyn Clipboard>,
                Err(e) => {
                    eprintln!(
                        "hello-textfield: ArboardClipboard init failed \
                         ({e}); falling back to InMemoryClipboard \
                         (cross-process clipboard disabled)",
                    );
                    Box::new(InMemoryClipboard::new()) as Box<dyn Clipboard>
                }
            })
        });
    cb
}

/// Sized newtype around `Box<dyn Clipboard>` — `Owner::cache<V>` slot
/// requires a single concrete `V`, so the runtime impl choice
/// (arboard vs in-memory) hides inside the box.
struct AppClipboard(Box<dyn Clipboard>);

impl Clipboard for AppClipboard {
    fn copy(&self, text: String) {
        self.0.copy(text);
    }
    fn paste(&self) -> Option<String> {
        self.0.paste()
    }
}

/// (R665 §3 §5.15) Sized newtype around `Box<dyn Storage>` — the
/// `Owner::cache<V>` slot requires a single concrete `V`, so the
/// runtime impl choice (`FileStorage` / [`InMemoryStorage`]) hides inside
/// the box. Mirror of [`AppClipboard`] (R56.1.e §5.22 / R56.2.b §5.22)
/// — the boxed-dyn newtype pattern is the canonical
/// [[owner-cache-typed-key]] shape for platform substrate hooks.
struct AppStorage(Box<dyn Storage>);

impl Storage for AppStorage {
    fn load(&self, key: &str) -> Option<Vec<u8>> {
        self.0.load(key)
    }
    fn save(&self, key: &str, bytes: &[u8]) {
        self.0.save(key, bytes);
    }
    fn remove(&self, key: &str) {
        self.0.remove(key);
    }
}

/// (R665 §3 §5.15) Build the platform storage handle once per
/// process. Priority:
///
/// 1. [`STORAGE_DIR_ENV`] env var, if set — points at a custom root
///    (used by the R665 RPC verify demo's tempdir). Creates the dir
///    via [`FileStorage::try_new`] (idempotent).
/// 2. [`pinion_platform_storage::open_app_storage`] under
///    [`STORAGE_APP_NAME`] — the canonical per-OS data dir.
/// 3. [`InMemoryStorage`] fall-back so headless CI / sandboxed
///    containers still wire up (persistence becomes session-local,
///    every other code path is unchanged).
fn build_app_storage() -> Box<dyn Storage> {
    if let Ok(custom_dir) = std::env::var(STORAGE_DIR_ENV) {
        match FileStorage::try_new(std::path::PathBuf::from(&custom_dir)) {
            Ok(file) => return Box::new(file) as Box<dyn Storage>,
            Err(e) => eprintln!(
                "todomvc: FileStorage init at {custom_dir:?} via {STORAGE_DIR_ENV} failed \
                 ({e}); falling back to default app data dir",
            ),
        }
    }
    match open_app_storage(STORAGE_APP_NAME) {
        Ok(file) => Box::new(file) as Box<dyn Storage>,
        Err(e) => {
            eprintln!(
                "todomvc: open_app_storage({STORAGE_APP_NAME:?}) failed ({e}); \
                 persistence will be in-memory only (state lost on exit)",
            );
            Box::new(InMemoryStorage::new()) as Box<dyn Storage>
        }
    }
}

/// (R665 §3 §5.15) `Owner::cache`-keyed storage hook. First call
/// allocates the platform handle via [`build_app_storage`]; every
/// subsequent call returns the same `Rc<AppStorage>` so the entire
/// binding shares one storage instance (atomic-write contract holds
/// across consumers).
fn use_storage() -> Rc<AppStorage> {
    Owner::current()
        .expect("use_storage requires an active Owner scope")
        .cache("todomvc.storage", || AppStorage(build_app_storage()))
}

/// (R665 §3 §5.15) Dehydrated todomvc state. Single blob written to
/// [`STORAGE_STATE_KEY`] so the [`FileStorage`] tempfile + rename
/// guarantees cover the whole persistence transaction (no torn read
/// where `todos` survived but `next_id` reverted).
///
/// `schema_version` is checked on load — mismatches start fresh
/// (silent fall-through to defaults). Additive field changes can
/// land via `#[serde(default)]` without bumping the version;
/// removal / type changes bump [`PERSISTED_SCHEMA_VERSION`].
///
/// `editing_id` is intentionally **not** persisted. Per the
/// `TasteJS` `TodoMVC` reference, edit mode is transient UI state
/// scoped to a single user session — relaunching with a row still
/// "in edit mode" would be surprising. Same rationale as why
/// `next_id` is persisted but the current text in the new-todo
/// `TextField` is not.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
struct PersistedState {
    schema_version: u32,
    todos: Vec<TodoItem>,
    filter: FilterMode,
    next_id: u64,
}

impl PersistedState {
    /// Snapshot the current reactive store into a serializable blob.
    /// Called by the save Effect on every observed `todos` / `filter`
    /// change. The `next_id` read is non-reactive but always happens
    /// *after* a `todos.set_with` in [`allocate_todo_id`] +
    /// `push` — so the Effect's `todos.get()` subscription guarantees
    /// the snapshot sees the freshest next-id alongside the todos.
    fn snapshot(
        todos: &Signal<Vec<TodoItem>>,
        filter: &Signal<FilterMode>,
        next_id: &Cell<u64>,
    ) -> Self {
        Self {
            schema_version: PERSISTED_SCHEMA_VERSION,
            todos: todos.get(),
            filter: filter.get(),
            next_id: next_id.get(),
        }
    }
}

/// (R665 §3 §5.15) Owns the persistence save [`Effect`] for the
/// application's lifetime. `Owner::cache` returns `Rc<Self>` so the
/// Effect handle is retained inside the cached slot — the Effect's
/// `Rc<EffectInner>` is the only strong reference (the `Owner`
/// cleanup queue holds only a `Weak`, R37.5 #2 leak fix), so
/// dropping the handle on factory exit would GC the Effect and
/// silently stop save fires. Caching this struct around the handle
/// pins the Effect for as long as the framework's `root_owner`
/// holds the cache slot.
struct PersistenceBootMarker {
    /// The save Effect — subscribes to `use_todos` + `use_filter` +
    /// reads `use_next_todo_id` (non-reactive Cell, follows the
    /// todos sub on every push). Underscore-prefixed because the
    /// field is never read directly; its lifetime is the value.
    _save_effect: Effect,
}

/// (R665 §3 §5.15) Run the persistence boot pass exactly once per
/// `Owner`. First call:
///
/// 1. resolves the storage handle via [`use_storage`];
/// 2. attempts to load [`STORAGE_STATE_KEY`] — on a hit the
///    [`PersistedState`] is deserialized and used to seed the
///    `Signal<Vec<TodoItem>>` / `Signal<FilterMode>` / `Cell<u64>`
///    reactive stores (in that order — Signals first so the save
///    Effect's eager initial pass sees the hydrated values);
/// 3. installs an [`Effect`] that subscribes to the two Signals and
///    writes a freshly-serialized [`PersistedState`] back to storage
///    on every change. The Effect's owner is the framework's
///    `root_owner`, so the Effect stays alive for the entire
///    application lifetime (no manual cancellation needed).
///
/// Subsequent calls (e.g. a unit test that re-enters the same
/// `Owner`) are no-ops — the cache dedup guarantees one boot pass
/// per `Owner::cache` slot.
///
/// # Panics
///
/// Panics outside an `Owner::run(...)` scope (same shape as
/// [`use_todos`]).
/// R682.B §5.16 — parse [`SEED_N_ENV`] into the requested seed
/// count. Returns `Some(n)` only when the env var is present, parses
/// as a non-negative integer, and falls within `[1, SEED_N_MAX]`.
/// Returns `None` for unset / empty / `"0"` / non-numeric / out-of-
/// range values so the seed block is a strict no-op when no stress
/// scenario was requested.
///
/// Public-via-`pub(crate)` so the R682.B test module can drive the
/// parser without invoking the full `use_persistence_boot` arc.
pub(crate) fn parse_seed_n_env(value: Option<&str>) -> Option<u32> {
    let raw = value?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parsed: u32 = trimmed.parse().ok()?;
    if parsed == 0 || parsed > SEED_N_MAX {
        return None;
    }
    Some(parsed)
}

/// R682.B §5.16 — pure builder for the seed row at index `i`.
///
/// `next_id` is the stable id allocated for this row (the caller
/// advances `next_id` by `n` after `seed_rows(n)` so the existing
/// monotonic-counter contract holds). Completion is deterministic
/// (`i % SEED_COMPLETION_STRIDE == 0`) so demo assertions on the
/// filter-driven cache eviction count are stable.
///
/// `pub(crate)` so the R682.B test module can pin the deterministic
/// completion pattern without re-implementing it.
pub(crate) fn seed_todo_row(i: u32, next_id: u64) -> TodoItem {
    let completed = (u64::from(i) % SEED_COMPLETION_STRIDE) == 0;
    TodoItem {
        id: next_id,
        text: format!("Seed task {}", i + 1),
        completed,
    }
}

/// R682.B §5.16 — build a `Vec<TodoItem>` of the requested length
/// using [`seed_todo_row`]. Ids start at `start_id` and run
/// monotonically (`start_id`, `start_id+1`, … `start_id+n-1`); the
/// caller then advances [`use_next_todo_id`] by `n` so subsequent
/// row inserts pick up where the seed left off.
///
/// `pub(crate)` so tests can verify the row count + id range +
/// completion split without going through `use_persistence_boot`.
pub(crate) fn build_seed_rows(n: u32, start_id: u64) -> Vec<TodoItem> {
    (0..n).map(|i| seed_todo_row(i, start_id + u64::from(i))).collect()
}

fn use_persistence_boot() -> Rc<PersistenceBootMarker> {
    // Resolve all dependent `Owner::cache` slots BEFORE entering the
    // boot slot. `Owner::cache` holds a `RefCell` mutable borrow on
    // its inner map for the duration of the factory closure; nested
    // `Owner::cache` calls (e.g. `use_storage` inside the factory)
    // would re-enter `borrow_mut` and panic. Pre-resolving here puts
    // every dependent cache hit ahead of the outer borrow.
    let storage = use_storage();
    let todos = use_todos();
    let filter = use_filter();
    let next_id_cell = use_next_todo_id();
    let owner = Owner::current().expect("use_persistence_boot requires an active Owner scope");
    let owner_for_effect = owner.clone();

    owner
        .cache("todomvc.persistence.boot", move || {
            // (1) Hydrate from disk. Load misses + schema mismatches
            // silently fall through to defaults (the Signals stay at
            // their `Owner::cache` factory seeds).
            if let Some(bytes) = storage.load(STORAGE_STATE_KEY) {
                match serde_json::from_slice::<PersistedState>(&bytes) {
                    Ok(state) if state.schema_version == PERSISTED_SCHEMA_VERSION => {
                        // Seed in single batch so Effect subscribers
                        // observe one atomic update instead of three
                        // partial writes ([[signal-batch-atomic-multi-axis-update]]).
                        // The Effect itself is installed AFTER this
                        // hydration block so the batch is observable
                        // only by *future* Effects (currently none).
                        pinion_core::reactive::batch(|| {
                            todos.set(state.todos);
                            filter.set(state.filter);
                            next_id_cell.set(state.next_id);
                        });
                    }
                    Ok(state) => {
                        eprintln!(
                            "todomvc: persisted schema {} ≠ supported {} \
                             — starting fresh (saved state will be overwritten)",
                            state.schema_version, PERSISTED_SCHEMA_VERSION,
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "todomvc: persisted state at {STORAGE_STATE_KEY:?} \
                             failed to deserialize ({e}) — starting fresh",
                        );
                    }
                }
            }

            // (1.5) R682.B §5.16 — paint-fragment cache stress seed.
            // When [`SEED_N_ENV`] is set + parses + `todos` is still
            // empty post-hydration, seed N synthetic TodoItem rows
            // through the same batch arc the hydration path uses so
            // a single atomic Signal mutation lands. The
            // `todos.get().is_empty()` gate guarantees the seed
            // never clobbers user data: real persisted state always
            // hydrates non-empty, so the seed becomes a no-op as
            // soon as the binding has been used once. The 2nd
            // consumer of [`pinion_runtime::FragmentCacheStats`]
            // per [[abstraction-needs-second-consumer]].
            //
            // env-var check fires first so the steady-state (no
            // env) path never reads the signal during boot.
            if let Some(n) = parse_seed_n_env(std::env::var(SEED_N_ENV).ok().as_deref()) {
                if todos.get().is_empty() {
                    let start_id = next_id_cell.get();
                    let rows = build_seed_rows(n, start_id);
                    let new_next_id = start_id + u64::from(n);
                    pinion_core::reactive::batch(|| {
                        todos.set(rows);
                        next_id_cell.set(new_next_id);
                    });
                }
            }

            // (2) Install the save Effect. Eager initial run also
            // writes — idempotent on a freshly-hydrated state (the
            // written bytes equal the loaded bytes), so one extra
            // fsync at boot is the entire cost. The Effect's owner
            // is `Owner::current()` (the framework's root_owner) so
            // the Effect stays alive for the application lifetime.
            let storage_e = storage.clone();
            let todos_e = todos.clone();
            let filter_e = filter.clone();
            let next_id_e = next_id_cell.clone();
            let save_effect = Effect::new(&owner_for_effect, move || {
                let snapshot = PersistedState::snapshot(&todos_e, &filter_e, &next_id_e);
                match serde_json::to_vec(&snapshot) {
                    Ok(bytes) => storage_e.save(STORAGE_STATE_KEY, &bytes),
                    Err(e) => eprintln!(
                        "todomvc: persistence serialize failed ({e}); \
                         this write was skipped",
                    ),
                }
            });

            PersistenceBootMarker { _save_effect: save_effect }
        })
}

// R657 §5.16 §5.38 — saturating_f32_to_u32 lifted to
// `pinion_widget_paint::text_field` (private helper used by
// `view_field` + `ime_caret_rect_for`).

/// §6.3 view-fn — sync `(state, frame) -> Scene`. Reactive reads
/// (`use_text_edit_state` / `use_caret_blink` / `use_todos` /
/// `use_filter` / `use_scroll_state` / `use_scrollbar_interaction`)
/// auto-subscribe; "purity" holds modulo the shared reactive stores.
///
/// Top-to-bottom: title, textfield, status line, segmented filter
/// row, scroll region (todo list + scrollbar peer side-by-side).
#[allow(
    clippy::trivially_copy_pass_by_ref,
    clippy::too_many_lines,
    reason = "one paint cycle composing 5 sections"
)]
fn view(state: (TextFieldState, u32, FilterRadioStates, TextFieldState, u32), _frame: &Frame) -> Scene {
    let (interaction, caret_byte, filter_radios, edit_interaction, edit_caret_byte) = state;

    // R657 §5.16 §5.38 — TextField paint composition lifted to
    // `tf_paint::view_field`. The binding's view fn composes only
    // its surrounding chrome (title + status + todo list +
    // delete-X) around the lifted field substrate.
    //
    // (R57.X.textfield §5.50) Active palette — `use_theme` auto-
    // R57.X.theme-fade — auto-subscribed cross-fade palette ([R586 §5.50]).
    let theme = use_theme(THEME_TAG).theme_animated();

    let field = tf_paint::view_field(
        TF_TAG,
        interaction,
        caret_byte,
        &theme,
        &tf_paint::TextFieldStyle::m3_filled(),
        "Text input",
    );

    let text_state = use_text_edit_state(TF_TAG);
    let text = text_state.text();
    let preedit = text_state.preedit();

    let title = Scene::Text(TextNode::styled(
        "TextField",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_SIZE_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    // R56.1.g.3 §5.22 — status line mirrors the `scene/query` preedit
    // slot so IME composition lifecycle is visible + AI-introspectable.
    let preedit_status = match preedit.as_ref() {
        Some(p) => format!(" | preedit=\"{p}\""),
        None => String::new(),
    };
    let status_str = format!(
        "{} | caret={} | text=\"{}\"{}",
        interaction.as_name(),
        caret_byte,
        text,
        preedit_status,
    );
    let status = Scene::Text(TextNode::styled(
        status_str,
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_SIZE_PX)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    let todos = use_todos();
    let entries: Vec<TodoItem> = todos.get();
    let filter_mode = use_filter().get();

    // Derived visible list — pure O(n) filter. No `Computed` memoization
    // because the per-paint cost is negligible vs the subscription
    // bookkeeping; revisit if a 1000-entry stress test surfaces.
    let visible: Vec<TodoItem> = entries
        .iter()
        .filter(|item| filter_mode.matches(item))
        .cloned()
        .collect();

    let filter_row = build_filter_row(&theme, filter_mode, &filter_radios);
    // R664 §5.16 — auto-subscribe the editing_id signal so the
    // double-click row swap re-renders on activation + commit + cancel.
    let editing_id = use_editing_id().get();
    let todos_list_content = build_todos_list(
        &theme,
        &visible,
        filter_mode,
        entries.len(),
        editing_id,
        edit_interaction,
        edit_caret_byte,
    );

    let scroll_state = use_scroll_state(LIST_SCROLL_KEY);
    let todos_scroll = Scene::Scroll(ScrollNode::from_state(
        scroll_state.clone(),
        Rect::new(0, 0, LIST_VIEWPORT_W, LIST_VIEWPORT_H),
        todos_list_content,
    ));

    // R659/R660 visible scrollbar peer — auto-subscribes the
    // interaction-state mirror so hover/drag re-paint the M3 thumb
    // overlay. Paired write lands in `create_extra_externals` via
    // `.attach_interaction(...)`.
    let scrollbar_style = VerticalScrollbarStyle::material(LIST_VIEWPORT_H, SCROLLBAR_TAG);
    let scrollbar_interaction = use_scrollbar_interaction(SCROLLBAR_TAG);
    let scrollbar_visual = view_vertical_scrollbar(
        &scroll_state,
        &theme,
        &scrollbar_style,
        scrollbar_interaction.get(),
    );

    let scroll_region = Scene::Container(
        ContainerNode::new(vec![todos_scroll, scrollbar_visual])
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    );

    Scene::Container(
        ContainerNode::new(vec![title, field, status, filter_row, scroll_region])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(ROW_GAP),
            ),
    )
}

/// (R659/R660 §5.16) M3 segmented button row (Active / Completed /
/// All). Active segment = Accent fill; inactive = transparent +
/// Outline border. R660 state-layer overlay lerps fill toward
/// `OnSurface` (Hover 0.08, Pressed 0.12) per
/// [[m3-surface-tier-expansion]]. Single-source labels through
/// [`FilterMode::label`] keep screen-reader + visible text in sync.
fn build_filter_row(
    theme: &Theme,
    current: FilterMode,
    radios: &FilterRadioStates,
) -> Scene {
    let modes = [FilterMode::Active, FilterMode::Completed, FilterMode::All];
    let on_accent = theme.resolve(ColorRole::OnAccent);
    let accent = theme.resolve(ColorRole::Accent);
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let outline = theme.resolve(ColorRole::Outline);
    let transparent = Color::rgba(0, 0, 0, 0);

    let buttons: Vec<Scene> = modes
        .iter()
        .map(|&mode| {
            let idx = mode.to_index();
            let active = mode == current;
            let radio_state = radios.states.get(idx).copied().unwrap_or(RadioState::Idle);
            let (mut fill, label_color, border) = if active {
                (accent, on_accent, None)
            } else {
                (transparent, on_surface, Some(Border::new(outline, 1)))
            };
            fill = match radio_state {
                RadioState::Hover => fill.lerp(on_surface, HOVER),
                RadioState::Pressed => fill.lerp(on_surface, PRESSED),
                RadioState::Disabled | RadioState::Idle => fill,
            };
            let mut style = BoxStyle::filled(fill).with_corner_radius(4);
            if let Some(b) = border {
                style = style.with_border(b);
            }
            let label = Scene::Text(TextNode::styled(
                mode.label(),
                Rect::default(),
                TextStyle::new()
                    .with_size_px(FILTER_FONT_SIZE_PX)
                    .with_fg(label_color),
            ));
            let button_tag = format!("{FILTER_TAG_PREFIX}#{idx}");
            Scene::Container(
                ContainerNode::new(vec![label])
                    .with_tag(button_tag)
                    .with_style(style)
                    .with_aria_label(mode.label())
                    .with_layout(
                        LayoutStyle::new()
                            .flex(FlexDirection::Row)
                            .with_justify(JustifyContent::Center)
                            .with_align_items(AlignItems::Center)
                            .with_size(Size::px(FILTER_BUTTON_W, FILTER_BUTTON_H)),
                    ),
            )
        })
        .collect();

    Scene::Container(
        ContainerNode::new(buttons)
            .with_tag(FILTER_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(FILTER_BUTTON_GAP),
            ),
    )
}

/// (R656 §5.16) Per-item delete handler. Registered via
/// [`WidgetCore::create_extra_externals`] under [`DELETE_TAG`] — the
/// R51.42 composite-tag wire forwards `"<id>:PointerDown"` payloads
/// from `todo_delete#<id>` clicks. `delete_by_id` retains the
/// `Signal<Vec<TodoItem>>` minus the matched id; the framework's
/// reactive equality-skip suppresses the no-op case (already deleted).
///
/// Synchronous-pure delete — bypasses the §5.23 R27 `update` reducer
/// `Command` channel so the next paint cycle observes the mutation
/// directly (the standard pinion convention for paint-side hit events,
/// mirroring hello-listbox's per-row selection wire).
///
/// SSOT note: `hello-input-chip`'s `ChipDeleteExternal` (R756) is the 2nd
/// composite-delete-over-`Signal<Vec>` consumer of this shape. The shared
/// core (retain-by-id + count/ids/delete schema + `parse_send_payload` send)
/// is a `CompositeCollection<T: HasId>` lift candidate, deliberately deferred
/// to the 3rd consumer — see that type's doc for the rationale + commit-policy
/// seam ([[abstraction-needs-second-consumer]]).
#[derive(Debug)]
pub struct TodoDeleteExternal {
    todos: Rc<Signal<Vec<TodoItem>>>,
}

impl TodoDeleteExternal {
    #[must_use]
    pub fn new(todos: Rc<Signal<Vec<TodoItem>>>) -> Self {
        Self { todos }
    }

    fn delete_by_id(&self, target_id: u64) {
        self.todos.set_with(|prev| {
            let mut next = prev.clone();
            next.retain(|item| item.id != target_id);
            next
        });
    }
}

impl External for TodoDeleteExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(
            &[Backend::Gui, Backend::Tui, Backend::Rpc],
            BackendFallback::Skip,
        )
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
}

impl ExternalIntrospect for TodoDeleteExternal {
    /// `send` = R51.42 composite-tag wire (`<id>:PointerDown` deletes;
    /// other phases accepted-as-no-op). `delete` = direct typed-id
    /// shortcut (RPC AI-driving path); returns `Bool(was_present)`
    /// so callers distinguish "deleted existing" from "no-op stale id".
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("count", "int"),
            ("ids", "json"),
            ("send", "string"),
            ("delete", "int"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "count" => {
                let n = self.todos.get().len();
                Some(IntrospectValue::Int(
                    i64::try_from(n).expect("todo count must fit in i64"),
                ))
            }
            "ids" => {
                let snapshot = self.todos.get();
                let arr: Vec<serde_json::Value> = snapshot
                    .iter()
                    .map(|item| serde_json::Value::from(item.id))
                    .collect();
                Some(IntrospectValue::Json(serde_json::Value::Array(arr)))
            }
            _ => None,
        }
    }

    fn intervene(
        &mut self,
        path: &str,
        _value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        match path {
            "count" | "ids" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // R656: only PointerDown commits — canonical "destructive
            // icon: press commits" UX. Non-Down phases return Bool(false)
            // so the dispatch loop does not re-route to a sibling.
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    let (id, event_name): (u64, &str) =
                        parse_send_payload(payload).ok_or(InvokeError::Rejected)?;
                    if event_name == "PointerDown" {
                        let was_present = self
                            .todos
                            .get()
                            .iter()
                            .any(|item| item.id == id);
                        self.delete_by_id(id);
                        Ok(IntrospectValue::Bool(was_present))
                    } else {
                        Ok(IntrospectValue::Bool(false))
                    }
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "delete" => match args {
                IntrospectValue::Int(i) => {
                    let id = u64::try_from(i).map_err(|_| InvokeError::Rejected)?;
                    let was_present = self
                        .todos
                        .get()
                        .iter()
                        .any(|item| item.id == id);
                    self.delete_by_id(id);
                    Ok(IntrospectValue::Bool(was_present))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// (R658 §5.16) Per-item toggle handler — 2nd `ExtraExternal`. Mirror
/// of [`TodoDeleteExternal`] over the same shared
/// `Rc<Signal<Vec<TodoItem>>>`. Wire: paint emits `todo_toggle#<id>`;
/// `invoke("send", "<id>:PointerDown")` flips `completed` (idempotent
/// double-click reverts — canonical W3C toggle UX). Direct path:
/// `invoke("toggle", Int(<id>))`.
#[derive(Debug)]
pub struct TodoToggleExternal {
    todos: Rc<Signal<Vec<TodoItem>>>,
}

impl TodoToggleExternal {
    #[must_use]
    pub fn new(todos: Rc<Signal<Vec<TodoItem>>>) -> Self {
        Self { todos }
    }

    fn toggle_by_id(&self, target_id: u64) {
        self.todos.set_with(|prev| {
            prev.iter()
                .map(|item| {
                    if item.id == target_id {
                        TodoItem {
                            completed: !item.completed,
                            ..item.clone()
                        }
                    } else {
                        item.clone()
                    }
                })
                .collect()
        });
    }
}

impl External for TodoToggleExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(
            &[Backend::Gui, Backend::Tui, Backend::Rpc],
            BackendFallback::Skip,
        )
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
}

impl ExternalIntrospect for TodoToggleExternal {
    /// `count` / `completed_count` / `ids_completed` = read-only
    /// derived state. `send` = composite-tag wire (`PointerDown` flips).
    /// `toggle` = direct typed-id shortcut; returns `Bool(new_completed)`.
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("count", "int"),
            ("completed_count", "int"),
            ("ids_completed", "json"),
            ("send", "string"),
            ("toggle", "int"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let snapshot = self.todos.get();
        match path {
            "count" => Some(IntrospectValue::Int(
                i64::try_from(snapshot.len()).expect("todo count must fit in i64"),
            )),
            "completed_count" => {
                let n = snapshot.iter().filter(|i| i.completed).count();
                Some(IntrospectValue::Int(
                    i64::try_from(n).expect("completed count must fit in i64"),
                ))
            }
            "ids_completed" => {
                let arr: Vec<serde_json::Value> = snapshot
                    .iter()
                    .filter(|i| i.completed)
                    .map(|item| serde_json::Value::from(item.id))
                    .collect();
                Some(IntrospectValue::Json(serde_json::Value::Array(arr)))
            }
            _ => None,
        }
    }

    fn intervene(
        &mut self,
        path: &str,
        _value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        match path {
            "count" | "completed_count" | "ids_completed" => {
                Err(InterveneError::ReadOnly)
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    let (id, event_name): (u64, &str) =
                        parse_send_payload(payload).ok_or(InvokeError::Rejected)?;
                    if event_name == "PointerDown" {
                        let was_present = self
                            .todos
                            .get()
                            .iter()
                            .any(|item| item.id == id);
                        self.toggle_by_id(id);
                        // Bool(was_present) lets the AI client tell
                        // "flipped existing item" from "no-op stale id".
                        Ok(IntrospectValue::Bool(was_present))
                    } else {
                        Ok(IntrospectValue::Bool(false))
                    }
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R658 §5.16 — direct toggle by id. Returns the
            // **post-toggle** `completed` value as `Bool(...)` so the
            // AI client can confirm the new state in one
            // round-trip (rather than re-querying `ids_completed`).
            // Unknown ids return `Bool(false)` — semantically
            // "no flip happened" without distinguishing "unknown id"
            // vs "id existed and is now false". For the rare case
            // the caller needs the distinction, `query("count")` /
            // `query("ids_completed")` already expose enough state.
            "toggle" => match args {
                IntrospectValue::Int(i) => {
                    let id = u64::try_from(i).map_err(|_| InvokeError::Rejected)?;
                    self.toggle_by_id(id);
                    let post_completed = self
                        .todos
                        .get()
                        .iter()
                        .any(|item| item.id == id && item.completed);
                    Ok(IntrospectValue::Bool(post_completed))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// R660 §5.16 — R659 `TodoFilterExternal` retired for `RadioGroupExternal`
// (Option β walk-back, [[r650-widget-tag-walk-back]]). Substrate keeps,
// application sheds 199 LOC of bespoke handler.

/// (R664 §5.16 §5.49) Per-row edit-in-place activation handler — 5th
/// `ExtraExternal`. Listens on the [`ITEM_TAG`] primary tag for
/// composite-tag `"<id>:DoubleClick"` payloads dispatched by the
/// framework's W3C [`DOUBLE_CLICK_TIME_MS`](pinion_runtime::input)
/// double-click detection (native winit path) or by the
/// `scene/double_click` RPC drain (AI-introspection path, R663). On
/// activation:
/// 1. Mark this row as the active editor via
///    [`use_editing_id`]`().set(Some(id))` — the
///    [`build_todos_list`] swap observes the signal and renders an
///    inline [`view_field`] in place of the row's text.
/// 2. Seed the editor's [`TextEditState`] with the current item
///    text so the editor starts pre-filled (`TasteJS` `TodoMVC`
///    canonical: double-click reveals the current text, ready to
///    edit, caret at the end).
/// 3. Request focus on [`EDIT_TF_TAG`] via the substrate
///    [`pinion_core::focus_request`] channel so the very next
///    keystroke types into the editor — the shell's `handle_tail`
///    drain applies the focus mutation before the next paint cycle.
///
/// Single-row edit invariant — the cache slot is shared, so a
/// double-click on row B while row A is editing seamlessly migrates
/// the editor (commit-on-blur is *not* the chosen semantics here
/// because the activation arm overwrites the editor text + signal
/// atomically; the in-flight edit on row A is dropped — `TasteJS`
/// behaviour is mouse-blur-discards on the canonical reference).
///
/// Other composite-tag events (`PointerEnter` / `PointerDown` /
/// `PointerUp` / `PointerLeave`) accept-as-no-op so the dispatch
/// path doesn't re-route to a sibling. The framework's single-click
/// `PointerDown` continues to dispatch to [`TodoToggleExternal`] /
/// [`TodoDeleteExternal`] siblings unchanged — `DoubleClick` fires
/// *in addition to* `PointerDown` on the second press per the W3C
/// substrate contract (R664 §5.49 native dispatch + R663 §5.49 RPC
/// drain unify here).
#[derive(Debug)]
pub struct TodoEditExternal {
    todos: Rc<Signal<Vec<TodoItem>>>,
    /// `Rc<Signal<Option<u64>>>` shared with [`use_editing_id`].
    /// Cached at construction so [`begin_edit`] does not call
    /// `Owner::current()` from the dispatch path (the
    /// [`InputRouter`](pinion_runtime::InputRouter) calls
    /// `external.invoke(...)` outside any `Owner::run(...)` wrap).
    editing_id: Rc<Signal<Option<u64>>>,
    /// `Rc<TextEditState>` shared with [`use_text_edit_state`]`(EDIT_TF_TAG)`
    /// — same cache slot the inline `view_field` editor reads, so
    /// the seed value populates the live buffer atomically.
    editor: Rc<pinion_core::widgets::text_edit::TextEditState>,
}

impl TodoEditExternal {
    /// (R664 §5.16) Construct the per-item edit handler with all
    /// reactive handles pre-resolved. Must be called inside an
    /// [`Owner::run`] scope (the substrate wraps `create_extra_externals`
    /// in `root_owner.run(...)` by contract per R51.146-152
    /// `[[callback-root-owner-wrap]]` family). Cached `Rc`s let
    /// [`begin_edit`] run from the dispatch path (which is
    /// `Owner`-less) without panicking.
    #[must_use]
    pub fn new(todos: Rc<Signal<Vec<TodoItem>>>) -> Self {
        let editing_id = use_editing_id();
        let editor = use_text_edit_state(EDIT_TF_TAG);
        Self {
            todos,
            editing_id,
            editor,
        }
    }

    /// (R664 §5.16) Enter edit mode for the row identified by
    /// `target_id`. Returns `true` when the id matched an existing
    /// row (so the caller can distinguish "started editing" from
    /// "stale id ignored"). Idempotent under the equality-skip
    /// `Signal::set` path: double-clicking the *currently editing*
    /// row reseats the editor text from the original item (the
    /// `TasteJS` "restart editing" UX).
    fn begin_edit(&self, target_id: u64) -> bool {
        let Some(item) = self
            .todos
            .get()
            .into_iter()
            .find(|i| i.id == target_id)
        else {
            return false;
        };
        self.editing_id.set(Some(target_id));
        // R664 §5.16 — seed text + park caret at the end (`TasteJS`
        // canonical: double-click reveals the row text with the caret
        // ready to append / Backspace from the trailing edge). The
        // R56.1 [`TextEditState::set_text`] clamps the caret to the
        // *previous* position, which is `0` on the first edit and the
        // commit's leftover offset on subsequent edits — neither is
        // the right "open the file at the end" UX, so we explicitly
        // [`TextEditState::set_caret`] after seeding to land the caret
        // on the trailing edge.
        let len = item.text.len();
        self.editor.set_text(item.text.clone());
        self.editor.set_caret(len);
        // R664 §5.39 — substrate drains this on the next
        // `handle_tail` call (which is the dispatch that delivered
        // the DoubleClick → invoke this method). Focus moves to the
        // newly-painted editor before the user's next keystroke.
        pinion_core::focus_request::request(EDIT_TF_TAG);
        true
    }
}

impl External for TodoEditExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(
            &[Backend::Gui, Backend::Tui, Backend::Rpc],
            BackendFallback::Skip,
        )
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
}

impl ExternalIntrospect for TodoEditExternal {
    /// `editing_id` — current edit-mode row (Int) or `null`.
    /// `send` — composite-tag wire `"<id>:DoubleClick"` activates the
    /// edit mode; other events accepted-as-no-op. `begin` — direct
    /// typed-id activation (RPC AI-driving path); returns
    /// `Bool(was_present)` so callers distinguish "row existed +
    /// editing now" from "no-op stale id".
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("editing_id", "json"),
            ("send", "string"),
            ("begin", "int"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "editing_id" => Some(IntrospectValue::Json(
                self.editing_id
                    .get()
                    .map_or(serde_json::Value::Null, serde_json::Value::from),
            )),
            _ => None,
        }
    }

    fn intervene(
        &mut self,
        path: &str,
        _value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        match path {
            "editing_id" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    let (id, event_name): (u64, &str) =
                        parse_send_payload(payload).ok_or(InvokeError::Rejected)?;
                    if event_name == "DoubleClick" {
                        let was_present = self.begin_edit(id);
                        Ok(IntrospectValue::Bool(was_present))
                    } else {
                        // PointerEnter/Down/Up/Leave + future event
                        // names — silent no-op so the dispatch loop
                        // does not reject + reroute to a sibling.
                        Ok(IntrospectValue::Bool(false))
                    }
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // (R664 §5.16) Direct typed-id activation — the RPC AI
            // path skips the composite-tag wire and addresses the
            // row by id. Same semantics as a DoubleClick payload.
            "begin" => match args {
                IntrospectValue::Int(i) => {
                    let id = u64::try_from(i).map_err(|_| InvokeError::Rejected)?;
                    let was_present = self.begin_edit(id);
                    Ok(IntrospectValue::Bool(was_present))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// Build the todo list section: tagged `Scene::Container`(`LIST_TAG`)
/// holding a header label + one row per entry. Empty store renders
/// the "type and press Enter" hint. Header shape is filter-aware
/// (three orthogonal cases below). Lifted from view fn so the
/// [[r655-todomvc-scaffolding]] test battery can pin item ordering.
///
/// (R664 §5.16 §5.38) When `editing_id == Some(item.id)`, the row's
/// middle slot swaps from the static `entry_text` to an inline
/// [`view_field`](pinion_widget_paint::text_field::view_field)
/// editor — `EDIT_TF_TAG` paints as a `TextField` with the row's
/// current text pre-seeded into [`use_text_edit_state`]`(EDIT_TF_TAG)`.
/// All other rows render their normal text label. The toggle + delete
/// affordances stay visible while editing so the user can toggle
/// "completed" mid-edit (the row text continues to live in
/// [`use_todos`] until [`commit_edit`] commits the edited text on
/// `Enter`).
#[allow(
    clippy::too_many_lines,
    reason = "single-purpose list builder — per-row trio (toggle/text-or-editor/delete) reads better inline than split"
)]
fn build_todos_list(
    theme: &Theme,
    entries: &[TodoItem],
    filter_mode: FilterMode,
    total_count: usize,
    editing_id: Option<u64>,
    edit_interaction: TextFieldState,
    edit_caret_byte: u32,
) -> Scene {
    let header_style = TextStyle::new()
        .with_size_px(LIST_TITLE_FONT_SIZE_PX)
        .with_fg(theme.resolve(ColorRole::OnSurfaceMuted));
    // R664 §5.16 — completed-row text now combines the original
    // `OnSurfaceMuted` colour fade (R658) with a horizontal
    // `Strikethrough` `TextDecoration` (2nd consumer of the
    // `pinion-core::style::TextDecoration` substrate; the 1st is
    // `pinion-core::widgets::toggle`'s focus-ring decoration). The
    // line + colour pair is the canonical `TasteJS` `TodoMVC`
    // "done" visual — both signals reinforce so the row reads as
    // completed even when colour-blind users cannot discriminate
    // the muted fade.
    let item_style_active = TextStyle::new()
        .with_size_px(LIST_ITEM_FONT_SIZE_PX)
        .with_fg(theme.resolve(ColorRole::OnSurface));
    let item_style_completed = TextStyle::new()
        .with_size_px(LIST_ITEM_FONT_SIZE_PX)
        .with_fg(theme.resolve(ColorRole::OnSurfaceMuted))
        .with_decoration(pinion_core::TextDecoration::strikethrough());
    let delete_style = TextStyle::new()
        .with_size_px(DELETE_FONT_SIZE_PX)
        .with_fg(delete_glyph_color(theme));
    let toggle_style_active = TextStyle::new()
        .with_size_px(TOGGLE_FONT_SIZE_PX)
        .with_fg(theme.resolve(ColorRole::OnSurface));
    let toggle_style_completed = TextStyle::new()
        .with_size_px(TOGGLE_FONT_SIZE_PX)
        .with_fg(theme.resolve(ColorRole::OnSurfaceMuted));

    // Header — three cases: empty store hint, unfiltered count
    // `Todos (X of N completed)`, or filtered `<Filter>: visible of total`.
    let header_text = if total_count == 0 {
        String::from("No todos yet — type and press Enter")
    } else if entries.len() == total_count {
        // No filter hiding rows — preserve pre-R659 shape.
        let completed = entries.iter().filter(|i| i.completed).count();
        if completed == 0 {
            format!("Todos ({total_count})")
        } else {
            format!("Todos ({completed} of {total_count} completed)")
        }
    } else {
        // R659 — filter is hiding rows. Show <FilterName>: visible of
        // total so the user knows the discrepancy.
        format!(
            "{}: {} of {}",
            filter_mode.label(),
            entries.len(),
            total_count,
        )
    };
    let header = Scene::Text(TextNode::styled(
        header_text,
        Rect::default(),
        header_style,
    ));

    // R656 §5.16 — each row tagged `todo_item#<id>` (stable u64 id,
    // NOT array index — the R655 index-based tagging would alias
    // `todo_item#1` to a different logical row after a sibling
    // delete, breaking any in-flight AI-side reference). The
    // per-row delete affordance is a sub-`Scene::Container` tagged
    // `todo_delete#<id>` that hosts the visible `×` glyph; the
    // R51.42 §5.35 composite-tag wire splits this paint tag at the
    // `#` so the [`InputRouter`] routes the click into the singleton
    // [`TodoDeleteExternal`] registered under primary tag
    // [`DELETE_TAG`] (`"todo_delete"`). Mirrors `hello-listbox`'s
    // `listbox_row(i, ...)` (R55.G.20) and `RadioGroupExternal`'s
    // composite-tag pattern (R51.43) — no new framework substrate
    // needed.
    let mut children: Vec<Scene> = Vec::with_capacity(entries.len() + 1);
    children.push(header);
    for item in entries {
        let row_tag = format!("{ITEM_TAG_PREFIX}#{}", item.id);
        let delete_tag = format!("{DELETE_TAG_PREFIX}#{}", item.id);
        let toggle_tag = format!("{TOGGLE_TAG_PREFIX}#{}", item.id);

        // (R658 §5.16) Toggle button — left-side `☐`/`☑` glyph
        // mirroring the right-side `×` delete button's hit-target +
        // composite-tag wire shape. The chosen glyph reflects the
        // current `completed` state: U+2610 (BALLOT BOX) for active,
        // U+2611 (BALLOT BOX WITH CHECK) for completed.
        let (toggle_glyph_str, toggle_style_for_row) = if item.completed {
            (TOGGLE_GLYPH_CHECKED, toggle_style_completed.clone())
        } else {
            (TOGGLE_GLYPH_UNCHECKED, toggle_style_active.clone())
        };
        let toggle_glyph = Scene::Text(TextNode::styled(
            toggle_glyph_str,
            Rect::default(),
            toggle_style_for_row,
        ));
        let toggle_button = Scene::Container(
            ContainerNode::new(vec![toggle_glyph])
                .with_tag(toggle_tag)
                // (R658 §5.16) Static "Toggle complete" aria-label
                // so screen readers announce a stable affordance
                // name. The dynamic checked / unchecked state lives
                // on the per-item AriaRole::CheckBox AccessNode
                // emitted by [`access_node`] (R658 §5.40), which is
                // the W3C-canonical place for state in `aria-checked`.
                .with_aria_label("Toggle complete")
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_justify(JustifyContent::Center)
                        .with_align_items(AlignItems::Center)
                        .with_size(Size::px(TOGGLE_BUTTON_W, TOGGLE_BUTTON_H)),
                ),
        );

        let item_style_for_row = if item.completed {
            item_style_completed.clone()
        } else {
            item_style_active.clone()
        };
        // R664 §5.16 §5.38 — row middle slot: static text in normal
        // mode, inline `view_field` editor (3rd consumer of the
        // R657-lifted `tf_paint::view_field` substrate; 1st was
        // `hello-textfield`, 2nd the main `TF_TAG` field above) when
        // this row is the active edit target. The shared
        // `EDIT_TF_TAG` editor instance reuses one [`TextEditState`]
        // / [`CaretBlink`] across edit sessions per
        // [[abstraction-needs-second-consumer]] — a per-item slot
        // (dynamic `Owner::cache` key) lifts only when a multi-row
        // simultaneous-edit consumer surfaces, which today's
        // `TasteJS` `TodoMVC` does not have.
        let row_middle: Scene = if editing_id == Some(item.id) {
            tf_paint::view_field(
                EDIT_TF_TAG,
                edit_interaction,
                edit_caret_byte,
                theme,
                &tf_paint::TextFieldStyle::m3_filled(),
                "Edit todo",
            )
        } else {
            Scene::Text(TextNode::styled(
                item.text.clone(),
                Rect::default(),
                item_style_for_row,
            ))
        };

        // R656 §5.16 — the delete button is a tagged Container
        // hosting the `×` glyph centred inside the 24×24 hit-target.
        // No [`External`] per-item — the singleton
        // [`TodoDeleteExternal`] resolves the sub-index off the
        // composite tag at dispatch time. The button is NOT listed
        // in [`focusable_tags`] so a mouse click does not steal
        // focus from the text field (the user can press Enter
        // again immediately to add the next todo without a manual
        // refocus hop).
        let delete_glyph = Scene::Text(TextNode::styled(
            DELETE_GLYPH,
            Rect::default(),
            delete_style.clone(),
        ));
        let delete_button = Scene::Container(
            ContainerNode::new(vec![delete_glyph])
                .with_tag(delete_tag)
                // R51.69 §5.40 — accessible name carried as
                // `aria-label` so screen readers announce
                // "Delete <item text>" instead of just the glyph.
                // The static "Delete" label is intentional — the
                // per-item descriptive name lives on the parent
                // ListItem AccessNode via `enrich_names_from_scene`.
                .with_aria_label("Delete")
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_justify(JustifyContent::Center)
                        .with_align_items(AlignItems::Center)
                        .with_size(Size::px(DELETE_BUTTON_W, DELETE_BUTTON_H)),
                ),
        );

        // R658 §5.16 — row children become
        // `[toggle_button, entry_text, delete_button]` with
        // `JustifyContent::SpaceBetween`: toggle pins to the left
        // edge, delete pins to the right, the entry text floats
        // between them. The row's `todo_item#<id>` outer tag stays
        // the catch-all for clicks that land on the text (which
        // produce no External dispatch today — R660+ edit candidate
        // per [[abstraction-needs-second-consumer]]).
        let row = Scene::Container(
            ContainerNode::new(vec![toggle_button, row_middle, delete_button])
                .with_tag(row_tag)
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_justify(JustifyContent::SpaceBetween)
                        .with_align_items(AlignItems::Center)
                        .with_gap(LIST_ITEM_GAP),
                ),
        );
        children.push(row);
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_tag(LIST_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_gap(LIST_ITEM_GAP),
            ),
    )
}

/// `WidgetView` binding. `Self::State = (TextFieldState, u32, FilterRadioStates, TextFieldState, u32)`
/// stays `Copy` per R51.173; non-Copy reactive stores reach the
/// binding through `Owner::cache` hooks (see module doc).
struct TodoMvcView;

impl WidgetCore for TodoMvcView {
    type State = (TextFieldState, u32, FilterRadioStates, TextFieldState, u32);
    type Event = TextFieldEvent;

    fn create_external() -> Box<dyn External> {
        // R56.1.b.1 — runs inside `root_owner.run(...)` so the hooks
        // resolve to the same `Owner::cache` slots the view fn sees.
        let text_state = use_text_edit_state(TF_TAG);
        let blink = use_caret_blink(TF_TAG);
        let clipboard = use_clipboard(TF_TAG);
        Box::new(
            TextFieldExternal::new()
                .attach_state(text_state)
                .attach_blink(blink)
                .attach_clipboard(clipboard),
        )
    }

    fn tag() -> &'static str {
        TF_TAG
    }

    /// R55.D.5 multi-External composition — primary `TextField` plus
    /// six `ExtraExternal`s (delete / toggle / filter group / scrollbar
    /// / item double-click / inline editor). R660 filter group seeded
    /// with persisted mode so the first paint shows the engaged
    /// segment without a separate sync pass. R664 adds:
    /// - [`ITEM_TAG`] / [`TodoEditExternal`] — handles `"<id>:DoubleClick"`
    ///   payloads from the framework's W3C double-click detection;
    /// - [`EDIT_TF_TAG`] / [`TextFieldExternal`] — the inline editor's
    ///   own `TextField` SCXML, wired through the dedicated
    ///   [`use_text_edit_state`] / [`use_caret_blink`] /
    ///   [`use_clipboard`] cache slots so the editor + main field
    ///   keep independent text + caret + clipboard state across the
    ///   focus transitions.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        // (R665 §3 §5.15) Persistence boot — runs ONCE per `Owner`.
        // Hydrates `use_todos` / `use_filter` / `use_next_todo_id`
        // from disk before any other hook resolves their cache slot,
        // then installs a save Effect that fires on every subsequent
        // change. Placed FIRST so the seeded Signals propagate into
        // the rest of `create_extra_externals` (e.g. the filter
        // group's initial selected index reads from the hydrated
        // `use_filter()` snapshot).
        let _persistence = use_persistence_boot();

        let todos = use_todos();
        let filter = use_filter();
        let scroll_state = use_scroll_state(LIST_SCROLL_KEY);
        let mut filter_group = RadioGroupExternal::new(3);
        let _ = filter_group.intervene(
            "selected_index",
            IntrospectValue::Int(
                i64::try_from(filter.get().to_index()).expect("filter index fits in i64"),
            ),
        );
        let edit_text_state = use_text_edit_state(EDIT_TF_TAG);
        let edit_blink = use_caret_blink(EDIT_TF_TAG);
        let edit_clipboard = use_clipboard(EDIT_TF_TAG);
        vec![
            ExtraExternal::new(
                DELETE_TAG,
                Box::new(TodoDeleteExternal::new(todos.clone())),
            ),
            ExtraExternal::new(
                TOGGLE_TAG,
                Box::new(TodoToggleExternal::new(todos.clone())),
            ),
            ExtraExternal::new(FILTER_TAG, Box::new(filter_group)),
            // R746 §5.45 — shared scrollbar peer wiring (state + M3
            // interaction-state mirror, registered under SCROLLBAR_TAG).
            scrollbar_extra_external(scroll_state, SCROLLBAR_TAG),
            ExtraExternal::new(ITEM_TAG, Box::new(TodoEditExternal::new(todos))),
            ExtraExternal::new(
                EDIT_TF_TAG,
                Box::new(
                    TextFieldExternal::new()
                        .attach_state(edit_text_state)
                        .attach_blink(edit_blink)
                        .attach_clipboard(edit_clipboard),
                ),
            ),
        ]
    }

    fn read_state(scene: &Scene) -> (TextFieldState, u32, FilterRadioStates, TextFieldState, u32) {
        let (interaction, caret) = tf_paint::read_text_field_state(scene, TF_TAG);
        let (edit_interaction, edit_caret) =
            tf_paint::read_text_field_state(scene, EDIT_TF_TAG);
        let filter_radios = read_filter_radio_states(scene);
        (interaction, caret, filter_radios, edit_interaction, edit_caret)
    }

    fn view(state: (TextFieldState, u32, FilterRadioStates, TextFieldState, u32), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(event: TextFieldEvent) -> &'static str {
        // R699 §5.16 — route the forward Event->name mapping through the
        // WidgetEventName SSOT (`as_name`), retiring the hand-written
        // match table. `as_name` is total over internal variants too;
        // only external events reach this path via `ShellCore::forward`.
        pinion_core::WidgetEventName::as_name(&event)
    }

    fn title() -> &'static str {
        "pinion todomvc (R655 §5.16) — first composed app"
    }

    /// (R664 §5.16) Three tab stops — `TF_TAG`, `EDIT_TF_TAG`,
    /// `FILTER_TAG`. Per-focus `apply_key` arm picks the matching
    /// keymap. `EDIT_TF_TAG` is enumerated unconditionally so the
    /// programmatic [`pinion_core::focus_request`] dispatched from
    /// the [`TodoEditExternal`] activation arm succeeds — the
    /// inline editor is only painted while
    /// [`use_editing_id`]`().get().is_some()`, so the focus is
    /// effectively a "phantom" tab stop when no row is in edit mode.
    /// The user-visible cost is one ghost Tab cycle through an
    /// invisible target; the alternative (refresh `focusable_tags`
    /// per paint, which the substrate does not do today) is a
    /// framework-tier change deferred until a 2nd consumer of
    /// dynamic-focusable-tags surfaces per
    /// [[abstraction-needs-second-consumer]].
    fn focusable_tags() -> Vec<&'static str> {
        vec![TF_TAG, EDIT_TF_TAG, FILTER_TAG]
    }

    /// R660 reducer — bridge `RadioGroupExternal`'s `"selected"` intent
    /// into `Signal<FilterMode>` so the view-fn `.get()` subscription
    /// repaints with the new visible subset. Side-effect-only
    /// ([[scxml-as-model-update-transient]]) — empty `Vec<Command>`
    /// return; the `Signal::set` write is the mutation.
    fn update(
        _state: (TextFieldState, u32, FilterRadioStates, TextFieldState, u32),
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        if intent.tag_str() == TODO_FILTER_SELECTED_INTENT_TAG {
            if let IntrospectValue::Int(i) = intent.payload {
                if let Ok(idx) = usize::try_from(i) {
                    if let Some(mode) = FilterMode::from_index(idx) {
                        use_filter().set(mode);
                    }
                }
            }
        }
        Vec::new()
    }

    /// R666 §5.37 — no letter-key bindings. The R655 scaffolding
    /// copy-pasted `"d" → Disable / "e" → Enable` from
    /// `hello-textfield`, where letter-key intercepts make sense as
    /// a smoke-test affordance. todomvc has no UX requirement to
    /// disable the input from the keyboard, and once R666 #3 closed
    /// `[[scene-key-character-named-gap]]` the intercept actively
    /// broke typing — every "delete" / "edit" / "send" attempt
    /// disabled the field on the second character. The `Disable` /
    /// `Enable` transitions remain reachable through SCXML direct
    /// dispatch when a future round wires a real UX (toolbar button,
    /// menu item) to them.
    fn keybinding(_key: &str) -> Option<TextFieldEvent> {
        None
    }

    /// W3C UI Events delegation. R660 splits the keymap per-focus
    /// (`TF_TAG` → text edit keys; `FILTER_TAG` → ARIA radio-group cycle);
    /// R664 adds the `EDIT_TF_TAG` arm for the in-place row editor
    /// (Enter commits / Escape cancels / everything else delegates
    /// to the embedded `TextField`).
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        match focused {
            Some(TF_TAG) => apply_key_textfield(scene, key),
            Some(EDIT_TF_TAG) => apply_key_edit(scene, key),
            Some(FILTER_TAG) => apply_key_filter(scene, key),
            _ => false,
        }
    }

    /// R56.2.a §5.13 — winit `Ime` → `CompositionEvent` → JSON wire to
    /// [`TextFieldExternal::invoke`]`("composition", ...)`. The AI-RPC
    /// path and the platform IME path funnel through one widget invoke.
    ///
    /// | variant         | action key |
    /// |-----------------|------------|
    /// | `Start`         | `start`    |
    /// | `Update(text)`  | `update`   |
    /// | `Commit(text)`  | `end`      |
    /// | `Cancel`        | `cancel`   |
    fn apply_composition(
        scene: &mut Scene,
        focused: Option<&str>,
        event: &pinion_core::CompositionEvent,
    ) -> bool {
        // R664 — route IME composition to whichever text-input field
        // is focused: main "add todo" (TF_TAG) or in-place editor
        // (EDIT_TF_TAG). Both wire through the canonical TextField
        // `invoke("composition", ...)` handler so the platform IME
        // bridge funnels through one substrate per field.
        let target = match focused {
            Some(TF_TAG) => TF_TAG,
            Some(EDIT_TF_TAG) => EDIT_TF_TAG,
            _ => return false,
        };
        let Some(node) = scene.find_external_with_tag_mut(target) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        let args = match event {
            pinion_core::CompositionEvent::Start => {
                IntrospectValue::Json(serde_json::json!({ "action": "start" }))
            }
            pinion_core::CompositionEvent::Update(text) => IntrospectValue::Json(
                serde_json::json!({ "action": "update", "data": text }),
            ),
            pinion_core::CompositionEvent::Commit(text) => IntrospectValue::Json(
                serde_json::json!({ "action": "end", "data": text }),
            ),
            pinion_core::CompositionEvent::Cancel => {
                IntrospectValue::Json(serde_json::json!({ "action": "cancel" }))
            }
            // `CompositionEvent` is `#[non_exhaustive]`; defer
            // any future variant (delete_surrounding etc.) to the
            // shell's fallback by reporting unhandled here.
            _ => return false,
        };
        intro.invoke("composition", args).is_ok()
    }

    /// R56.2.e — X11/Wayland canonical middle-click PRIMARY paste.
    /// Funnels through the textfield widget's `paste-primary` invoke.
    /// Modifiers ignored (plain middle-click is the unambiguous
    /// PRIMARY-paste case across desktops). R664 — both
    /// [`TF_TAG`] and [`EDIT_TF_TAG`] receive the paste affordance.
    fn apply_middle_click(
        scene: &mut Scene,
        focused: Option<&str>,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        let target = match focused {
            Some(TF_TAG) => TF_TAG,
            Some(EDIT_TF_TAG) => EDIT_TF_TAG,
            _ => return false,
        };
        let Some(node) = scene.find_external_with_tag_mut(target) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        match intro.invoke("paste-primary", IntrospectValue::Null) {
            Ok(IntrospectValue::Bool(handled)) => handled,
            _ => false,
        }
    }

    fn fmt_state_log(state: &(TextFieldState, u32, FilterRadioStates, TextFieldState, u32)) -> String {
        let radios = state
            .2
            .states
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let short = match s {
                    RadioState::Hover => "H",
                    RadioState::Pressed => "P",
                    RadioState::Disabled => "D",
                    RadioState::Idle => "i",
                };
                format!("{i}={short}")
            })
            .collect::<Vec<_>>()
            .join(" ");
        let focused = match state.2.focused {
            Some(i) => format!(" focus={i}"),
            None => String::new(),
        };
        format!(
            "{} / caret={} / filter[{radios}]{focused} / edit={} caret={}",
            state.0.as_name(),
            state.1,
            state.3.as_name(),
            state.4,
        )
    }
}

impl WidgetA11y for TodoMvcView {
    /// R56.1.b.1 §5.40 multi-node tree:
    /// 1. `TF_TAG` → `AriaRole::TextInput` (R656)
    /// 2. `FILTER_TAG` → `AriaRole::RadioGroup` + 3 `RadioButton` children
    ///    with R660 live hovered/pressed/disabled + checked bits
    /// 3. `LIST_TAG` → `AriaRole::List` + per-item children
    /// 4. Per-row → `ListItem` + `CheckBox` (R658 toggle, aria-checked) +
    ///    `Button` (delete)
    ///
    /// Labels come from paint-side `with_aria_label` via
    /// `enrich_names_from_scene` so literal strings live in one place.
    fn access_node(state: &(TextFieldState, u32, FilterRadioStates, TextFieldState, u32), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, _caret, filter_radios, _edit_interaction, _edit_caret) = state;
        let text = use_text_edit_state(TF_TAG).text();
        let access_state = AccessState {
            focused: focused == Some(<Self as WidgetCore>::tag()),
            disabled: matches!(interaction, TextFieldState::Disabled),
            hovered: false,
            pressed: false,
            checked: None,
        };

        let mut nodes = vec![
            AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::TextInput)
                .with_value(AccessValue::Text(text))
                .with_state(access_state),
        ];

        // R659 §5.40 — filter group root + 3 RadioButton children
        // (W3C WAI-ARIA "radiogroup" canonical mapping for a
        // mutually-exclusive single-select segmented control).
        //
        // R660 §5.40 — the per-radio AccessState now carries live
        // `hovered` / `pressed` / `disabled` bits derived from the
        // R660 [`RadioGroupExternal`] snapshot ([`filter_radios`]) so
        // screen readers announce focus / press feedback through the
        // same channel a sighted user sees via the M3 state-layer.
        // `focused` follows the group's AT-side active-descendant
        // ([`R51.87`] `focused_index`) — falls back to the selected
        // mode when no `Focus` action has pinned a row yet, matching
        // the [`access_focus_target`] resolution.
        let current_filter = use_filter().get();
        let filter_modes = [FilterMode::Active, FilterMode::Completed, FilterMode::All];
        let group_focused_here = focused == Some(FILTER_TAG);
        let active_descendant_idx = filter_radios
            .focused
            .unwrap_or_else(|| current_filter.to_index());
        let mut filter_group_node =
            AccessNode::new(FILTER_TAG, AriaRole::RadioGroup).with_name("Filter");
        for mode in &filter_modes {
            let tag = format!("{FILTER_TAG_PREFIX}#{}", mode.to_index());
            filter_group_node = filter_group_node.with_child(tag);
        }
        nodes.push(filter_group_node);
        for mode in &filter_modes {
            let idx = mode.to_index();
            let tag = format!("{FILTER_TAG_PREFIX}#{idx}");
            let radio_state = filter_radios
                .states
                .get(idx)
                .copied()
                .unwrap_or(RadioState::Idle);
            nodes.push(
                AccessNode::new(tag, AriaRole::RadioButton).with_state(AccessState {
                    focused: group_focused_here && idx == active_descendant_idx,
                    // W3C-canonical: RadioButton uses `aria-checked` for the
                    // selected-bit (Some(true) = engaged, Some(false) = inactive).
                    ..AccessState::from_interaction(radio_state, Some(*mode == current_filter))
                }),
            );
        }

        // R656 §5.40 — list root + items + delete buttons.
        // R658 §5.40 — per-row also emits an AriaRole::CheckBox
        // node for the toggle button with `aria-checked` reflecting
        // the entry's `completed` field. The W3C WAI-ARIA 1.2
        // canonical mapping for a "checkbox" widget puts the
        // checked/unchecked state on `aria-checked`, not on any
        // visible glyph — screen readers read this announcement
        // independent of the row's text.
        let entries = use_todos().get();
        let mut list_node = AccessNode::new(LIST_TAG, AriaRole::List);
        for item in &entries {
            let row_tag = format!("{ITEM_TAG_PREFIX}#{}", item.id);
            let delete_tag = format!("{DELETE_TAG_PREFIX}#{}", item.id);
            let toggle_tag = format!("{TOGGLE_TAG_PREFIX}#{}", item.id);
            list_node = list_node.with_child(row_tag.clone());
            nodes.push(
                AccessNode::new(row_tag, AriaRole::ListItem)
                    .with_value(AccessValue::Text(item.text.clone())),
            );
            // R658 §5.40 — checkbox node carries `aria-checked` via
            // `AccessState::checked = Some(<bool>)`. The
            // `enrich_names_from_scene` pass picks up the static
            // "Toggle complete" aria-label from the paint scene; the
            // dynamic state lives here (W3C-canonical split).
            nodes.push(
                AccessNode::new(toggle_tag, AriaRole::CheckBox).with_state(AccessState {
                    focused: false,
                    disabled: false,
                    hovered: false,
                    pressed: false,
                    checked: Some(item.completed),
                }),
            );
            // R656 §5.40 — delete button is a Button child of the
            // ListItem (semantically) but pinion's flat AccessNode
            // surface keeps siblings at the same level; the AT
            // tree builder ([`pinion_a11y::AccessTreeBuilder`])
            // resolves children via `with_child` references —
            // future work on parent/child wiring for nested-button
            // semantics is the R660+ a11y carry (no current AT
            // consumer requires it — screen readers walk Children
            // pointers anyway).
            nodes.push(AccessNode::new(delete_tag, AriaRole::Button));
        }
        // The list root must register every item as a child so the
        // AT cursor traversal order matches the visible paint
        // order — `with_child` records reference tags that the
        // tree builder resolves to NodeIds.
        nodes.push(list_node);

        nodes
    }

    /// R660 — composite focus for `FILTER_TAG` (roving-tabindex active
    /// descendant). AT-side `focused_index` wins; falls back to the
    /// engaged segment so the AT cursor lands on the visible
    /// selection. Other focused tags use the atomic-fallback shape.
    fn access_focus_target(
        state: &(TextFieldState, u32, FilterRadioStates, TextFieldState, u32),
        focused: Option<&str>,
    ) -> Option<pinion_a11y::AccessFocus> {
        if focused == Some(FILTER_TAG) {
            let active_idx = state
                .2
                .focused
                .unwrap_or_else(|| use_filter().get().to_index());
            return Some(pinion_a11y::AccessFocus::composite(
                FILTER_TAG,
                format!("{FILTER_TAG_PREFIX}#{active_idx}"),
            ));
        }
        focused.map(pinion_a11y::AccessFocus::atomic)
    }

    /// R662 §5.40 — multi-composite dispatch: route AT actions on
    /// `todo_filter#<i>` sub-tags into the framework `RadioGroupExternal`
    /// via the composite-tag wire. Click / Default fire the full
    /// PointerEnter/Down/Up/Leave activation cycle; Focus moves the
    /// AT-side active-descendant without committing (R51.87 roving-
    /// tabindex). Hello-radio-group precedent, R662 5-of-5 framework
    /// consumer push (mirror of R660 `composite_tag`).
    ///
    /// R664 §5.40 — adds AT-action wires for the per-item composites:
    /// - `todo_delete#<id>`  → `Click`/`Default` invokes
    ///   [`TodoDeleteExternal`]`("delete", Int(id))` for AT-driven
    ///   row deletion;
    /// - `todo_toggle#<id>`  → `Click`/`Default` invokes
    ///   [`TodoToggleExternal`]`("toggle", Int(id))` for AT-driven
    ///   completion toggle;
    /// - `todo_item#<id>`    → `Click`/`Default` invokes
    ///   [`TodoEditExternal`]`("begin", Int(id))` for AT-driven
    ///   edit-in-place activation (the AT equivalent of the W3C
    ///   double-click idiom — assistive tech doesn't distinguish
    ///   "double" from "single" activation, so the canonical
    ///   activation idiom enters edit mode directly).
    ///
    /// `parent_tag` is the disambiguation key — each composite's
    /// sub-tags are stable `u64` ids that can numerically collide
    /// across composites.
    fn access_child_invoke(
        scene: &mut Scene,
        parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        match parent_tag {
            FILTER_TAG => filter_at_action(scene, sub_tag, action),
            DELETE_TAG => per_item_int_action(scene, DELETE_TAG, "delete", sub_tag, action),
            TOGGLE_TAG => per_item_int_action(scene, TOGGLE_TAG, "toggle", sub_tag, action),
            ITEM_TAG => per_item_int_action(scene, ITEM_TAG, "begin", sub_tag, action),
            _ => false,
        }
    }
}

/// (R662 §5.40) Filter group AT-action handler — split out of
/// [`TodoMvcView::access_child_invoke`] so the per-item composites
/// keep symmetric structure. Click / Default cycles the full
/// activation arc; Focus moves the active descendant without
/// committing (W3C ARIA radiogroup roving-tabindex).
fn filter_at_action(scene: &mut Scene, sub_tag: &str, action: AccessAction) -> bool {
    let Ok(idx) = sub_tag.parse::<usize>() else {
        return false;
    };
    if idx >= 3 {
        return false;
    }
    let Some(node) = scene.find_external_with_tag_mut(FILTER_TAG) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    match action {
        AccessAction::Click | AccessAction::Default => {
            for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
                let _ = intro.invoke(
                    "send",
                    IntrospectValue::Text(format!("{idx}:{ev}")),
                );
            }
            true
        }
        AccessAction::Focus => {
            if let Ok(i) = i64::try_from(idx) {
                let _ = intro.intervene(
                    "focused_index",
                    IntrospectValue::Int(i),
                );
            }
            true
        }
        _ => false,
    }
}

/// (R664 §5.40) Generic per-item AT-action helper —
/// `Click`/`Default` invokes `external_tag`.`method`(Int(id)) on the
/// External registered under `external_tag` (delete / toggle / edit).
/// Sub-tag must parse as `u64` (the stable per-item id). Used by
/// the AT bridge for the `todo_delete` / `todo_toggle` / `todo_item`
/// composites — they all share the "address row by stable id +
/// invoke the typed-id method" shape, so the AT layer reads as one
/// declarative table of `parent_tag → invoke(method, id)` arrows.
fn per_item_int_action(
    scene: &mut Scene,
    external_tag: &str,
    method: &str,
    sub_tag: &str,
    action: AccessAction,
) -> bool {
    let Ok(id) = sub_tag.parse::<u64>() else {
        return false;
    };
    let Ok(id_i64) = i64::try_from(id) else {
        return false;
    };
    let Some(node) = scene.find_external_with_tag_mut(external_tag) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    match action {
        AccessAction::Click | AccessAction::Default => {
            let _ = intro.invoke(method, IntrospectValue::Int(id_i64));
            true
        }
        _ => false,
    }
}

impl WidgetView for TodoMvcView {
    type Renderer = TodoMvcRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }

    /// R56.2.c — publish caret rect for platform IME candidate window.
    /// Composition: field rect (window-coord, via `rect_for_tag`) +
    /// `FIELD_PAD` text origin + `caret_rect_for_byte_offset` on the
    /// visual caret byte (preedit-end during composition). Width =
    /// `CARET_WIDTH`; height carries the `FONT_SIZE_PX` floor.
    ///
    /// R664 §5.16 §5.38 — handles two focusable text-input contexts:
    /// the main "add todo" field ([`TF_TAG`]) and the in-place row
    /// editor ([`EDIT_TF_TAG`]). The focused tag selects the matching
    /// field rect + interaction + caret byte so platform IME windows
    /// (Wayland text-input-v3, macOS `NSTextInputContext`, …) anchor
    /// under the correct caret regardless of which field the user is
    /// composing in.
    fn ime_caret_rect(
        state: &(TextFieldState, u32, FilterRadioStates, TextFieldState, u32),
        scene: &Scene,
        focused: Option<&str>,
    ) -> Option<CaretRect> {
        let (interaction, caret_byte, _, edit_interaction, edit_caret) = *state;
        let (tag, fld_interaction, fld_caret) = match focused {
            Some(TF_TAG) => (TF_TAG, interaction, caret_byte),
            Some(EDIT_TF_TAG) => (EDIT_TF_TAG, edit_interaction, edit_caret),
            _ => return None,
        };
        // R657 §5.16 §5.38 — field rect walk stays binding-side; the
        // caret composition (splice + LayoutCache lookup + window-
        // coord sum) is the lifted helper.
        let field_rect = pinion_shell::rect_for_tag(scene, tag)?;
        let theme = use_theme(THEME_TAG).theme_animated();
        Some(tf_paint::ime_caret_rect_for(
            tag,
            fld_interaction,
            fld_caret,
            field_rect,
            &theme,
            &tf_paint::TextFieldStyle::m3_filled(),
        ))
    }
}

// R698 §5.16 §5.38 — TextField state <-> SCXML id mapping now routes
// through the `pinion_core::WidgetStateName` trait
// (`TextFieldState::as_name` / `TextFieldState::from_name_or_default`),
// retiring the local + paint-crate helpers.

fn main() {
    pinion_shell::run::<TodoMvcView>();
}

#[cfg(test)]
mod tests {
    //! R655 / R656 §5.16 — todomvc-specific regression battery.
    //! Substrate correctness for the embedded `TextField` widget is
    //! covered by `hello-textfield`'s own tests (R56.1.b.1) — this
    //! module pins only the **composition** + **stable-id** layers
    //! this binding introduces: the todo list section presence,
    //! item-count → child-count invariant, per-item tag = stable id
    //! (R656), [`use_todos`] reactive hook dedup contract, the
    //! [`use_next_todo_id`] monotonic counter contract, and the
    //! [`TodoDeleteExternal`] composite-tag delete wire (parsed
    //! through both `invoke("send", "<id>:PointerDown")` and the
    //! direct `invoke("delete", Int(<id>))` paths). The R55.G.22
    //! paint-root tag convention is pinned via the framework fixture
    //! call.
    //!
    //! Note: the Enter handler itself runs against a `&mut Scene`
    //! that the runtime owns; testing it requires the shell's input
    //! loop. The handler's signal mutation is exercised indirectly
    //! here by manipulating the `Signal<Vec<TodoItem>>` directly
    //! under a private `Owner` and asserting the view-fn renders
    //! the new entries — the same observable surface the visible
    //! app depends on.
    use super::{
        allocate_todo_id, build_filter_row, build_todos_list, use_filter, use_next_todo_id,
        use_todos, view, FilterMode, FilterRadioStates, TodoDeleteExternal, TodoItem, TodoMvcView,
        TodoToggleExternal, DELETE_GLYPH, DELETE_TAG, DELETE_TAG_PREFIX, EDIT_TF_TAG, FILTER_TAG,
        FILTER_TAG_PREFIX, ITEM_TAG, ITEM_TAG_PREFIX, LIST_SCROLL_KEY, LIST_TAG, SCROLLBAR_TAG,
        TF_TAG, TOGGLE_GLYPH_CHECKED, TOGGLE_GLYPH_UNCHECKED, TOGGLE_TAG, TOGGLE_TAG_PREFIX,
    };
    use pinion_a11y::{AriaRole, WidgetA11y};
    use pinion_core::external::{External, IntrospectValue};
    use pinion_core::reactive::Owner;
    use pinion_core::theme::Theme;
    use pinion_core::widget_core::ExtraExternal;
    use pinion_core::widgets::text_field::TextFieldState;
    use pinion_core::{Frame, Scene, WidgetCore};

    /// Run `f` inside a fresh `Owner` scope so reactive hooks
    /// resolve. Mirrors the framework's
    /// `root_owner.run(|| V::view(...))` wrap; tests use a private
    /// scope so each test starts with empty Owner cache state.
    fn with_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    /// Walk the scene tree and return the children of the
    /// `Scene::Container` whose tag matches `tag`. Returns an empty
    /// vec when the tag is not found. Used to confirm the todo list
    /// section's child count + per-item tag layout.
    fn find_children_for_tag<'a>(scene: &'a Scene, tag: &str) -> Vec<&'a Scene> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(tag) {
                    return c.children.iter().collect();
                }
                for child in &c.children {
                    let n = find_children_for_tag(child, tag);
                    if !n.is_empty() {
                        return n;
                    }
                }
                Vec::new()
            }
            Scene::Scroll(s) => find_children_for_tag(&s.content, tag),
            _ => Vec::new(),
        }
    }

    /// Extract the text content of every `Scene::Text` node reachable
    /// under `scene`, depth-first in declaration order. Used to
    /// confirm submitted entries appear as visible text in the same
    /// order the user typed them.
    fn walk_text(s: &Scene, out: &mut Vec<String>) {
        match s {
            Scene::Text(t) => out.push(t.content.clone()),
            Scene::Container(c) => {
                for child in &c.children {
                    walk_text(child, out);
                }
            }
            Scene::Scroll(s) => walk_text(&s.content, out),
            _ => {}
        }
    }
    fn collect_text_nodes(scene: &Scene) -> Vec<String> {
        let mut out = Vec::new();
        walk_text(scene, &mut out);
        out
    }

    /// (R664) Walk the scene tree and collect every `TextNode`'s
    /// resolved `TextStyle` for decoration assertions.
    fn collect_text_styles(scene: &Scene) -> Vec<super::TextStyle> {
        let mut out = Vec::new();
        walk_text_styles(scene, &mut out);
        out
    }
    fn walk_text_styles(scene: &Scene, out: &mut Vec<super::TextStyle>) {
        match scene {
            Scene::Text(t) => out.push(t.style.clone()),
            Scene::Container(c) => {
                for ch in &c.children {
                    walk_text_styles(ch, out);
                }
            }
            Scene::Scroll(s) => walk_text_styles(&s.content, out),
            _ => {}
        }
    }

    // ─────────────────────────────────────────────────────────────
    // Composition — tag presence + list region existence
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r655_view_carries_tf_tag() {
        with_owner(|| {
            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            assert!(
                scene.contains_tag(TF_TAG),
                "paint scene must carry the TextField widget tag",
            );
        });
    }

    #[test]
    fn r655_view_carries_list_tag() {
        with_owner(|| {
            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            assert!(
                scene.contains_tag(LIST_TAG),
                "paint scene must carry the todo-list tag even when empty",
            );
        });
    }

    #[test]
    fn r655_g20_view_contains_composite_paint_root_tag() {
        // R55.G.22 §5.49 — pinned via the framework helper which
        // calls `V::view` under an `Owner::new()` scope and asserts
        // `Scene::contains_tag(V::tag())`.
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<TodoMvcView>(
            (TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0),
            &Frame::default(),
        );
    }

    // ─────────────────────────────────────────────────────────────
    // List rendering — entry count → child count invariant
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r655_empty_list_renders_placeholder_header_only() {
        with_owner(|| {
            // `use_todos()` returns an empty Vec by default
            // (Signal::new(Vec::new()) seed via Owner::cache).
            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            let children = find_children_for_tag(&scene, LIST_TAG);
            assert_eq!(
                children.len(),
                1,
                "empty list has just the placeholder header (no item rows)",
            );
        });
    }

    #[test]
    fn r655_list_grows_with_signal_pushes() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "milk".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 2,
                    text: "eggs".to_owned(),
                    completed: false,
                });
                next
            });
            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            let children = find_children_for_tag(&scene, LIST_TAG);
            // header + 2 item rows
            assert_eq!(
                children.len(),
                3,
                "list children = header + N items (N=2)",
            );
        });
    }

    #[test]
    fn r656_list_items_carry_stable_id_tags() {
        with_owner(|| {
            // R656 — per-item paint tag is `todo_item#<id>` (stable
            // u64), NOT `todo_item#<array_index>`. Use non-sequential
            // ids to lock in the "id, not index" contract — `7 / 42
            // / 99` are visibly distinct from `0 / 1 / 2`.
            let todos = use_todos();
            let ids = [7_u64, 42, 99];
            let names = ["alpha", "beta", "gamma"];
            todos.set_with(|prev| {
                let mut next = prev.clone();
                for (id, name) in ids.iter().zip(names.iter()) {
                    next.push(TodoItem {
                        id: *id,
                        text: (*name).to_owned(),
                        completed: false,
                    });
                }
                next
            });
            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            for id in &ids {
                let needle = format!("{ITEM_TAG_PREFIX}#{id}");
                assert!(
                    scene.contains_tag(needle.as_str()),
                    "scene must carry {needle} for stable id {id}",
                );
                let delete_needle = format!("{DELETE_TAG_PREFIX}#{id}");
                assert!(
                    scene.contains_tag(delete_needle.as_str()),
                    "scene must carry {delete_needle} delete-button tag for stable id {id}",
                );
            }
            // R656 — confirm the OLD R655 index-based tags are NOT
            // present (the contract migration is complete, no dual-
            // namespacing of `todo_item#0` + `todo_item#7` for the
            // same logical item).
            for idx in 0..3 {
                let stale = format!("{ITEM_TAG_PREFIX}#{idx}");
                assert!(
                    !scene.contains_tag(stale.as_str()),
                    "R655 index-based tag {stale} must NOT survive the R656 migration",
                );
            }
        });
    }

    fn find_tagged_container<'a>(
        s: &'a Scene,
        tag: &str,
        acc: &mut Option<&'a Scene>,
    ) {
        if acc.is_some() {
            return;
        }
        // R658 §5.16 — recurse through `Scene::Scroll` content (the
        // todo list is now wrapped in a ScrollNode so the tagged
        // container lives inside `scroll.content`, not at the
        // top-level Container layer).
        if let Scene::Scroll(sc) = s {
            find_tagged_container(&sc.content, tag, acc);
            return;
        }
        if let Scene::Container(c) = s
            && c.tag.as_deref() == Some(tag)
        {
            *acc = Some(s);
            return;
        }
        if let Scene::Container(c) = s {
            for child in &c.children {
                find_tagged_container(child, tag, acc);
            }
        }
    }

    #[test]
    fn r655_list_items_render_entry_text_in_order() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "first".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 2,
                    text: "second".to_owned(),
                    completed: false,
                });
                next
            });
            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            // The list container holds the header + per-item rows;
            // walk the list region only (not the textfield section)
            // so the textfield's status line text doesn't leak in.
            let mut list_root = None;
            find_tagged_container(&scene, LIST_TAG, &mut list_root);
            let list = list_root.expect("LIST_TAG present after push");
            let texts = collect_text_nodes(list);
            // Header text first, then entries in submission order.
            // Header now reads `Todos (2)`; entries follow. R656 —
            // each row also paints a `DELETE_GLYPH` `\u{00D7}` (×)
            // text node, so the per-row text walk yields
            // `[entry_text, "×"]` in declaration order.
            assert_eq!(texts.first().map(String::as_str), Some("Todos (2)"));
            assert!(
                texts.iter().any(|t| t == "first"),
                "list must render 'first' entry",
            );
            assert!(
                texts.iter().any(|t| t == "second"),
                "list must render 'second' entry",
            );
            // Order: 'first' must appear before 'second' in the
            // text-node walk (declaration order = paint order).
            let i_first = texts.iter().position(|t| t == "first").expect("first");
            let i_second = texts.iter().position(|t| t == "second").expect("second");
            assert!(
                i_first < i_second,
                "submission order preserved (first before second)",
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // use_todos hook — Owner::cache dedup + Signal contract
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r655_use_todos_dedups_across_calls() {
        with_owner(|| {
            let a = use_todos();
            let b = use_todos();
            // Two calls in the same Owner scope must resolve to the
            // identical `Rc<Signal<Vec<String>>>` (Owner::cache key
            // dedup), so a mutation on one is visible to the other.
            assert!(
                std::rc::Rc::ptr_eq(&a, &b),
                "use_todos() must return the same Rc within an Owner scope",
            );
        });
    }

    #[test]
    fn r655_use_todos_set_with_appends_in_order() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "a".to_owned(),
                    completed: false,
                });
                next
            });
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 2,
                    text: "b".to_owned(),
                    completed: false,
                });
                next
            });
            let snapshot = todos.get();
            assert_eq!(
                snapshot,
                vec![
                    TodoItem {
                        id: 1,
                        text: "a".to_owned(),
                        completed: false,
                    },
                    TodoItem {
                        id: 2,
                        text: "b".to_owned(),
                        completed: false,
                    },
                ],
                "set_with closure semantics — sequential appends preserve order",
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // build_todos_list — pure helper for the list region. Tests
    // exercise the helper directly with theme + entries so the
    // shape contract is pinned without going through the reactive
    // Owner cache.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r655_build_todos_list_empty_returns_placeholder_header() {
        let theme = Theme::light();
        // R659 — total_count==0 path takes precedence over filter
        // mode, so passing FilterMode::All gives the empty-store
        // header.
        let scene =
            build_todos_list(&theme, &[], FilterMode::All, 0, None, TextFieldState::Idle, 0);
        let texts = collect_text_nodes(&scene);
        assert_eq!(
            texts,
            vec!["No todos yet — type and press Enter".to_owned()],
            "empty list shows the placeholder hint",
        );
    }

    #[test]
    fn r656_build_todos_list_header_reflects_entry_count() {
        let theme = Theme::light();
        // R659 — `visible.len() == total_count` path preserves the
        // R655/R656/R658 header shape: `Todos (N)` when nothing is
        // completed.
        let entries = [
            TodoItem {
                id: 10,
                text: "x".to_owned(),
                completed: false,
            },
            TodoItem {
                id: 20,
                text: "y".to_owned(),
                completed: false,
            },
            TodoItem {
                id: 30,
                text: "z".to_owned(),
                completed: false,
            },
        ];
        let scene = build_todos_list(
            &theme,
            &entries,
            FilterMode::All,
            entries.len(),
            None,
            TextFieldState::Idle,
            0,
        );
        let texts = collect_text_nodes(&scene);
        assert_eq!(texts.first().map(String::as_str), Some("Todos (3)"));
        // R658 — text walk = header + per-row(toggle_glyph +
        // entry_text + delete_glyph) = 1 + 3 * 3 = 10. R656 was
        // header + per-row(entry + delete) = 1 + 3*2 = 7; the R658
        // toggle glyph adds one more text node per row.
        assert_eq!(
            texts.len(),
            10,
            "R658: header + 3 * (toggle + entry + delete glyph) = 10 text nodes",
        );
    }

    // ─────────────────────────────────────────────────────────────
    // R656 — stable id contract: per-item tag survives sibling delete
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r656_stable_id_survives_sibling_delete() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "alpha".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 2,
                    text: "beta".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 3,
                    text: "gamma".to_owned(),
                    completed: false,
                });
                next
            });
            // Delete the middle item (id=2).
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.retain(|item| item.id != 2);
                next
            });
            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            // Surviving items keep their ORIGINAL stable ids — no
            // resequencing to {0,1}. This is the R656 invariant.
            assert!(
                scene.contains_tag("todo_item#1"),
                "alpha (id=1) survives with its original tag",
            );
            assert!(
                !scene.contains_tag("todo_item#2"),
                "beta (id=2) is removed (tag gone)",
            );
            assert!(
                scene.contains_tag("todo_item#3"),
                "gamma (id=3) survives with its original tag",
            );
            // The list count drops from 3 to 2; the header reflects.
            let mut list_root = None;
            find_tagged_container(&scene, LIST_TAG, &mut list_root);
            let list = list_root.expect("LIST_TAG present");
            let texts = collect_text_nodes(list);
            assert_eq!(
                texts.first().map(String::as_str),
                Some("Todos (2)"),
                "header reflects post-delete count",
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R656 — use_next_todo_id / allocate_todo_id monotonic contract
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r656_allocate_todo_id_is_monotonic() {
        with_owner(|| {
            let a = allocate_todo_id();
            let b = allocate_todo_id();
            let c = allocate_todo_id();
            assert!(a < b && b < c, "ids must be strictly increasing");
        });
    }

    #[test]
    fn r656_use_next_todo_id_dedups_across_calls() {
        with_owner(|| {
            let a = use_next_todo_id();
            let b = use_next_todo_id();
            assert!(
                std::rc::Rc::ptr_eq(&a, &b),
                "use_next_todo_id must return the same Rc within an Owner scope",
            );
        });
    }

    #[test]
    fn r656_allocate_does_not_collide_with_existing_ids() {
        // Worked scenario from `apply_key`'s Enter handler: allocate
        // ids monotonically through `allocate_todo_id`, push into
        // `use_todos`, and assert every per-item id is unique.
        with_owner(|| {
            let todos = use_todos();
            for text in ["one", "two", "three", "four"] {
                let id = allocate_todo_id();
                todos.set_with(|prev| {
                    let mut next = prev.clone();
                    next.push(TodoItem {
                        id,
                        text: text.to_owned(),
                        completed: false,
                    });
                    next
                });
            }
            let snapshot = todos.get();
            let mut ids: Vec<u64> = snapshot.iter().map(|i| i.id).collect();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(
                ids.len(),
                snapshot.len(),
                "every allocated id must be unique",
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R656 — TodoDeleteExternal: composite-tag wire + direct invoke
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r656_delete_external_send_pointerdown_removes_item() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 11,
                    text: "alpha".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 22,
                    text: "beta".to_owned(),
                    completed: false,
                });
                next
            });
            let mut handler = TodoDeleteExternal::new(use_todos());
            let result = handler
                .introspect_mut()
                .expect("introspect_mut wired")
                .invoke("send", IntrospectValue::Text("11:PointerDown".to_owned()))
                .expect("PointerDown for id=11 must succeed");
            // R51.42 wire — Bool(was_present) reports whether the
            // delete observed an existing id.
            assert_eq!(result, IntrospectValue::Bool(true));
            let snapshot = use_todos().get();
            assert_eq!(snapshot.len(), 1);
            assert_eq!(snapshot[0].id, 22);
            assert_eq!(snapshot[0].text, "beta");
        });
    }

    #[test]
    fn r656_delete_external_send_pointerup_is_no_op() {
        // PointerUp / Enter / Leave / Cancel are accepted-but-ignored
        // (Bool(false)) so the InputRouter's normal dispatch cycle
        // does not see a Rejected for routine paint events.
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 5,
                    text: "stays".to_owned(),
                    completed: false,
                });
                next
            });
            let mut handler = TodoDeleteExternal::new(use_todos());
            let result = handler
                .introspect_mut()
                .expect("introspect_mut wired")
                .invoke("send", IntrospectValue::Text("5:PointerUp".to_owned()))
                .expect("PointerUp accepted as no-op");
            assert_eq!(result, IntrospectValue::Bool(false));
            // Item remains — PointerUp is ignored.
            assert_eq!(use_todos().get().len(), 1);
        });
    }

    #[test]
    fn r656_delete_external_direct_invoke_path() {
        // Direct RPC route: `scene/invoke {path: "/external/todo_delete",
        // method: "delete", args: <id>}` reaches `invoke("delete",
        // Int(id))` without going through the composite-tag wire.
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 100,
                    text: "first".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 200,
                    text: "second".to_owned(),
                    completed: false,
                });
                next
            });
            let mut handler = TodoDeleteExternal::new(use_todos());
            let result = handler
                .introspect_mut()
                .expect("introspect_mut wired")
                .invoke("delete", IntrospectValue::Int(100))
                .expect("direct delete must succeed");
            assert_eq!(result, IntrospectValue::Bool(true));
            let snapshot = use_todos().get();
            assert_eq!(snapshot.len(), 1);
            assert_eq!(snapshot[0].id, 200);
        });
    }

    #[test]
    fn r656_delete_external_unknown_id_is_idempotent() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 7,
                    text: "alone".to_owned(),
                    completed: false,
                });
                next
            });
            let mut handler = TodoDeleteExternal::new(use_todos());
            let result = handler
                .introspect_mut()
                .expect("introspect_mut wired")
                .invoke("delete", IntrospectValue::Int(999))
                .expect("delete of unknown id returns Ok(Bool(false))");
            // R656 — was_present = false because id=999 never existed.
            assert_eq!(result, IntrospectValue::Bool(false));
            // The surviving item is unaffected.
            assert_eq!(use_todos().get().len(), 1);
        });
    }

    #[test]
    fn r656_delete_external_query_count_matches_signal() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "a".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 2,
                    text: "b".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 3,
                    text: "c".to_owned(),
                    completed: false,
                });
                next
            });
            let handler = TodoDeleteExternal::new(use_todos());
            let count = handler
                .introspect()
                .expect("introspect wired")
                .query("count")
                .expect("count slot exposed");
            assert_eq!(count, IntrospectValue::Int(3));
        });
    }

    #[test]
    fn r656_delete_external_query_ids_returns_json_array() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 7,
                    text: "seven".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 42,
                    text: "answer".to_owned(),
                    completed: false,
                });
                next
            });
            let handler = TodoDeleteExternal::new(use_todos());
            let ids = handler
                .introspect()
                .expect("introspect wired")
                .query("ids")
                .expect("ids slot exposed");
            // Snapshot ordering matches insertion order.
            assert_eq!(
                ids,
                IntrospectValue::Json(serde_json::json!([7, 42])),
            );
        });
    }

    #[test]
    fn r656_delete_external_malformed_send_payload_rejected() {
        with_owner(|| {
            let mut handler = TodoDeleteExternal::new(use_todos());
            // Missing colon → Rejected.
            let no_colon = handler
                .introspect_mut()
                .expect("introspect_mut wired")
                .invoke("send", IntrospectValue::Text("noseparator".to_owned()));
            assert!(no_colon.is_err());
            // Non-integer sub-index → Rejected.
            let bad_id = handler
                .introspect_mut()
                .expect("introspect_mut wired")
                .invoke("send", IntrospectValue::Text("xx:PointerDown".to_owned()));
            assert!(bad_id.is_err());
            // Empty event name → Rejected.
            let no_event = handler
                .introspect_mut()
                .expect("introspect_mut wired")
                .invoke("send", IntrospectValue::Text("1:".to_owned()));
            assert!(no_event.is_err());
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R656 — WidgetCore::create_extra_externals registers the singleton
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r664_create_extra_externals_registers_six_siblings() {
        // R664 §5.16 — extras = [delete, toggle, filter, scrollbar,
        // item-edit-handler, editor-field] (6 entries; R659 added
        // entries 1-4, R664 adds ITEM_TAG + EDIT_TF_TAG). The R656
        // invariant (DELETE_TAG present) continues to hold under
        // additive composition.
        with_owner(|| {
            let extras: Vec<ExtraExternal> =
                <TodoMvcView as WidgetCore>::create_extra_externals();
            assert_eq!(
                extras.len(),
                6,
                "R664: 6 extras (delete + toggle + filter + scrollbar + item-edit + editor-field)",
            );
            let tags: Vec<&str> = extras.iter().map(|e| e.tag.as_ref()).collect();
            assert!(tags.contains(&DELETE_TAG), "DELETE_TAG still registered");
            assert!(tags.contains(&TOGGLE_TAG), "R658 TOGGLE_TAG registered");
            assert!(tags.contains(&FILTER_TAG), "R659 FILTER_TAG registered");
            assert!(tags.contains(&SCROLLBAR_TAG), "R659 SCROLLBAR_TAG registered");
            assert!(tags.contains(&ITEM_TAG), "R664 ITEM_TAG (TodoEditExternal) registered");
            assert!(tags.contains(&EDIT_TF_TAG), "R664 EDIT_TF_TAG (editor TextField) registered");
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R656 §5.40 — WidgetA11y::access_node emits List + ListItem + Button
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r656_access_node_emits_list_root_with_no_items() {
        with_owner(|| {
            let nodes = <TodoMvcView as WidgetA11y>::access_node(
                &(TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0),
                Some(TF_TAG),
            );
            // R659 §5.40 — order: [textbox, RadioGroup(filter),
            // RadioButton×3, List(items)] = 1 + 1 + 3 + 1 = 6 nodes.
            assert_eq!(nodes.len(), 6, "R659: textbox + filter group + 3 radios + list");
            assert_eq!(nodes[0].role, AriaRole::TextInput);
            assert_eq!(nodes[1].role, AriaRole::RadioGroup);
            assert_eq!(nodes[1].tag, FILTER_TAG);
            for i in 0..3 {
                assert_eq!(nodes[2 + i].role, AriaRole::RadioButton);
                assert_eq!(
                    nodes[2 + i].tag,
                    format!("{FILTER_TAG_PREFIX}#{i}"),
                );
            }
            assert_eq!(nodes[5].role, AriaRole::List);
            assert_eq!(nodes[5].tag, LIST_TAG);
            assert!(nodes[5].children.is_empty(), "empty list, no children");
        });
    }

    #[test]
    fn r656_access_node_emits_listitem_and_button_per_entry() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "milk".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 2,
                    text: "eggs".to_owned(),
                    completed: false,
                });
                next
            });
            let nodes = <TodoMvcView as WidgetA11y>::access_node(
                &(TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0),
                Some(TF_TAG),
            );
            // R659 §5.40 — order:
            //   [textbox,
            //    RadioGroup(filter), RadioButton×3,
            //    listitem#1, checkbox(toggle#1), button(delete#1),
            //    listitem#2, checkbox(toggle#2), button(delete#2),
            //    list_root]
            // = 1 + 4 + 6 + 1 = 12 nodes. Index-based assertions
            // walk the tail after the 4-node filter prefix.
            assert_eq!(nodes.len(), 12, "R659: 1 + 4 + 2*3 + 1 = 12 nodes");
            assert_eq!(nodes[0].role, AriaRole::TextInput);
            assert_eq!(nodes[1].role, AriaRole::RadioGroup);
            assert_eq!(nodes[1].tag, FILTER_TAG);
            for i in 0..3 {
                assert_eq!(nodes[2 + i].role, AriaRole::RadioButton);
            }
            // R658 list region after the 4-node filter prefix.
            assert_eq!(nodes[5].role, AriaRole::ListItem);
            assert_eq!(nodes[5].tag, "todo_item#1");
            assert_eq!(nodes[6].role, AriaRole::CheckBox);
            assert_eq!(nodes[6].tag, "todo_toggle#1");
            assert_eq!(
                nodes[6].state.checked,
                Some(false),
                "R658: aria-checked starts false for fresh entries",
            );
            assert_eq!(nodes[7].role, AriaRole::Button);
            assert_eq!(nodes[7].tag, "todo_delete#1");
            assert_eq!(nodes[8].role, AriaRole::ListItem);
            assert_eq!(nodes[8].tag, "todo_item#2");
            assert_eq!(nodes[9].role, AriaRole::CheckBox);
            assert_eq!(nodes[9].tag, "todo_toggle#2");
            assert_eq!(nodes[9].state.checked, Some(false));
            assert_eq!(nodes[10].role, AriaRole::Button);
            assert_eq!(nodes[10].tag, "todo_delete#2");
            // list root references both items by tag.
            assert_eq!(nodes[11].role, AriaRole::List);
            assert_eq!(
                nodes[11].children,
                vec!["todo_item#1".to_owned(), "todo_item#2".to_owned()],
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R656 — DELETE_GLYPH appears in the per-row text walk
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r656_delete_glyph_appears_per_row() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "row".to_owned(),
                    completed: false,
                });
                next
            });
            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            let mut list_root = None;
            find_tagged_container(&scene, LIST_TAG, &mut list_root);
            let list = list_root.expect("LIST_TAG present");
            let texts = collect_text_nodes(list);
            // header + toggle glyph + entry + × glyph
            assert!(
                texts.iter().any(|t| t == DELETE_GLYPH),
                "per-row × glyph present",
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R658 §5.16 — TodoItem.completed migration + toggle glyph
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r658_fresh_todo_item_starts_active() {
        // The Enter handler stamps `completed: false` on freshly
        // pushed entries (mirrored in `r656_allocate_does_not_collide`
        // setup). Confirm directly via a `TodoItem` ctor: the field
        // is a plain `bool` with no derived default, so a Clone/Debug
        // round-trip preserves the explicit value.
        let item = TodoItem {
            id: 1,
            text: "fresh".to_owned(),
            completed: false,
        };
        assert!(!item.completed, "R658: fresh todo defaults to active");
        let toggled = TodoItem {
            completed: !item.completed,
            ..item.clone()
        };
        assert!(toggled.completed, "R658: flip flips the bool");
        assert_eq!(toggled.id, item.id, "id stays stable under toggle");
        assert_eq!(toggled.text, item.text, "text stays stable under toggle");
    }

    #[test]
    fn r658_unchecked_toggle_glyph_appears_per_active_row() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "milk".to_owned(),
                    completed: false,
                });
                next
            });
            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            let mut list_root = None;
            find_tagged_container(&scene, LIST_TAG, &mut list_root);
            let list = list_root.expect("LIST_TAG present (inside Scroll)");
            let texts = collect_text_nodes(list);
            assert!(
                texts.iter().any(|t| t == TOGGLE_GLYPH_UNCHECKED),
                "R658: active row paints unchecked U+2610 glyph",
            );
            assert!(
                !texts.iter().any(|t| t == TOGGLE_GLYPH_CHECKED),
                "R658: active row must NOT paint checked U+2611",
            );
        });
    }

    #[test]
    fn r658_checked_toggle_glyph_appears_per_completed_row() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "milk".to_owned(),
                    completed: true,
                });
                next
            });
            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            let mut list_root = None;
            find_tagged_container(&scene, LIST_TAG, &mut list_root);
            let list = list_root.expect("LIST_TAG present (inside Scroll)");
            let texts = collect_text_nodes(list);
            assert!(
                texts.iter().any(|t| t == TOGGLE_GLYPH_CHECKED),
                "R658: completed row paints checked U+2611 glyph",
            );
            assert!(
                !texts.iter().any(|t| t == TOGGLE_GLYPH_UNCHECKED),
                "R658: completed row must NOT paint unchecked U+2610",
            );
        });
    }

    #[test]
    fn r658_view_carries_toggle_tag_per_item() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 11,
                    text: "x".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 22,
                    text: "y".to_owned(),
                    completed: true,
                });
                next
            });
            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            for id in [11_u64, 22] {
                let needle = format!("{TOGGLE_TAG_PREFIX}#{id}");
                assert!(
                    scene.contains_tag(needle.as_str()),
                    "R658: scene must carry {needle} per-item toggle tag",
                );
            }
        });
    }

    #[test]
    fn r658_completed_header_reflects_progress() {
        // Header text is `"Todos (N)"` when no entry is completed
        // and `"Todos (X of N completed)"` when at least one is.
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "a".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 2,
                    text: "b".to_owned(),
                    completed: true,
                });
                next.push(TodoItem {
                    id: 3,
                    text: "c".to_owned(),
                    completed: false,
                });
                next
            });
            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            let mut list_root = None;
            find_tagged_container(&scene, LIST_TAG, &mut list_root);
            let list = list_root.expect("LIST_TAG present");
            let texts = collect_text_nodes(list);
            assert_eq!(
                texts.first().map(String::as_str),
                Some("Todos (1 of 3 completed)"),
                "R658: progress header reflects completed count",
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R658 §5.16 — TodoToggleExternal: composite-tag + direct invoke
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r658_toggle_external_send_pointerdown_flips_completed() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 11,
                    text: "alpha".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 22,
                    text: "beta".to_owned(),
                    completed: false,
                });
                next
            });
            let mut handler = TodoToggleExternal::new(use_todos());
            let result = handler
                .introspect_mut()
                .expect("introspect_mut wired")
                .invoke(
                    "send",
                    IntrospectValue::Text("11:PointerDown".to_owned()),
                )
                .expect("PointerDown for id=11 must succeed");
            assert_eq!(
                result,
                IntrospectValue::Bool(true),
                "send/PointerDown returns Bool(was_present)",
            );
            let snapshot = use_todos().get();
            // id=11 flipped, id=22 untouched. Stable count.
            assert_eq!(snapshot.len(), 2);
            assert!(
                snapshot.iter().find(|i| i.id == 11).unwrap().completed,
                "id=11 toggled to completed=true",
            );
            assert!(
                !snapshot.iter().find(|i| i.id == 22).unwrap().completed,
                "id=22 untouched (sibling completed flag preserved)",
            );
        });
    }

    #[test]
    fn r658_toggle_external_send_double_flips_back() {
        // Two consecutive PointerDowns flip the completed flag on +
        // off — the toggle is monoidal, NOT idempotent (a 2nd click
        // un-completes the entry).
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 5,
                    text: "flip".to_owned(),
                    completed: false,
                });
                next
            });
            let mut handler = TodoToggleExternal::new(use_todos());
            handler
                .introspect_mut()
                .unwrap()
                .invoke(
                    "send",
                    IntrospectValue::Text("5:PointerDown".to_owned()),
                )
                .unwrap();
            assert!(use_todos().get()[0].completed, "1st flip → true");
            handler
                .introspect_mut()
                .unwrap()
                .invoke(
                    "send",
                    IntrospectValue::Text("5:PointerDown".to_owned()),
                )
                .unwrap();
            assert!(
                !use_todos().get()[0].completed,
                "2nd flip → false (monoidal toggle, NOT idempotent)",
            );
        });
    }

    #[test]
    fn r658_toggle_external_send_pointerup_is_no_op() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 5,
                    text: "stays".to_owned(),
                    completed: false,
                });
                next
            });
            let mut handler = TodoToggleExternal::new(use_todos());
            let result = handler
                .introspect_mut()
                .unwrap()
                .invoke(
                    "send",
                    IntrospectValue::Text("5:PointerUp".to_owned()),
                )
                .unwrap();
            assert_eq!(
                result,
                IntrospectValue::Bool(false),
                "PointerUp accepted as no-op (no Rejected)",
            );
            assert!(
                !use_todos().get()[0].completed,
                "PointerUp does NOT flip completed",
            );
        });
    }

    #[test]
    fn r658_toggle_external_direct_invoke_returns_post_state() {
        // Direct `invoke("toggle", Int(id))` returns the **post-flip**
        // completed bool (parallel to delete's `was_present`, but for
        // toggle the post-state is the AI-useful value).
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 100,
                    text: "x".to_owned(),
                    completed: false,
                });
                next
            });
            let mut handler = TodoToggleExternal::new(use_todos());
            let result = handler
                .introspect_mut()
                .unwrap()
                .invoke("toggle", IntrospectValue::Int(100))
                .unwrap();
            assert_eq!(
                result,
                IntrospectValue::Bool(true),
                "1st toggle of id=100 returns Bool(true) post-flip",
            );
            let result2 = handler
                .introspect_mut()
                .unwrap()
                .invoke("toggle", IntrospectValue::Int(100))
                .unwrap();
            assert_eq!(
                result2,
                IntrospectValue::Bool(false),
                "2nd toggle of id=100 returns Bool(false) post-flip",
            );
        });
    }

    #[test]
    fn r658_toggle_external_unknown_id_is_silent() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 7,
                    text: "alone".to_owned(),
                    completed: false,
                });
                next
            });
            let mut handler = TodoToggleExternal::new(use_todos());
            let result = handler
                .introspect_mut()
                .unwrap()
                .invoke("toggle", IntrospectValue::Int(999))
                .unwrap();
            // Unknown id → Bool(false) (no flip happened, item not
            // found). The existing entry is unaffected.
            assert_eq!(result, IntrospectValue::Bool(false));
            assert!(!use_todos().get()[0].completed);
        });
    }

    #[test]
    fn r658_toggle_external_query_completed_count() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "a".to_owned(),
                    completed: true,
                });
                next.push(TodoItem {
                    id: 2,
                    text: "b".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 3,
                    text: "c".to_owned(),
                    completed: true,
                });
                next
            });
            let handler = TodoToggleExternal::new(use_todos());
            let intro = handler.introspect().unwrap();
            assert_eq!(
                intro.query("count").unwrap(),
                IntrospectValue::Int(3),
                "count slot mirrors total entries",
            );
            assert_eq!(
                intro.query("completed_count").unwrap(),
                IntrospectValue::Int(2),
                "completed_count slot mirrors completed entries",
            );
            // ids_completed lists only completed ids in declaration order.
            assert_eq!(
                intro.query("ids_completed").unwrap(),
                IntrospectValue::Json(serde_json::json!([1, 3])),
                "ids_completed JSON array preserves insertion order",
            );
        });
    }

    #[test]
    fn r658_toggle_external_malformed_send_rejected() {
        with_owner(|| {
            let mut handler = TodoToggleExternal::new(use_todos());
            // Missing colon → Rejected.
            let no_colon = handler
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("noseparator".to_owned()));
            assert!(no_colon.is_err());
            // Non-integer sub-index → Rejected.
            let bad_id = handler
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("xx:PointerDown".to_owned()));
            assert!(bad_id.is_err());
            // Empty event name → Rejected.
            let no_event = handler
                .introspect_mut()
                .unwrap()
                .invoke("send", IntrospectValue::Text("1:".to_owned()));
            assert!(no_event.is_err());
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R658 §5.16 — ScrollNode wrap (WIN_H magic cleanup)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r658_view_wraps_list_in_scroll_node() {
        // R658 — the LIST_TAG container must live inside a
        // Scene::Scroll wrapper (WIN_H magic cleanup). R659 wraps
        // that Scroll + scrollbar peer in a flex Row "scroll
        // region" Container, so the Scroll is now a grandchild of
        // the outer Container (not a direct child). Walk recursively.
        fn walk(scene: &Scene) -> Option<&pinion_core::scene::ScrollNode> {
            match scene {
                Scene::Scroll(s) => Some(s),
                Scene::Container(c) => c.children.iter().find_map(walk),
                _ => None,
            }
        }
        with_owner(|| {
            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            let scroll =
                walk(&scene).expect("R659: Scroll wrapping LIST_TAG must exist");
            assert!(
                scroll.content.contains_tag(LIST_TAG),
                "R659: Scroll content must contain LIST_TAG",
            );
            assert_eq!(
                scroll.tag.as_deref(),
                Some(LIST_SCROLL_KEY),
                "R658: ScrollNode tag derived from LIST_SCROLL_KEY",
            );
        });
    }

    #[test]
    fn r658_scroll_state_persists_across_view_calls() {
        with_owner(|| {
            // First view call instantiates ScrollState in the Owner
            // cache.
            let _ = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            let scroll_state_a =
                pinion_core::widgets::scroll::use_scroll_state(LIST_SCROLL_KEY);
            let scroll_state_b =
                pinion_core::widgets::scroll::use_scroll_state(LIST_SCROLL_KEY);
            assert!(
                std::rc::Rc::ptr_eq(&scroll_state_a, &scroll_state_b),
                "R658: use_scroll_state(LIST_SCROLL_KEY) dedups across calls",
            );
        });
    }

    #[test]
    fn r658_scroll_state_offset_round_trips() {
        with_owner(|| {
            let _ = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            let scroll_state =
                pinion_core::widgets::scroll::use_scroll_state(LIST_SCROLL_KEY);
            // Declare a max bound + scroll. The view fn's
            // ScrollNode::from_state reads offset() on the next paint
            // so the value flows out to the rendered geometry.
            scroll_state.set_max(0, 200);
            scroll_state.scroll_to(0, 80);
            assert_eq!(
                scroll_state.offset(),
                (0, 80),
                "R658: scroll_to round-trips into offset()",
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R659 §5.16 — FilterMode enum + use_filter() hook contracts
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r659_filter_mode_default_is_all() {
        // Boot default = All (mirrors TasteJS TodoMVC reference + the
        // macOS/iOS Reminders defaults — no entry hidden until the
        // user opts in).
        assert_eq!(FilterMode::default_mode(), FilterMode::All);
    }

    #[test]
    fn r659_filter_mode_index_round_trip() {
        for mode in [FilterMode::Active, FilterMode::Completed, FilterMode::All] {
            assert_eq!(
                FilterMode::from_index(mode.to_index()),
                Some(mode),
                "round-trip: from_index(to_index({mode:?})) == Some({mode:?})",
            );
        }
        // Out-of-range index → None (no panic).
        assert_eq!(FilterMode::from_index(3), None);
        assert_eq!(FilterMode::from_index(usize::MAX), None);
    }

    #[test]
    fn r659_filter_mode_canonical_discriminants() {
        // The wire form depends on these — pin them so a future
        // accidental re-ordering doesn't silently break AI clients.
        assert_eq!(FilterMode::Active.to_index(), 0);
        assert_eq!(FilterMode::Completed.to_index(), 1);
        assert_eq!(FilterMode::All.to_index(), 2);
    }

    #[test]
    fn r659_filter_mode_matches_predicate() {
        let active = TodoItem {
            id: 1,
            text: "a".into(),
            completed: false,
        };
        let done = TodoItem {
            id: 2,
            text: "d".into(),
            completed: true,
        };
        assert!(FilterMode::Active.matches(&active));
        assert!(!FilterMode::Active.matches(&done));
        assert!(!FilterMode::Completed.matches(&active));
        assert!(FilterMode::Completed.matches(&done));
        assert!(FilterMode::All.matches(&active));
        assert!(FilterMode::All.matches(&done));
    }

    #[test]
    fn r659_use_filter_dedups_across_calls() {
        with_owner(|| {
            let a = use_filter();
            let b = use_filter();
            assert!(
                std::rc::Rc::ptr_eq(&a, &b),
                "use_filter() must return the same Rc within an Owner scope",
            );
        });
    }

    #[test]
    fn r659_use_filter_seeds_to_all() {
        with_owner(|| {
            let f = use_filter();
            assert_eq!(f.get(), FilterMode::All, "fresh Owner seeds to All");
        });
    }

    #[test]
    fn r659_filter_mode_labels_match_taste_js_reference() {
        // Localisation lift is a future axis; for R659 the English
        // literals follow the canonical TasteJS TodoMVC reference.
        assert_eq!(FilterMode::Active.label(), "Active");
        assert_eq!(FilterMode::Completed.label(), "Completed");
        assert_eq!(FilterMode::All.label(), "All");
    }

    // ─────────────────────────────────────────────────────────────
    // R660 §5.16 — Option β walk-back: V::update reducer bridges the
    // RadioGroupExternal's "selected" intent into Signal<FilterMode>.
    // Test the application's observable surface (the Signal write),
    // not the framework substrate (RadioGroupExternal::send + intent
    // emission is covered by pinion-core's own unit suite).
    // ─────────────────────────────────────────────────────────────

    use pinion_core::intent::Intent;

    /// Drive the [`TodoMvcView::update`] reducer with the wire-form
    /// `"todo_filter.selected"` intent the framework
    /// [`RadioGroupExternal`] emits (after the runtime's
    /// `intent_queue::drain_one` prefix walk; the bare event name is
    /// `"selected"`, the wire-form match arm sees the dotted form per
    /// [[intent-tag-dotted-wire-form]]).
    fn drive_filter_selected_intent(idx: i64) {
        let intent = Intent::new_owned(
            "todo_filter.selected".to_owned(),
            IntrospectValue::Int(idx),
        );
        let _ = TodoMvcView::update(
            (TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0),
            &intent,
        );
    }

    #[test]
    fn r660_update_reducer_selected_zero_writes_active() {
        with_owner(|| {
            drive_filter_selected_intent(0);
            assert_eq!(use_filter().get(), FilterMode::Active);
        });
    }

    #[test]
    fn r660_update_reducer_selected_one_writes_completed() {
        with_owner(|| {
            drive_filter_selected_intent(1);
            assert_eq!(use_filter().get(), FilterMode::Completed);
        });
    }

    #[test]
    fn r660_update_reducer_selected_two_writes_all() {
        with_owner(|| {
            use_filter().set(FilterMode::Active);
            drive_filter_selected_intent(2);
            assert_eq!(use_filter().get(), FilterMode::All);
        });
    }

    #[test]
    fn r660_update_reducer_out_of_range_silently_no_ops() {
        with_owner(|| {
            use_filter().set(FilterMode::Completed);
            drive_filter_selected_intent(99);
            // Out-of-range index falls through the `from_index`
            // narrowing — the Signal stays at the prior value rather
            // than panicking, matching the
            // [[reducer-cascade-discipline]] silent-fall-through
            // convention.
            assert_eq!(use_filter().get(), FilterMode::Completed);
        });
    }

    #[test]
    fn r660_update_reducer_ignores_unrelated_intent_tag() {
        with_owner(|| {
            use_filter().set(FilterMode::Active);
            let intent = Intent::new_owned(
                "todo_scrollbar.scroll_committed".to_owned(),
                IntrospectValue::Null,
            );
            let _ = TodoMvcView::update(
                (TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0),
                &intent,
            );
            // Reducer is intent-tag-specific; the
            // [[intent-tag-dotted-wire-form]] full prefix-match arm
            // protects unrelated intents from accidental writes.
            assert_eq!(use_filter().get(), FilterMode::Active);
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R659 §5.16 — view fn: filter row presence + filtered list shape
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r659_view_carries_filter_tag_and_three_buttons() {
        with_owner(|| {
            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            assert!(scene.contains_tag(FILTER_TAG), "filter group root tag present");
            for i in 0..3 {
                let tag = format!("{FILTER_TAG_PREFIX}#{i}");
                assert!(
                    scene.contains_tag(&tag),
                    "filter button {tag} must be paint-addressable",
                );
            }
        });
    }

    #[test]
    fn r659_view_carries_scrollbar_tag() {
        with_owner(|| {
            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            assert!(
                scene.contains_tag(SCROLLBAR_TAG),
                "scrollbar peer Container tag must be paint-addressable",
            );
        });
    }

    #[test]
    fn r659_filter_active_hides_completed_rows() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "a".into(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 2,
                    text: "b".into(),
                    completed: true,
                });
                next.push(TodoItem {
                    id: 3,
                    text: "c".into(),
                    completed: false,
                });
                next
            });
            use_filter().set(FilterMode::Active);

            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            // Only a + c paint (b is completed → filtered out).
            assert!(scene.contains_tag("todo_item#1"));
            assert!(!scene.contains_tag("todo_item#2"), "completed row hidden under Active filter");
            assert!(scene.contains_tag("todo_item#3"));
        });
    }

    #[test]
    fn r659_filter_completed_hides_active_rows() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "a".into(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 2,
                    text: "b".into(),
                    completed: true,
                });
                next
            });
            use_filter().set(FilterMode::Completed);

            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            assert!(!scene.contains_tag("todo_item#1"), "active row hidden under Completed filter");
            assert!(scene.contains_tag("todo_item#2"));
        });
    }

    #[test]
    fn r659_filter_all_shows_everything() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: 1,
                    text: "a".into(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: 2,
                    text: "b".into(),
                    completed: true,
                });
                next
            });
            use_filter().set(FilterMode::All);

            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            assert!(scene.contains_tag("todo_item#1"));
            assert!(scene.contains_tag("todo_item#2"));
        });
    }

    #[test]
    fn r659_header_under_filter_shows_filtername_visible_of_total() {
        with_owner(|| {
            let todos = use_todos();
            todos.set_with(|prev| {
                let mut next = prev.clone();
                for (id, completed) in [(1, false), (2, true), (3, false)] {
                    next.push(TodoItem {
                        id,
                        text: format!("t{id}"),
                        completed,
                    });
                }
                next
            });
            use_filter().set(FilterMode::Active);

            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            let mut list_root = None;
            find_tagged_container(&scene, LIST_TAG, &mut list_root);
            let list = list_root.expect("LIST_TAG present");
            let texts = collect_text_nodes(list);
            assert_eq!(
                texts.first().map(String::as_str),
                Some("Active: 2 of 3"),
                "R659 header: <FilterName>: visible of total when filter hides rows",
            );
        });
    }

    // R664 — R659 duplicate test (registers_four_siblings) retired
    // for the consolidated `r664_create_extra_externals_registers_six_siblings`
    // above which pins the per-R664 count + tag membership in one
    // place.

    #[test]
    fn r659_build_filter_row_emits_all_three_buttons_with_aria_labels() {
        let theme = Theme::light();
        let scene = build_filter_row(&theme, FilterMode::All, &FilterRadioStates::default());
        let Scene::Container(group) = &scene else {
            panic!("filter row must be a Container");
        };
        assert_eq!(group.tag.as_deref(), Some(FILTER_TAG));
        assert_eq!(group.children.len(), 3, "3 segmented buttons");
        for (i, child) in group.children.iter().enumerate() {
            let Scene::Container(btn) = child else {
                panic!("filter button {i} must be Container");
            };
            assert_eq!(btn.tag.as_deref(), Some(format!("{FILTER_TAG_PREFIX}#{i}").as_str()));
            // aria-label populated for screen reader announcement.
            assert!(
                btn.aria_label.is_some(),
                "filter button {i} carries aria-label",
            );
        }
    }

    #[test]
    fn r659_build_filter_row_active_button_uses_accent_fill() {
        let theme = Theme::light();
        let scene = build_filter_row(&theme, FilterMode::Completed, &FilterRadioStates::default());
        let Scene::Container(group) = &scene else {
            panic!("group");
        };
        // The Completed button (index 1) must have accent fill;
        // others get transparent.
        let Scene::Container(active_btn) = &group.children[1] else {
            panic!("button 1");
        };
        assert_eq!(
            active_btn.style.fill,
            theme.resolve(super::ColorRole::Accent),
            "engaged segment uses Accent fill",
        );
        let Scene::Container(other_btn) = &group.children[0] else {
            panic!("button 0");
        };
        assert_ne!(
            other_btn.style.fill,
            theme.resolve(super::ColorRole::Accent),
            "inactive segment does NOT use Accent",
        );
    }

    #[test]
    fn r659_access_node_emits_filter_radiogroup_with_correct_checked() {
        // R659 §5.40 — filter group contributes 1 RadioGroup parent +
        // 3 RadioButton children with aria-checked reflecting the
        // current FilterMode.
        with_owner(|| {
            use_filter().set(FilterMode::Completed);
            let nodes = <TodoMvcView as WidgetA11y>::access_node(
                &(TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0),
                Some(TF_TAG),
            );
            // Find the RadioGroup with FILTER_TAG.
            let rg = nodes
                .iter()
                .find(|n| n.role == AriaRole::RadioGroup && n.tag == FILTER_TAG)
                .expect("RadioGroup with FILTER_TAG present");
            assert_eq!(rg.children.len(), 3, "RadioGroup claims 3 child radios");
            assert_eq!(rg.name.as_deref(), Some("Filter"));

            // Pull each per-button RadioButton node.
            for mode in [FilterMode::Active, FilterMode::Completed, FilterMode::All] {
                let tag = format!("{FILTER_TAG_PREFIX}#{}", mode.to_index());
                let node = nodes
                    .iter()
                    .find(|n| n.role == AriaRole::RadioButton && n.tag == tag)
                    .unwrap_or_else(|| panic!("RadioButton {tag} present"));
                let expect = mode == FilterMode::Completed;
                assert_eq!(
                    node.state.checked,
                    Some(expect),
                    "{tag} aria-checked must reflect current filter (expect {expect})",
                );
            }
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R659 §5.45 — scrollbar peer wire (R658 honest carry payment)
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r659_scroll_region_holds_scroll_plus_scrollbar_side_by_side() {
        with_owner(|| {
            let scene = view((TextFieldState::Idle, 0, FilterRadioStates::default(), TextFieldState::Idle, 0), &Frame::default());
            let Scene::Container(outer) = &scene else {
                panic!("outer Container");
            };
            // Find the scroll region wrapper — it's the child that
            // holds both a Scroll and a Container tagged SCROLLBAR_TAG.
            let region = outer.children.iter().find_map(|c| match c {
                Scene::Container(inner) => {
                    let has_scroll =
                        inner.children.iter().any(|ch| matches!(ch, Scene::Scroll(_)));
                    let has_scrollbar =
                        inner.children.iter().any(|ch| match ch {
                            Scene::Container(cc) => cc.tag.as_deref() == Some(SCROLLBAR_TAG),
                            _ => false,
                        });
                    if has_scroll && has_scrollbar {
                        Some(inner)
                    } else {
                        None
                    }
                }
                _ => None,
            });
            let region = region.expect("scroll_region with Scroll + scrollbar peer present");
            assert_eq!(region.children.len(), 2, "exactly Scroll + scrollbar peer");
            assert_eq!(
                region.layout.flex_direction,
                super::FlexDirection::Row,
                "scroll region must be flex Row so the peer sits beside the list",
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R664 §5.16 §5.38 — edit-in-place axis
    // ─────────────────────────────────────────────────────────────

    /// (R664) Default `editing_id` is `None` — no row in edit mode at
    /// boot. The `use_editing_id` hook's `Signal::new(None)` factory
    /// runs lazily under `Owner::cache`; first read materialises the
    /// signal in the steady state.
    #[test]
    fn r664_use_editing_id_defaults_to_none() {
        with_owner(|| {
            assert_eq!(super::use_editing_id().get(), None);
        });
    }

    /// (R664) `TodoEditExternal::begin_edit` activates row by id:
    /// `editing_id := Some(id)` + editor `TextEditState` seeded with
    /// the current item text. Returns `true` when id matched.
    #[test]
    fn r664_begin_edit_activates_row_and_seeds_text() {
        with_owner(|| {
            // Seed one entry.
            let id = super::allocate_todo_id();
            use_todos().set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id,
                    text: "first thing".to_owned(),
                    completed: false,
                });
                next
            });
            let edit = super::TodoEditExternal::new(use_todos());
            let was = edit.begin_edit(id);
            assert!(was, "existing id matches");
            assert_eq!(super::use_editing_id().get(), Some(id));
            assert_eq!(
                pinion_core::widgets::text_edit::use_text_edit_state(EDIT_TF_TAG)
                    .text(),
                "first thing",
            );
        });
    }

    /// (R664) `begin_edit` on a stale id is a silent no-op — returns
    /// `false`, leaves `editing_id` unchanged.
    #[test]
    fn r664_begin_edit_with_stale_id_returns_false() {
        with_owner(|| {
            let edit = super::TodoEditExternal::new(use_todos());
            let was = edit.begin_edit(99_999);
            assert!(!was);
            assert_eq!(super::use_editing_id().get(), None);
        });
    }

    /// (R664) view fn paints an inline `view_field` editor (tagged
    /// `EDIT_TF_TAG`) inside the edit-mode row when
    /// `editing_id == Some(item.id)`. The non-editing rows keep the
    /// static text label — the swap is row-local.
    #[test]
    fn r664_view_renders_editor_in_active_row() {
        with_owner(|| {
            let id_a = super::allocate_todo_id();
            let id_b = super::allocate_todo_id();
            use_todos().set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id: id_a,
                    text: "alpha".to_owned(),
                    completed: false,
                });
                next.push(TodoItem {
                    id: id_b,
                    text: "beta".to_owned(),
                    completed: false,
                });
                next
            });
            super::use_editing_id().set(Some(id_b));
            let scene = view(
                (
                    TextFieldState::Idle,
                    0,
                    FilterRadioStates::default(),
                    TextFieldState::Idle,
                    0,
                ),
                &super::Frame::default(),
            );
            assert!(
                scene.contains_tag(EDIT_TF_TAG),
                "edit-mode row must paint the EDIT_TF_TAG editor",
            );
        });
    }

    /// (R664) When no row is editing, the paint scene contains no
    /// `EDIT_TF_TAG` tag — the row swap is conditional, not always-on.
    #[test]
    fn r664_view_omits_editor_when_not_editing() {
        with_owner(|| {
            let id = super::allocate_todo_id();
            use_todos().set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id,
                    text: "x".to_owned(),
                    completed: false,
                });
                next
            });
            super::use_editing_id().set(None);
            let scene = view(
                (
                    TextFieldState::Idle,
                    0,
                    FilterRadioStates::default(),
                    TextFieldState::Idle,
                    0,
                ),
                &super::Frame::default(),
            );
            assert!(
                !scene.contains_tag(EDIT_TF_TAG),
                "no editor when editing_id is None",
            );
        });
    }

    /// (R664) `commit_edit` writes the editor text back to the
    /// matching row + clears the edit state. The original item's
    /// `completed` field is preserved (commit edits the text only).
    #[test]
    fn r664_commit_edit_updates_row_text_preserving_completed() {
        with_owner(|| {
            let id = super::allocate_todo_id();
            use_todos().set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id,
                    text: "old".to_owned(),
                    completed: true,
                });
                next
            });
            super::use_editing_id().set(Some(id));
            pinion_core::widgets::text_edit::use_text_edit_state(EDIT_TF_TAG)
                .set_text("new".to_owned());
            let mut scene = super::Scene::Container(super::ContainerNode::new(vec![]));
            super::commit_edit(&mut scene);
            let items = use_todos().get();
            let updated = items.iter().find(|i| i.id == id).unwrap();
            assert_eq!(updated.text, "new", "row text updated");
            assert!(updated.completed, "completed flag preserved");
            assert_eq!(super::use_editing_id().get(), None, "edit cleared");
        });
    }

    /// (R664) `commit_edit` with whitespace-only text removes the
    /// row — the `TasteJS` `TodoMVC` "erase to delete" convention.
    #[test]
    fn r664_commit_edit_blank_text_deletes_row() {
        with_owner(|| {
            let id = super::allocate_todo_id();
            use_todos().set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id,
                    text: "x".to_owned(),
                    completed: false,
                });
                next
            });
            super::use_editing_id().set(Some(id));
            pinion_core::widgets::text_edit::use_text_edit_state(EDIT_TF_TAG)
                .set_text("   ".to_owned());
            let mut scene = super::Scene::Container(super::ContainerNode::new(vec![]));
            super::commit_edit(&mut scene);
            assert!(
                !use_todos().get().iter().any(|i| i.id == id),
                "blank commit removes the row",
            );
        });
    }

    /// (R664) `cancel_edit` clears `editing_id` without mutating
    /// the row text.
    #[test]
    fn r664_cancel_edit_clears_state_without_writing() {
        with_owner(|| {
            let id = super::allocate_todo_id();
            use_todos().set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id,
                    text: "keep".to_owned(),
                    completed: false,
                });
                next
            });
            super::use_editing_id().set(Some(id));
            pinion_core::widgets::text_edit::use_text_edit_state(EDIT_TF_TAG)
                .set_text("ignored typing".to_owned());
            super::cancel_edit();
            assert_eq!(super::use_editing_id().get(), None);
            let items = use_todos().get();
            assert_eq!(
                items.iter().find(|i| i.id == id).unwrap().text,
                "keep",
                "row text untouched on cancel",
            );
        });
    }

    /// (R664) `end_edit_mode` queues a focus return to `TF_TAG` via
    /// the substrate [`pinion_core::focus_request`] channel — the
    /// shell drains it on the next `handle_tail` call.
    #[test]
    fn r664_end_edit_queues_focus_request_for_main_field() {
        with_owner(|| {
            // Drain any prior request from sibling tests in the
            // same thread.
            let _ = pinion_core::focus_request::drain();
            super::end_edit_mode();
            assert_eq!(
                pinion_core::focus_request::drain(),
                Some(TF_TAG.to_owned()),
                "end_edit_mode requests focus back on the main field",
            );
        });
    }

    /// (R664) `begin_edit` queues a focus request for `EDIT_TF_TAG`
    /// so the substrate drain focuses the editor before the next
    /// paint cycle.
    #[test]
    fn r664_begin_edit_queues_focus_request_for_editor() {
        with_owner(|| {
            let id = super::allocate_todo_id();
            use_todos().set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id,
                    text: "z".to_owned(),
                    completed: false,
                });
                next
            });
            let _ = pinion_core::focus_request::drain();
            let edit = super::TodoEditExternal::new(use_todos());
            let _ = edit.begin_edit(id);
            assert_eq!(
                pinion_core::focus_request::drain(),
                Some(EDIT_TF_TAG.to_owned()),
                "begin_edit requests focus on the editor",
            );
        });
    }

    /// (R664) `TodoEditExternal::invoke("send", "<id>:DoubleClick")`
    /// activates edit mode via the composite-tag wire (the framework's
    /// W3C double-click detection + RPC `scene/double_click` drain
    /// both feed this path).
    #[test]
    fn r664_todo_edit_external_send_double_click_begins_edit() {
        with_owner(|| {
            let id = super::allocate_todo_id();
            use_todos().set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id,
                    text: "wire".to_owned(),
                    completed: false,
                });
                next
            });
            let mut edit = super::TodoEditExternal::new(use_todos());
            let res = super::ExternalIntrospect::invoke(
                &mut edit,
                "send",
                super::IntrospectValue::Text(format!("{id}:DoubleClick")),
            );
            assert!(matches!(res, Ok(super::IntrospectValue::Bool(true))));
            assert_eq!(super::use_editing_id().get(), Some(id));
        });
    }

    /// (R664) Non-DoubleClick composite-tag events on
    /// `TodoEditExternal` no-op — `PointerEnter` / Down / Up / Leave
    /// stay routed to sibling externals (toggle / delete) for normal
    /// single-click semantics.
    #[test]
    fn r664_todo_edit_external_ignores_pointer_events() {
        with_owner(|| {
            let mut edit = super::TodoEditExternal::new(use_todos());
            for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
                let res = super::ExternalIntrospect::invoke(
                    &mut edit,
                    "send",
                    super::IntrospectValue::Text(format!("1:{ev}")),
                );
                assert!(
                    matches!(res, Ok(super::IntrospectValue::Bool(false))),
                    "{ev} must not enter edit mode",
                );
            }
            assert_eq!(super::use_editing_id().get(), None);
        });
    }

    /// (R664) `EDIT_TF_TAG` joins the focusable enumeration so the
    /// programmatic `focus_request` from `begin_edit` succeeds. The
    /// main field + editor + filter group give exactly 3 tab stops.
    #[test]
    fn r664_focusable_tags_includes_editor_slot() {
        let tags = <TodoMvcView as super::WidgetCore>::focusable_tags();
        assert_eq!(tags, vec![TF_TAG, EDIT_TF_TAG, FILTER_TAG]);
    }

    /// (R664) Completed rows render their text label with
    /// `TextDecoration::strikethrough()` so the visual + colour cues
    /// reinforce the done state (WCAG 1.4.1 non-colour-only signalling).
    #[test]
    fn r664_completed_row_text_has_strikethrough_decoration() {
        with_owner(|| {
            let id = super::allocate_todo_id();
            use_todos().set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id,
                    text: "done".to_owned(),
                    completed: true,
                });
                next
            });
            super::use_editing_id().set(None);
            let scene = view(
                (
                    TextFieldState::Idle,
                    0,
                    FilterRadioStates::default(),
                    TextFieldState::Idle,
                    0,
                ),
                &super::Frame::default(),
            );
            let text_styles = collect_text_styles(&scene);
            // The row text node carries strikethrough; sibling
            // toggle glyph + delete glyph + headers do not. At
            // least one text node must have the decoration.
            assert!(
                text_styles.iter().any(|s| s.decoration.strikethrough),
                "completed-row text must carry strikethrough TextDecoration",
            );
        });
    }

    /// (R664) `access_child_invoke` routes Click on `todo_item#<id>`
    /// into `TodoEditExternal::invoke("begin", Int(id))` — the AT
    /// equivalent of the double-click idiom (assistive tech doesn't
    /// distinguish single from double activation).
    #[test]
    fn r664_at_click_on_item_enters_edit_mode() {
        with_owner(|| {
            let id = super::allocate_todo_id();
            use_todos().set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id,
                    text: "at-edit".to_owned(),
                    completed: false,
                });
                next
            });
            // The shell registered ITEM_TAG external alongside the
            // others; re-register a fresh tagged state-scene to
            // exercise the dispatch path independently from the
            // running shell.
            let mut state_scene = build_state_scene_for_at_test();
            let handled = <TodoMvcView as super::WidgetA11y>::access_child_invoke(
                &mut state_scene,
                ITEM_TAG,
                &id.to_string(),
                super::AccessAction::Click,
            );
            assert!(handled, "AT Click on todo_item is routed");
            assert_eq!(super::use_editing_id().get(), Some(id));
        });
    }

    /// (R664) AT Click on `todo_delete#<id>` invokes the typed-id
    /// delete path, mirroring the mouse single-click on the `×`
    /// glyph. Confirms the R662 `parent_tag` substrate has 4 application
    /// consumers (filter + delete + toggle + item).
    #[test]
    fn r664_at_click_on_delete_removes_row() {
        with_owner(|| {
            let id = super::allocate_todo_id();
            use_todos().set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id,
                    text: "doomed".to_owned(),
                    completed: false,
                });
                next
            });
            let mut state_scene = build_state_scene_for_at_test();
            let handled = <TodoMvcView as super::WidgetA11y>::access_child_invoke(
                &mut state_scene,
                DELETE_TAG,
                &id.to_string(),
                super::AccessAction::Click,
            );
            assert!(handled, "AT Click on todo_delete is routed");
            assert!(
                !use_todos().get().iter().any(|i| i.id == id),
                "AT-driven delete removed the row",
            );
        });
    }

    /// (R664) AT Click on `todo_toggle#<id>` flips `completed` —
    /// mirror of the mouse single-click on the ballot-box glyph.
    #[test]
    fn r664_at_click_on_toggle_flips_completed() {
        with_owner(|| {
            let id = super::allocate_todo_id();
            use_todos().set_with(|prev| {
                let mut next = prev.clone();
                next.push(TodoItem {
                    id,
                    text: "togme".to_owned(),
                    completed: false,
                });
                next
            });
            let mut state_scene = build_state_scene_for_at_test();
            let handled = <TodoMvcView as super::WidgetA11y>::access_child_invoke(
                &mut state_scene,
                TOGGLE_TAG,
                &id.to_string(),
                super::AccessAction::Click,
            );
            assert!(handled);
            let items = use_todos().get();
            assert!(items.iter().find(|i| i.id == id).unwrap().completed);
        });
    }

    /// (R664) Build a state scene tree that hosts every R664-wired
    /// External (delete + toggle + item + filter) at the top level so
    /// the AT-action helpers can resolve their primary tag via
    /// `find_external_with_tag_mut`. Used only by the AT-action test
    /// arms above.
    fn build_state_scene_for_at_test() -> Scene {
        use pinion_core::scene::ExternalNode;
        let todos = use_todos();
        let delete = Scene::External(
            ExternalNode::new(Box::new(TodoDeleteExternal::new(todos.clone())))
                .with_tag(DELETE_TAG),
        );
        let toggle = Scene::External(
            ExternalNode::new(Box::new(TodoToggleExternal::new(todos.clone())))
                .with_tag(TOGGLE_TAG),
        );
        let item = Scene::External(
            ExternalNode::new(Box::new(super::TodoEditExternal::new(todos)))
                .with_tag(ITEM_TAG),
        );
        Scene::Container(super::ContainerNode::new(vec![delete, toggle, item]))
    }

    // ─────────────────────────────────────────────────────────────
    // R682.B §5.16 — paint-fragment cache stress seed
    // ─────────────────────────────────────────────────────────────
    //
    // The seed env var ([`super::SEED_N_ENV`]) feeds N synthetic
    // [`super::TodoItem`] rows into the freshly-booted binding so
    // the §5.16 paint-fragment cache (R682.A FragmentCache +
    // per-Container `paint_hash` keying + mark-and-sweep eviction)
    // can be exercised under load. These tests pin the pure pieces
    // (env parse / row builder / completion stride) and the
    // view-fn-level pin that N seeded rows produce N rows in the
    // painted scene. Cross-paint cache hit/miss behaviour is
    // exercised in `pinion-runtime::paint_adapter::tests` against a
    // synthetic 100-Container scene + verified end-to-end through
    // the `scene/cache_stats` RPC method by
    // `tools/demos/r682_dirty_subtree_cache.py`.

    use super::{build_seed_rows, parse_seed_n_env, seed_todo_row, SEED_N_MAX};

    #[test]
    fn r682b_parse_seed_n_env_accepts_in_range() {
        assert_eq!(parse_seed_n_env(Some("1")), Some(1));
        assert_eq!(parse_seed_n_env(Some("100")), Some(100));
        assert_eq!(parse_seed_n_env(Some("10000")), Some(SEED_N_MAX));
    }

    #[test]
    fn r682b_parse_seed_n_env_rejects_zero_and_overflow() {
        assert!(parse_seed_n_env(Some("0")).is_none());
        assert!(parse_seed_n_env(Some("10001")).is_none());
        assert!(parse_seed_n_env(Some("4294967296")).is_none()); // > u32::MAX
    }

    #[test]
    fn r682b_parse_seed_n_env_rejects_malformed() {
        assert!(parse_seed_n_env(None).is_none());
        assert!(parse_seed_n_env(Some("")).is_none());
        assert!(parse_seed_n_env(Some("abc")).is_none());
        assert!(parse_seed_n_env(Some("-5")).is_none());
        assert!(parse_seed_n_env(Some("3.14")).is_none());
    }

    #[test]
    fn r682b_parse_seed_n_env_handles_whitespace() {
        assert_eq!(parse_seed_n_env(Some("  100  ")), Some(100));
        assert_eq!(parse_seed_n_env(Some("\t50\n")), Some(50));
    }

    #[test]
    fn r682b_build_seed_rows_returns_n_items_with_contiguous_ids() {
        let rows = build_seed_rows(5, 42);
        assert_eq!(rows.len(), 5);
        for (idx, row) in rows.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation, reason = "idx < 5")]
            let expected_id = 42 + idx as u64;
            assert_eq!(row.id, expected_id);
        }
    }

    #[test]
    fn r682b_build_seed_rows_completion_pattern_is_third_indices() {
        let rows = build_seed_rows(10, 0);
        // indices 0, 3, 6, 9 completed; 1, 2, 4, 5, 7, 8 active.
        let completed: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.completed)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(completed, vec![0, 3, 6, 9]);
    }

    #[test]
    fn r682b_build_seed_rows_text_carries_one_indexed_position() {
        let rows = build_seed_rows(3, 1);
        assert_eq!(rows[0].text, "Seed task 1");
        assert_eq!(rows[1].text, "Seed task 2");
        assert_eq!(rows[2].text, "Seed task 3");
    }

    #[test]
    fn r682b_seed_todo_row_completion_alternates_on_stride() {
        // SEED_COMPLETION_STRIDE = 3 — row 0 completed, 1 active,
        // 2 active, 3 completed, 4 active, 5 active, …
        assert!(seed_todo_row(0, 0).completed);
        assert!(!seed_todo_row(1, 0).completed);
        assert!(!seed_todo_row(2, 0).completed);
        assert!(seed_todo_row(3, 0).completed);
        assert!(!seed_todo_row(4, 0).completed);
        assert!(!seed_todo_row(5, 0).completed);
        assert!(seed_todo_row(6, 0).completed);
    }

    #[test]
    fn r682b_view_fn_paints_one_item_container_per_seeded_row() {
        with_owner(|| {
            // Seed `use_todos` with N=12 rows via the helper.
            let rows = build_seed_rows(12, 100);
            use_todos().set(rows);
            let scene = view(
                (
                    TextFieldState::Idle,
                    0,
                    FilterRadioStates::default(),
                    TextFieldState::Idle,
                    0,
                ),
                &Frame::default(),
            );
            // Every seeded id should produce exactly one
            // `Container[item#<id>]` child under [`LIST_TAG`].
            let list_children = find_children_for_tag(&scene, LIST_TAG);
            // The list container holds the per-row item Containers
            // plus the header row; 12 seeded → at least 12 item
            // children plus the header.
            let item_count = list_children
                .iter()
                .filter(|c| match c {
                    Scene::Container(inner) => inner
                        .tag
                        .as_deref()
                        .is_some_and(|t| t.starts_with(ITEM_TAG_PREFIX)),
                    _ => false,
                })
                .count();
            assert_eq!(item_count, 12, "12 seeded rows → 12 item Containers");
        });
    }

    #[test]
    fn r682b_view_fn_seeded_row_carries_strikethrough_on_completed() {
        with_owner(|| {
            // Seed 3 rows so indices 0 + ¬1 + ¬2 land — only
            // index 0 should paint strikethrough.
            let rows = build_seed_rows(3, 1);
            assert!(rows[0].completed);
            assert!(!rows[1].completed);
            assert!(!rows[2].completed);
            use_todos().set(rows);
            let scene = view(
                (
                    TextFieldState::Idle,
                    0,
                    FilterRadioStates::default(),
                    TextFieldState::Idle,
                    0,
                ),
                &Frame::default(),
            );
            let styles = collect_text_styles(&scene);
            // At least one paint emitted a strikethrough — the
            // completed-row text decoration. Smoke-pin only (full
            // strikethrough audit is covered by R664 tests above).
            let has_strikethrough = styles
                .iter()
                .any(|s| s.decoration == pinion_core::style::TextDecoration::strikethrough());
            assert!(
                has_strikethrough,
                "completed seed row must paint strikethrough",
            );
        });
    }
}
