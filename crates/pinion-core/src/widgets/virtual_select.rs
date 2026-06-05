//! R746 §5.27 / §5.38 — **selection coordinator for a virtualized list**.
//!
//! R744/R745 land *display-only* virtualization: only the visible window
//! of an N-row dataset ever exists in the scene tree. The natural next
//! Model/View slice is **selection** — but the existing
//! [`selection`](crate::widgets::selection) substrate (R735.1) is
//! fundamentally a *leaf-based* model: it operates on a `&mut [L]` slice of
//! materialized leaves, each carrying its own selection bit (a `Radio`, a
//! `ListBoxItem`). That is exactly what a virtualized list **cannot**
//! provide — the whole point is that the 9 995 off-window leaves do not
//! exist. Reusing it would require materializing all N leaves, defeating
//! virtualization.
//!
//! So selection on a virtualized collection is held the way every real
//! data grid holds it: as a **selected data index**, owned by a
//! coordinator and decoupled from materialization. Selecting row 4 200 and
//! scrolling away does not drop the selection (no leaf to lose it on); the
//! view paints `selected == index` for the handful of *visible* rows. This
//! is the canonical virtualized-selection model (Qt `QItemSelectionModel`
//! over a `QAbstractItemModel`, Flutter `ListView` + a selection
//! controller, web `aria-activedescendant` over windowed rows).
//!
//! Like [`SpinButtonExternal`](crate::widgets::spin_button) and
//! [`ProgressBarExternal`](crate::widgets::progress_bar) this widget owns
//! **no interaction statechart** — there is no per-row hover/press SCXML at
//! the list level (the rows are plain windowed `Scene` nodes, not
//! externals). It is a plain index holder: *operability* is "set the
//! selected index", driven by the R51.42 §5.35 composite pointer channel
//! (`vlist#<i>` → `invoke("send", "<i>:PointerUp")`) and the AI-first
//! `invoke("select", <i>)` path. Single-select this slice (the listbox /
//! data-grid default); multi-select by index is a later additive axis when
//! a consumer needs it.
//!
//! a11y: the binding lowers `selected == index` to
//! [`AccessNode::with_selected`](pinion_a11y::AccessNode::with_selected)
//! (`aria-selected`) on each *rendered* `ListItem`, on a single-select
//! `List` (no `aria-multiselectable`) — exactly the windowed-AT model the
//! R744/R745 lists already use for `aria-setsize` / `aria-posinset`.

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::scroll::ScrollState;
use crate::widgets::IntentEmitter;
use crate::Scene;
use std::collections::BTreeSet;

/// R780 §5.40 — selection cardinality policy for [`VirtualSelect`], the
/// `QItemSelectionModel` `SelectionMode` analogue.
///
/// `Single` (the default and every pre-R780 consumer) holds at most one
/// row; `Multi` holds an arbitrary set with an `anchor` for range
/// extension. The mode is fixed at construction — a list is built either
/// single- or multi-select, exactly as `QAbstractItemView::setSelectionMode`
/// is a property of the view, not a per-interaction flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    /// At most one selected row (the pre-R780 behaviour). Range/toggle
    /// operations collapse to a plain replace.
    Single,
    /// An arbitrary set of selected rows with an `anchor` origin for
    /// `Shift`-range extension and per-row `Ctrl`-toggle.
    Multi,
}

/// R746 §5.27 §5.38 / R780 §5.40 — the index-set holder wrapped by
/// [`VirtualSelectExternal`].
///
/// Pure selection-by-index state, no interaction statechart and no §5.20
/// queue of its own — the [`IntentEmitter`] wrapper owns the pending
/// intents (exactly as [`RadioGroup`](crate::widgets::radio_group) is the
/// plain widget inside `IntentEmitter<RadioGroup>`). Holding the selection
/// as **data indices** (not per-leaf bits) is what decouples it from
/// materialization; `item_count` bounds every mutation so a malformed wire
/// payload can never select a non-existent row.
///
/// The set generalises the pre-R780 `selected: Option<usize>`: a `Single`
/// model keeps the set at cardinality ≤ 1, so [`cursor`](Self::cursor) (the
/// active row, the `query("selected")` value) coincides with the sole
/// member and every old assertion holds. `Multi` lets the set grow, with
/// `anchor` recording where a `Shift`-range began.
#[derive(Debug, Clone)]
struct VirtualSelect {
    /// The selected data indices. In `Single` mode this is held at
    /// cardinality ≤ 1.
    selection: BTreeSet<usize>,
    /// Range-extension origin — the row a `Shift`-extend grows from,
    /// unchanged while extending. Set by every plain move / toggle.
    anchor: Option<usize>,
    /// The active row (WAI-ARIA active descendant / `QItemSelectionModel`
    /// `currentIndex`): the keyboard-navigation reference and the
    /// `query("selected")` value. Coincides with the sole selected row in
    /// `Single` mode.
    cursor: Option<usize>,
    /// Total dataset size — the validity bound for any selection.
    item_count: usize,
    /// Single- vs multi-select cardinality policy.
    mode: SelectionMode,
}

impl VirtualSelect {
    fn new(item_count: usize, mode: SelectionMode) -> Self {
        Self {
            selection: BTreeSet::new(),
            anchor: None,
            cursor: None,
            item_count,
            mode,
        }
    }

    /// Move the active row to `index` and replace the selection with just
    /// it (a plain click / unmodified arrow). Out-of-range indices are
    /// ignored. Sets the `anchor` (a later `Shift`-extend grows from here).
    /// Returns `true` if anything changed.
    fn select(&mut self, index: usize) -> bool {
        if index >= self.item_count {
            return false;
        }
        if self.cursor == Some(index) && self.selection.len() == 1 && self.selection.contains(&index)
        {
            return false;
        }
        self.selection.clear();
        self.selection.insert(index);
        self.anchor = Some(index);
        self.cursor = Some(index);
        true
    }

    /// `Ctrl`-toggle `index`: flip its membership, leaving the rest of the
    /// selection intact, and make it the active row + `anchor`. In `Single`
    /// mode this collapses to a plain [`select`](Self::select) (a single
    /// model cannot hold two rows). Out-of-range indices are ignored.
    /// Returns `true` if anything changed.
    fn toggle(&mut self, index: usize) -> bool {
        if index >= self.item_count {
            return false;
        }
        if self.mode == SelectionMode::Single {
            return self.select(index);
        }
        if self.selection.contains(&index) {
            self.selection.remove(&index);
        } else {
            self.selection.insert(index);
        }
        self.anchor = Some(index);
        self.cursor = Some(index);
        true
    }

    /// `Shift`-extend to `index`: replace the selection with the inclusive
    /// range from the `anchor` to `index`, moving the active row to
    /// `index` while leaving the `anchor` put. With no anchor yet, behaves
    /// as a plain [`select`](Self::select). In `Single` mode it also
    /// collapses to a plain select. Out-of-range indices are ignored.
    /// Returns `true` if anything changed.
    fn extend_to(&mut self, index: usize) -> bool {
        if index >= self.item_count {
            return false;
        }
        if self.mode == SelectionMode::Single {
            return self.select(index);
        }
        let Some(anchor) = self.anchor else {
            return self.select(index);
        };
        let (lo, hi) = if anchor <= index { (anchor, index) } else { (index, anchor) };
        let next: BTreeSet<usize> = (lo..=hi).collect();
        let changed = next != self.selection || self.cursor != Some(index);
        self.selection = next;
        self.cursor = Some(index);
        changed
    }

