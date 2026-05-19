//! R51.96 §5.38 — `ListBox` widget: framework-owned mutual exclusion
//! across N [`ListBoxItem`] instances, mirroring
//! [`crate::widgets::RadioGroup`] but exposed at the composite layer
//! with WAI-ARIA Listbox semantics (`AriaRole::Listbox` parent +
//! `AriaRole::Option` children, Arrow-keys-focus-only + Space/Enter-
//! commits keyboard model).
//!
//! Pinion's atomic widgets (`ListBoxItem`) own the per-item state
//! machine; the composite owns mutual exclusion + AT-side active
//! descendant + the `"selected"` index intent. The split mirrors the
//! R51.15 `RadioGroup` factoring (framework-owned mutual exclusion;
//! industry consensus = HTML `<select>`, Material `DropdownMenu`,
//! `SwiftUI` `List` with `selection:`, Qt `QListWidget`).
//!
//! Semantic axis vs [`crate::widgets::RadioGroup`]:
//!
//! * **Keyboard model** — `RadioGroup` (W3C ARIA Radio Group): Arrow
//!   keys move focus AND activate the new row immediately. `ListBox`
//!   (W3C ARIA Listbox single-select): Arrow keys move focus only;
//!   `Space` / `Enter` commits the focused row. The composite
//!   primitives are identical (`send` for activate, `set_focused_index`
//!   for focus-only); the application's `apply_key` maps the keys
//!   per the active ARIA pattern.
//! * **ARIA role surface** — `Listbox` + `Option` (vs `RadioGroup` +
//!   `RadioButton`). Chosen by the application's `access_node`
//!   override; the composite stays role-agnostic so the same
//!   primitive can host other ARIA "select one" roles
//!   (`Menu` + `MenuItemRadio`, `TabList` + `Tab`, …).
//! * **Activate-event wire channel** — `ListBoxItem` raises
//!   `listbox_item.activate`; `Radio` raises `radio.activate`. The
//!   composite addresses items by index and does not depend on the
//!   wire channel; the SCXML-level distinction is for future
//!   observers / tooling.
//!
//! Inherits the framework patterns paid through R51.x:
//!
//! * R51.87 §5.40 — `focused_index` carries the AT-side active
//!   descendant independent of `selected_index`. First-class for
//!   `ListBox` (the WAI-ARIA Listbox model genuinely separates the
//!   focused / selected cursors; `RadioGroup`'s separation matters
//!   only for AT-only navigation).
//! * R51.90 §5.40 — activation through [`send`](ListBox::send) syncs
//!   `focused_index` with the new `selected_index` on the activation
//!   edge.
//! * R51.91 §5.40 — `intervene "selected_index"` / `"focused_index"`
//!   use [`InterveneError::OutOfRange`] (not `TypeMismatch`) for
//!   value-domain failures.
//! * R51.93 §5.35 — `pointer_cancel` propagation: touch revoke on a
//!   row mid-press routes `Pressed → Idle` without firing the
//!   activate intent (inherited from the `ListBoxItem` template).
//! * R51.92.2 §5.40 — [`set_selected`](ListBox::set_selected) is a
//!   **slot-assignment** setter (no intent, no focused-sync).

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner,
    ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::listbox_item::{
    listbox_item_state_name, parse_listbox_item_event, ListBoxItem,
    ListboxItemEvent, ListboxItemState,
};
use crate::widgets::{IntentEmitter, WidgetTransition};

/// Logical group of N `ListBoxItem` widgets with framework-owned
/// mutual exclusion. See module docs for the full design rationale
/// and the comparison axis vs [`crate::widgets::RadioGroup`].
pub struct ListBox {
    items: Vec<ListBoxItem>,
    selected: Option<usize>,
    /// R51.87 §5.40 — AT-side active descendant. First-class for
    /// `ListBox` (the WAI-ARIA Listbox keyboard model separates
    /// focus from selection: Arrow moves focus, Space/Enter commits).
    /// Mutated by Arrow-key handling (via `set_focused_index`), AT
    /// `Focus` actions, programmatic restore, or by the
    /// [`send`](Self::send) activation edge (R51.90 sync). `None`
    /// falls back to `selected`-or-0 at the application's
    /// `access_focus_target` resolution.
    focused: Option<usize>,
}

