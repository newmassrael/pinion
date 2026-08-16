//! R51.15 §5.38 — `RadioGroup` widget: framework-owned mutual
//! exclusion across N Radio widgets. R47-class framework-primitive
//! boundary fix — before R51.15, the "deselect siblings on
//! activate" rule was application-side responsibility (each app
//! had to call [`crate::widgets::radio::Radio::set_selected`] on
//! the other Radios after one was activated). Industry consensus
//! (HTML `<input type="radio" name="...">`, Material `RadioGroup`,
//! `SwiftUI` `Picker`, the toolkit button group) treats mutual exclusion as
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

use crate::WidgetStateName;
use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner, SchemaArg,
    SchemaField, ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::radio::{Radio, RadioEvent, RadioState};
use crate::widgets::selection;
use crate::widgets::{IntentEmitter, WidgetTransition};

/// Logical group of N Radio widgets with framework-owned mutual
/// exclusion. See module docs for the full design rationale.
///
/// R51.87 §5.40 — carries a separate `focused_index` alongside
/// `selected` to model the WAI-ARIA roving-tabindex distinction
/// between "the addressed (active) descendant" and "the user's
/// choice". The two indices conflate naturally for radio groups
/// because arrow keys activate immediately (ARIA Authoring
/// Practices radio-group convention — distinct from listbox /
/// menu / tab where roving moves focus without activating). The
/// split lets AT clients issue `Focus` actions to navigate
/// between rows without committing a selection, and lets the
/// `access_focus_target` `active_descendant` honour the AT-side
/// addressed row instead of always reporting the selected one.
pub struct RadioGroup {
    radios: Vec<Radio>,
    selected: Option<usize>,
    /// R51.87 §5.40 — AT-side active descendant. Independent of
    /// `selected`; mutated by AT `Focus` actions or programmatic
    /// `set_focused_index`. `None` falls back to `selected`-or-0
    /// at the application's `access_focus_target` resolution.
    focused: Option<usize>,
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
            focused: None,
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
        // R735.1 §5.38 — single-select sibling-deselect is the shared
        // 4-consumer substrate; on a fresh activation it clears every
        // other radio and reports the gain so the cursor + focus sync.
        if selection::select_exclusive(&mut self.radios, index, was_selected) {
            self.selected = Some(index);
            // R51.90 §5.40 — WAI-ARIA Authoring Practices roving-
            // tabindex: activation moves focus. When a Radio's
            // selection state flips `false → true` (the activation
            // edge), the active descendant follows. This applies to
            // every activate path that funnels through `send`
            // (`apply_key` arrow / Home / End / letter keys, AT
            // `Click`/`Default` actions). AT-only `Focus` actions on
            // a sibling reach the group through `set_focused_index`
            // (no `send`) so user-initiated AT navigation can still
            // diverge `focused_index` from `selected_index` until the
            // next activation re-syncs them.
            self.focused = Some(index);
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
    /// Semantic axis (R51.92.2 §5.40 evaluation): this is a
    /// **slot-assignment** setter — it mutates `self.selected`
    /// directly without firing the `"selected"` intent and without
    /// touching `self.focused`. Only the interactive [`send`](Self::send)
    /// path (`PointerUp` activation) fires the intent through
    /// [`WidgetTransition::detect`] and syncs focused per R51.90.
    /// This matches the pinion-wide convention that
    /// [`ExternalIntrospect::intervene`] mutates slots without
    /// commit-class side effects; the RPC `intervene "selected_index"`
    /// route is the AI-client surface and lands here directly.
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

    /// R51.87 §5.40 — AT-side active descendant index.
    ///
    /// The WAI-ARIA roving-tabindex active descendant. `None` until
    /// any of three paths first lands a value:
    ///
    /// * Activation through [`send`](Self::send) (R51.90 §5.40) —
    ///   the activation edge syncs `focused_index` with the new
    ///   `selected_index`.
    /// * AT-side `Focus` action on a specific child reaching the
    ///   widget through [`set_focused_index`](Self::set_focused_index)
    ///   (typically routed by the application's
    ///   `access_child_invoke` hook).
    /// * Programmatic [`set_focused_index`](Self::set_focused_index)
    ///   for tests / persisted-AT-state restore.
    ///
    /// The application's `access_focus_target` falls back to
    /// `selected_index()`-or-0 in the `None` case.
    #[must_use]
    pub fn focused_index(&self) -> Option<usize> {
        self.focused
    }

    /// R51.87 §5.40 — set the AT-side active descendant.
    ///
    /// Independent of `selected_index` — calling this neither
    /// activates the row nor deselects siblings; it only marks the
    /// addressed item so AT clients (and the application's
    /// `access_focus_target`) can report it as the active
    /// descendant of the focused parent.
    ///
    /// R51.90 §5.40 — activation through [`send`](Self::send) keeps
    /// `focused_index` in sync with `selected_index` on the
    /// activation edge; this setter is the AT-only divergence path
    /// for navigating without committing a selection (arrow keys
    /// without Enter, AT-side `Focus` actions).
    ///
    /// # Panics
    /// Panics if `idx` is `Some(i)` with `i >= count()`.
    pub fn set_focused_index(&mut self, idx: Option<usize>) {
        if let Some(i) = idx {
            assert!(
                i < self.radios.len(),
                "RadioGroup::set_focused_index index {i} out of range (count={})",
                self.radios.len()
            );
        }
        self.focused = idx;
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

    fn detect(before: Self::Snapshot, _event: Self::Event, after: Self::Snapshot) -> Vec<Intent> {
        if before != after {
            if let Some(idx) = after {
                return vec![Intent::new_static(
                    "selected",
                    IntrospectValue::Int(
                        i64::try_from(idx).expect("RadioGroup index must fit in i64"),
                    ),
                )];
            }
        }
        Vec::new()
    }
}

/// `External` adapter wrapping a [`RadioGroup`]. Surfaces group
/// state to the §5.12 `scene/query` / `scene/rewind` / `scene/invoke`
/// paths and emits a `"selected"` intent (with the new index as
/// [`IntrospectValue::Int`] payload) on selection-change
/// transitions.
pub struct RadioGroupExternal {
    em: IntentEmitter<RadioGroup>,
    /// R1703 §5.45 — the wheel's whole-notch accumulator; see
    /// [`WheelSteps`](crate::widgets::wheel::WheelSteps).
    wheel: crate::widgets::wheel::WheelSteps,
    /// R1703 §5.45 — whether a wheel over this group moves its selection.
    ///
    /// `false` by default, and the asymmetry with the listbox is the point: a
    /// **tab strip** is a strip of destinations a wheel walks (the reference's
    /// tab bar does exactly that — measured: a wheel down at tab 0 lands on tab
    /// 1, up returns to 0), whereas a **radio group in a form** is a set of
    /// mutually exclusive answers, and a wheel that changed the answer while a
    /// person scrolled past it would be the reference's combo-box hazard in a
    /// second place. One type serves both roles here, so the role is declared
    /// by the binding rather than guessed from the shape.
    takes_the_wheel: bool,
}

impl RadioGroupExternal {
    /// Construct with `count` Radios, all unselected.
    #[must_use]
    pub fn new(count: usize) -> Self {
        Self {
            em: IntentEmitter::new(RadioGroup::new(count)),
            wheel: crate::widgets::wheel::WheelSteps::new(),
            takes_the_wheel: false,
        }
    }

    /// R1703 §5.45 — declare that a wheel over this group walks it, which is
    /// what a **tab strip** wants and what a form's radio set must not have.
    /// See the [`takes_the_wheel`](Self#structfield.takes_the_wheel) field for
    /// why the default is the other way.
    #[must_use]
    pub fn with_wheel(mut self, takes: bool) -> Self {
        self.takes_the_wheel = takes;
        self
    }

    /// R1703 §5.45 — move the selection by `steps`, clamped and **not
    /// wrapping**; returns whether it moved.
    ///
    /// Not wrapping is what makes the decline below meaningful, and it is the
    /// reference's own tab-bar rule: at the last tab a further wheel is not the
    /// strip's, and belongs to whatever scrolls behind it. The keyboard still
    /// wraps (`ArrowRight` at the end returns to the start, the WAI-ARIA tab
    /// pattern) because a key press is a request for "the next one" and a wheel
    /// is a continuous push.
    pub fn wheel_steps(&mut self, steps: i32) -> bool {
        let count = self.count();
        if count == 0 || steps == 0 {
            return false;
        }
        let from = self.selected_index();
        let target = match from {
            None => {
                if steps > 0 {
                    0
                } else {
                    count - 1
                }
            }
            Some(at) => {
                let moved = i64::try_from(at).unwrap_or(0) + i64::from(steps);
                let last = i64::try_from(count - 1).unwrap_or(0);
                usize::try_from(moved.clamp(0, last)).unwrap_or(0)
            }
        };
        if from == Some(target) {
            return false;
        }
        self.em.inner.set_selected(Some(target));
        self.em.inner.set_focused_index(Some(target));
        true
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

    /// R51.91 §5.40 — shared validation for `selected_index` /
    /// `focused_index` intervene. Negative `i`, `i` overflowing
    /// `usize`, and `idx >= count` all map to
    /// [`InterveneError::OutOfRange`] (R51.91 replaces the prior
    /// `TypeMismatch` misuse — variant mismatch is reserved for
    /// `Value` shape errors).
    fn resolve_index_intervene(&self, i: i64) -> Result<usize, InterveneError> {
        crate::widgets::wire::resolve_index("button", i, self.count())
    }

    /// R51.87 §5.40 — AT-side active descendant index, or `None`.
    /// See [`RadioGroup::focused_index`].
    #[must_use]
    pub fn focused_index(&self) -> Option<usize> {
        self.em.inner.focused_index()
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
            .field("focused_index", &self.focused_index())
            .finish()
    }
}

impl External for RadioGroupExternal {
    /// R1703 §5.45 — a wheel walks the group when the binding declared that it
    /// should (a tab strip), by whole items, clamped at the ends.
    fn wheel(&mut self, reading: &crate::widgets::wheel::WheelReading) -> bool {
        let notches = self.wheel.feed(reading);
        if notches == 0 {
            return true;
        }
        if self.wheel_steps(notches) {
            true
        } else {
            self.wheel.reset();
            false
        }
    }

    fn wheel_intent(&self, _at: (f32, f32)) -> Option<crate::widgets::wheel::WheelIntent> {
        self.takes_the_wheel
            .then_some(crate::widgets::wheel::WheelIntent::Step(
                crate::widgets::wheel::StepUnit::Item,
            ))
    }

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
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("count", "int"),
                    SchemaField::new("selected_index", "int"),
                    SchemaField::new("focused_index", "int"),
                    SchemaField::parametric(
                        "state.<index>",
                        "string",
                        const { &[SchemaArg::index("index", "count")] },
                    ),
                    SchemaField::parametric(
                        "selected.<index>",
                        "bool",
                        const { &[SchemaArg::index("index", "count")] },
                    ),
                    SchemaField::send("string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        match path {
            "count" => Ok(IntrospectValue::Int(
                i64::try_from(self.count()).expect("RadioGroup count must fit in i64"),
            )),
            "selected_index" => Ok(match self.selected_index() {
                Some(idx) => IntrospectValue::Int(i64::try_from(idx).expect("index fits in i64")),
                None => IntrospectValue::Null,
            }),
            // R51.87 §5.40 — AT-side active descendant. `Null` until
            // a `Focus` action or `intervene "focused_index" = Int(i)`
            // sets it.
            "focused_index" => Ok(match self.focused_index() {
                Some(idx) => IntrospectValue::Int(i64::try_from(idx).expect("index fits in i64")),
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
                    let idx: usize = idx_str
                        .parse()
                        .map_err(|_| ReadRefusal::QueryTypeMismatch)?;
                    if idx >= self.count() {
                        return Err(ReadRefusal::no_such_member(format!(
                            "radio {idx} is outside 0..{}",
                            self.count()
                        )));
                    }
                    return Ok(IntrospectValue::Text(self.state(idx).as_name().to_string()));
                }
                if let Some(idx_str) = path.strip_prefix("selected.") {
                    let idx: usize = idx_str
                        .parse()
                        .map_err(|_| ReadRefusal::QueryTypeMismatch)?;
                    if idx >= self.count() {
                        return Err(ReadRefusal::no_such_member(format!(
                            "radio {idx} is outside 0..{}",
                            self.count()
                        )));
                    }
                    return Ok(IntrospectValue::Bool(self.is_selected(idx)));
                }
                Err(ReadRefusal::UnknownPath)
            }
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
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
            // R51.87 §5.40 — AT-side active descendant intervene.
            // Mutates only `focused_index`; siblings stay
            // unchanged, no selection commit, no `"selected"` intent.
            // AT `Focus` actions on a specific radio reach this path
            // through the application's `access_child_invoke` hook.
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
            // drives a PointerUp on the Radio at index 2. R660 §5.16
            // routes the parse through [[parse_send_payload]] so the
            // framework composite shares the 5-of-5 substrate with the
            // application consumers (TodoDelete / Toggle / Filter) and
            // ListBoxExternal — the inline `split_once(':')` carry from
            // R51.43 is now repaid.
            "send" => match args {
                IntrospectValue::Text(ref s) => {
                    // R781 — modifiers ignored (no modifier-aware activation).
                    let (
                        idx,
                        crate::composite_tag::SendPayload {
                            event: event_name, ..
                        },
                    ): (usize, _) =
                        crate::composite_tag::require_parsed_send_payload("radio_group.send", s)?;
                    if idx >= self.count() {
                        return Err(InvokeError::rejected(format!(
                            "radio_group.send: no button {idx} in this group (it has {})",
                            self.count()
                        )));
                    }
                    let ev =
                        crate::widget_core::require_event::<RadioEvent>("radio_group", event_name)?;
                    self.send(idx, ev);
                    Ok(match self.selected_index() {
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
    use super::*;
    use crate::test_fixtures::assert_out_of_range_saying;
    use crate::test_fixtures::assert_refused_saying;

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

    // ─────────────────────────────────────────────────────────────────
    // R1703 §5.45 — the wheel a TAB BAR has on the reference toolkit (measured
    // by probe: a wheel down at tab 0 lands on tab 1, up returns to 0) and a
    // form's radio set must not have.
    // ─────────────────────────────────────────────────────────────────

    const NOTCH: f32 = crate::event::LINE_HEIGHT_PX;

    fn wheel(g: &mut RadioGroupExternal, dy: f32) -> bool {
        External::wheel(
            g,
            &crate::widgets::wheel::WheelReading::new(
                (0.5, 0.5),
                (0.0, dy),
                crate::GesturePhase::Update,
                crate::input::Modifiers::empty(),
            ),
        )
    }

    #[test]
    fn r1703_a_tab_strip_walks_under_the_wheel() {
        let mut g = RadioGroupExternal::new(3).with_wheel(true);
        g.em.inner.set_selected(Some(0));
        assert!(wheel(&mut g, NOTCH));
        assert_eq!(
            g.selected_index(),
            Some(1),
            "a notch down took the next tab"
        );
        assert!(wheel(&mut g, -NOTCH));
        assert_eq!(g.selected_index(), Some(0), "and up came back");
    }

    #[test]
    fn r1703_a_forms_radio_set_does_not_answer_a_wheel_at_all() {
        // ★★ The default, and the reason it differs from the listbox: a set of
        // mutually exclusive ANSWERS must not change because a person scrolled
        // the page it sits on. The reference has no way to say this — 75
        // properties and 39 methods on its tab bar, none about the wheel — so
        // there the two roles share one behaviour and one of them is wrong.
        let g = RadioGroupExternal::new(3);
        assert!(
            External::wheel_intent(&g, (0.5, 0.5)).is_none(),
            "a radio group takes no wheel unless its binding says it is a strip"
        );
    }

    #[test]
    fn r1703_the_last_tab_gives_the_wheel_back() {
        let mut g = RadioGroupExternal::new(3).with_wheel(true);
        g.em.inner.set_selected(Some(2));
        assert!(
            !wheel(&mut g, NOTCH),
            "past the last tab the wheel is the page's"
        );
        assert_eq!(g.selected_index(), Some(2), "and nothing wrapped");
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
        assert_eq!(g.query("selected_index").unwrap(), IntrospectValue::Null);
        activate_ext(&mut g, 3);
        assert_eq!(g.query("selected_index").unwrap(), IntrospectValue::Int(3));
    }

    #[test]
    fn external_query_unknown_path_returns_none() {
        let g = RadioGroupExternal::new(2);
        assert!(g.query("nope").is_err());
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
    fn external_intervene_selected_index_out_of_range_is_out_of_range() {
        // R51.91 §5.40 — `Int(5)` is the correct Value variant; the
        // failure is the index, not the variant. Pre-R51.91 this
        // returned `TypeMismatch` (framework-table-stakes carry).
        let mut g = RadioGroupExternal::new(2);
        assert_out_of_range_saying(
            &g.intervene("selected_index", IntrospectValue::Int(5)),
            "no button 5 here (it has 2, so 0..2)",
        );
    }

    // R51.91 §5.40 — InterveneError taxonomy regression. Verify
    // OutOfRange covers value-domain failures while TypeMismatch
    // stays scoped to wrong-variant failures.

    #[test]
    fn r51_91_selected_index_negative_int_is_out_of_range() {
        let mut g = RadioGroupExternal::new(3);
        assert_out_of_range_saying(
            &g.intervene("selected_index", IntrospectValue::Int(-1)),
            "-1 is not a button index",
        );
    }

    #[test]
    fn r51_91_selected_index_wrong_variant_is_type_mismatch() {
        // String at an Int slot — variant-level error, not value-
        // domain error. Stays TypeMismatch (semantic axis preserved).
        let mut g = RadioGroupExternal::new(3);
        assert_eq!(
            g.intervene("selected_index", IntrospectValue::Text("zero".to_string())),
            Err(InterveneError::TypeMismatch)
        );
        assert_eq!(
            g.intervene("selected_index", IntrospectValue::Bool(true)),
            Err(InterveneError::TypeMismatch)
        );
    }

    #[test]
    fn r51_91_focused_index_wrong_variant_is_type_mismatch() {
        let mut g = RadioGroupExternal::new(3);
        assert_eq!(
            g.intervene("focused_index", IntrospectValue::Float(1.0)),
            Err(InterveneError::TypeMismatch)
        );
    }

    #[test]
    fn r51_91_selected_index_at_boundary_is_accepted() {
        // `idx == count - 1` is in range; `idx == count` is OutOfRange.
        let mut g = RadioGroupExternal::new(3);
        g.intervene("selected_index", IntrospectValue::Int(2))
            .unwrap();
        assert_eq!(g.selected_index(), Some(2));
        assert_out_of_range_saying(
            &g.intervene("selected_index", IntrospectValue::Int(3)),
            "no button 3 here (it has 3, so 0..3)",
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

    // R51.93 §5.35 — composite cancel-propagation test. RadioGroup
    // forwards `idx:PointerCancel` wire events to the indexed Radio,
    // whose template-driven SCXML routes `Pressed → Idle` without
    // raising `radio.activate`. Verify that touch cancel mid-press
    // does NOT select the row, does NOT change `selected_index`, and
    // does NOT fire a `"selected"` intent through the composite's
    // WidgetTransition path.

    #[test]
    fn r51_93_composite_pointer_cancel_does_not_select_row() {
        let mut g = RadioGroupExternal::new(3);
        let _ = g
            .invoke("send", IntrospectValue::Text("1:PointerEnter".to_string()))
            .unwrap();
        let _ = g
            .invoke("send", IntrospectValue::Text("1:PointerDown".to_string()))
            .unwrap();
        assert_eq!(g.state(1), RadioState::Pressed);
        let before_selected = g.selected_index();
        let before_focused = g.focused_index();
        // Touch revoked mid-press (4-finger gesture / phone call / ...).
        let _ = g
            .invoke("send", IntrospectValue::Text("1:PointerCancel".to_string()))
            .unwrap();
        assert_eq!(g.state(1), RadioState::Idle, "Pressed→Idle on cancel");
        assert_eq!(
            g.selected_index(),
            before_selected,
            "PointerCancel must not commit selection"
        );
        assert_eq!(
            g.focused_index(),
            before_focused,
            "PointerCancel must not trigger R51.90 focused sync"
        );
        // No `"selected"` intent in the drain.
        let mut harvested = Vec::new();
        g.drain_intents(&mut |i| harvested.push(i));
        assert!(
            harvested.iter().all(|i| i.tag_str() != "selected"),
            "PointerCancel must not fire `selected` intent through the group"
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
                .invoke("send", IntrospectValue::Text(format!("2:{ev}")))
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
        assert_refused_saying(
            &g.invoke("send", IntrospectValue::Text("PointerEnter".to_string())),
            "malformed send payload \"PointerEnter\"",
        );
        // Index out of range.
        assert_refused_saying(
            &g.invoke("send", IntrospectValue::Text("99:PointerUp".to_string())),
            "no button 99 in this group (it has 3)",
        );
        // Unknown event name.
        assert_refused_saying(
            &g.invoke("send", IntrospectValue::Text("0:Teleport".to_string())),
            "\"Teleport\" is not an event this widget accepts",
        );
    }

    #[test]
    fn external_schema_declares_six_slots() {
        // R51.43 §5.38 — `state.<index>` + `selected.<index>` join
        // the bare `count` / `selected_index` / `send` triple so AI
        // clients discovering the schema see the per-radio access
        // paths used by `WidgetView` impls.
        //
        // R51.87 §5.40 — `focused_index` joins the schema so AT
        // clients (and the application's `access_focus_target`) can
        // read / write the active descendant independently of
        // `selected_index`.
        let g = RadioGroupExternal::new(3);
        assert_eq!(
            g.schema().fields,
            &[
                SchemaField::new("count", "int"),
                SchemaField::new("selected_index", "int"),
                SchemaField::new("focused_index", "int"),
                SchemaField::parametric(
                    "state.<index>",
                    "string",
                    const { &[SchemaArg::index("index", "count")] }
                ),
                SchemaField::parametric(
                    "selected.<index>",
                    "bool",
                    const { &[SchemaArg::index("index", "count")] }
                ),
                SchemaField::send("string"),
            ]
        );
    }

    // R51.87 §5.40 — focused_index regression tests.

    #[test]
    fn r51_87_new_group_has_no_focused_index() {
        let g = RadioGroup::new(3);
        assert_eq!(g.focused_index(), None);
        assert_eq!(g.selected_index(), None);
    }

    #[test]
    fn r51_87_set_focused_index_independent_of_selected() {
        let mut g = RadioGroup::new(3);
        g.set_focused_index(Some(2));
        // Setting the active descendant does NOT activate the row
        // (radio-group's roving-tabindex semantic — Focus moves the
        // addressed item; Click / Enter / arrow keys activate).
        assert_eq!(g.focused_index(), Some(2));
        assert_eq!(g.selected_index(), None);
        for i in 0..3 {
            assert!(!g.is_selected(i));
        }
    }

    #[test]
    fn r51_87_set_focused_index_none_clears() {
        let mut g = RadioGroup::new(3);
        g.set_focused_index(Some(1));
        assert_eq!(g.focused_index(), Some(1));
        g.set_focused_index(None);
        assert_eq!(g.focused_index(), None);
    }

    #[test]
    fn r51_87_focused_index_and_selected_can_diverge() {
        let mut g = RadioGroup::new(3);
        // User selected radio 0 (mouse / arrow path).
        activate(&mut g, 0);
        assert_eq!(g.selected_index(), Some(0));
        // AT navigates Focus to radio 2 without committing.
        g.set_focused_index(Some(2));
        assert_eq!(g.focused_index(), Some(2));
        assert_eq!(g.selected_index(), Some(0));
        // R51.90 §5.40 — activation now syncs focused_index back to
        // the selected index. The AT-only Focus divergence (above)
        // collapses the moment the user commits a selection.
        activate(&mut g, 2);
        assert_eq!(g.selected_index(), Some(2));
        assert_eq!(g.focused_index(), Some(2));
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn r51_87_set_focused_index_out_of_range_panics() {
        let mut g = RadioGroup::new(2);
        g.set_focused_index(Some(5));
    }

    #[test]
    fn r51_87_external_query_focused_index_initially_null() {
        let g = RadioGroupExternal::new(3);
        assert_eq!(g.query("focused_index").unwrap(), IntrospectValue::Null);
    }

    #[test]
    fn r51_87_external_intervene_focused_index_sets_value() {
        let mut g = RadioGroupExternal::new(3);
        g.intervene("focused_index", IntrospectValue::Int(2))
            .unwrap();
        assert_eq!(g.focused_index(), Some(2));
        assert_eq!(g.query("focused_index").unwrap(), IntrospectValue::Int(2));
        // No `"selected"` intent — focused_index is AT navigation,
        // not a commit.
        assert!(!g.is_dirty());
    }

    #[test]
    fn r51_87_external_intervene_focused_index_null_clears() {
        let mut g = RadioGroupExternal::new(3);
        g.intervene("focused_index", IntrospectValue::Int(1))
            .unwrap();
        g.intervene("focused_index", IntrospectValue::Null).unwrap();
        assert_eq!(g.focused_index(), None);
    }

    #[test]
    fn r51_87_external_intervene_focused_index_out_of_range_rejects() {
        // R51.91 §5.40 — both arms now report `OutOfRange` (the
        // `Int(_)` variant is correct; only the value is bad). The
        // pre-R51.91 `TypeMismatch` was a framework-expressivity
        // carry now repaid.
        let mut g = RadioGroupExternal::new(2);
        assert_out_of_range_saying(
            &g.intervene("focused_index", IntrospectValue::Int(5)),
            "no button 5 here (it has 2, so 0..2)",
        );
        assert_out_of_range_saying(
            &g.intervene("focused_index", IntrospectValue::Int(-1)),
            "-1 is not a button index",
        );
    }

    // R51.90 §5.40 — activate path syncs focused_index regression
    // tests. WAI-ARIA Authoring Practices: activation moves focus.

    #[test]
    fn r51_90_first_activate_syncs_focused_to_selected() {
        let mut g = RadioGroup::new(3);
        assert_eq!(g.focused_index(), None);
        activate(&mut g, 1);
        assert_eq!(g.selected_index(), Some(1));
        assert_eq!(g.focused_index(), Some(1));
    }

    #[test]
    fn r51_90_switching_activation_moves_focused() {
        let mut g = RadioGroup::new(3);
        activate(&mut g, 0);
        assert_eq!(g.focused_index(), Some(0));
        activate(&mut g, 2);
        assert_eq!(g.selected_index(), Some(2));
        assert_eq!(g.focused_index(), Some(2));
    }

    #[test]
    fn r51_90_reactivating_same_radio_keeps_focused_stable() {
        let mut g = RadioGroup::new(3);
        activate(&mut g, 1);
        assert_eq!(g.focused_index(), Some(1));
        // Reactivating the same row is idempotent on selection AND
        // on focused — no `!was && now` transition fires, so neither
        // index changes. (The PointerEnter/Down/Up cycle still runs
        // visual state through the Radio leaf, but `selected` stays
        // true throughout, so the activate-edge branch is skipped.)
        activate(&mut g, 1);
        assert_eq!(g.selected_index(), Some(1));
        assert_eq!(g.focused_index(), Some(1));
    }

    #[test]
    fn r51_90_cancelled_press_leaves_focused_untouched() {
        let mut g = RadioGroup::new(3);
        activate(&mut g, 0);
        let before = g.focused_index();
        // Press but cancel on a different radio — no selection
        // change → no focused sync to row 2.
        g.send(2, RadioEvent::PointerEnter);
        g.send(2, RadioEvent::PointerDown);
        g.send(2, RadioEvent::PointerLeave);
        assert_eq!(g.selected_index(), Some(0));
        assert_eq!(g.focused_index(), before);
        assert_eq!(g.focused_index(), Some(0));
    }

    #[test]
    fn r51_90_set_selected_programmatic_does_not_touch_focused() {
        // R51.90 sync is scoped to the user-interactive `send`
        // activate edge. Programmatic `set_selected` (persisted-
        // preference restore, form default) intentionally leaves
        // `focused_index` alone so the AT active descendant is not
        // pre-committed before the user has interacted.
        let mut g = RadioGroup::new(3);
        assert_eq!(g.focused_index(), None);
        g.set_selected(Some(2));
        assert_eq!(g.selected_index(), Some(2));
        assert_eq!(g.focused_index(), None);
    }

    #[test]
    fn r51_90_at_focus_then_activate_collapses_divergence() {
        let mut g = RadioGroup::new(4);
        activate(&mut g, 0);
        // AT-side Focus walks to row 2 (no commit).
        g.set_focused_index(Some(2));
        assert_eq!(g.selected_index(), Some(0));
        assert_eq!(g.focused_index(), Some(2));
        // User commits on row 3 → both indices snap to 3.
        activate(&mut g, 3);
        assert_eq!(g.selected_index(), Some(3));
        assert_eq!(g.focused_index(), Some(3));
    }

    #[test]
    fn r51_90_external_activate_via_send_invoke_syncs_focused() {
        let mut g = RadioGroupExternal::new(3);
        // Wire-format activation through the introspect surface
        // (mirrors what `apply_key` and `access_child_invoke` issue).
        for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
            g.invoke("send", IntrospectValue::Text(format!("1:{ev}")))
                .unwrap();
        }
        assert_eq!(g.selected_index(), Some(1));
        assert_eq!(g.focused_index(), Some(1));
        assert_eq!(g.query("focused_index").unwrap(), IntrospectValue::Int(1));
    }

    #[test]
    fn external_query_state_per_radio_returns_state_name() {
        // R51.43 §5.38 — `state.<i>` returns the canonical state
        // name string ("Idle" / "Hover" / "Pressed" / "Disabled")
        // matching the single-Radio `query("state")` convention.
        let mut g = RadioGroupExternal::new(3);
        assert_eq!(
            g.query("state.0"),
            Ok(IntrospectValue::Text("Idle".to_string())),
        );
        g.send(1, RadioEvent::PointerEnter);
        assert_eq!(
            g.query("state.1"),
            Ok(IntrospectValue::Text("Hover".to_string())),
        );
        g.send(1, RadioEvent::PointerDown);
        assert_eq!(
            g.query("state.1"),
            Ok(IntrospectValue::Text("Pressed".to_string())),
        );
        g.send(2, RadioEvent::Disable);
        assert_eq!(
            g.query("state.2"),
            Ok(IntrospectValue::Text("Disabled".to_string())),
        );
        // Untouched radios stay Idle.
        assert_eq!(
            g.query("state.0"),
            Ok(IntrospectValue::Text("Idle".to_string())),
        );
    }

    #[test]
    fn external_query_selected_per_radio_returns_bool() {
        // R51.43 §5.38 — `selected.<i>` returns the selected bit as
        // `IntrospectValue::Bool`, mirroring single-Radio
        // `query("selected")`. After activating radio 1, only index
        // 1 reads as `true`.
        let mut g = RadioGroupExternal::new(3);
        assert_eq!(g.query("selected.0"), Ok(IntrospectValue::Bool(false)));
        assert_eq!(g.query("selected.1"), Ok(IntrospectValue::Bool(false)));
        // Full activate sequence on radio 1.
        for ev in [
            RadioEvent::PointerEnter,
            RadioEvent::PointerDown,
            RadioEvent::PointerUp,
        ] {
            g.send(1, ev);
        }
        assert_eq!(g.query("selected.0"), Ok(IntrospectValue::Bool(false)));
        assert_eq!(g.query("selected.1"), Ok(IntrospectValue::Bool(true)));
        assert_eq!(g.query("selected.2"), Ok(IntrospectValue::Bool(false)));
    }

    #[test]
    fn external_query_out_of_range_index_is_none() {
        // R51.43 §5.38 — out-of-range index returns `None` (matches
        // unknown-path semantics), not an error variant. AI clients
        // observe the same silent-fall-through that hits unknown
        // top-level paths.
        let g = RadioGroupExternal::new(3);
        assert!(matches!(
            g.query("state.99"),
            Err(ReadRefusal::NoSuchMember(_))
        ));
        assert!(matches!(
            g.query("selected.99"),
            Err(ReadRefusal::NoSuchMember(_))
        ));
    }

    /// R1667 — a malformed ARGUMENT and an unknown PATH are different refusals,
    /// and this test is where the difference is stated for this widget.
    ///
    /// Every assertion below answered the same word before the read channel
    /// could carry a reason, so the test's own name described the
    /// only answer available rather than the one that was true (the name was
    /// `external_query_malformed_per_radio_path_is_none`).
    #[test]
    fn external_query_malformed_per_radio_path_states_which_half_is_wrong() {
        let g = RadioGroupExternal::new(3);
        // A declared family, and the argument is not the declared type.
        assert_eq!(
            g.query("state.abc"),
            Err(ReadRefusal::QueryTypeMismatch),
            "`abc` is not an index",
        );
        assert_eq!(
            g.query("selected.-1"),
            Err(ReadRefusal::QueryTypeMismatch),
            "a negative index is not a `usize`",
        );
        // R1667 — the bare stem is NOT a member of the family (the template's
        // separator never matched), so it stays the unknown-path answer.
        assert_eq!(g.query("state"), Err(ReadRefusal::UnknownPath));
        assert_eq!(g.query("selected"), Err(ReadRefusal::UnknownPath));
        // …and the empty member is now the surface's call, not the matcher's.
        assert_eq!(
            g.query("state."),
            Err(ReadRefusal::QueryTypeMismatch),
            "R1667 - the family owns its empty member; the argument is malformed",
        );
    }
}