    /// Select every row (`Ctrl+A`). A no-op in `Single` mode or on an empty
    /// dataset. Leaves the active row and `anchor` put. Returns `true` if
    /// the selection grew.
    fn select_all(&mut self) -> bool {
        if self.mode == SelectionMode::Single || self.item_count == 0 {
            return false;
        }
        if self.selection.len() == self.item_count {
            return false;
        }
        self.selection = (0..self.item_count).collect();
        true
    }

    fn clear(&mut self) -> bool {
        let had = !self.selection.is_empty();
        self.selection.clear();
        self.anchor = None;
        self.cursor = None;
        had
    }

    /// Admin replace to a single selection (or clear) — the persisted /
    /// form-default channel. Mirrors the pre-R780 single-index restore.
    fn set_selected(&mut self, index: Option<usize>) -> bool {
        let next = match index {
            Some(i) if i < self.item_count => Some(i),
            _ => None,
        };
        if self.cursor == next
            && self.selection.len() == usize::from(next.is_some())
            && next.is_none_or(|i| self.selection.contains(&i))
        {
            return false;
        }
        self.selection.clear();
        if let Some(i) = next {
            self.selection.insert(i);
        }
        self.anchor = next;
        self.cursor = next;
        true
    }

    /// Admin replace to an arbitrary set (the multi-select restore channel).
    /// Out-of-range indices are dropped. The active row / anchor become the
    /// greatest selected index (or `None` when empty). Returns `true` if the
    /// set changed.
    fn set_selection(&mut self, indices: &BTreeSet<usize>) -> bool {
        let next: BTreeSet<usize> = indices.iter().copied().filter(|&i| i < self.item_count).collect();
        if next == self.selection {
            return false;
        }
        let last = next.iter().next_back().copied();
        self.selection = next;
        self.anchor = last;
        self.cursor = last;
        true
    }
}

/// R746 §5.27 §5.38 — single-select-by-index coordinator for a virtualized
/// list.
///
/// Holds the selected **data index** (not a per-leaf bit), so selection is
/// independent of which rows are currently materialized. `item_count`
/// bounds every mutation: an out-of-range index is rejected (a malformed
/// wire payload can never select a non-existent row).
///
/// Like every selection coordinator in the catalogue
/// ([`RadioGroup`](crate::widgets::radio_group) /
/// [`ListBox`](crate::widgets::listbox) / [`Table`](crate::widgets::table))
/// it emits a §5.20 `"selected"` intent (the new index as
/// [`IntrospectValue::Int`]) on the *interaction* path so AI / automation
/// observe the selection on the intent channel — not only by polling
/// `query("selected")`. The admin restore path
/// ([`set_selected`](Self::set_selected) / [`clear`](Self::clear)) is
/// silent, exactly as [`selection::replace_selection`](crate::widgets::selection::replace_selection)
/// is (restoration is not interaction).
///
/// The §5.20 pending queue is owned by the shared
/// [`IntentEmitter`] wrapper — the same one
/// `RadioGroupExternal` / `ListBoxExternal` / `TableExternal` use — rather
/// than a hand-rolled `pending: Vec<Intent>` field (the pre-R51.5
/// anti-pattern that `IntentEmitter` exists to eliminate; R746.3 brought
/// this lone outlier back into that SSOT). This widget is a plain holder,
/// so it does *not* implement [`WidgetTransition`](crate::widgets::WidgetTransition)
/// auto-dispatch — it pushes the intent explicitly on the interaction edge.
pub struct VirtualSelectExternal {
    em: IntentEmitter<VirtualSelect>,
}

impl core::fmt::Debug for VirtualSelectExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VirtualSelectExternal")
            .field("selected", &self.selected())
            .field("item_count", &self.item_count())
            .finish()
    }
}

impl VirtualSelectExternal {
    /// Construct a **single-select** coordinator over an `item_count`-row
    /// dataset, nothing selected (the pre-R780 default — every existing
    /// consumer keeps its exact behaviour).
    #[must_use]
    pub fn new(item_count: usize) -> Self {
        Self { em: IntentEmitter::new(VirtualSelect::new(item_count, SelectionMode::Single)) }
    }

    /// Construct a **multi-select** coordinator (R780): `Shift`-range,
    /// `Ctrl`-toggle, and `Ctrl+A` select-all hold an arbitrary index set
    /// with an `anchor` origin. The `QItemSelectionModel` `ExtendedSelection`
    /// analogue.
    #[must_use]
    pub fn new_multi(item_count: usize) -> Self {
        Self { em: IntentEmitter::new(VirtualSelect::new(item_count, SelectionMode::Multi)) }
    }