impl ListBox {
    /// Construct a list with `count` items, all unselected.
    /// Use [`set_selected`](Self::set_selected) to seed an initial
    /// selection.
    #[must_use]
    pub fn new(count: usize) -> Self {
        Self {
            items: (0..count).map(|_| ListBoxItem::new()).collect(),
            selected: None,
            focused: None,
        }
    }

    /// Number of items in this list.
    #[must_use]
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Drive `event` to the item at `index`. If the event causes
    /// that item to activate (false → true selected), every other
    /// item in the list is deselected and the list's
    /// `selected_index` snaps to `Some(index)`. R51.90 §5.40 —
    /// activation also syncs `focused_index` to the new index.
    ///
    /// # Panics
    /// Panics if `index >= count()`.
    pub fn send(&mut self, index: usize, event: ListboxItemEvent) {
        let was_selected = self.items[index].is_selected();
        self.items[index].send(event);
        let now_selected = self.items[index].is_selected();
        if !was_selected && now_selected {
            for (j, r) in self.items.iter_mut().enumerate() {
                if j != index {
                    r.set_selected(false);
                }
            }
            self.selected = Some(index);
            // R51.90 §5.40 — activation moves focus. Mirrors
            // `RadioGroup::send`. Differs from RadioGroup in keyboard
            // routing (Arrow keys do not reach `send` in the ARIA
            // Listbox model — only Space/Enter on a focused row does)
            // but the sync rule is identical.
            self.focused = Some(index);
        }
    }

    /// Interaction state of the item at `index`.
    ///
    /// # Panics
    /// Panics if `index >= count()`.
    #[must_use]
    pub fn state(&self, index: usize) -> ListboxItemState {
        self.items[index].state()
    }

    /// Selection state of the item at `index`.
    ///
    /// # Panics
    /// Panics if `index >= count()`.
    #[must_use]
    pub fn is_selected(&self, index: usize) -> bool {
        self.items[index].is_selected()
    }

    /// Index of the currently selected item, or `None` if none.
    #[must_use]
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// Restore the list to a specific selection (persisted preference
    /// restore, form default, programmatic clear). `None` deselects
    /// all; `Some(i)` selects index `i` and deselects all others.
    ///
    /// R51.92.2 semantic axis: **slot-assignment** setter — mutates
    /// `self.selected` directly without firing the `"selected"`
    /// intent and without touching `self.focused`. Only the
    /// interactive [`send`](Self::send) activation edge fires the
    /// intent through [`WidgetTransition::detect`] and syncs focused
    /// per R51.90. The RPC `intervene "selected_index"` route lands
    /// here directly (commit-class side effects are reserved for
    /// genuine user activation).
    ///
    /// # Panics
    /// Panics if `idx` is `Some(i)` with `i >= count()`.
    pub fn set_selected(&mut self, idx: Option<usize>) {
        if let Some(i) = idx {
            assert!(
                i < self.items.len(),
                "ListBox::set_selected index {i} out of range (count={})",
                self.items.len()
            );
        }
        for (j, r) in self.items.iter_mut().enumerate() {
            r.set_selected(idx == Some(j));
        }
        self.selected = idx;
    }

    /// R51.87 §5.40 — AT-side active descendant index. First-class
    /// for `ListBox`: the WAI-ARIA Listbox model uses Arrow keys to
    /// move focus without activating (vs `RadioGroup` where Arrow
    /// activates), so this index is the primary navigation cursor
    /// and `selected` only updates on Space/Enter.
    #[must_use]
    pub fn focused_index(&self) -> Option<usize> {
        self.focused
    }

