//! R51.15 §5.38 — `RadioGroup` widget: framework-owned mutual
//! exclusion across N Radio widgets. R47-class framework-primitive
//! boundary fix — before R51.15, the "deselect siblings on
//! activate" rule was application-side responsibility (each app
//! had to call [`crate::widgets::radio::Radio::set_selected`] on
//! the other Radios after one was activated). Industry consensus
//! (HTML `<input type="radio" name="...">`, Material `RadioGroup`,
//! `SwiftUI` `Picker`, Qt `QButtonGroup`) treats mutual exclusion as
//! framework-owned; pinion now matches.
//!
//! `RadioGroup` owns N [`Radio`] instances and provides indexed
//! access: [`state`](RadioGroup::state) /
//! [`is_selected`](RadioGroup::is_selected) /
//! [`send`](RadioGroup::send). The group enforces "at most one
//! selected" — when one Radio activates (false → true selected),
//! all others get `set_selected(false)`. The model-driven path
//! [`set_selected`](RadioGroup::set_selected) is for persisted
//! preference restore, form defaults, or programmatic clear.
//!
//! Visual scene placement is the application's responsibility:
//! applications query the group for per-Radio state and render
//! their own scene tree (typical pattern: vertical Box layout
//! of N rows, each row = `Scene::Text` label + `Radio` indicator).
//! Future R51.x may add a Scene-side `RadioGroupNode` helper that
//! consumes the group's introspect schema; the logical group
//! lands first because mutual exclusion is the framework debt.
//!
//! The [`RadioGroupExternal`] adapter exposes the group on the
//! §5.12 RPC surface:
//!
//! * `query "count"` → [`IntrospectValue::Int`] — N
//! * `query "selected_index"` → [`IntrospectValue::Int`] or
//!   [`IntrospectValue::Null`]
//! * `intervene "selected_index"` (Int or Null) — restore path
//! * `invoke "send" → "<index>:<EventName>"` — drive one Radio
//!
//! On selection-change transitions, the §5.20 channel emits a
//! `"selected"` intent carrying the new index as
//! [`IntrospectValue::Int`].

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner,
    ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::radio::{Radio, RadioEvent, RadioState, parse_radio_event, radio_state_name};
use crate::widgets::{IntentEmitter, WidgetTransition};

/// Logical group of N Radio widgets with framework-owned mutual
/// exclusion. See module docs for the full design rationale.
pub struct RadioGroup {
    radios: Vec<Radio>,
    selected: Option<usize>,
}

impl RadioGroup {
    /// Construct a group with `count` Radios, all unselected.
    /// Use [`set_selected`](Self::set_selected) to seed an initial
    /// selection.
    #[must_use]
    pub fn new(count: usize) -> Self {
        Self {
            radios: (0..count).map(|_| Radio::new()).collect(),
            selected: None,
        }
    }

    /// Number of Radios in this group.
    #[must_use]
    pub fn count(&self) -> usize {
        self.radios.len()
    }

    /// Drive `event` to the Radio at `index`. If the event causes
    /// that Radio to activate (false → true selected), every other
    /// Radio in the group is deselected and the group's
    /// `selected_index` snaps to `Some(index)`.
    ///
    /// # Panics
    /// Panics if `index >= count()`.
    pub fn send(&mut self, index: usize, event: RadioEvent) {
        let was_selected = self.radios[index].is_selected();
        self.radios[index].send(event);
        let now_selected = self.radios[index].is_selected();
        if !was_selected && now_selected {
            for (j, r) in self.radios.iter_mut().enumerate() {
                if j != index {
                    r.set_selected(false);
                }
            }
            self.selected = Some(index);
        }
    }

    /// Interaction state of the Radio at `index`.
    ///
    /// # Panics
    /// Panics if `index >= count()`.
    #[must_use]
    pub fn state(&self, index: usize) -> RadioState {
        self.radios[index].state()
    }

    /// Selection state of the Radio at `index`.
    ///
    /// # Panics
    /// Panics if `index >= count()`.
    #[must_use]
    pub fn is_selected(&self, index: usize) -> bool {
        self.radios[index].is_selected()
    }