    /// The active row (the `query("selected")` value, the
    /// `QItemSelectionModel` `currentIndex`): the sole selected row in a
    /// single-select model, the keyboard-navigation reference in a
    /// multi-select one. `None` when nothing is selected.
    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        self.em.inner.cursor
    }

    /// The full set of selected data indices (sorted). Cardinality ≤ 1 in a
    /// single-select model.
    #[must_use]
    pub fn selection(&self) -> Vec<usize> {
        self.em.inner.selection.iter().copied().collect()
    }

    /// Whether `index` is selected.
    #[must_use]
    pub fn is_selected(&self, index: usize) -> bool {
        self.em.inner.selection.contains(&index)
    }

    /// The range-extension origin (`anchor`), or `None`.
    #[must_use]
    pub fn anchor(&self) -> Option<usize> {
        self.em.inner.anchor
    }

    /// The selection-cardinality policy this coordinator was built with.
    #[must_use]
    pub fn mode(&self) -> SelectionMode {
        self.em.inner.mode
    }

    /// Total dataset size.
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.em.inner.item_count
    }

    /// Move the active row to `index` and replace the selection with just it
    /// (a plain click / unmodified arrow) — the **interaction** path. Sets
    /// the `anchor`. Out-of-range indices are ignored. On a real change,
    /// queues the §5.20 selection intent. Returns `true` if it changed.
    pub fn select(&mut self, index: usize) -> bool {
        if !self.em.inner.select(index) {
            return false;
        }
        self.push_selection_intent();
        true
    }

    /// `Ctrl`-toggle `index` (multi-select interaction): flip its membership,
    /// leaving the rest selected. Collapses to [`select`](Self::select) in a
    /// single-select model. On a real change, queues the selection intent.
    /// Returns `true` if it changed.
    pub fn toggle(&mut self, index: usize) -> bool {
        if !self.em.inner.toggle(index) {
            return false;
        }
        self.push_selection_intent();
        true
    }

    /// `Shift`-extend the selection to `index` (multi-select interaction):
    /// the inclusive range from the `anchor`. Collapses to
    /// [`select`](Self::select) in a single-select model (or with no anchor
    /// yet). On a real change, queues the selection intent. Returns `true`
    /// if it changed.
    pub fn extend_to(&mut self, index: usize) -> bool {
        if !self.em.inner.extend_to(index) {
            return false;
        }
        self.push_selection_intent();
        true
    }

    /// `Ctrl+A` select-all (multi-select interaction). A no-op in a
    /// single-select model. On a real change, queues the selection intent.
    /// Returns `true` if the selection grew.
    pub fn select_all(&mut self) -> bool {
        if !self.em.inner.select_all() {
            return false;
        }
        self.push_selection_intent();
        true
    }

    /// Clear the selection. Returns `true` if something was selected.
    pub fn clear(&mut self) -> bool {
        self.em.inner.clear()
    }

    /// Replace the selection directly with a single index (the admin /
    /// persisted-restore / form-default channel — not an interaction).
    /// `None` or an out-of-range index clears. Returns `true` if it changed.
    pub fn set_selected(&mut self, index: Option<usize>) -> bool {
        self.em.inner.set_selected(index)
    }

    /// Replace the selection directly with an arbitrary set (the
    /// multi-select admin / persisted-restore channel — not an
    /// interaction). Out-of-range indices are dropped. Returns `true` if it
    /// changed.
    pub fn set_selection(&mut self, indices: &BTreeSet<usize>) -> bool {
        self.em.inner.set_selection(indices)
    }

    /// Queue the §5.20 selection intent for the **interaction** path,
    /// carrying the mode-appropriate payload: a single-select model emits
    /// `"selected"` with the active index (the pre-R780 wire, so every
    /// existing consumer is unchanged), a multi-select model emits
    /// `"selection"` with the full set as a JSON array of indices.
    fn push_selection_intent(&mut self) {
        match self.em.inner.mode {
            SelectionMode::Single => {
                let value = self.selected_value();
                self.em.push(Intent::new_static("selected", value));
            }
            SelectionMode::Multi => {
                let value = self.selection_value();
                self.em.push(Intent::new_static("selection", value));
            }
        }
    }

    /// Drive the composite pointer channel: on the activation edge
    /// (`PointerUp` or `KeyboardActivate`) select the addressed **row**.
    /// Every other pointer-arc event (`PointerEnter` / `PointerDown` /
    /// `PointerLeave`) the router replays is a harmless no-op — single
    /// selection has no hover/press feedback at the row level.
    ///
    /// The same coordinator serves both a virtualized **list** and a
    /// virtualized **grid** (R777), so the composite key is one of two
    /// shapes, both selecting the row:
    ///
    /// - **list item** `"<row>"` — the windowed `vlist#<row>` row.
    /// - **grid cell** `"<row>_<col>"` — the windowed `vtbl#<row>_<col>`
    ///   cell. Selecting any cell selects its row (the WAI-ARIA / Qt
    ///   `QItemSelectionModel` `SelectRows` behaviour: the column is
    ///   irrelevant to a single-row selection). A grid column-header click
    ///   arrives as `"h<col>"`, which has no leading row index and is
    ///   ignored here (sort is a separate axis, not this coordinator's).
    ///
    /// The grid grammar is decoded by the shared
    /// [`GridSendKey`](crate::composite_tag::GridSendKey) SSOT (R777.1) — a
    /// cell `"<row>_<col>"` yields its row, a header `"h<col>"` yields
    /// `None` (ignored). A bare list-item key `"<row>"` has no grid
    /// structure, so it falls back to a plain integer parse: one
    /// coordinator, both collection shapes, one wire grammar.
    fn handle_send(&mut self, payload: &str) {
        let Some((key, event_name, modifiers)) =
            crate::composite_tag::split_send_payload(payload)
        else {
            return;
        };
        let row = match crate::composite_tag::GridSendKey::parse(key) {
            Some(grid_key) => grid_key.row(),
            None => key.parse::<usize>().ok(),
        };
        let Some(index) = row else {
            return;
        };
        if crate::input::is_activation_event(event_name) {
            // R781 §5.35 §5.40 — modifier-aware click selection, the pointer
            // peer of the `nav_select_key` keyboard ops: `Ctrl`-click toggles
            // the row's membership, `Shift`-click extends the range from the
            // anchor, a plain click moves + replaces. In a single-select
            // model `toggle` / `extend_to` collapse to a plain `select`, so a
            // `Shift` / `Ctrl`-click on a single-select list still just
            // selects — exactly the pre-R781 behaviour.
            if modifiers.ctrl {
                self.toggle(index);
            } else if modifiers.shift {
                self.extend_to(index);
            } else {
                self.select(index);
            }
        }
    }

    /// The active index as an `IntrospectValue` (`Int` or `Null`) — the
    /// uniform return for the single-select mutating `invoke` paths and the
    /// `"selected"` query.
    fn selected_value(&self) -> IntrospectValue {
        self.selected()
            .and_then(|i| i64::try_from(i).ok())
            .map_or(IntrospectValue::Null, IntrospectValue::Int)
    }

    /// The full selection as a JSON array of indices — the uniform return
    /// for the multi-select mutating `invoke` paths and the `"selection"`
    /// query. Always an array (empty `[]` when nothing is selected), never
    /// `Null`, so an AI consumer can treat the slot as a list unconditionally.
    fn selection_value(&self) -> IntrospectValue {
        IntrospectValue::Json(serde_json::Value::Array(
            self.em
                .inner
                .selection
                .iter()
                .map(|&i| serde_json::Value::from(i))
                .collect(),
        ))
    }

    /// The mode as its canonical wire string (`"single"` / `"multi"`).
    fn mode_value(&self) -> IntrospectValue {
        IntrospectValue::Text(
            match self.mode() {
                SelectionMode::Single => "single",
                SelectionMode::Multi => "multi",
            }
            .to_string(),
        )
    }
}

impl External for VirtualSelectExternal {
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

    /// Drain the queued §5.20 `"selected"` intents (one per interaction
    /// that changed the selection) — the same contract
    /// [`RadioGroup`](crate::widgets::radio_group) /
    /// [`ListBox`](crate::widgets::listbox) honour, so AI / automation see
    /// the selection on the intent channel.
    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        self.em.drain(sink);
    }

    /// Dirty exactly while a `"selected"` intent awaits draining; the
    /// selection value itself only changes through `invoke` / `intervene`,
    /// which the framework already follows with a repaint.
    fn is_dirty(&self) -> bool {
        self.em.is_dirty()
    }
}

