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
use crate::widgets::scroll::ScrollState;
use crate::widgets::virtual_list::scroll_offset_to_reveal;
use crate::widgets::IntentEmitter;
use crate::Scene;

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
        let Some((key, event_name)) = payload.split_once(':') else {
            return;
        };
        if event_name.is_empty() {
            return;
        }
        let row = match crate::composite_tag::GridSendKey::parse(key) {
            Some(grid_key) => grid_key.row(),
            None => key.parse::<usize>().ok(),
        };
        let Some(index) = row else {
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

/// R777 §5.27 — drive keyboard navigation for an index-model virtualized
/// **collection** (a virtualized list *or* grid) backed by a
/// [`VirtualSelectExternal`] at `tag` and a flex-viewport [`ScrollState`].
///
/// This is the shared `WidgetCore::apply_key` body behind `hello-virtual-nav`
/// (list) and `hello-grid-nav` (grid): the wiring is byte-identical between
/// the two (only the tag, scroll state, row pitch, and item count differ),
/// and a divergence — selecting or revealing differently in the grid than
/// the list — would be a bug, not a style choice. So it lifts here on the
/// second consumer (the R758 self-grep mandate) rather than living twice.
///
/// On a handled key it:
/// 1. resolves the page size from the measured viewport (`measured_h /
///    row_pitch`, at least 1);
/// 2. reads the coordinator's current `selected` index;
/// 3. computes the next index via the linear-clamp policy ([`clamp_nav`]);
/// 4. sets it through the coordinator's AI-first `invoke("select", …)` path
///    (the same wire a `scene/invoke` drives — keyboard and RPC selection
///    are one funnel);
/// 5. scrolls the new selection into view with [`scroll_offset_to_reveal`]
///    (so navigating to a never-materialized row scrolls there).
///
/// Returns `true` when the key was handled (the grid/list was focused and
/// the key is a navigation key), `false` otherwise — the exact bool
/// `apply_key` must return. Keys only route when `focused == Some(tag)`
/// (single tab stop, no sibling aliasing).
///
/// `row_pitch` must be the uniform per-row pitch the body windows against
/// (the list row pitch / the grid data-row height); `item_count` is the
/// full dataset size.
pub fn nav_select_key(
    scene: &mut Scene,
    scroll: &ScrollState,
    tag: &str,
    focused: Option<&str>,
    key: &str,
    item_count: usize,
    row_pitch: u32,
) -> bool {
    if focused != Some(tag) {
        return false;
    }
    let (_, measured_h) = scroll.measured_viewport();
    let page = usize::try_from(measured_h / row_pitch.max(1)).unwrap_or(1).max(1);

    let Some(node) = scene.find_external_with_tag_mut(tag) else {
        return false;
    };
    let current = node
        .handle
        .introspect()
        .and_then(|intro| match intro.query("selected") {
            Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
            _ => None,
        });
    let Some(target) = clamp_nav(current, key, item_count, page) else {
        return false;
    };
    if let (Some(intro), Ok(t)) = (node.handle.introspect_mut(), i64::try_from(target)) {
        let _ = intro.invoke("select", IntrospectValue::Int(t));
    }
    let reveal = scroll_offset_to_reveal(target, scroll.offset_y(), measured_h, row_pitch);
    scroll.scroll_to(0, reveal);
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

    fn grid_scene(tag: &str) -> Scene {
        Scene::External(
            crate::scene::ExternalNode::new(Box::new(VirtualSelectExternal::new(10_000)))
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

    #[test]
    fn nav_select_key_unfocused_or_unknown_key_is_a_noop() {
        let mut scene = grid_scene("vlist");
        let scroll = ScrollState::new();
        scroll.set_max(0, 320_000);
        scroll.set_measured_viewport(360, 384);
        // Not focused → ignored.
        assert!(!nav_select_key(&mut scene, &scroll, "vlist", Some("other"), "End", 10_000, 32));
        assert_eq!(selected_of(&scene, "vlist"), None);
        // Focused but a non-nav key → ignored.
        assert!(!nav_select_key(&mut scene, &scroll, "vlist", Some("vlist"), "Tab", 10_000, 32));
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
        assert!(nav_select_key(&mut scene, &scroll, "vlist", Some("vlist"), "End", 10_000, 32));
        assert_eq!(selected_of(&scene, "vlist"), Some(9_999));
        assert!(scroll.offset_y() > 300_000, "End scrolled deep, offset {}", scroll.offset_y());
        // Home brings selection + scroll back to the top.
        assert!(nav_select_key(&mut scene, &scroll, "vlist", Some("vlist"), "Home", 10_000, 32));
        assert_eq!(selected_of(&scene, "vlist"), Some(0));
        assert_eq!(scroll.offset_y(), 0, "Home revealed the top");
    }
}
