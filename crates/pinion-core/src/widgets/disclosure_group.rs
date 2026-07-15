//! R700 §5.38 — `DisclosureGroup` widget: framework-owned
//! single-expand mutual exclusion across N [`Disclosure`] sections
//! (the single-open accordion coordinator). R47-class framework-
//! primitive boundary: like sibling-deselect on a radio group,
//! "collapse the open section when another opens" is framework-owned,
//! not application responsibility. The R697 multi-open accordion
//! composed N independent [`DisclosureExternal`](crate::widgets::disclosure::DisclosureExternal)s precisely because
//! the APG default accordion is multi-open; the single-open variant
//! needs cross-section coordination, so it lands as a coordinator
//! mirroring [`RadioGroup`](crate::widgets::radio_group::RadioGroup).
//!
//! Industry consensus treats single-open accordion exclusion as
//! framework-owned (Material `ExpansionPanel` `accordion` mode, Qt
//! `QToolBox`, `WinUI` `Expander` groups) — pinion matches via this
//! coordinator rather than an application loop calling
//! [`Disclosure::set_expanded`] on the siblings.
//!
//! `DisclosureGroup` owns N [`Disclosure`] instances and provides
//! indexed access: [`state`](DisclosureGroup::state) /
//! [`is_expanded`](DisclosureGroup::is_expanded) /
//! [`send`](DisclosureGroup::send). It enforces "at most one
//! expanded" — when one section expands (collapsed → expanded), every
//! other section is collapsed. The model-driven path
//! [`set_expanded`](DisclosureGroup::set_expanded) is for persisted
//! state restore, defaults, or programmatic open/close.
//!
//! Unlike [`RadioGroup`](crate::widgets::radio_group::RadioGroup), the coordinator carries **no**
//! `focused_index` roving state: WAI-ARIA APG accordion keyboard
//! navigation gives each header its own Tab stop (arrow keys move
//! focus *without* expanding — distinct from radio where arrows
//! activate). That focus model stays in the binding's scene-derived
//! `.with_focusable(true)` marks + `focus_request` plumbing (R697 accordion
//! pattern); the coordinator owns only expand-exclusion.
//!
//! Visual scene placement is the application's responsibility (same
//! contract as [`RadioGroup`](crate::widgets::radio_group::RadioGroup)): the binding composes the N section
//! headers + panels and queries the group for per-section state.
//!
//! The [`DisclosureGroupExternal`] adapter exposes the group on the
//! §5.12 RPC surface:
//!
//! * `query "count"` → [`IntrospectValue::Int`] — N
//! * `query "expanded_index"` → [`IntrospectValue::Int`] or
//!   [`IntrospectValue::Null`]
//! * `query "state.<i>"` / `query "expanded.<i>"` — per-section
//! * `intervene "expanded_index"` (Int or Null) — restore path
//! * `invoke "send" → "<index>:<EventName>"` — drive one section
//!
//! On expand-change transitions, the §5.20 channel emits an
//! `"expanded"` intent carrying the new index as
//! [`IntrospectValue::Int`], or [`IntrospectValue::Null`] when the
//! group collapses to none (reachable by re-activating the open
//! section — distinct from [`RadioGroup`](crate::widgets::radio_group::RadioGroup), where clear is only
//! reachable via `set_selected`).

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaArg, SchemaField,
    ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::disclosure::{Disclosure, DisclosureEvent, DisclosureState};
use crate::widgets::{IntentEmitter, WidgetTransition};
use crate::{WidgetEventName, WidgetStateName};

/// Logical group of N [`Disclosure`] sections with framework-owned
/// single-expand mutual exclusion. See module docs for rationale.
pub struct DisclosureGroup {
    sections: Vec<Disclosure>,
    expanded: Option<usize>,
}

impl DisclosureGroup {
    /// Construct a group with `count` sections, all collapsed. Use
    /// [`set_expanded`](Self::set_expanded) to seed an initial open
    /// section.
    #[must_use]
    pub fn new(count: usize) -> Self {
        Self {
            sections: (0..count).map(|_| Disclosure::new()).collect(),
            expanded: None,
        }
    }