impl ExternalIntrospect for VirtualSelectExternal {
    fn schema(&self) -> IntrospectSchema {
        // `selected` — settable active index (query + intervene).
        // `selection` — settable full index set as a JSON array (R780
        //   multi-select; query + intervene).
        // `anchor` — the range-extension origin (query only).
        // `mode` — `"single"` / `"multi"` cardinality policy (query only).
        // `item_count` — construction-fixed dataset size (query only).
        // `send` — the R51.42 §5.35 composite pointer channel (`<i>:Event`).
        IntrospectSchema::new(&[
            ("selected", "int"),
            ("selection", "json"),
            ("anchor", "int"),
            ("mode", "string"),
            ("item_count", "int"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            // A schema-listed path must always return a value (the RPC
            // layer treats a `None` from a declared slot as
            // `UnknownIntrospectPath`); an empty selection reports
            // `Null` (present-but-empty) for the single index, an empty
            // JSON array for the set.
            "selected" => Some(self.selected_value()),
            "selection" => Some(self.selection_value()),
            "anchor" => Some(
                self.anchor()
                    .and_then(|i| i64::try_from(i).ok())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int),
            ),
            "mode" => Some(self.mode_value()),
            "item_count" => Some(
                i64::try_from(self.item_count())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int),
            ),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // The active index is a writable axis (admin / restore). `Int`
            // selects a single row (out-of-range clears); `Null` clears.
            "selected" => match value {
                IntrospectValue::Int(i) => {
                    let index = usize::try_from(i).ok();
                    self.set_selected(index);
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.set_selected(None);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // The full set is a writable axis (multi-select admin / restore):
            // a JSON array of indices replaces the selection (out-of-range
            // dropped); `Null` clears.
            "selection" => match value {
                IntrospectValue::Json(serde_json::Value::Array(items)) => {
                    let indices: BTreeSet<usize> = items
                        .iter()
                        .filter_map(serde_json::Value::as_u64)
                        .filter_map(|i| usize::try_from(i).ok())
                        .collect();
                    self.set_selection(&indices);
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.set_selection(&BTreeSet::new());
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "anchor" | "mode" | "item_count" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(&mut self, path: &str, args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        match path {
            // AI-first direct selection — returns the resulting selected
            // index (or Null) so the caller sees the outcome in one
            // round-trip.
            "select" => match args {
                IntrospectValue::Int(i) => {
                    if let Ok(index) = usize::try_from(i) {
                        self.select(index);
                    }
                    Ok(self.selected_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R780 multi-select interaction funnel — `Ctrl`-toggle a row,
            // `Shift`-extend the range to a row, or `Ctrl+A` select all.
            // Each returns the resulting full set as a JSON array so the
            // caller (keyboard controller / AI / future modifier-click wire)
            // sees the outcome in one round-trip. In a single-select model
            // `toggle` / `extend_to` collapse to a plain move and
            // `select_all` is a no-op.
            "toggle" => match args {
                IntrospectValue::Int(i) => {
                    if let Ok(index) = usize::try_from(i) {
                        self.toggle(index);
                    }
                    Ok(self.selection_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "extend_to" => match args {
                IntrospectValue::Int(i) => {
                    if let Ok(index) = usize::try_from(i) {
                        self.extend_to(index);
                    }
                    Ok(self.selection_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "select_all" => {
                self.select_all();
                Ok(self.selection_value())
            }
            "clear" => {
                self.clear();
                Ok(self.selected_value())
            }
            // R51.42 §5.35 composite pointer channel: the windowed
            // `vlist#<i>` rows route the full pointer arc here as
            // `invoke("send", "<i>:PointerEnter")` … `"<i>:PointerUp")`.
            // The `"<i>:<EventName>"` wire is split by the R660
            // [`composite_tag::parse_send_payload`] SSOT; the key type is
            // `usize` because the sub-region is a numeric data index (the
            // Table-cell model, not the named-region SpinButton model).
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    self.handle_send(payload);
                    Ok(self.selected_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// R782 §5.40 — **deserialize peer** of the coordinator's `"selection"`
/// query (the `selection_value` encode): decode the multi-select index set
/// from an introspection surface that delegates it to a
/// [`VirtualSelectExternal`] in `Multi` mode. The inverse of that encode,
/// kept in the same module so a slot rename can't silently break a binding's
/// hand-decode (the R743.1 `read_reorder` decode-of-encode rule; the
/// `"selection"` array is now read by both `hello-multi-select` and
/// `hello-grid-multi-select`). Returns the indices in the coordinator's order
/// (ascending — a `BTreeSet`); the empty array decodes to an empty `Vec`, and
/// an absent / mistyped slot likewise yields `Vec::new()`. Bindings map this
/// into their own `Copy` paint projection (e.g. a per-row bitmap).
#[must_use]
pub fn read_selection(intro: &dyn ExternalIntrospect) -> Vec<usize> {
    match intro.query("selection") {
        Some(IntrospectValue::Json(serde_json::Value::Array(items))) => items
            .iter()
            .filter_map(serde_json::Value::as_u64)
            .filter_map(|v| usize::try_from(v).ok())
            .collect(),
        _ => Vec::new(),
    }
}

/// R783.1 §5.40 — **deserialize peer** of the coordinator's `"selected"`
/// query (the `selected_value` encode): decode the single active row index
/// from an introspection surface that delegates it to a
/// [`VirtualSelectExternal`]. The scalar sibling of [`read_selection`] (same
/// R743.1 decode-of-encode rule): the `"selected"` `Int` slot is read by every
/// single-select index-model binding (`hello-virtual-select` / `-nav`,
/// `hello-grid-nav` / `-sort` / `-filter`, `hello-virtual-sort`), so the
/// decode lives next to its encode and a slot rename can't silently break six
/// hand-decoders. An absent / `Null` / mistyped slot yields `None`.
#[must_use]
pub fn read_selected(intro: &dyn ExternalIntrospect) -> Option<usize> {
    match intro.query("selected") {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
        _ => None,
    }
}

/// R777 §5.27 — the standard **linear-clamp** keyboard navigation policy
/// for a finite virtualized collection: map a key to the next selected
/// index given the current selection and a `page` size (rows per measured
/// viewport-ful).
///
/// Single-select, **clamp** (no wrap) — a finite data list / grid has ends,
/// unlike the cyclic roving of a small `ListBox` / `RadioGroup` (those wrap
/// because every option is a peer tab stop). With no current selection,
/// every navigation key lands on the first row (the W3C "first key focuses
/// the first option" convention). Returns `None` for an unhandled key (or
/// an empty collection) so the caller falls through to the shell's
/// unrecognised-key swallow contract.
///
/// This is the policy half of [`nav_select_key`]; it is `pub` so a binding
/// that wants the same key→index mapping without the full controller (or a
/// different mechanism) can reuse it. A *cyclic* peer is a later additive
/// policy when a wrapping virtualized collection needs one.
#[must_use]
pub fn clamp_nav(current: Option<usize>, key: &str, item_count: usize, page: usize) -> Option<usize> {
    let last = item_count.checked_sub(1)?;
    let next = match key {
        "ArrowDown" => current.map_or(0, |i| (i + 1).min(last)),
        "ArrowUp" => current.map_or(0, |i| i.saturating_sub(1)),
        "Home" => 0,
        "End" => last,
        "PageDown" => current.map_or(0, |i| i.saturating_add(page).min(last)),
        "PageUp" => current.map_or(0, |i| i.saturating_sub(page)),
        _ => return None,
    };
    Some(next)
}

/// R780 §5.27 — the uniform-pitch geometry of a virtualized collection that
/// [`nav_select_key`] navigates: the full dataset size and the per-row
/// pitch the body windows against. Bundled into one `Copy` struct (the
/// R775 `VirtualTableData` precedent) so the controller stays within the
/// 7-argument budget after R780 added the `modifiers` axis.
#[derive(Debug, Clone, Copy)]
pub struct RowMetrics {
    /// Full dataset size — the navigation clamp bound.
    pub item_count: usize,
    /// Uniform per-row pitch the body windows against (the list row pitch /
    /// the grid data-row height).
    pub row_pitch: u32,
}

/// R777 §5.27 / R780 §5.40 — drive keyboard navigation **and multi-select
/// modifiers** for an index-model virtualized **collection** (a virtualized
/// list *or* grid) backed by a [`VirtualSelectExternal`] at `tag` and a
/// flex-viewport [`ScrollState`].
///
/// This is the shared `WidgetCore::apply_key` body behind `hello-virtual-nav`
/// (list), `hello-grid-nav` (grid), and `hello-multi-select` (multi list):
/// the wiring is byte-identical between them (only the tag, scroll state,
/// row pitch, and item count differ), and a divergence — selecting or
/// revealing differently in one than another — would be a bug, not a style
/// choice. So it lifts here (the R758 self-grep mandate) rather than living
/// thrice.
///
/// The `modifiers` come straight from `WidgetCore::apply_key` (already W3C
/// four-bit). They only change behaviour when the coordinator is a
/// **multi-select** model (`query("mode") == "multi"`); a single-select
/// coordinator ignores them, so the two existing consumers are unchanged:
///
/// - **`Ctrl+A`** — select every row (`invoke("select_all")`).
/// - **`Ctrl+Space`** — toggle the active row (`invoke("toggle", cursor)`).
/// - **`Shift`+nav** (`Arrow` / `Home` / `End` / `Page`) — extend the
///   contiguous range from the `anchor` to the navigated row
///   (`invoke("extend_to", target)`).
/// - **plain nav** — move the active row and replace the selection
///   (`invoke("select", target)`), exactly as R777.
///
/// Every selection mutation goes through the coordinator's AI-first `invoke`
/// funnel (the same wire a `scene/invoke` drives — keyboard and RPC
/// selection are one funnel), then the navigated row is scrolled into view
/// with [`reveal_row`](crate::widgets::virtual_list::reveal_row) (so
/// navigating to a never-materialized row scrolls there).
///
/// Returns `true` when the key was handled (the grid/list was focused and
/// the key is a navigation / multi-select key), `false` otherwise — the
/// exact bool `apply_key` must return. Keys only route when
/// `focused == Some(tag)` (single tab stop, no sibling aliasing).
///
/// `metrics` carries the dataset size and the uniform per-row pitch the
/// body windows against ([`RowMetrics`]).
pub fn nav_select_key(
    scene: &mut Scene,
    scroll: &ScrollState,
    tag: &str,
    focused: Option<&str>,
    key: &str,
    modifiers: crate::input::Modifiers,
    metrics: RowMetrics,
) -> bool {
    let RowMetrics { item_count, row_pitch } = metrics;
    if focused != Some(tag) {
        return false;
    }
    let page = crate::widgets::virtual_list::page_rows(scroll, row_pitch);

    let Some(node) = scene.find_external_with_tag_mut(tag) else {
        return false;
    };
    let Some(intro) = node.handle.introspect() else {
        return false;
    };
    let multi = matches!(intro.query("mode"), Some(IntrospectValue::Text(ref m)) if m == "multi");
    let current = match intro.query("selected") {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
        _ => None,
    };

    // Multi-select set ops that are *not* navigation: Ctrl+A select-all and
    // Ctrl+Space toggle-active. These handle the key (swallow it) without a
    // navigation target / scroll.
    if multi && modifiers.ctrl && key.eq_ignore_ascii_case("a") {
        if let Some(intro) = node.handle.introspect_mut() {
            let _ = intro.invoke("select_all", IntrospectValue::Null);
        }
        return true;
    }
    if multi && modifiers.ctrl && (key == " " || key == "Space") {
        if let (Some(intro), Some(c)) = (node.handle.introspect_mut(), current) {
            if let Ok(c) = i64::try_from(c) {
                let _ = intro.invoke("toggle", IntrospectValue::Int(c));
            }
        }
        return current.is_some();
    }

    let Some(target) = clamp_nav(current, key, item_count, page) else {
        return false;
    };
    // Shift+nav extends the range (multi only); everything else moves +
    // replaces. The coordinator collapses `extend_to` to a plain move in a
    // single-select model, so the branch is harmless there.
    let action = if multi && modifiers.shift { "extend_to" } else { "select" };
    if let (Some(intro), Ok(t)) = (node.handle.introspect_mut(), i64::try_from(target)) {
        let _ = intro.invoke(action, IntrospectValue::Int(t));
    }
    crate::widgets::virtual_list::reveal_row(scroll, target, row_pitch);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_unselected() {
        let s = VirtualSelectExternal::new(100);
        assert_eq!(s.selected(), None);
        assert_eq!(s.item_count(), 100);
    }

    #[test]
    fn select_sets_and_reports_change() {
        let mut s = VirtualSelectExternal::new(100);
        assert!(s.select(42));
        assert_eq!(s.selected(), Some(42));
        // Re-selecting the same index is a no-op.
        assert!(!s.select(42));
        // Moving the selection is a change.
        assert!(s.select(7));
        assert_eq!(s.selected(), Some(7));
    }

    #[test]
    fn select_out_of_range_is_ignored() {
        let mut s = VirtualSelectExternal::new(10);
        assert!(!s.select(10), "index == count is out of range");
        assert!(!s.select(9999));
        assert_eq!(s.selected(), None);
    }

    #[test]
    fn select_a_deep_index_works_without_materialization() {
        // The headline: selecting row 9 999 never requires the other
        // 9 999 leaves to exist (no leaf slice at all).
        let mut s = VirtualSelectExternal::new(10_000);
        assert!(s.select(9_999));
        assert_eq!(s.selected(), Some(9_999));
    }

    #[test]
    fn clear_resets() {
        let mut s = VirtualSelectExternal::new(10);
        s.select(3);
        assert!(s.clear());
        assert_eq!(s.selected(), None);
        assert!(!s.clear(), "clearing an empty selection is a no-op");
    }

    #[test]
    fn set_selected_admin_channel_validates() {
        let mut s = VirtualSelectExternal::new(10);
        assert!(s.set_selected(Some(5)));
        assert_eq!(s.selected(), Some(5));
        // Out-of-range clears.
        assert!(s.set_selected(Some(100)));
        assert_eq!(s.selected(), None);
        // None clears (and is a no-op when already empty).
        assert!(!s.set_selected(None));
    }

    #[test]
    fn composite_send_selects_only_on_activation_edge() {
        let mut s = VirtualSelectExternal::new(100);
        // The router replays the full arc; only PointerUp selects.
        s.handle_send("4:PointerEnter");
        assert_eq!(s.selected(), None, "hover does not select");
        s.handle_send("4:PointerDown");
        assert_eq!(s.selected(), None, "press alone does not select");
        s.handle_send("4:PointerUp");
        assert_eq!(s.selected(), Some(4), "release selects");
        // KeyboardActivate also selects.
        s.handle_send("9:KeyboardActivate");
        assert_eq!(s.selected(), Some(9));
        // Malformed / out-of-range payloads are harmless no-ops.
        s.handle_send("noseparator");
        s.handle_send("4:");
        s.handle_send("9999:PointerUp");
        assert_eq!(s.selected(), Some(9), "no-op payloads leave selection intact");
    }

    #[test]
    fn grid_cell_send_selects_the_row_column_irrelevant() {
        // R777 — the same coordinator drives a virtualized grid: a cell
        // key `<row>_<col>` selects the ROW (SelectRows). Clicking any
        // column of row 4 selects row 4.
        let mut s = VirtualSelectExternal::new(100);
        s.handle_send("4_0:PointerEnter");
        assert_eq!(s.selected(), None, "hover does not select");
        s.handle_send("4_2:PointerUp");
        assert_eq!(s.selected(), Some(4), "cell in column 2 selects row 4");
        // A different column of a different row moves the selection.
        s.handle_send("9_1:PointerUp");
        assert_eq!(s.selected(), Some(9), "cell in column 1 selects row 9");
        // KeyboardActivate on a cell selects its row too.
        s.handle_send("3_0:KeyboardActivate");
        assert_eq!(s.selected(), Some(3));
    }

    #[test]
    fn grid_header_send_is_ignored() {
        // A column-header click `h<col>` has no leading row index — it is
        // the sort axis, not this coordinator's, and must be a no-op.
        let mut s = VirtualSelectExternal::new(100);
        s.select(5);
        s.handle_send("h2:PointerUp");
        assert_eq!(s.selected(), Some(5), "header click leaves the selection intact");
    }

    #[test]
    fn query_reports_selected_and_count() {
        let mut s = VirtualSelectExternal::new(50);
        assert_eq!(
            s.query("selected"),
            Some(IntrospectValue::Null),
            "unset selection reports Null (present-but-empty), not absence",
        );
        assert_eq!(s.query("item_count"), Some(IntrospectValue::Int(50)));
        s.select(12);
        assert_eq!(s.query("selected"), Some(IntrospectValue::Int(12)));
        assert_eq!(s.query("nope"), None, "an undeclared path is genuinely absent");
    }

    #[test]
    fn intervene_selected_sets_clears_and_guards() {
        let mut s = VirtualSelectExternal::new(50);
        s.intervene("selected", IntrospectValue::Int(20)).expect("int selects");
        assert_eq!(s.selected(), Some(20));
        s.intervene("selected", IntrospectValue::Null).expect("null clears");
        assert_eq!(s.selected(), None);
        assert_eq!(
            s.intervene("item_count", IntrospectValue::Int(1)),
            Err(InterveneError::ReadOnly),
        );
        assert_eq!(
            s.intervene("selected", IntrospectValue::Bool(true)),
            Err(InterveneError::TypeMismatch),
        );
        assert_eq!(
            s.intervene("nope", IntrospectValue::Int(0)),
            Err(InterveneError::UnknownPath),
        );
    }

    fn drained(s: &mut VirtualSelectExternal) -> Vec<Intent> {
        let mut out = Vec::new();
        s.drain_intents(&mut |i| out.push(i));
        out
    }

    #[test]
    fn interaction_emits_selected_intent_admin_is_silent() {
        let mut s = VirtualSelectExternal::new(100);
        // Interaction (select) emits one "selected" intent with the index.
        assert!(s.select(7));
        let intents = drained(&mut s);
        assert_eq!(intents.len(), 1, "one selected intent per interaction");
        assert_eq!(intents[0], Intent::new_static("selected", IntrospectValue::Int(7)));
        assert!(drained(&mut s).is_empty(), "drain is idempotent (queue emptied)");
        // A no-op re-select emits nothing.
        assert!(!s.select(7));
        assert!(drained(&mut s).is_empty(), "unchanged selection emits nothing");
        // Composite send (the click wire) is also an interaction → emits.
        s.handle_send("9:PointerUp");
        assert_eq!(drained(&mut s).len(), 1, "composite send emits on activation");
        // Admin paths (intervene / set_selected / clear) are SILENT.
        s.intervene("selected", IntrospectValue::Int(3)).unwrap();
        s.set_selected(Some(5));
        s.clear();
        assert!(drained(&mut s).is_empty(), "admin restore/clear is silent on §5.20");
    }

    #[test]
    fn is_dirty_tracks_pending_intent() {
        let mut s = VirtualSelectExternal::new(10);
        assert!(!s.is_dirty(), "clean at rest");
        s.select(2);
        assert!(s.is_dirty(), "dirty while a selected intent is queued");
        let _ = drained(&mut s);
        assert!(!s.is_dirty(), "clean after drain");
    }

    #[test]
    fn invoke_select_clear_send_return_outcome() {
        let mut s = VirtualSelectExternal::new(100);
        assert_eq!(s.invoke("select", IntrospectValue::Int(7)), Ok(IntrospectValue::Int(7)));
        assert_eq!(s.invoke("clear", IntrospectValue::Null), Ok(IntrospectValue::Null));
        assert_eq!(
            s.invoke("send", IntrospectValue::Text("3:PointerUp".into())),
            Ok(IntrospectValue::Int(3)),
        );
        assert_eq!(s.invoke("bogus", IntrospectValue::Null), Err(InvokeError::UnknownPath));
        assert_eq!(
            s.invoke("select", IntrospectValue::Text("x".into())),
            Err(InvokeError::TypeMismatch),
        );
    }

    // ── R777 keyboard navigation policy + controller ────────────────

    #[test]
    fn clamp_nav_steps_clamps_and_pages() {
        // Arrows step one, clamped at both ends (no wrap).
        assert_eq!(clamp_nav(Some(5), "ArrowDown", 100, 12), Some(6));
        assert_eq!(clamp_nav(Some(5), "ArrowUp", 100, 12), Some(4));
        assert_eq!(clamp_nav(Some(0), "ArrowUp", 100, 12), Some(0), "top clamps, no wrap");
        assert_eq!(clamp_nav(Some(99), "ArrowDown", 100, 12), Some(99), "bottom clamps");
        // Home / End.
        assert_eq!(clamp_nav(Some(50), "Home", 100, 12), Some(0));
        assert_eq!(clamp_nav(Some(50), "End", 100, 12), Some(99));
        // Page steps by `page`, clamped.
        assert_eq!(clamp_nav(Some(50), "PageDown", 100, 12), Some(62));
        assert_eq!(clamp_nav(Some(50), "PageUp", 100, 12), Some(38));
        assert_eq!(clamp_nav(Some(5), "PageUp", 100, 12), Some(0));
        assert_eq!(clamp_nav(Some(95), "PageDown", 100, 12), Some(99));
    }

    #[test]
    fn clamp_nav_from_none_lands_on_first_and_rejects_unknown() {
        for key in ["ArrowDown", "ArrowUp", "PageDown", "PageUp"] {
            assert_eq!(clamp_nav(None, key, 100, 12), Some(0), "{key} from None -> 0");
        }
        assert_eq!(clamp_nav(Some(3), "Tab", 100, 12), None, "unhandled key -> None");
        assert_eq!(clamp_nav(Some(0), "ArrowDown", 0, 12), None, "empty collection -> None");
    }

    use crate::input::Modifiers;

    const NONE: Modifiers = Modifiers::empty();
    const SHIFT: Modifiers = Modifiers { shift: true, ctrl: false, alt: false, meta: false };
    const CTRL: Modifiers = Modifiers { shift: false, ctrl: true, alt: false, meta: false };

    fn grid_scene(tag: &str) -> Scene {
        Scene::External(
            crate::scene::ExternalNode::new(Box::new(VirtualSelectExternal::new(10_000)))
                .with_tag(tag.to_string()),
        )
    }

    fn multi_scene(tag: &str) -> Scene {
        Scene::External(
            crate::scene::ExternalNode::new(Box::new(VirtualSelectExternal::new_multi(10_000)))
                .with_tag(tag.to_string()),
        )
    }

    fn selected_of(scene: &Scene, tag: &str) -> Option<usize> {
        scene
            .find_external_with_tag(tag)
            .and_then(|n| n.handle.introspect())
            .and_then(|i| match i.query("selected") {
                Some(IntrospectValue::Int(v)) => usize::try_from(v).ok(),
                _ => None,
            })
    }

    fn selection_of(scene: &Scene, tag: &str) -> Vec<usize> {
        scene
            .find_external_with_tag(tag)
            .and_then(|n| n.handle.introspect())
            .and_then(|i| match i.query("selection") {
                Some(IntrospectValue::Json(serde_json::Value::Array(items))) => Some(
                    items
                        .iter()
                        .filter_map(serde_json::Value::as_u64)
                        .filter_map(|v| usize::try_from(v).ok())
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_default()
    }

    #[test]
    fn nav_select_key_unfocused_or_unknown_key_is_a_noop() {
        let mut scene = grid_scene("vlist");
        let scroll = ScrollState::new();
        scroll.set_max(0, 320_000);
        scroll.set_measured_viewport(360, 384);
        // Not focused → ignored.
        assert!(!nav_select_key(&mut scene, &scroll, "vlist", Some("other"), "End", NONE, RowMetrics { item_count: 10_000, row_pitch: 32 }));
        assert_eq!(selected_of(&scene, "vlist"), None);
        // Focused but a non-nav key → ignored.
        assert!(!nav_select_key(&mut scene, &scroll, "vlist", Some("vlist"), "Tab", NONE, RowMetrics { item_count: 10_000, row_pitch: 32 }));
        assert_eq!(selected_of(&scene, "vlist"), None);
    }

    #[test]
    fn nav_select_key_selects_and_reveals_a_deep_row() {
        let mut scene = grid_scene("vlist");
        let scroll = ScrollState::new();
        scroll.set_max(0, 320_000);
        scroll.set_measured_viewport(360, 384);
        // End selects the last row and scrolls the offset deep so the row
        // is revealed (a row never materialized at offset 0).
        assert!(nav_select_key(&mut scene, &scroll, "vlist", Some("vlist"), "End", NONE, RowMetrics { item_count: 10_000, row_pitch: 32 }));
        assert_eq!(selected_of(&scene, "vlist"), Some(9_999));
        assert!(scroll.offset_y() > 300_000, "End scrolled deep, offset {}", scroll.offset_y());
        // Home brings selection + scroll back to the top.
        assert!(nav_select_key(&mut scene, &scroll, "vlist", Some("vlist"), "Home", NONE, RowMetrics { item_count: 10_000, row_pitch: 32 }));
        assert_eq!(selected_of(&scene, "vlist"), Some(0));
        assert_eq!(scroll.offset_y(), 0, "Home revealed the top");
    }

    // ── R780 multi-select model ─────────────────────────────────────

    #[test]
    fn single_mode_holds_at_most_one_and_keeps_the_old_wire() {
        let mut s = VirtualSelectExternal::new(100);
        assert_eq!(s.mode(), SelectionMode::Single);
        assert!(s.select(4));
        assert!(s.select(9));
        assert_eq!(s.selection(), vec![9], "single replaces, never accumulates");
        // toggle / extend / select_all all collapse to a plain move.
        assert!(s.toggle(3));
        assert_eq!(s.selection(), vec![3]);
        assert!(s.extend_to(7));
        assert_eq!(s.selection(), vec![7]);
        assert!(!s.select_all(), "select_all is a no-op in single mode");
        assert_eq!(s.selection(), vec![7]);
    }

    #[test]
    fn multi_toggle_accumulates_and_flips() {
        let mut s = VirtualSelectExternal::new_multi(100);
        assert_eq!(s.mode(), SelectionMode::Multi);
        assert!(s.toggle(2));
        assert!(s.toggle(5));
        assert!(s.toggle(9));
        assert_eq!(s.selection(), vec![2, 5, 9], "toggles accumulate");
        assert!(s.is_selected(5));
        assert!(s.toggle(5), "toggling a selected row removes it");
        assert_eq!(s.selection(), vec![2, 9]);
        assert_eq!(s.selected(), Some(5), "the toggled row stays the active row");
        assert_eq!(s.anchor(), Some(5));
    }

    #[test]
    fn multi_modifier_click_toggles_extends_and_replaces() {
        // R781 — the composite pointer wire carries the held modifiers as a
        // third `:` segment; multi-select interprets them.
        let mut s = VirtualSelectExternal::new_multi(100);
        // Plain click moves + replaces (anchor = 4).
        s.handle_send("4:PointerUp");
        assert_eq!(s.selection(), vec![4]);
        // Ctrl-click toggles a second row in (Ctrl = "c").
        s.handle_send("9:PointerUp:c");
        assert_eq!(s.selection(), vec![4, 9], "Ctrl-click accumulates");
        // Ctrl-click an existing member removes it.
        s.handle_send("4:PointerUp:c");
        assert_eq!(s.selection(), vec![9], "Ctrl-click toggles off");
        // Shift-click extends the range from the anchor (now 4, the last
        // toggled row) to the clicked row.
        s.handle_send("6:PointerUp:s");
        assert_eq!(s.selection(), vec![4, 5, 6], "Shift-click extends from anchor 4");
    }

    #[test]
    fn read_selection_round_trips_the_encode() {
        // R782 §5.40 — `read_selection` is the exact inverse of the
        // coordinator's `"selection"` query encode (the R743.1 decode-of-encode
        // contract): selecting a set and decoding through the introspect
        // surface yields the same ascending index list, and an empty
        // selection decodes to an empty `Vec`.
        let mut s = VirtualSelectExternal::new_multi(10_000);
        s.toggle(9);
        s.toggle(2);
        s.toggle(5_000);
        let intro: &dyn ExternalIntrospect = &s;
        assert_eq!(
            read_selection(intro),
            vec![2, 9, 5_000],
            "decode is the inverse of the selection_value encode (ascending set)",
        );
        let empty = VirtualSelectExternal::new_multi(10);
        let empty_intro: &dyn ExternalIntrospect = &empty;
        assert_eq!(read_selection(empty_intro), Vec::<usize>::new(), "empty selection decodes to []");
    }

    #[test]
    fn read_selected_round_trips_the_scalar_encode() {
        // R783.1 — `read_selected` is the exact inverse of the coordinator's
        // `"selected"` query encode (the scalar sibling of `read_selection`):
        // a selected index decodes back to itself, and an empty selection
        // decodes to `None`.
        let mut s = VirtualSelectExternal::new(10_000);
        assert_eq!(read_selected(&s as &dyn ExternalIntrospect), None, "empty → None");
        s.select(4_200);
        assert_eq!(
            read_selected(&s as &dyn ExternalIntrospect),
            Some(4_200),
            "decode is the inverse of the selected_value encode",
        );
    }

    #[test]
    fn single_modifier_click_still_just_selects() {
        // R781 — a Shift / Ctrl-click on a *single*-select coordinator
        // collapses to a plain move (no range, no accumulation): the
        // pre-R781 behaviour is preserved for every existing consumer.
        let mut s = VirtualSelectExternal::new(100);
        s.handle_send("4:PointerUp:s");
        assert_eq!(s.selection(), vec![4]);
        s.handle_send("9:PointerUp:c");
        assert_eq!(s.selection(), vec![9], "single-select replaces, never accumulates");
    }

    #[test]
    fn multi_shift_extends_a_contiguous_range_from_anchor() {
        let mut s = VirtualSelectExternal::new_multi(100);
        s.select(10); // anchor = 10
        assert!(s.extend_to(13));
        assert_eq!(s.selection(), vec![10, 11, 12, 13], "range grows down from anchor");
        // Re-extend the other side: anchor unchanged, range replaced.
        assert!(s.extend_to(8));
        assert_eq!(s.selection(), vec![8, 9, 10], "range grows up, anchor stays at 10");
        assert_eq!(s.anchor(), Some(10));
        assert_eq!(s.selected(), Some(8), "the navigated end is the active row");
    }

    #[test]
    fn multi_select_all_and_clear() {
        let mut s = VirtualSelectExternal::new_multi(5);
        assert!(s.select_all());
        assert_eq!(s.selection(), vec![0, 1, 2, 3, 4]);
        assert!(!s.select_all(), "already-all is a no-op");
        assert!(s.clear());
        assert!(s.selection().is_empty());
        assert_eq!(s.anchor(), None);
    }

    #[test]
    fn multi_select_intent_carries_the_full_set_admin_is_silent() {
        let mut s = VirtualSelectExternal::new_multi(100);
        assert!(s.toggle(2));
        assert!(s.toggle(7));
        let intents = drained(&mut s);
        assert_eq!(intents.len(), 2, "one selection intent per interaction");
        assert_eq!(
            intents[1],
            Intent::new_static(
                "selection",
                IntrospectValue::Json(serde_json::json!([2, 7])),
            ),
            "the intent carries the full set, not the single index",
        );
        // Admin restore is silent on §5.20.
        let restore: BTreeSet<usize> = [1, 4].into_iter().collect();
        s.set_selection(&restore);
        assert!(drained(&mut s).is_empty(), "admin set_selection is silent");
        assert_eq!(s.selection(), vec![1, 4]);
    }

    #[test]
    fn multi_query_and_intervene_round_trip_the_set() {
        let mut s = VirtualSelectExternal::new_multi(100);
        assert_eq!(s.query("mode"), Some(IntrospectValue::Text("multi".into())));
        assert_eq!(
            s.query("selection"),
            Some(IntrospectValue::Json(serde_json::json!([]))),
            "empty selection is an empty array, never Null",
        );
        s.intervene("selection", IntrospectValue::Json(serde_json::json!([3, 8, 800])))
            .expect("array replaces selection");
        assert_eq!(s.selection(), vec![3, 8], "out-of-range index 800 is dropped");
        assert_eq!(s.query("anchor"), Some(IntrospectValue::Int(8)));
        // Null clears.
        s.intervene("selection", IntrospectValue::Null).expect("null clears");
        assert!(s.selection().is_empty());
        // mode / anchor / item_count are read-only.
        assert_eq!(s.intervene("mode", IntrospectValue::Text("single".into())), Err(InterveneError::ReadOnly));
        assert_eq!(s.intervene("anchor", IntrospectValue::Int(0)), Err(InterveneError::ReadOnly));
    }

    #[test]
    fn invoke_multi_paths_return_the_set() {
        let mut s = VirtualSelectExternal::new_multi(100);
        assert_eq!(
            s.invoke("toggle", IntrospectValue::Int(4)),
            Ok(IntrospectValue::Json(serde_json::json!([4]))),
        );
        assert_eq!(
            s.invoke("toggle", IntrospectValue::Int(9)),
            Ok(IntrospectValue::Json(serde_json::json!([4, 9]))),
        );
        // extend_to from anchor 9 (last toggle) up to 11.
        assert_eq!(
            s.invoke("extend_to", IntrospectValue::Int(11)),
            Ok(IntrospectValue::Json(serde_json::json!([9, 10, 11]))),
        );
    }

    #[test]
    fn nav_select_key_shift_extends_in_multi_mode() {
        let mut scene = multi_scene("vlist");
        let scroll = ScrollState::new();
        scroll.set_max(0, 320_000);
        scroll.set_measured_viewport(360, 384);
        // Plain ArrowDown from nothing lands on row 0 (anchor = 0).
        assert!(nav_select_key(&mut scene, &scroll, "vlist", Some("vlist"), "ArrowDown", NONE, RowMetrics { item_count: 10_000, row_pitch: 32 }));
        assert_eq!(selection_of(&scene, "vlist"), vec![0]);
        // Shift+End extends the range to the last row.
        assert!(nav_select_key(&mut scene, &scroll, "vlist", Some("vlist"), "ArrowDown", NONE, RowMetrics { item_count: 10_000, row_pitch: 32 }));
        assert_eq!(selected_of(&scene, "vlist"), Some(1), "plain move resets the anchor to 1");
        assert!(nav_select_key(&mut scene, &scroll, "vlist", Some("vlist"), "ArrowDown", SHIFT, RowMetrics { item_count: 10_000, row_pitch: 32 }));
        assert_eq!(selection_of(&scene, "vlist"), vec![1, 2], "Shift extends from anchor 1");
        assert!(nav_select_key(&mut scene, &scroll, "vlist", Some("vlist"), "ArrowDown", SHIFT, RowMetrics { item_count: 10_000, row_pitch: 32 }));
        assert_eq!(selection_of(&scene, "vlist"), vec![1, 2, 3]);
    }

    #[test]
    fn nav_select_key_ctrl_a_selects_all_and_ctrl_space_toggles() {
        let mut scene = multi_scene("vlist");
        let scroll = ScrollState::new();
        scroll.set_max(0, 320_000);
        scroll.set_measured_viewport(360, 384);
        // Move to a row so there is an active row to toggle.
        assert!(nav_select_key(&mut scene, &scroll, "vlist", Some("vlist"), "ArrowDown", NONE, RowMetrics { item_count: 10_000, row_pitch: 32 }));
        // Ctrl+Space toggles the active row OFF (it was selected by the move).
        assert!(nav_select_key(&mut scene, &scroll, "vlist", Some("vlist"), "Space", CTRL, RowMetrics { item_count: 10_000, row_pitch: 32 }));
        assert!(selection_of(&scene, "vlist").is_empty(), "Ctrl+Space toggled row 0 off");
        // Ctrl+A selects every row.
        assert!(nav_select_key(&mut scene, &scroll, "vlist", Some("vlist"), "a", CTRL, RowMetrics { item_count: 10_000, row_pitch: 32 }));
        assert_eq!(selection_of(&scene, "vlist").len(), 10_000, "Ctrl+A selects all");
    }

    #[test]
    fn nav_select_key_single_mode_ignores_modifiers() {
        // Shift in a single-select model still just moves (no range).
        let mut scene = grid_scene("vlist");
        let scroll = ScrollState::new();
        scroll.set_max(0, 320_000);
        scroll.set_measured_viewport(360, 384);
        assert!(nav_select_key(&mut scene, &scroll, "vlist", Some("vlist"), "ArrowDown", NONE, RowMetrics { item_count: 10_000, row_pitch: 32 }));
        assert!(nav_select_key(&mut scene, &scroll, "vlist", Some("vlist"), "ArrowDown", SHIFT, RowMetrics { item_count: 10_000, row_pitch: 32 }));
        assert_eq!(selection_of(&scene, "vlist"), vec![1], "single mode never accumulates");
        // Ctrl+A is inert in single mode.
        assert!(!nav_select_key(&mut scene, &scroll, "vlist", Some("vlist"), "a", CTRL, RowMetrics { item_count: 10_000, row_pitch: 32 }));
        assert_eq!(selection_of(&scene, "vlist"), vec![1]);
    }
}