    /// Index of the currently selected Radio, or `None` if none.
    #[must_use]
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// Restore the group to a specific selection (persisted
    /// preference restore, form default, programmatic clear).
    /// `None` deselects all; `Some(i)` selects index `i` and
    /// deselects all others.
    ///
    /// # Panics
    /// Panics if `idx` is `Some(i)` with `i >= count()`.
    pub fn set_selected(&mut self, idx: Option<usize>) {
        if let Some(i) = idx {
            assert!(
                i < self.radios.len(),
                "RadioGroup::set_selected index {i} out of range (count={})",
                self.radios.len()
            );
        }
        for (j, r) in self.radios.iter_mut().enumerate() {
            r.set_selected(idx == Some(j));
        }
        self.selected = idx;
    }
}

impl Default for RadioGroup {
    /// Default constructs an empty group (count = 0). Applications
    /// typically call `RadioGroup::new(N)` with a concrete count;
    /// Default exists to satisfy `IntentEmitter<W: Default>`-style
    /// generic bounds when needed.
    fn default() -> Self {
        Self::new(0)
    }
}

/// `RadioGroup` transition contract (R51.12 substrate). The event
/// type pairs the Radio index with the underlying [`RadioEvent`];
/// the snapshot is the group's selected-index option; detect emits
/// `"selected"` with the new index as [`IntrospectValue::Int`]
/// whenever the selection actually moves.
///
/// Selection transitions that emit:
///
/// * `None → Some(i)` — first selection
/// * `Some(a) → Some(b)` where `a != b` — switch
///
/// Transitions that stay silent (idempotent + clear):
///
/// * `Some(a) → Some(a)` — re-activate same Radio
/// * `Some(a) → None` — clear (only reachable via
///   [`set_selected`](RadioGroup::set_selected), not via send)
/// * `None → None` — no-op (non-activating event)
impl WidgetTransition for RadioGroup {
    type Event = (usize, RadioEvent);
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
                            .expect("RadioGroup index must fit in i64"),
                    ),
                ));
            }
        }
        None
    }
}

/// `External` adapter wrapping a [`RadioGroup`]. Surfaces group
/// state to the §5.12 `scene/query` / `scene/rewind` / `scene/invoke`
/// paths and emits a `"selected"` intent (with the new index as
/// [`IntrospectValue::Int`] payload) on selection-change
/// transitions.
pub struct RadioGroupExternal {
    em: IntentEmitter<RadioGroup>,
}

impl RadioGroupExternal {
    /// Construct with `count` Radios, all unselected.
    #[must_use]
    pub fn new(count: usize) -> Self {
        Self { em: IntentEmitter::new(RadioGroup::new(count)) }
    }

    /// Drive `event` to the Radio at `index`. Queues a `"selected"`
    /// intent on selection-change transitions.
    pub fn send(&mut self, index: usize, event: RadioEvent) {
        self.em.dispatch((index, event));
    }

    /// Number of Radios in the wrapped group.
    #[must_use]
    pub fn count(&self) -> usize {
        self.em.inner.count()
    }

    /// Interaction state of the Radio at `index`.
    #[must_use]
    pub fn state(&self, index: usize) -> RadioState {
        self.em.inner.state(index)
    }

    /// Selection state of the Radio at `index`.
    #[must_use]
    pub fn is_selected(&self, index: usize) -> bool {
        self.em.inner.is_selected(index)
    }

    /// Index of the currently selected Radio, or `None`.
    #[must_use]
    pub fn selected_index(&self) -> Option<usize> {
        self.em.inner.selected_index()
    }
}

impl Default for RadioGroupExternal {
    /// Default constructs an empty wrapping group (count = 0).
    fn default() -> Self {
        Self::new(0)
    }
}

impl core::fmt::Debug for RadioGroupExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RadioGroupExternal")
            .field("count", &self.count())
            .field("selected_index", &self.selected_index())
            .finish()
    }
}