    /// R51.87 §5.40 — set the AT-side active descendant.
    ///
    /// Independent of `selected_index` — calling this neither
    /// activates the row nor deselects siblings; it only marks the
    /// addressed item. The Listbox composite's `apply_key` typically
    /// routes Arrow / Home / End / letter-key navigation here, and
    /// `Space` / `Enter` to [`send`](Self::send).
    ///
    /// R51.90 §5.40 — activation through [`send`](Self::send) keeps
    /// `focused_index` in sync with `selected_index` on the
    /// activation edge.
    ///
    /// # Panics
    /// Panics if `idx` is `Some(i)` with `i >= count()`.
    pub fn set_focused_index(&mut self, idx: Option<usize>) {
        if let Some(i) = idx {
            assert!(
                i < self.items.len(),
                "ListBox::set_focused_index index {i} out of range (count={})",
                self.items.len()
            );
        }
        self.focused = idx;
    }
}

impl Default for ListBox {
    /// Default constructs an empty list (count = 0). Applications
    /// typically call `ListBox::new(N)` with a concrete count;
    /// Default exists to satisfy `IntentEmitter<W: Default>`-style
    /// generic bounds when needed.
    fn default() -> Self {
        Self::new(0)
    }
}

/// `ListBox` transition contract (R51.12 substrate). Same shape as
/// [`crate::widgets::RadioGroup`]: event pairs the item index with
/// the underlying [`ListboxItemEvent`]; snapshot is the list's
/// selected-index option; detect emits `"selected"` with the new
/// index as [`IntrospectValue::Int`] whenever selection moves.
///
/// Selection transitions that emit:
///
/// * `None → Some(i)` — first selection
/// * `Some(a) → Some(b)` where `a != b` — switch
///
/// Transitions that stay silent (idempotent + clear):
///
/// * `Some(a) → Some(a)` — re-activate same item
/// * `Some(a) → None` — clear (only reachable via
///   [`set_selected`](ListBox::set_selected), not via `send`)
/// * `None → None` — no-op (non-activating event)
impl WidgetTransition for ListBox {
    type Event = (usize, ListboxItemEvent);
    type Snapshot = Option<usize>;

    fn snapshot(&self) -> Self::Snapshot {
        self.selected
    }

    fn drive(&mut self, event: Self::Event) {
        let (idx, ev) = event;
        self.send(idx, ev);
    }

    fn detect(
        before: Self::Snapshot,
        _event: Self::Event,
        after: Self::Snapshot,
    ) -> Option<Intent> {
        if before != after {
            if let Some(idx) = after {
                return Some(Intent::new_static(
                    "selected",
                    IntrospectValue::Int(
                        i64::try_from(idx)
                            .expect("ListBox index must fit in i64"),
                    ),
                ));
            }
        }
        None
    }
}

/// `External` adapter wrapping a [`ListBox`]. Surfaces list state to
/// the §5.12 `scene/query` / `scene/rewind` / `scene/invoke` paths
/// and emits a `"selected"` intent (with the new index as
/// [`IntrospectValue::Int`] payload) on selection-change transitions.
pub struct ListBoxExternal {
    em: IntentEmitter<ListBox>,
}

impl ListBoxExternal {
    /// Construct with `count` items, all unselected.
    #[must_use]
    pub fn new(count: usize) -> Self {
        Self { em: IntentEmitter::new(ListBox::new(count)) }
    }

    /// Drive `event` to the item at `index`. Queues a `"selected"`
    /// intent on selection-change transitions.
    pub fn send(&mut self, index: usize, event: ListboxItemEvent) {
        self.em.dispatch((index, event));
    }

    /// Number of items in the wrapped list.
    #[must_use]
    pub fn count(&self) -> usize {
        self.em.inner.count()
    }

    /// Interaction state of the item at `index`.
    #[must_use]
    pub fn state(&self, index: usize) -> ListboxItemState {
        self.em.inner.state(index)
    }

    /// Selection state of the item at `index`.
    #[must_use]
    pub fn is_selected(&self, index: usize) -> bool {
        self.em.inner.is_selected(index)
    }