    /// Number of sections in this group.
    #[must_use]
    pub fn count(&self) -> usize {
        self.sections.len()
    }

    /// Drive `event` to the section at `index`. If the event toggles
    /// that section open (collapsed → expanded), every other section
    /// is collapsed and `expanded_index` snaps to `Some(index)`. If
    /// it toggles closed (expanded → collapsed), `expanded_index`
    /// snaps to `None`.
    ///
    /// # Panics
    /// Panics if `index >= count()`.
    pub fn send(&mut self, index: usize, event: DisclosureEvent) {
        let was_expanded = self.sections[index].is_expanded();
        self.sections[index].send(event);
        let now_expanded = self.sections[index].is_expanded();
        if now_expanded && !was_expanded {
            for (j, s) in self.sections.iter_mut().enumerate() {
                if j != index {
                    s.set_expanded(false);
                }
            }
            self.expanded = Some(index);
        } else if !now_expanded && was_expanded {
            self.expanded = None;
        }
    }

    /// Interaction state of the section at `index`.
    ///
    /// # Panics
    /// Panics if `index >= count()`.
    #[must_use]
    pub fn state(&self, index: usize) -> DisclosureState {
        self.sections[index].state()
    }

    /// Whether the section at `index` is expanded.
    ///
    /// # Panics
    /// Panics if `index >= count()`.
    #[must_use]
    pub fn is_expanded(&self, index: usize) -> bool {
        self.sections[index].is_expanded()
    }

    /// Index of the currently expanded section, or `None`.
    #[must_use]
    pub fn expanded_index(&self) -> Option<usize> {
        self.expanded
    }

    /// Restore the group to a specific open section (persisted state
    /// restore, defaults, programmatic open/close). `None` collapses
    /// all; `Some(i)` expands index `i` and collapses all others.
    ///
    /// Semantic axis: this is a **slot-assignment** setter — it
    /// mutates section expansion directly without firing the
    /// `"expanded"` intent. Only the interactive [`send`](Self::send)
    /// path fires the intent through [`WidgetTransition::detect`].
    /// Matches the pinion-wide convention that
    /// [`ExternalIntrospect::intervene`] mutates slots without
    /// commit-class side effects.
    ///
    /// # Panics
    /// Panics if `idx` is `Some(i)` with `i >= count()`.
    pub fn set_expanded(&mut self, idx: Option<usize>) {
        if let Some(i) = idx {
            assert!(
                i < self.sections.len(),
                "DisclosureGroup::set_expanded index {i} out of range (count={})",
                self.sections.len()
            );
        }
        for (j, s) in self.sections.iter_mut().enumerate() {
            s.set_expanded(idx == Some(j));
        }
        self.expanded = idx;
    }
}

impl Default for DisclosureGroup {
    /// Default constructs an empty group (count = 0). Applications
    /// call `DisclosureGroup::new(N)` with a concrete count.
    fn default() -> Self {
        Self::new(0)
    }
}

/// `DisclosureGroup` transition contract (R51.12 substrate). The
/// event pairs the section index with the underlying
/// [`DisclosureEvent`]; the snapshot is the group's expanded-index
/// option; detect emits `"expanded"` whenever the open section moves.
///
/// Transitions that emit:
///
/// * `None → Some(i)` — first open
/// * `Some(a) → Some(b)` where `a != b` — switch
/// * `Some(a) → None` — collapse the open section (reachable via
///   `send` re-activation — distinct from [`RadioGroup`](crate::widgets::radio_group::RadioGroup))
///
/// Transitions that stay silent (idempotent):
///
/// * `Some(a) → Some(a)` — non-toggling event on the open section
/// * `None → None` — non-toggling event while all collapsed
impl WidgetTransition for DisclosureGroup {
    type Event = (usize, DisclosureEvent);
    type Snapshot = Option<usize>;

    fn snapshot(&self) -> Self::Snapshot {
        self.expanded
    }

    fn drive(&mut self, event: Self::Event) {
        let (idx, ev) = event;
        self.send(idx, ev);
    }

