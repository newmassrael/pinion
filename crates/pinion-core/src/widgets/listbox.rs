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
//! R51.98 §5.38 — **single-select / multi-select mode** is fixed at
//! construction. Use [`ListBox::new`] for the WAI-ARIA single-select
//! Listbox model (mutual exclusion: activating one row deselects every
//! sibling) and [`ListBox::with_multiselect`] for the
//! `aria-multiselectable="true"` model (activation toggles only the
//! addressed row; siblings stay untouched). The two modes share every
//! primitive (`send`, `selected_indices`, `set_selected_indices`,
//! `set_focused_index`); the single-select-only convenience surfaces
//! ([`selected_index`](ListBox::selected_index) /
//! [`set_selected`](ListBox::set_selected)) panic in multi-select mode
//! to signal the mode mismatch at the call site.
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
    parse_listbox_item_event, ListBoxItem,
    ListboxItemEvent, ListboxItemState,
};
use crate::widgets::{IntentEmitter, WidgetTransition};
use crate::WidgetStateName;

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
    /// R51.98 §5.38 — selection-cardinality mode, fixed at
    /// construction. `false` (default via [`Self::new`]) = WAI-ARIA
    /// single-select Listbox (mutual exclusion on activate). `true`
    /// (via [`Self::with_multiselect`]) = `aria-multiselectable="true"`
    /// (activation toggles only the addressed row). Mode is immutable
    /// because flipping it mid-life would either invalidate the
    /// outstanding selection set or silently coerce it — neither
    /// matches any industry-precedent Listbox surface.
    multiselect: bool,
}

impl ListBox {
    /// Construct a single-select list with `count` items, all
    /// unselected.
    ///
    /// Use [`set_selected`](Self::set_selected) to seed an initial
    /// selection. The list is the canonical WAI-ARIA Listbox
    /// single-select model (activating one row deselects every
    /// sibling); for the `aria-multiselectable="true"` variant use
    /// [`Self::with_multiselect`] instead.
    #[must_use]
    pub fn new(count: usize) -> Self {
        Self {
            items: (0..count).map(|_| ListBoxItem::new()).collect(),
            selected: None,
            focused: None,
            multiselect: false,
        }
    }

    /// R51.98 §5.38 — construct a multi-select list with `count`
    /// items, all unselected.
    ///
    /// In multi-select mode each activation toggles only the
    /// addressed row (no sibling deselect). The
    /// [`set_selected`](Self::set_selected) /
    /// [`selected_index`](Self::selected_index) single-select-only
    /// surfaces panic in this mode; use
    /// [`set_selected_indices`](Self::set_selected_indices) and
    /// [`selected_indices`](Self::selected_indices) (which work in
    /// both modes) for programmatic admin.
    #[must_use]
    pub fn with_multiselect(count: usize) -> Self {
        Self {
            items: (0..count).map(|_| ListBoxItem::new()).collect(),
            selected: None,
            focused: None,
            multiselect: true,
        }
    }

    /// R51.98 §5.38 — `true` if this list was constructed via
    /// [`Self::with_multiselect`]. Drives the
    /// `aria-multiselectable` AccessKit attribute at the application's
    /// `access_node` and disambiguates the `send` toggle semantic for
    /// the External adapter's intent emit.
    #[must_use]
    pub fn is_multiselect(&self) -> bool {
        self.multiselect
    }