    /// Index of the currently selected item, or `None`.
    #[must_use]
    pub fn selected_index(&self) -> Option<usize> {
        self.em.inner.selected_index()
    }

    /// R51.91 §5.40 — shared validation for `selected_index` /
    /// `focused_index` intervene. Negative `i`, `i` overflowing
    /// `usize`, and `idx >= count` all map to
    /// [`InterveneError::OutOfRange`] (value-domain failures), not
    /// `TypeMismatch` (reserved for `Value` shape errors).
    fn resolve_index_intervene(&self, i: i64) -> Result<usize, InterveneError> {
        if i < 0 {
            return Err(InterveneError::OutOfRange);
        }
        let idx = usize::try_from(i).map_err(|_| InterveneError::OutOfRange)?;
        if idx >= self.count() {
            return Err(InterveneError::OutOfRange);
        }
        Ok(idx)
    }

    /// R51.87 §5.40 — AT-side active descendant index, or `None`.
    /// See [`ListBox::focused_index`].
    #[must_use]
    pub fn focused_index(&self) -> Option<usize> {
        self.em.inner.focused_index()
    }
}

impl Default for ListBoxExternal {
    fn default() -> Self {
        Self::new(0)
    }
}

impl core::fmt::Debug for ListBoxExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ListBoxExternal")
            .field("count", &self.count())
            .field("selected_index", &self.selected_index())
            .field("focused_index", &self.focused_index())
            .finish()
    }
}

impl External for ListBoxExternal {
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

    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        self.em.drain(sink);
    }

    fn is_dirty(&self) -> bool {
        self.em.is_dirty()
    }
}