    fn detect(before: Self::Snapshot, _event: Self::Event, after: Self::Snapshot) -> Vec<Intent> {
        if before == after {
            return Vec::new();
        }
        let payload = match after {
            Some(idx) => IntrospectValue::Int(
                i64::try_from(idx).expect("DisclosureGroup index must fit in i64"),
            ),
            None => IntrospectValue::Null,
        };
        vec![Intent::new_static("expanded", payload)]
    }
}

/// `External` adapter wrapping a [`DisclosureGroup`]. Surfaces group
/// state to the §5.12 `scene/query` / `scene/rewind` / `scene/invoke`
/// paths and emits an `"expanded"` intent (new index as
/// [`IntrospectValue::Int`], or `Null` on collapse-to-none) on
/// expand-change transitions.
pub struct DisclosureGroupExternal {
    em: IntentEmitter<DisclosureGroup>,
}

impl DisclosureGroupExternal {
    /// Construct with `count` sections, all collapsed.
    #[must_use]
    pub fn new(count: usize) -> Self {
        Self {
            em: IntentEmitter::new(DisclosureGroup::new(count)),
        }
    }

    /// Drive `event` to the section at `index`. Queues an `"expanded"`
    /// intent on expand-change transitions.
    pub fn send(&mut self, index: usize, event: DisclosureEvent) {
        self.em.dispatch((index, event));
    }

    /// Number of sections in the wrapped group.
    #[must_use]
    pub fn count(&self) -> usize {
        self.em.inner.count()
    }

    /// Interaction state of the section at `index`.
    #[must_use]
    pub fn state(&self, index: usize) -> DisclosureState {
        self.em.inner.state(index)
    }

    /// Whether the section at `index` is expanded.
    #[must_use]
    pub fn is_expanded(&self, index: usize) -> bool {
        self.em.inner.is_expanded(index)
    }

    /// Index of the currently expanded section, or `None`.
    #[must_use]
    pub fn expanded_index(&self) -> Option<usize> {
        self.em.inner.expanded_index()
    }

    /// Shared validation for `expanded_index` intervene. Negative
    /// `i`, `i` overflowing `usize`, and `idx >= count` all map to
    /// [`InterveneError::OutOfRange`] (variant mismatch is reserved
    /// for `Value` shape errors, per R51.91).
    fn resolve_index_intervene(&self, i: i64) -> Result<usize, InterveneError> {
        crate::widgets::wire::resolve_index(i, self.count())
    }
}

impl Default for DisclosureGroupExternal {
    /// Default constructs an empty wrapping group (count = 0).
    fn default() -> Self {
        Self::new(0)
    }
}

impl core::fmt::Debug for DisclosureGroupExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DisclosureGroupExternal")
            .field("count", &self.count())
            .field("expanded_index", &self.expanded_index())
            .finish()
    }
}

