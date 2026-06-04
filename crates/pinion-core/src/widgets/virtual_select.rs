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
use crate::input::PointerWireEvent;
use crate::intent::Intent;
use crate::widgets::IntentEmitter;

/// R746 §5.27 §5.38 — the plain index holder wrapped by
/// [`VirtualSelectExternal`].
///
/// Pure single-select-by-index state, no interaction statechart and no
/// §5.20 queue of its own — the [`IntentEmitter`] wrapper owns the pending
/// intents (exactly as [`RadioGroup`](crate::widgets::radio_group) is the
/// plain widget inside `IntentEmitter<RadioGroup>`). Holding the selection
/// as a **data index** (not a per-leaf bit) is what decouples it from
/// materialization; `item_count` bounds every mutation so a malformed wire
/// payload can never select a non-existent row.
#[derive(Debug, Clone)]
struct VirtualSelect {
    /// Selected data index, or `None` when nothing is selected.
    selected: Option<usize>,
    /// Total dataset size — the validity bound for any selection.
    item_count: usize,
}

impl VirtualSelect {
    fn new(item_count: usize) -> Self {
        Self { selected: None, item_count }
    }

    /// Set the selection to `index` (single-select). Out-of-range indices
    /// are ignored. Returns `true` if the selection changed — the caller
    /// ([`VirtualSelectExternal::select`]) turns that into the §5.20 intent.
    fn select(&mut self, index: usize) -> bool {
        if index >= self.item_count || self.selected == Some(index) {
            return false;
        }
        self.selected = Some(index);
        true
    }

    fn clear(&mut self) -> bool {
        let had = self.selected.is_some();
        self.selected = None;
        had
    }

    fn set_selected(&mut self, index: Option<usize>) -> bool {
        let next = match index {
            Some(i) if i < self.item_count => Some(i),
            _ => None,
        };
        if next == self.selected {
            return false;
        }
        self.selected = next;
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
    /// Construct a coordinator over an `item_count`-row dataset, nothing
    /// selected.
    #[must_use]
    pub fn new(item_count: usize) -> Self {
        Self { em: IntentEmitter::new(VirtualSelect::new(item_count)) }
    }

    /// The selected data index, or `None`.
    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        self.em.inner.selected
    }

    /// Total dataset size.
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.em.inner.item_count
    }

    /// Set the selection to `index` (single-select) — the **interaction**
    /// path (pointer click / AI `invoke`). Out-of-range indices are
    /// ignored. On a real change, queues a §5.20 `"selected"` intent
    /// carrying the new index. Returns `true` if the selection changed.
    pub fn select(&mut self, index: usize) -> bool {
        if !self.em.inner.select(index) {
            return false;
        }
        if let Ok(i) = i64::try_from(index) {
            self.em
                .push(Intent::new_static("selected", IntrospectValue::Int(i)));
        }
        true
    }

    /// Clear the selection. Returns `true` if something was selected.
    pub fn clear(&mut self) -> bool {
        self.em.inner.clear()
    }

    /// Replace the selection directly (the admin / persisted-restore /
    /// form-default channel — not an interaction). `None` or an
    /// out-of-range index clears. Returns `true` if it changed.
    pub fn set_selected(&mut self, index: Option<usize>) -> bool {
        self.em.inner.set_selected(index)
    }

    /// Drive the composite pointer channel: on the activation edge
    /// (`PointerUp` or `KeyboardActivate`) select the addressed row.
    /// Every other pointer-arc event (`PointerEnter` / `PointerDown` /
    /// `PointerLeave`) the router replays is a harmless no-op — single
    /// selection has no hover/press feedback at the list level.
    fn handle_send(&mut self, payload: &str) {
        let Some((index, event_name)) =
            crate::composite_tag::parse_send_payload::<usize>(payload)
        else {
            return;
        };
        if event_name == "KeyboardActivate"
            || event_name == PointerWireEvent::Up.as_wire_name()
        {
            self.select(index);
        }
    }

    /// The selected index as an `IntrospectValue` (`Int` or `Null`) — the
    /// uniform return for the mutating `invoke` paths.
    fn selected_value(&self) -> IntrospectValue {
        self.selected()
            .and_then(|i| i64::try_from(i).ok())
            .map_or(IntrospectValue::Null, IntrospectValue::Int)
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
        // `selected` — settable selected index (query + intervene).
        // `item_count` — construction-fixed dataset size (query only).
        // `send` — the R51.42 §5.35 composite pointer channel (`<i>:Event`).
        IntrospectSchema::new(&[
            ("selected", "int"),
            ("item_count", "int"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            // A schema-listed path must always return a value (the RPC
            // layer treats a `None` from a declared slot as
            // `UnknownIntrospectPath`); an empty selection reports
            // `Null` (present-but-empty), not absence.
            "selected" => Some(self.selected_value()),
            "item_count" => Some(
                i64::try_from(self.item_count())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int),
            ),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // The selected index is the single writable axis (admin /
            // restore). `Int` selects (out-of-range clears); `Null` clears.
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
            "item_count" => Err(InterveneError::ReadOnly),
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
}