impl ExternalIntrospect for ListBoxExternal {
    fn schema(&self) -> IntrospectSchema {
        // Per-item paths use the same `<index>` placeholder
        // convention as `RadioGroupExternal` (R51.43 §5.38). Schema
        // is discovery metadata for AI clients (`scene/schema` RPC),
        // not a static enumeration of every concrete path.
        IntrospectSchema::new(&[
            ("count", "int"),
            ("selected_index", "int"),
            ("focused_index", "int"),
            ("state.<index>", "string"),
            ("selected.<index>", "bool"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "count" => Some(IntrospectValue::Int(
                i64::try_from(self.count())
                    .expect("ListBox count must fit in i64"),
            )),
            "selected_index" => Some(match self.selected_index() {
                Some(idx) => IntrospectValue::Int(
                    i64::try_from(idx).expect("index fits in i64"),
                ),
                None => IntrospectValue::Null,
            }),
            "focused_index" => Some(match self.focused_index() {
                Some(idx) => IntrospectValue::Int(
                    i64::try_from(idx).expect("index fits in i64"),
                ),
                None => IntrospectValue::Null,
            }),
            _ => {
                if let Some(idx_str) = path.strip_prefix("state.") {
                    let idx: usize = idx_str.parse().ok()?;
                    if idx >= self.count() {
                        return None;
                    }
                    return Some(IntrospectValue::Text(
                        listbox_item_state_name(self.state(idx)).to_string(),
                    ));
                }
                if let Some(idx_str) = path.strip_prefix("selected.") {
                    let idx: usize = idx_str.parse().ok()?;
                    if idx >= self.count() {
                        return None;
                    }
                    return Some(IntrospectValue::Bool(self.is_selected(idx)));
                }
                None
            }
        }
    }

    fn intervene(
        &mut self,
        path: &str,
        value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        match path {
            "count" => Err(InterveneError::ReadOnly),
            "selected_index" => match value {
                IntrospectValue::Int(i) => {
                    let idx = self.resolve_index_intervene(i)?;
                    self.em.inner.set_selected(Some(idx));
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.em.inner.set_selected(None);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "focused_index" => match value {
                IntrospectValue::Int(i) => {
                    let idx = self.resolve_index_intervene(i)?;
                    self.em.inner.set_focused_index(Some(idx));
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.em.inner.set_focused_index(None);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Wire format: "<index>:<EventName>" — e.g. "2:PointerUp"
            // drives a PointerUp on the item at index 2. Mirrors
            // `RadioGroupExternal::invoke "send"`.
            "send" => match args {
                IntrospectValue::Text(ref s) => {
                    let (idx_str, event_name) =
                        s.split_once(':').ok_or(InvokeError::Rejected)?;
                    let idx: usize = idx_str
                        .parse()
                        .map_err(|_| InvokeError::Rejected)?;
                    if idx >= self.count() {
                        return Err(InvokeError::Rejected);
                    }
                    let ev = parse_listbox_item_event(event_name)
                        .ok_or(InvokeError::Rejected)?;
                    self.send(idx, ev);
                    Ok(match self.selected_index() {
                        Some(i) => IntrospectValue::Int(
                            i64::try_from(i).expect("index fits in i64"),
                        ),
                        None => IntrospectValue::Null,
                    })
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn activate(list: &mut ListBox, i: usize) {
        list.send(i, ListboxItemEvent::PointerEnter);
        list.send(i, ListboxItemEvent::PointerDown);
        list.send(i, ListboxItemEvent::PointerUp);
        list.send(i, ListboxItemEvent::PointerLeave);
    }

    #[test]
    fn new_list_has_correct_count_and_no_selection() {
        let l = ListBox::new(4);
        assert_eq!(l.count(), 4);
        assert_eq!(l.selected_index(), None);
        assert_eq!(l.focused_index(), None);
        for i in 0..4 {
            assert!(!l.is_selected(i));
            assert_eq!(l.state(i), ListboxItemState::Idle);
        }
    }

    #[test]
    fn activating_one_selects_only_it() {
        let mut l = ListBox::new(3);
        activate(&mut l, 1);
        assert_eq!(l.selected_index(), Some(1));
        assert!(!l.is_selected(0));
        assert!(l.is_selected(1));
        assert!(!l.is_selected(2));
    }

    #[test]
    fn activating_another_deselects_first() {
        let mut l = ListBox::new(3);
        activate(&mut l, 0);
        activate(&mut l, 2);
        assert_eq!(l.selected_index(), Some(2));
        assert!(!l.is_selected(0));
        assert!(l.is_selected(2));
    }

    #[test]
    fn set_selected_restores_explicit_index() {
        let mut l = ListBox::new(3);
        l.set_selected(Some(1));
        assert_eq!(l.selected_index(), Some(1));
        assert!(l.is_selected(1));
        // R51.92.2 — set_selected does NOT sync focused.
        assert_eq!(l.focused_index(), None);
    }

    #[test]
    fn set_selected_none_clears_all() {
        let mut l = ListBox::new(3);
        activate(&mut l, 1);
        assert!(l.is_selected(1));
        l.set_selected(None);
        assert_eq!(l.selected_index(), None);
        assert!(!l.is_selected(1));
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn set_selected_out_of_range_panics() {
        let mut l = ListBox::new(2);
        l.set_selected(Some(5));
    }

    // R51.87 — focused_index regression.

    #[test]
    fn r51_87_set_focused_index_independent_of_selected() {
        let mut l = ListBox::new(3);
        l.set_focused_index(Some(2));
        assert_eq!(l.focused_index(), Some(2));
        assert_eq!(l.selected_index(), None);
    }

    #[test]
    fn r51_87_focused_and_selected_can_diverge() {
        let mut l = ListBox::new(4);
        activate(&mut l, 0);
        assert_eq!(l.selected_index(), Some(0));
        // ARIA Listbox: Arrow key moves focus only.
        l.set_focused_index(Some(2));
        assert_eq!(l.focused_index(), Some(2));
        assert_eq!(l.selected_index(), Some(0));
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn r51_87_set_focused_index_out_of_range_panics() {
        let mut l = ListBox::new(2);
        l.set_focused_index(Some(5));
    }

    // R51.90 — activate edge syncs focused.

    #[test]
    fn r51_90_first_activate_syncs_focused_to_selected() {
        let mut l = ListBox::new(3);
        assert_eq!(l.focused_index(), None);
        activate(&mut l, 1);
        assert_eq!(l.selected_index(), Some(1));
        assert_eq!(l.focused_index(), Some(1));
    }

    #[test]
    fn r51_90_at_focus_then_activate_collapses_divergence() {
        let mut l = ListBox::new(4);
        activate(&mut l, 0);
        // Arrow-key navigation in Listbox model.
        l.set_focused_index(Some(2));
        assert_eq!(l.focused_index(), Some(2));
        assert_eq!(l.selected_index(), Some(0));
        // Space/Enter on row 3 → both indices snap to 3.
        activate(&mut l, 3);
        assert_eq!(l.selected_index(), Some(3));
        assert_eq!(l.focused_index(), Some(3));
    }

    // R51.93 — touch-cancel propagation.

    #[test]
    fn r51_93_pointer_cancel_does_not_select() {
        let mut l = ListBox::new(3);
        l.send(1, ListboxItemEvent::PointerEnter);
        l.send(1, ListboxItemEvent::PointerDown);
        assert_eq!(l.state(1), ListboxItemState::Pressed);
        l.send(1, ListboxItemEvent::PointerCancel);
        assert_eq!(l.state(1), ListboxItemState::Idle);
        assert_eq!(l.selected_index(), None);
        assert!(!l.is_selected(1));
    }

    // External adapter coverage.

    #[test]
    fn external_emits_selected_intent_with_index_payload() {
        let mut lx = ListBoxExternal::new(3);
        for ev in [
            ListboxItemEvent::PointerEnter,
            ListboxItemEvent::PointerDown,
            ListboxItemEvent::PointerUp,
        ] {
            lx.send(2, ev);
        }
        let mut harvested = Vec::new();
        lx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "selected");
        assert_eq!(harvested[0].payload, IntrospectValue::Int(2));
    }

    #[test]
    fn external_query_count_and_indices() {
        let mut lx = ListBoxExternal::new(3);
        assert_eq!(lx.query("count"), Some(IntrospectValue::Int(3)));
        assert_eq!(
            lx.query("selected_index"),
            Some(IntrospectValue::Null)
        );
        assert_eq!(
            lx.query("focused_index"),
            Some(IntrospectValue::Null)
        );
        // Activate index 1.
        for ev in [
            ListboxItemEvent::PointerEnter,
            ListboxItemEvent::PointerDown,
            ListboxItemEvent::PointerUp,
        ] {
            lx.send(1, ev);
        }
        assert_eq!(
            lx.query("selected_index"),
            Some(IntrospectValue::Int(1))
        );
        assert_eq!(
            lx.query("focused_index"),
            Some(IntrospectValue::Int(1))
        );
    }

    #[test]
    fn external_query_per_item_paths() {
        let mut lx = ListBoxExternal::new(3);
        lx.send(0, ListboxItemEvent::PointerEnter);
        assert_eq!(
            lx.query("state.0"),
            Some(IntrospectValue::Text("Hover".to_string()))
        );
        assert_eq!(
            lx.query("selected.0"),
            Some(IntrospectValue::Bool(false))
        );
        assert_eq!(
            lx.query("state.5"),
            None,
            "out-of-range per-item query returns None"
        );
    }

    #[test]
    fn external_intervene_selected_index_writes_value() {
        let mut lx = ListBoxExternal::new(3);
        lx.intervene("selected_index", IntrospectValue::Int(2))
            .unwrap();
        assert_eq!(lx.selected_index(), Some(2));
        // R51.92.2 — slot-assignment, no intent.
        assert!(!lx.is_dirty());
    }

    #[test]
    fn external_intervene_selected_index_null_clears() {
        let mut lx = ListBoxExternal::new(3);
        lx.intervene("selected_index", IntrospectValue::Int(1)).unwrap();
        lx.intervene("selected_index", IntrospectValue::Null).unwrap();
        assert_eq!(lx.selected_index(), None);
    }

    // R51.91 — OutOfRange variant.

    #[test]
    fn r51_91_selected_index_out_of_range_is_out_of_range() {
        let mut lx = ListBoxExternal::new(2);
        assert_eq!(
            lx.intervene("selected_index", IntrospectValue::Int(5)),
            Err(InterveneError::OutOfRange)
        );
        assert_eq!(
            lx.intervene("selected_index", IntrospectValue::Int(-1)),
            Err(InterveneError::OutOfRange)
        );
    }

    #[test]
    fn r51_91_focused_index_out_of_range_is_out_of_range() {
        let mut lx = ListBoxExternal::new(2);
        assert_eq!(
            lx.intervene("focused_index", IntrospectValue::Int(5)),
            Err(InterveneError::OutOfRange)
        );
    }

    #[test]
    fn r51_91_wrong_variant_is_type_mismatch() {
        let mut lx = ListBoxExternal::new(3);
        assert_eq!(
            lx.intervene("selected_index", IntrospectValue::Bool(true)),
            Err(InterveneError::TypeMismatch)
        );
    }

    #[test]
    fn external_intervene_count_is_read_only() {
        let mut lx = ListBoxExternal::new(2);
        assert_eq!(
            lx.intervene("count", IntrospectValue::Int(3)),
            Err(InterveneError::ReadOnly)
        );
    }

    #[test]
    fn external_invoke_send_drives_indexed_item() {
        let mut lx = ListBoxExternal::new(3);
        let out = lx
            .invoke("send", IntrospectValue::Text("1:PointerEnter".to_string()))
            .unwrap();
        // PointerEnter does not commit, so selected stays None.
        assert_eq!(out, IntrospectValue::Null);
        assert_eq!(lx.state(1), ListboxItemState::Hover);
    }

    #[test]
    fn external_invoke_send_full_activate_returns_new_index() {
        let mut lx = ListBoxExternal::new(3);
        for ev in ["PointerEnter", "PointerDown", "PointerUp"] {
            let out = lx
                .invoke("send", IntrospectValue::Text(format!("0:{ev}")))
                .unwrap();
            if ev == "PointerUp" {
                assert_eq!(out, IntrospectValue::Int(0));
            } else {
                assert_eq!(out, IntrospectValue::Null);
            }
        }
    }

    #[test]
    fn external_invoke_send_out_of_range_rejected() {
        let mut lx = ListBoxExternal::new(2);
        let r = lx.invoke(
            "send",
            IntrospectValue::Text("5:PointerEnter".to_string()),
        );
        assert!(matches!(r, Err(InvokeError::Rejected)));
    }

    #[test]
    fn external_invoke_send_malformed_wire_rejected() {
        let mut lx = ListBoxExternal::new(2);
        let r =
            lx.invoke("send", IntrospectValue::Text("no_colon".to_string()));
        assert!(matches!(r, Err(InvokeError::Rejected)));
    }

    #[test]
    fn r51_93_composite_pointer_cancel_via_wire_format() {
        let mut lx = ListBoxExternal::new(3);
        lx.invoke("send", IntrospectValue::Text("1:PointerEnter".to_string()))
            .unwrap();
        lx.invoke("send", IntrospectValue::Text("1:PointerDown".to_string()))
            .unwrap();
        assert_eq!(lx.state(1), ListboxItemState::Pressed);
        let before_selected = lx.selected_index();
        let before_focused = lx.focused_index();
        lx.invoke("send", IntrospectValue::Text("1:PointerCancel".to_string()))
            .unwrap();
        assert_eq!(lx.state(1), ListboxItemState::Idle);
        assert_eq!(lx.selected_index(), before_selected);
        assert_eq!(lx.focused_index(), before_focused);
        let mut harvested = Vec::new();
        lx.drain_intents(&mut |i| harvested.push(i));
        assert!(
            harvested.iter().all(|i| i.tag_str() != "selected"),
            "PointerCancel must not fire `selected` through the composite"
        );
    }
}