impl External for DisclosureGroupExternal {
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

impl ExternalIntrospect for DisclosureGroupExternal {
    fn schema(&self) -> IntrospectSchema {
        // Per-section paths advertise their `<index>` placeholder the
        // same way `send` documents its `"<index>:<EventName>"` wire
        // format — discovery metadata for AI clients (`scene/schema`
        // RPC), not a static enumeration of concrete paths.
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("count", "int"),
                    SchemaField::new("expanded_index", "int"),
                    SchemaField::parametric(
                        "state.<index>",
                        "string",
                        const { &[SchemaArg::index("index", "count")] },
                    ),
                    SchemaField::parametric(
                        "expanded.<index>",
                        "bool",
                        const { &[SchemaArg::index("index", "count")] },
                    ),
                    SchemaField::new("send", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "count" => Some(IntrospectValue::Int(
                i64::try_from(self.count()).expect("DisclosureGroup count must fit in i64"),
            )),
            "expanded_index" => Some(match self.expanded_index() {
                Some(idx) => IntrospectValue::Int(i64::try_from(idx).expect("index fits in i64")),
                None => IntrospectValue::Null,
            }),
            _ => {
                // Per-section query paths: `state.<i>` (interaction
                // state name, mirrors `Disclosure::query("state")`)
                // and `expanded.<i>` (boolean expanded bit). Out-of-
                // range indices and malformed suffixes return `None`.
                if let Some(idx_str) = path.strip_prefix("state.") {
                    let idx: usize = idx_str.parse().ok()?;
                    if idx >= self.count() {
                        return None;
                    }
                    return Some(IntrospectValue::Text(self.state(idx).as_name().to_string()));
                }
                if let Some(idx_str) = path.strip_prefix("expanded.") {
                    let idx: usize = idx_str.parse().ok()?;
                    if idx >= self.count() {
                        return None;
                    }
                    return Some(IntrospectValue::Bool(self.is_expanded(idx)));
                }
                None
            }
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "count" => Err(InterveneError::ReadOnly),
            "expanded_index" => match value {
                IntrospectValue::Int(i) => {
                    let idx = self.resolve_index_intervene(i)?;
                    self.em.inner.set_expanded(Some(idx));
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.em.inner.set_expanded(None);
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
            // drives a PointerUp on the section at index 2. Shares the
            // R660 `parse_send_payload` composite substrate with
            // RadioGroup / ListBox / application consumers.
            "send" => match args {
                IntrospectValue::Text(ref s) => {
                    // R781 — modifiers ignored (no modifier-aware activation).
                    let (idx, event_name, _): (usize, &str, _) =
                        crate::composite_tag::parse_send_payload(s).ok_or(InvokeError::Rejected)?;
                    if idx >= self.count() {
                        return Err(InvokeError::Rejected);
                    }
                    let ev = DisclosureEvent::from_name(event_name).ok_or(InvokeError::Rejected)?;
                    self.send(idx, ev);
                    Ok(match self.expanded_index() {
                        Some(i) => {
                            IntrospectValue::Int(i64::try_from(i).expect("index fits in i64"))
                        }
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
    //! R700 §5.38 — single-expand mutual exclusion, model-driven
    //! restore, transition intent emission, and the §5.12 RPC surface
    //! (query / intervene / invoke) of [`DisclosureGroupExternal`].

    use super::*;

    /// Drive the full pointer click cycle (Enter -> Down -> Up) on
    /// section `i` — the sequence the `InputRouter` produces for a
    /// click, which toggles the Disclosure's expanded state.
    fn click(g: &mut DisclosureGroup, i: usize) {
        g.send(i, DisclosureEvent::PointerEnter);
        g.send(i, DisclosureEvent::PointerDown);
        g.send(i, DisclosureEvent::PointerUp);
    }

    #[test]
    fn new_group_all_collapsed() {
        let g = DisclosureGroup::new(3);
        assert_eq!(g.count(), 3);
        assert_eq!(g.expanded_index(), None);
        for i in 0..3 {
            assert!(!g.is_expanded(i));
            assert_eq!(g.state(i), DisclosureState::Idle);
        }
    }

    #[test]
    fn activate_expands_section() {
        let mut g = DisclosureGroup::new(3);
        click(&mut g, 1);
        assert_eq!(g.expanded_index(), Some(1));
        assert!(g.is_expanded(1));
        assert!(!g.is_expanded(0));
        assert!(!g.is_expanded(2));
    }

    #[test]
    fn single_open_collapses_previous() {
        let mut g = DisclosureGroup::new(3);
        click(&mut g, 0);
        assert_eq!(g.expanded_index(), Some(0));
        // Opening section 2 must collapse section 0 (mutual exclusion).
        click(&mut g, 2);
        assert_eq!(g.expanded_index(), Some(2));
        assert!(!g.is_expanded(0), "section 0 collapsed when 2 opened");
        assert!(g.is_expanded(2));
    }

    #[test]
    fn reactivate_collapses_to_none() {
        let mut g = DisclosureGroup::new(2);
        click(&mut g, 0);
        assert_eq!(g.expanded_index(), Some(0));
        // Re-activating the open section collapses it -> none open.
        click(&mut g, 0);
        assert_eq!(g.expanded_index(), None);
        assert!(!g.is_expanded(0));
    }

    #[test]
    fn set_expanded_model_driven_collapses_others() {
        let mut g = DisclosureGroup::new(3);
        g.set_expanded(Some(2));
        assert_eq!(g.expanded_index(), Some(2));
        assert!(g.is_expanded(2));
        assert!(!g.is_expanded(0) && !g.is_expanded(1));
        // Switch via model path.
        g.set_expanded(Some(0));
        assert_eq!(g.expanded_index(), Some(0));
        assert!(!g.is_expanded(2));
        // Clear all.
        g.set_expanded(None);
        assert_eq!(g.expanded_index(), None);
        for i in 0..3 {
            assert!(!g.is_expanded(i));
        }
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn set_expanded_out_of_range_panics() {
        let mut g = DisclosureGroup::new(2);
        g.set_expanded(Some(5));
    }

    // ── transition intent emission (via External drain) ──────────────

    fn click_ext(ext: &mut DisclosureGroupExternal, i: usize) {
        ext.send(i, DisclosureEvent::PointerEnter);
        ext.send(i, DisclosureEvent::PointerDown);
        ext.send(i, DisclosureEvent::PointerUp);
    }

    #[test]
    fn emits_expanded_intent_on_open_switch_and_close() {
        let mut ext = DisclosureGroupExternal::new(3);
        // First open: None -> Some(1) emits expanded=Int(1).
        click_ext(&mut ext, 1);
        let mut harvested = Vec::new();
        ext.drain_intents(&mut |i| harvested.push(i));
        let expanded: Vec<_> = harvested
            .iter()
            .filter(|i| i.tag_str() == "expanded")
            .collect();
        assert_eq!(expanded.len(), 1, "first open emits one expanded intent");
        assert_eq!(expanded[0].payload, IntrospectValue::Int(1));

        // Switch: Some(1) -> Some(0) emits expanded=Int(0).
        click_ext(&mut ext, 0);
        let mut harvested = Vec::new();
        ext.drain_intents(&mut |i| harvested.push(i));
        let expanded: Vec<_> = harvested
            .iter()
            .filter(|i| i.tag_str() == "expanded")
            .collect();
        assert_eq!(expanded.len(), 1, "switch emits one expanded intent");
        assert_eq!(expanded[0].payload, IntrospectValue::Int(0));

        // Close: Some(0) -> None emits expanded=Null (distinct from
        // RadioGroup, where clear is unreachable via send).
        click_ext(&mut ext, 0);
        let mut harvested = Vec::new();
        ext.drain_intents(&mut |i| harvested.push(i));
        let expanded: Vec<_> = harvested
            .iter()
            .filter(|i| i.tag_str() == "expanded")
            .collect();
        assert_eq!(expanded.len(), 1, "close emits one expanded intent");
        assert_eq!(expanded[0].payload, IntrospectValue::Null);
    }

    // ── §5.12 RPC surface ────────────────────────────────────────────

    #[test]
    fn query_count_expanded_index_and_per_section() {
        let mut ext = DisclosureGroupExternal::new(2);
        assert_eq!(ext.query("count"), Some(IntrospectValue::Int(2)));
        assert_eq!(ext.query("expanded_index"), Some(IntrospectValue::Null));
        assert_eq!(
            ext.query("state.0"),
            Some(IntrospectValue::Text("Idle".to_string()))
        );
        assert_eq!(ext.query("expanded.1"), Some(IntrospectValue::Bool(false)));
        click_ext(&mut ext, 1);
        assert_eq!(ext.query("expanded_index"), Some(IntrospectValue::Int(1)));
        assert_eq!(ext.query("expanded.1"), Some(IntrospectValue::Bool(true)));
        // Out-of-range per-section query -> None.
        assert_eq!(ext.query("expanded.9"), None);
        assert_eq!(ext.query("state.9"), None);
        assert_eq!(ext.query("bogus"), None);
    }

    #[test]
    fn intervene_expanded_index_int_null_and_out_of_range() {
        let mut ext = DisclosureGroupExternal::new(3);
        assert!(
            ext.intervene("expanded_index", IntrospectValue::Int(2))
                .is_ok()
        );
        assert_eq!(ext.expanded_index(), Some(2));
        assert!(ext.is_expanded(2) && !ext.is_expanded(0));
        // Null clears.
        assert!(
            ext.intervene("expanded_index", IntrospectValue::Null)
                .is_ok()
        );
        assert_eq!(ext.expanded_index(), None);
        // Out-of-range + negative -> OutOfRange.
        assert!(matches!(
            ext.intervene("expanded_index", IntrospectValue::Int(9)),
            Err(InterveneError::OutOfRange)
        ));
        assert!(matches!(
            ext.intervene("expanded_index", IntrospectValue::Int(-1)),
            Err(InterveneError::OutOfRange)
        ));
        // Wrong value shape -> TypeMismatch; count read-only.
        assert!(matches!(
            ext.intervene("expanded_index", IntrospectValue::Bool(true)),
            Err(InterveneError::TypeMismatch)
        ));
        assert!(matches!(
            ext.intervene("count", IntrospectValue::Int(1)),
            Err(InterveneError::ReadOnly)
        ));
        assert!(matches!(
            ext.intervene("bogus", IntrospectValue::Int(0)),
            Err(InterveneError::UnknownPath)
        ));
        // intervene must not fire an "expanded" intent (slot-assign).
        let mut harvested = Vec::new();
        ext.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.iter().all(|i| i.tag_str() != "expanded"));
    }

    #[test]
    fn invoke_send_wire_format_and_rejections() {
        let mut ext = DisclosureGroupExternal::new(2);
        // "<idx>:<EventName>" cycle opens section 0; returns the new
        // expanded index.
        assert!(
            ext.invoke("send", IntrospectValue::Text("0:PointerEnter".into()))
                .is_ok()
        );
        assert!(
            ext.invoke("send", IntrospectValue::Text("0:PointerDown".into()))
                .is_ok()
        );
        assert_eq!(
            ext.invoke("send", IntrospectValue::Text("0:PointerUp".into())),
            Ok(IntrospectValue::Int(0))
        );
        assert_eq!(ext.expanded_index(), Some(0));
        // Out-of-range index rejected.
        assert!(matches!(
            ext.invoke("send", IntrospectValue::Text("9:PointerUp".into())),
            Err(InvokeError::Rejected)
        ));
        // Unknown / internal event names rejected (DisclosureActivate
        // is an internal raise; Null is the SCXML sentinel).
        assert!(matches!(
            ext.invoke("send", IntrospectValue::Text("0:Bogus".into())),
            Err(InvokeError::Rejected)
        ));
        assert!(matches!(
            ext.invoke("send", IntrospectValue::Text("0:DisclosureActivate".into())),
            Err(InvokeError::Rejected)
        ));
        // Malformed payload (no ':') rejected.
        assert!(matches!(
            ext.invoke("send", IntrospectValue::Text("PointerUp".into())),
            Err(InvokeError::Rejected)
        ));
        // Wrong arg type / unknown path.
        assert!(matches!(
            ext.invoke("send", IntrospectValue::Int(0)),
            Err(InvokeError::TypeMismatch)
        ));
        assert!(matches!(
            ext.invoke("bogus", IntrospectValue::Text("x".into())),
            Err(InvokeError::UnknownPath)
        ));
    }

    #[test]
    fn invoke_send_enforces_single_open() {
        let mut ext = DisclosureGroupExternal::new(3);
        for ev in ["0:PointerEnter", "0:PointerDown", "0:PointerUp"] {
            ext.invoke("send", IntrospectValue::Text(ev.into()))
                .unwrap();
        }
        assert_eq!(ext.expanded_index(), Some(0));
        for ev in ["2:PointerEnter", "2:PointerDown", "2:PointerUp"] {
            ext.invoke("send", IntrospectValue::Text(ev.into()))
                .unwrap();
        }
        assert_eq!(ext.expanded_index(), Some(2));
        assert!(!ext.is_expanded(0), "single-open enforced through RPC send");
    }

    #[test]
    fn schema_advertises_paths() {
        let ext = DisclosureGroupExternal::new(1);
        let schema = ext.schema();
        let names: Vec<&str> = schema.fields.iter().map(|f| f.path).collect();
        for expected in [
            "count",
            "expanded_index",
            "state.<index>",
            "expanded.<index>",
            "send",
        ] {
            assert!(names.contains(&expected), "schema missing {expected}");
        }
    }
}