    /// Number of items in this list.
    #[must_use]
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Drive `event` to the item at `index`.
    ///
    /// **Single-select mode** ([`Self::new`]) — if the event causes
    /// the item to activate (false → true selected), every other
    /// item is deselected and the list's `selected_index` snaps to
    /// `Some(index)`. R51.90 §5.40 — activation also syncs
    /// `focused_index` to the new index.
    ///
    /// **Multi-select mode** ([`Self::with_multiselect`]) — R51.98
    /// §5.38 — activation toggles the addressed item only (already-
    /// selected → deselected; unselected → selected). Siblings stay
    /// untouched, and `selected_index` is *not* updated (it has no
    /// single meaningful value in multi-mode and is invalid to
    /// query). `focused_index` still syncs to the toggled row so the
    /// AT cursor follows the user's last interaction. R51.93 §5.35
    /// `PointerCancel` does not commit the toggle in either mode.
    ///
    /// Activation detection runs off the item's state-machine
    /// transition (`Pressed → Hover` for pointer release;
    /// non-`Disabled` `KeyboardActivate`) so multi-mode toggle-off
    /// (where `ListBoxItem::send` is intentionally idempotent on the
    /// already-`true` value) is still caught.
    ///
    /// # Panics
    /// Panics if `index >= count()`.
    pub fn send(&mut self, index: usize, event: ListboxItemEvent) {
        let pre_state = self.items[index].state();
        let was_selected = self.items[index].is_selected();
        self.items[index].send(event);
        let post_state = self.items[index].state();
        // R51.98 §5.38 — detect activation independent of the value
        // sidecar so multi-mode toggle-off is reachable. Mirrors the
        // `ListBoxItem::send` activation predicate (kept in sync with
        // `crates/pinion-core/widgets/standard_button.sce-template.xml`).
        let activation = (matches!(pre_state, ListboxItemState::Pressed)
            && matches!(post_state, ListboxItemState::Hover))
            || (matches!(event, ListboxItemEvent::KeyboardActivate)
                && !matches!(pre_state, ListboxItemState::Disabled));
        if self.multiselect {
            if activation {
                if was_selected {
                    // Toggle off: atom is set-not-flip, so the
                    // composite forces the new false here.
                    self.items[index].set_selected(false);
                }
                // Else: atom already set true on the activation edge.
                self.focused = Some(index);
            }
        } else {
            let now_selected = self.items[index].is_selected();
            if !was_selected && now_selected {
                for (j, r) in self.items.iter_mut().enumerate() {
                    if j != index {
                        r.set_selected(false);
                    }
                }
                self.selected = Some(index);
                // R51.90 §5.40 — activation moves focus. Mirrors
                // `RadioGroup::send`. Differs from RadioGroup in
                // keyboard routing (Arrow keys do not reach `send` in
                // the ARIA Listbox model — only Space/Enter on a
                // focused row does) but the sync rule is identical.
                self.focused = Some(index);
            }
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
    /// **Single-select only** — see [`Self::selected_indices`] for
    /// the multi-select-aware query.
    ///
    /// # Panics
    /// Panics if [`Self::is_multiselect`] is `true`. A multi-select
    /// list has no single meaningful selected index (zero or many
    /// items may be selected); callers in multi-mode must use
    /// [`Self::selected_indices`].
    #[must_use]
    pub fn selected_index(&self) -> Option<usize> {
        assert!(
            !self.multiselect,
            "ListBox::selected_index() is single-select-only; \
             use selected_indices() in multi-select mode"
        );
        self.selected
    }

    /// R51.98 §5.38 — every currently-selected index, ascending.
    /// Works in both modes: single-select returns 0 or 1 indices,
    /// multi-select returns 0..count indices. This is the canonical
    /// query for multi-mode and the mode-agnostic query for tooling.
    #[must_use]
    pub fn selected_indices(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.is_selected().then_some(i))
            .collect()
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
    /// **Single-select only** — see [`Self::set_selected_indices`]
    /// for the multi-select-aware setter.
    ///
    /// # Panics
    /// * Panics if [`Self::is_multiselect`] is `true`.
    /// * Panics if `idx` is `Some(i)` with `i >= count()`.
    pub fn set_selected(&mut self, idx: Option<usize>) {
        assert!(
            !self.multiselect,
            "ListBox::set_selected() is single-select-only; \
             use set_selected_indices() in multi-select mode"
        );
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

    /// R51.98 §5.38 — slot-assignment setter that works in both
    /// modes. Replaces the entire selection set with `indices`.
    ///
    /// Single-select mode: `indices` must contain 0 or 1 entries
    /// (`selected_index` is the canonical convenience for that
    /// case). Multi-select mode: any subset of `0..count` is valid;
    /// duplicates are deduplicated; order is irrelevant.
    ///
    /// Mirrors [`Self::set_selected`] in being silent on the §5.20
    /// intent channel — restoration is admin, not interaction.
    ///
    /// # Panics
    /// * Panics if any index in `indices` is `>= count()`.
    /// * In single-select mode, panics if `indices.len() > 1`.
    pub fn set_selected_indices(&mut self, indices: &[usize]) {
        for &i in indices {
            assert!(
                i < self.items.len(),
                "ListBox::set_selected_indices index {i} out of range (count={})",
                self.items.len()
            );
        }
        if !self.multiselect {
            assert!(
                indices.len() <= 1,
                "ListBox::set_selected_indices: single-select mode \
                 accepts at most one index, got {}",
                indices.len()
            );
            self.selected = indices.first().copied();
        }
        for (j, item) in self.items.iter_mut().enumerate() {
            item.set_selected(indices.contains(&j));
        }
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

/// R51.102 §5.38 — `ListBox` snapshot capturing both the
/// selection-cardinality mode and the per-item selection bitmap so
/// the trait-level `detect` can express single-select-replace and
/// multi-select-toggle in one rule. The Vec heap-allocates once per
/// `IntentEmitter::dispatch` call (i.e. once per user input event);
/// the cost is negligible for the AI / UI dispatch frequencies, but
/// motivates the `Snapshot: Clone` (not `Copy`) trait bound — see
/// [`WidgetTransition`] for the bound rationale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListBoxSnapshot {
    multiselect: bool,
    bits: Vec<bool>,
}

/// `ListBox` transition contract (R51.12 substrate). Event pairs the
/// item index with the underlying [`ListboxItemEvent`]. R51.102 §5.38
/// — snapshot captures mode + the full per-item bitmap so the
/// trait-level `detect` handles both single-select (replace, emit
/// one `"selected"` per gained row) and multi-select (toggle, emit
/// one `"selected"` per *changed* row in either direction).
///
/// Single-select transitions (`multiselect = false`):
///
/// * `None → Some(i)` — one row gained ⇒ `vec![Int(i)]`
/// * `Some(a) → Some(b)` (`a != b`) — `a` lost, `b` gained ⇒ filter
///   to gained ⇒ `vec![Int(b)]`
/// * `Some(a) → Some(a)` — bitmap unchanged ⇒ `Vec::new()`
/// * `Some(a) → None` — only reachable via
///   [`set_selected`](ListBox::set_selected) (admin), `send` cannot
///   produce it from a non-idle row
/// * `None → None` — no-op ⇒ `Vec::new()`
///
/// Multi-select transitions (`multiselect = true`):
///
/// * one row toggled (false → true *or* true → false) — emit
///   `vec![Int(i)]` for that row; AI follow-up `selected.<i>` query
///   learns the new boolean. Successive toggles on the same row emit
///   toggle-on then toggle-off intents back-to-back (both with the
///   same row index payload).
/// * unchanged bitmap — `Vec::new()`.
///
/// The rule is single-shape:
/// > Emit `Int(i)` for every index `i` whose bit changed, gated in
/// > single-select mode to the `false → true` direction only.
///
/// Single-select gating preserves the pre-R51.102 emit-shape
/// (siblings deselect silently, only the new selection raises an
/// intent) — RadioGroup-class semantic. Multi-select drops the gate
/// because the toggle-off carries genuine information the AI client
/// needs.
impl WidgetTransition for ListBox {
    type Event = (usize, ListboxItemEvent);
    type Snapshot = ListBoxSnapshot;

    fn snapshot(&self) -> Self::Snapshot {
        ListBoxSnapshot {
            multiselect: self.multiselect,
            bits: self.items.iter().map(ListBoxItem::is_selected).collect(),
        }
    }

    fn drive(&mut self, event: Self::Event) {
        let (idx, ev) = event;
        self.send(idx, ev);
    }

    fn detect(
        before: Self::Snapshot,
        _event: Self::Event,
        after: Self::Snapshot,
    ) -> Vec<Intent> {
        // Multi-select carries through the after snapshot's mode flag;
        // the contract panics on mismatched lengths only in
        // debug_assertions (the inner widget's bitmap can't change
        // length mid-dispatch by construction).
        debug_assert_eq!(before.bits.len(), after.bits.len());
        debug_assert_eq!(before.multiselect, after.multiselect);
        let multi = after.multiselect;
        let mut out = Vec::new();
        for (i, (b, a)) in
            before.bits.iter().zip(after.bits.iter()).enumerate()
        {
            if b == a {
                continue;
            }
            // Single-select: only emit on row gain (matches pre-R51.102
            // RadioGroup-class semantic of one intent per activation).
            // Multi-select: emit on every flip (toggle-on + toggle-off
            // carry independent meaning).
            if multi || (!*b && *a) {
                out.push(Intent::new_static(
                    "selected",
                    IntrospectValue::Int(
                        i64::try_from(i).expect("ListBox index fits in i64"),
                    ),
                ));
            }
        }
        out
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
    /// Construct a single-select adapter with `count` items, all
    /// unselected.
    #[must_use]
    pub fn new(count: usize) -> Self {
        Self { em: IntentEmitter::new(ListBox::new(count)) }
    }

    /// R51.98 §5.38 — construct a multi-select adapter with `count`
    /// items, all unselected. The `aria-multiselectable="true"`
    /// model: activating one row toggles only that row's value.
    #[must_use]
    pub fn with_multiselect(count: usize) -> Self {
        Self { em: IntentEmitter::new(ListBox::with_multiselect(count)) }
    }

    /// R51.98 §5.38 — `true` if this adapter was constructed via
    /// [`Self::with_multiselect`].
    #[must_use]
    pub fn is_multiselect(&self) -> bool {
        self.em.inner.is_multiselect()
    }

    /// Drive `event` to the item at `index`. Queues `"selected"`
    /// intents per row that flipped, payload = the flipped index as
    /// [`IntrospectValue::Int`].
    ///
    /// **Single-select mode** — emits on `None → Some(i)` /
    /// `Some(a) → Some(b)` via the [`WidgetTransition::detect`]
    /// gain-only filter; payload is the new `selected_index`.
    ///
    /// **Multi-select mode** (R51.98 §5.38) — emits on any `false ↔
    /// true` flip of the addressed row; payload is the toggled
    /// `index`. The follow-up `selected.<index>` query confirms the
    /// new boolean state. Two consecutive activations on the same
    /// row emit twice (toggle-on, toggle-off).
    ///
    /// R51.102 §5.38 — both modes flow through the same
    /// [`IntentEmitter::dispatch`] pipeline; the multi-mode adapter-
    /// side manual emit carry from R51.98 is retired (the
    /// trait-level `detect` now returns `Vec<Intent>`).
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
    /// **Single-select only** — see [`Self::selected_indices`] for
    /// the multi-select-aware query.
    ///
    /// # Panics
    /// Panics if [`Self::is_multiselect`] is `true`.
    #[must_use]
    pub fn selected_index(&self) -> Option<usize> {
        self.em.inner.selected_index()
    }

    /// R51.98 §5.38 — every currently-selected index, ascending.
    /// Works in both modes.
    #[must_use]
    pub fn selected_indices(&self) -> Vec<usize> {
        self.em.inner.selected_indices()
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
        let mut s = f.debug_struct("ListBoxExternal");
        s.field("count", &self.count())
            .field("multiselect", &self.is_multiselect());
        // R51.98 §5.38 — `selected_index` panics in multi-mode, so the
        // Debug output substitutes the mode-agnostic `selected_indices`
        // there. Single-mode keeps the legacy field for parity with
        // pre-R51.98 logs and tests.
        if self.is_multiselect() {
            s.field("selected_indices", &self.selected_indices());
        } else {
            s.field("selected_index", &self.selected_index());
        }
        s.field("focused_index", &self.focused_index()).finish()
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
        //
        // R51.98 §5.38 — `multiselect` (bool) is read-only mode
        // metadata. `selected.<index>` becomes write-enabled in
        // multi-select mode (canonical multi-mode admin path while
        // an array-typed `IntrospectValue` is still a substrate
        // carry); `selected_index` returns `null` in multi-mode and
        // rejects intervene.
        IntrospectSchema::new(&[
            ("count", "int"),
            ("multiselect", "bool"),
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
            // R51.98 §5.38 — mode metadata exposed to AI clients so a
            // mode-agnostic introspector can pick the right path
            // (`selected.<i>` per-row vs `selected_index` single).
            "multiselect" => Some(IntrospectValue::Bool(self.is_multiselect())),
            "selected_index" => {
                if self.is_multiselect() {
                    // R51.98 §5.38 — multi-mode has no single selected
                    // index; introspect returns Null rather than
                    // panicking like the Rust API. RPC clients use
                    // `selected.<i>` for per-row state.
                    Some(IntrospectValue::Null)
                } else {
                    Some(match self.selected_index() {
                        Some(idx) => IntrospectValue::Int(
                            i64::try_from(idx).expect("index fits in i64"),
                        ),
                        None => IntrospectValue::Null,
                    })
                }
            }
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
                        self.state(idx).as_name().to_string(),
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
            "count" | "multiselect" => Err(InterveneError::ReadOnly),
            "selected_index" => {
                if self.is_multiselect() {
                    // R51.98 §5.38 — multi-mode mode mismatch: no
                    // single index axis. Use `selected.<i>` instead.
                    return Err(InterveneError::ReadOnly);
                }
                match value {
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
                }
            }
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
            // R51.98 §5.38 — per-row write only in multi-select mode.
            // Single-select uses `selected_index` (or `set_selected`
            // Rust API) so the mutual-exclusion invariant cannot be
            // violated through the RPC surface.
            other if other.starts_with("selected.") => {
                if !self.is_multiselect() {
                    return Err(InterveneError::ReadOnly);
                }
                let idx_str = other.strip_prefix("selected.").unwrap_or("");
                let idx: usize =
                    idx_str.parse().map_err(|_| InterveneError::UnknownPath)?;
                if idx >= self.count() {
                    return Err(InterveneError::OutOfRange);
                }
                match value {
                    IntrospectValue::Bool(b) => {
                        let current: Vec<usize> = self.selected_indices();
                        let mut next: Vec<usize> = current
                            .iter()
                            .copied()
                            .filter(|&j| j != idx)
                            .collect();
                        if b {
                            next.push(idx);
                            next.sort_unstable();
                        }
                        self.em.inner.set_selected_indices(&next);
                        Ok(())
                    }
                    _ => Err(InterveneError::TypeMismatch),
                }
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
            // Wire format: "<index>:<EventName>" — e.g. "2:PointerUp"
            // drives a PointerUp on the item at index 2. R660 §5.16
            // routes the parse through [[parse_send_payload]] (the same
            // substrate the application TodoDelete / Toggle / Filter
            // composites and `RadioGroupExternal` use); R51.96's inline
            // `split_once(':')` carry is now repaid (5-of-5 framework
            // substrate maturity).
            "send" => match args {
                IntrospectValue::Text(ref s) => {
                    let (idx, event_name): (usize, &str) =
                        crate::composite_tag::parse_send_payload(s)
                            .ok_or(InvokeError::Rejected)?;
                    if idx >= self.count() {
                        return Err(InvokeError::Rejected);
                    }
                    let ev = parse_listbox_item_event(event_name)
                        .ok_or(InvokeError::Rejected)?;
                    self.send(idx, ev);
                    // R51.98 §5.38 — return path is mode-aware:
                    // single returns the (possibly new) `selected_index`
                    // for legacy parity with the R51.96 surface, while
                    // multi returns Null because there is no single
                    // index. AI clients can follow up with `selected.<i>`
                    // or `selected_indices()` for the new full set.
                    Ok(if self.is_multiselect() {
                        IntrospectValue::Null
                    } else {
                        match self.selected_index() {
                            Some(i) => IntrospectValue::Int(
                                i64::try_from(i).expect("index fits in i64"),
                            ),
                            None => IntrospectValue::Null,
                        }
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

    // R51.98 — multi-select mode regression.

    #[test]
    fn r51_98_new_constructs_single_select() {
        let l = ListBox::new(3);
        assert!(!l.is_multiselect());
        assert_eq!(l.selected_index(), None);
        assert_eq!(l.selected_indices(), Vec::<usize>::new());
    }

    #[test]
    fn r51_98_with_multiselect_constructs_multi() {
        let l = ListBox::with_multiselect(4);
        assert!(l.is_multiselect());
        assert_eq!(l.selected_indices(), Vec::<usize>::new());
        assert_eq!(l.focused_index(), None);
    }

    #[test]
    fn r51_98_multi_activate_toggles_only_addressed_row() {
        let mut l = ListBox::with_multiselect(4);
        activate(&mut l, 1);
        assert!(l.is_selected(1));
        assert!(!l.is_selected(0));
        assert!(!l.is_selected(2));
        assert!(!l.is_selected(3));
    }

    #[test]
    fn r51_98_multi_two_independent_selections_coexist() {
        let mut l = ListBox::with_multiselect(4);
        activate(&mut l, 0);
        activate(&mut l, 2);
        assert!(l.is_selected(0));
        assert!(l.is_selected(2));
        assert!(!l.is_selected(1));
        assert!(!l.is_selected(3));
        assert_eq!(l.selected_indices(), vec![0, 2]);
    }

    #[test]
    fn r51_98_multi_re_activate_deselects_addressed_row() {
        let mut l = ListBox::with_multiselect(3);
        activate(&mut l, 1);
        assert!(l.is_selected(1));
        // Re-activate the same row: toggle-off.
        activate(&mut l, 1);
        assert!(!l.is_selected(1));
        assert_eq!(l.selected_indices(), Vec::<usize>::new());
    }

    #[test]
    fn r51_98_multi_keyboard_activate_toggles() {
        let mut l = ListBox::with_multiselect(3);
        l.send(0, ListboxItemEvent::KeyboardActivate);
        assert!(l.is_selected(0));
        l.send(0, ListboxItemEvent::KeyboardActivate);
        assert!(!l.is_selected(0), "Space/Enter on selected multi-row deselects");
    }

    #[test]
    fn r51_98_multi_focused_follows_toggle() {
        let mut l = ListBox::with_multiselect(4);
        assert_eq!(l.focused_index(), None);
        activate(&mut l, 2);
        assert_eq!(l.focused_index(), Some(2));
        // Toggle-off also syncs focus (last interaction).
        activate(&mut l, 2);
        assert_eq!(l.focused_index(), Some(2));
        // New row's toggle moves focus.
        activate(&mut l, 0);
        assert_eq!(l.focused_index(), Some(0));
    }

    #[test]
    fn r51_98_multi_pointer_cancel_does_not_toggle() {
        let mut l = ListBox::with_multiselect(3);
        activate(&mut l, 1);
        assert!(l.is_selected(1));
        l.send(1, ListboxItemEvent::PointerEnter);
        l.send(1, ListboxItemEvent::PointerDown);
        assert_eq!(l.state(1), ListboxItemState::Pressed);
        l.send(1, ListboxItemEvent::PointerCancel);
        assert_eq!(l.state(1), ListboxItemState::Idle);
        assert!(l.is_selected(1), "PointerCancel must not flip selection");
    }

    #[test]
    fn r51_98_multi_disabled_ignores_keyboard_activate() {
        let mut l = ListBox::with_multiselect(3);
        l.send(0, ListboxItemEvent::Disable);
        l.send(0, ListboxItemEvent::KeyboardActivate);
        assert!(!l.is_selected(0));
        assert_eq!(l.state(0), ListboxItemState::Disabled);
    }

    #[test]
    #[should_panic(expected = "single-select-only")]
    fn r51_98_selected_index_panics_in_multi() {
        let l = ListBox::with_multiselect(2);
        let _ = l.selected_index();
    }

    #[test]
    #[should_panic(expected = "single-select-only")]
    fn r51_98_set_selected_panics_in_multi() {
        let mut l = ListBox::with_multiselect(2);
        l.set_selected(Some(0));
    }

    #[test]
    fn r51_98_set_selected_indices_works_in_multi() {
        let mut l = ListBox::with_multiselect(4);
        l.set_selected_indices(&[1, 3]);
        assert!(!l.is_selected(0));
        assert!(l.is_selected(1));
        assert!(!l.is_selected(2));
        assert!(l.is_selected(3));
        assert_eq!(l.selected_indices(), vec![1, 3]);
    }

    #[test]
    fn r51_98_set_selected_indices_works_in_single() {
        let mut l = ListBox::new(4);
        l.set_selected_indices(&[2]);
        assert!(l.is_selected(2));
        assert_eq!(l.selected_index(), Some(2));
    }

    #[test]
    fn r51_98_set_selected_indices_single_empty_clears() {
        let mut l = ListBox::new(3);
        l.set_selected(Some(1));
        l.set_selected_indices(&[]);
        assert_eq!(l.selected_index(), None);
        assert!(!l.is_selected(1));
    }

    #[test]
    #[should_panic(expected = "single-select mode")]
    fn r51_98_set_selected_indices_single_rejects_two() {
        let mut l = ListBox::new(4);
        l.set_selected_indices(&[0, 2]);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn r51_98_set_selected_indices_out_of_range_panics() {
        let mut l = ListBox::with_multiselect(2);
        l.set_selected_indices(&[5]);
    }

    #[test]
    fn r51_98_set_selected_indices_replaces_set() {
        let mut l = ListBox::with_multiselect(4);
        l.set_selected_indices(&[0, 1, 2]);
        l.set_selected_indices(&[3]);
        assert_eq!(l.selected_indices(), vec![3]);
    }

    #[test]
    fn r51_98_set_selected_indices_dedup_safe() {
        let mut l = ListBox::with_multiselect(3);
        l.set_selected_indices(&[1, 1, 1]);
        assert!(l.is_selected(1));
        assert_eq!(l.selected_indices(), vec![1]);
    }

    #[test]
    fn r51_98_external_multi_constructor() {
        let lx = ListBoxExternal::with_multiselect(3);
        assert!(lx.is_multiselect());
        assert_eq!(lx.count(), 3);
        assert_eq!(lx.selected_indices(), Vec::<usize>::new());
    }

    #[test]
    fn r51_98_external_multi_emits_selected_on_toggle_on() {
        let mut lx = ListBoxExternal::with_multiselect(3);
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
    fn r51_98_external_multi_emits_selected_on_toggle_off() {
        let mut lx = ListBoxExternal::with_multiselect(3);
        // First activation: emits toggle-on.
        for ev in [
            ListboxItemEvent::PointerEnter,
            ListboxItemEvent::PointerDown,
            ListboxItemEvent::PointerUp,
        ] {
            lx.send(1, ev);
        }
        // Second activation on same row: emits toggle-off.
        for ev in [
            ListboxItemEvent::PointerDown,
            ListboxItemEvent::PointerUp,
        ] {
            lx.send(1, ev);
        }
        let mut harvested = Vec::new();
        lx.drain_intents(&mut |i| harvested.push(i));
        // Two emits, both tag = "selected", both payload = Int(1).
        // The mode-agnostic semantic is "the index that flipped";
        // AI follows up with `selected.1` query to learn direction.
        assert_eq!(harvested.len(), 2);
        for intent in &harvested {
            assert_eq!(intent.tag_str(), "selected");
            assert_eq!(intent.payload, IntrospectValue::Int(1));
        }
    }

    #[test]
    fn r51_98_external_query_multiselect_path() {
        let single = ListBoxExternal::new(3);
        assert_eq!(
            single.query("multiselect"),
            Some(IntrospectValue::Bool(false))
        );
        let multi = ListBoxExternal::with_multiselect(3);
        assert_eq!(
            multi.query("multiselect"),
            Some(IntrospectValue::Bool(true))
        );
    }

    #[test]
    fn r51_98_external_query_selected_index_null_in_multi() {
        let mut lx = ListBoxExternal::with_multiselect(3);
        for ev in [
            ListboxItemEvent::PointerEnter,
            ListboxItemEvent::PointerDown,
            ListboxItemEvent::PointerUp,
        ] {
            lx.send(1, ev);
        }
        // selected_index returns Null in multi-mode even when items
        // are selected — there is no single "the" selected index.
        assert_eq!(
            lx.query("selected_index"),
            Some(IntrospectValue::Null)
        );
        // Per-row selection is still queryable.
        assert_eq!(
            lx.query("selected.1"),
            Some(IntrospectValue::Bool(true))
        );
    }

    #[test]
    fn r51_98_external_intervene_selected_index_read_only_in_multi() {
        let mut lx = ListBoxExternal::with_multiselect(3);
        assert_eq!(
            lx.intervene("selected_index", IntrospectValue::Int(1)),
            Err(InterveneError::ReadOnly)
        );
    }

    #[test]
    fn r51_98_external_intervene_multiselect_is_read_only() {
        let mut lx = ListBoxExternal::new(2);
        assert_eq!(
            lx.intervene("multiselect", IntrospectValue::Bool(true)),
            Err(InterveneError::ReadOnly)
        );
    }

    #[test]
    fn r51_98_external_intervene_selected_dot_writes_in_multi() {
        let mut lx = ListBoxExternal::with_multiselect(4);
        lx.intervene("selected.1", IntrospectValue::Bool(true)).unwrap();
        lx.intervene("selected.3", IntrospectValue::Bool(true)).unwrap();
        assert_eq!(lx.selected_indices(), vec![1, 3]);
        // Slot-assignment: no `"selected"` intent (mirrors
        // set_selected/set_selected_indices admin semantics).
        assert!(!lx.is_dirty());
        // Clear one.
        lx.intervene("selected.1", IntrospectValue::Bool(false)).unwrap();
        assert_eq!(lx.selected_indices(), vec![3]);
    }

    #[test]
    fn r51_98_external_intervene_selected_dot_read_only_in_single() {
        let mut lx = ListBoxExternal::new(3);
        assert_eq!(
            lx.intervene("selected.1", IntrospectValue::Bool(true)),
            Err(InterveneError::ReadOnly)
        );
    }

    #[test]
    fn r51_98_external_intervene_selected_dot_out_of_range() {
        let mut lx = ListBoxExternal::with_multiselect(2);
        assert_eq!(
            lx.intervene("selected.5", IntrospectValue::Bool(true)),
            Err(InterveneError::OutOfRange)
        );
    }

    #[test]
    fn r51_98_external_intervene_selected_dot_wrong_variant() {
        let mut lx = ListBoxExternal::with_multiselect(2);
        assert_eq!(
            lx.intervene("selected.0", IntrospectValue::Int(1)),
            Err(InterveneError::TypeMismatch)
        );
    }

    #[test]
    fn r51_98_external_invoke_send_in_multi_emits_selected() {
        let mut lx = ListBoxExternal::with_multiselect(3);
        for ev in ["PointerEnter", "PointerDown", "PointerUp"] {
            lx.invoke("send", IntrospectValue::Text(format!("0:{ev}")))
                .unwrap();
        }
        // invoke return for activation is selected_index; in multi
        // mode that is Null. The intent emit carries the toggled index.
        let mut harvested = Vec::new();
        lx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].payload, IntrospectValue::Int(0));
        assert!(lx.is_selected(0));
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