impl External for RadioGroupExternal {
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

impl ExternalIntrospect for RadioGroupExternal {
    fn schema(&self) -> IntrospectSchema {
        // The per-radio paths advertise their `<index>` placeholder
        // in the schema slot the same way `send` documents its
        // `"<index>:<EventName>"` wire format — the schema is
        // discovery metadata for AI clients (`scene/schema` RPC),
        // not a static enumeration of every concrete path. R51.43
        // §5.38 introduces the per-radio paths so a `WidgetView`
        // running on top of `RadioGroupExternal` can read each
        // radio's interaction state + selected bit through the same
        // introspect surface AI agents use.
        IntrospectSchema::new(&[
            ("count", "int"),
            ("selected_index", "int"),
            ("state.<index>", "string"),
            ("selected.<index>", "bool"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "count" => Some(IntrospectValue::Int(
                i64::try_from(self.count())
                    .expect("RadioGroup count must fit in i64"),
            )),
            "selected_index" => Some(match self.selected_index() {
                Some(idx) => IntrospectValue::Int(
                    i64::try_from(idx).expect("index fits in i64"),
                ),
                None => IntrospectValue::Null,
            }),
            _ => {
                // R51.43 §5.38 — per-radio query paths route through
                // `state.<i>` (interaction state name, mirrors
                // `Radio` widget's own `query("state")`) and
                // `selected.<i>` (boolean selected bit, mirrors
                // `Radio::query("selected")`). Out-of-range indices
                // and malformed suffixes return `None`; the router
                // and the `scene/query` RPC treat that as "no such
                // path" silently.
                if let Some(idx_str) = path.strip_prefix("state.") {
                    let idx: usize = idx_str.parse().ok()?;
                    if idx >= self.count() {
                        return None;
                    }
                    return Some(IntrospectValue::Text(
                        radio_state_name(self.state(idx)).to_string(),
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
                    if i < 0 {
                        return Err(InterveneError::TypeMismatch);
                    }
                    let idx = usize::try_from(i)
                        .map_err(|_| InterveneError::TypeMismatch)?;
                    if idx >= self.count() {
                        return Err(InterveneError::TypeMismatch);
                    }
                    self.em.inner.set_selected(Some(idx));
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.em.inner.set_selected(None);
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
            // drives a PointerUp on the Radio at index 2. Single-line
            // string keeps the JSON-RPC envelope simple; structured
            // args via IntrospectValue::Json is a future extension.
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
                    let ev = parse_radio_event(event_name)
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

    fn activate(group: &mut RadioGroup, i: usize) {
        group.send(i, RadioEvent::PointerEnter);
        group.send(i, RadioEvent::PointerDown);
        group.send(i, RadioEvent::PointerUp);
        group.send(i, RadioEvent::PointerLeave);
    }

    fn activate_ext(g: &mut RadioGroupExternal, i: usize) {
        g.send(i, RadioEvent::PointerEnter);
        g.send(i, RadioEvent::PointerDown);
        g.send(i, RadioEvent::PointerUp);
        g.send(i, RadioEvent::PointerLeave);
    }

    #[test]
    fn new_group_has_correct_count_and_no_selection() {
        let g = RadioGroup::new(3);
        assert_eq!(g.count(), 3);
        assert_eq!(g.selected_index(), None);
        for i in 0..3 {
            assert!(!g.is_selected(i));
            assert_eq!(g.state(i), RadioState::Idle);
        }
    }

    #[test]
    fn activating_one_radio_selects_it_and_no_others() {
        let mut g = RadioGroup::new(3);
        activate(&mut g, 1);
        assert_eq!(g.selected_index(), Some(1));
        assert!(!g.is_selected(0));
        assert!(g.is_selected(1));
        assert!(!g.is_selected(2));
    }

    #[test]
    fn activating_a_second_radio_deselects_the_first() {
        let mut g = RadioGroup::new(3);
        activate(&mut g, 0);
        assert!(g.is_selected(0));
        activate(&mut g, 2);
        assert_eq!(g.selected_index(), Some(2));
        assert!(!g.is_selected(0), "first must be deselected");
        assert!(!g.is_selected(1));
        assert!(g.is_selected(2));
    }

    #[test]
    fn reactivating_same_radio_is_idempotent() {
        let mut g = RadioGroup::new(3);
        activate(&mut g, 1);
        activate(&mut g, 1);
        assert_eq!(g.selected_index(), Some(1));
        assert!(g.is_selected(1));
    }

    #[test]
    fn cancel_during_press_does_not_change_selection() {
        let mut g = RadioGroup::new(3);
        activate(&mut g, 0);
        // Press but cancel via PointerLeave on a different radio.
        g.send(2, RadioEvent::PointerEnter);
        g.send(2, RadioEvent::PointerDown);
        g.send(2, RadioEvent::PointerLeave);
        assert_eq!(g.selected_index(), Some(0));
        assert!(g.is_selected(0));
        assert!(!g.is_selected(2));
    }

    #[test]
    fn set_selected_restores_explicit_index() {
        let mut g = RadioGroup::new(3);
        g.set_selected(Some(2));
        assert_eq!(g.selected_index(), Some(2));
        assert!(!g.is_selected(0));
        assert!(!g.is_selected(1));
        assert!(g.is_selected(2));
    }

    #[test]
    fn set_selected_none_clears_all() {
        let mut g = RadioGroup::new(3);
        activate(&mut g, 1);
        g.set_selected(None);
        assert_eq!(g.selected_index(), None);
        for i in 0..3 {
            assert!(!g.is_selected(i));
        }
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn set_selected_out_of_range_panics() {
        let mut g = RadioGroup::new(2);
        g.set_selected(Some(5));
    }

    #[test]
    fn external_first_activate_emits_selected_intent_with_index() {
        let mut g = RadioGroupExternal::new(3);
        activate_ext(&mut g, 1);
        assert!(g.is_dirty());
        let mut harvested = Vec::new();
        g.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "selected");
        assert_eq!(harvested[0].payload, IntrospectValue::Int(1));
    }

    #[test]
    fn external_switch_selection_emits_intent_with_new_index() {
        let mut g = RadioGroupExternal::new(3);
        activate_ext(&mut g, 0);
        let mut harvested = Vec::new();
        g.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1, "first activate emits");

        activate_ext(&mut g, 2);
        let mut harvested = Vec::new();
        g.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1, "switch emits");
        assert_eq!(harvested[0].payload, IntrospectValue::Int(2));
    }

    #[test]
    fn external_reactivate_same_index_is_silent_on_intent_channel() {
        let mut g = RadioGroupExternal::new(3);
        activate_ext(&mut g, 1);
        g.drain_intents(&mut |_| {});
        activate_ext(&mut g, 1);
        let mut harvested = Vec::new();
        g.drain_intents(&mut |i| harvested.push(i));
        assert!(
            harvested.is_empty(),
            "re-activate must stay silent on §5.20"
        );
    }

    #[test]
    fn external_query_count_and_selected_index() {
        let mut g = RadioGroupExternal::new(4);
        assert_eq!(g.query("count").unwrap(), IntrospectValue::Int(4));
        assert_eq!(
            g.query("selected_index").unwrap(),
            IntrospectValue::Null
        );
        activate_ext(&mut g, 3);
        assert_eq!(
            g.query("selected_index").unwrap(),
            IntrospectValue::Int(3)
        );
    }

    #[test]
    fn external_query_unknown_path_returns_none() {
        let g = RadioGroupExternal::new(2);
        assert!(g.query("nope").is_none());
    }

    #[test]
    fn external_intervene_selected_index_int() {
        let mut g = RadioGroupExternal::new(3);
        g.intervene("selected_index", IntrospectValue::Int(1))
            .unwrap();
        assert_eq!(g.selected_index(), Some(1));
        assert!(g.is_selected(1));
        // No intent fires on intervene (model-driven set).
        assert!(!g.is_dirty());
    }

    #[test]
    fn external_intervene_selected_index_null_clears() {
        let mut g = RadioGroupExternal::new(3);
        activate_ext(&mut g, 1);
        g.drain_intents(&mut |_| {});
        g.intervene("selected_index", IntrospectValue::Null)
            .unwrap();
        assert_eq!(g.selected_index(), None);
        assert!(!g.is_selected(1));
    }

    #[test]
    fn external_intervene_selected_index_out_of_range_is_type_mismatch() {
        let mut g = RadioGroupExternal::new(2);
        assert_eq!(
            g.intervene("selected_index", IntrospectValue::Int(5)),
            Err(InterveneError::TypeMismatch)
        );
    }

    #[test]
    fn external_intervene_count_is_read_only() {
        let mut g = RadioGroupExternal::new(2);
        assert_eq!(
            g.intervene("count", IntrospectValue::Int(3)),
            Err(InterveneError::ReadOnly)
        );
    }

    #[test]
    fn external_invoke_send_drives_specified_radio() {
        let mut g = RadioGroupExternal::new(3);
        let out = g
            .invoke("send", IntrospectValue::Text("0:PointerEnter".to_string()))
            .unwrap();
        // PointerEnter doesn't activate, so selected_index stays None.
        assert_eq!(out, IntrospectValue::Null);
        assert_eq!(g.state(0), RadioState::Hover);
    }

    #[test]
    fn external_invoke_send_full_activate_returns_new_index() {
        let mut g = RadioGroupExternal::new(3);
        for ev in ["PointerEnter", "PointerDown", "PointerUp"] {
            let out = g
                .invoke(
                    "send",
                    IntrospectValue::Text(format!("2:{ev}")),
                )
                .unwrap();
            // Only the PointerUp activates; the previous two return Null.
            if ev == "PointerUp" {
                assert_eq!(out, IntrospectValue::Int(2));
            }
        }
        assert_eq!(g.selected_index(), Some(2));
    }

    #[test]
    fn external_invoke_send_malformed_args_rejected() {
        let mut g = RadioGroupExternal::new(3);
        // Missing colon.
        assert_eq!(
            g.invoke("send", IntrospectValue::Text("PointerEnter".to_string())),
            Err(InvokeError::Rejected)
        );
        // Index out of range.
        assert_eq!(
            g.invoke("send", IntrospectValue::Text("99:PointerUp".to_string())),
            Err(InvokeError::Rejected)
        );
        // Unknown event name.
        assert_eq!(
            g.invoke("send", IntrospectValue::Text("0:Teleport".to_string())),
            Err(InvokeError::Rejected)
        );
    }

    #[test]
    fn external_schema_declares_five_slots() {
        // R51.43 §5.38 — `state.<index>` + `selected.<index>` join
        // the bare `count` / `selected_index` / `send` triple so AI
        // clients discovering the schema see the per-radio access
        // paths used by `WidgetView` impls.
        let g = RadioGroupExternal::new(3);
        assert_eq!(
            g.schema().fields,
            &[
                ("count", "int"),
                ("selected_index", "int"),
                ("state.<index>", "string"),
                ("selected.<index>", "bool"),
                ("send", "string"),
            ]
        );
    }

    #[test]
    fn external_query_state_per_radio_returns_state_name() {
        // R51.43 §5.38 — `state.<i>` returns the canonical state
        // name string ("Idle" / "Hover" / "Pressed" / "Disabled")
        // matching the single-Radio `query("state")` convention.
        let mut g = RadioGroupExternal::new(3);
        assert_eq!(
            g.query("state.0"),
            Some(IntrospectValue::Text("Idle".to_string())),
        );
        g.send(1, RadioEvent::PointerEnter);
        assert_eq!(
            g.query("state.1"),
            Some(IntrospectValue::Text("Hover".to_string())),
        );
        g.send(1, RadioEvent::PointerDown);
        assert_eq!(
            g.query("state.1"),
            Some(IntrospectValue::Text("Pressed".to_string())),
        );
        g.send(2, RadioEvent::Disable);
        assert_eq!(
            g.query("state.2"),
            Some(IntrospectValue::Text("Disabled".to_string())),
        );
        // Untouched radios stay Idle.
        assert_eq!(
            g.query("state.0"),
            Some(IntrospectValue::Text("Idle".to_string())),
        );
    }

    #[test]
    fn external_query_selected_per_radio_returns_bool() {
        // R51.43 §5.38 — `selected.<i>` returns the selected bit as
        // `IntrospectValue::Bool`, mirroring single-Radio
        // `query("selected")`. After activating radio 1, only index
        // 1 reads as `true`.
        let mut g = RadioGroupExternal::new(3);
        assert_eq!(g.query("selected.0"), Some(IntrospectValue::Bool(false)));
        assert_eq!(g.query("selected.1"), Some(IntrospectValue::Bool(false)));
        // Full activate sequence on radio 1.
        for ev in [
            RadioEvent::PointerEnter,
            RadioEvent::PointerDown,
            RadioEvent::PointerUp,
        ] {
            g.send(1, ev);
        }
        assert_eq!(g.query("selected.0"), Some(IntrospectValue::Bool(false)));
        assert_eq!(g.query("selected.1"), Some(IntrospectValue::Bool(true)));
        assert_eq!(g.query("selected.2"), Some(IntrospectValue::Bool(false)));
    }

    #[test]
    fn external_query_out_of_range_index_is_none() {
        // R51.43 §5.38 — out-of-range index returns `None` (matches
        // unknown-path semantics), not an error variant. AI clients
        // observe the same silent-fall-through that hits unknown
        // top-level paths.
        let g = RadioGroupExternal::new(3);
        assert_eq!(g.query("state.99"), None);
        assert_eq!(g.query("selected.99"), None);
    }

    #[test]
    fn external_query_malformed_per_radio_path_is_none() {
        // Non-numeric suffix → parse fails → `None`. The router
        // does not panic on a malformed AI request.
        let g = RadioGroupExternal::new(3);
        assert_eq!(g.query("state.abc"), None);
        assert_eq!(g.query("selected.-1"), None);
        // Bare prefix with no `.` returns `None` via the top-level
        // unknown-path fall-through.
        assert_eq!(g.query("state"), None);
        assert_eq!(g.query("selected"), None);
    }
}
