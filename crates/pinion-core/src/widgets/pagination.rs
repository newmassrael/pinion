//! R754 §5.38 — `Pagination`: a single-select page coordinator with
//! clamping previous / next controls.
//!
//! A pagination control is, at its core, a single-select group of numbered
//! page links (exactly one page is *current*) — so the N page cells reuse
//! the [`RadioGroupExternal`] coordinator verbatim (per-cell interaction
//! state, 1-of-N exclusion, the §5.20 `"selected"` intent, the roving
//! keyboard model, and the [`pinion_a11y::navigation_link_nodes`] tree),
//! exactly as `hello-breadcrumb` (R731) and `hello-nav-rail` (R751) do.
//!
//! What pagination adds is **previous / next** stepping. Unlike the cyclic
//! arrow roving of a radio group, prev / next *clamp* at the ends (page 0
//! has no previous, the last page has no next), and they are their own
//! pointer targets. This wrapper owns the page [`RadioGroupExternal`] and
//! routes the composite `'#'`-split wire (R51.42) so a click on the paint
//! tag `"{tag}#prev"` / `"{tag}#next"` arrives as `"prev:<Event>"` /
//! `"next:<Event>"` and steps the current page on the `PointerUp` edge —
//! the single-coordinator pattern `DatePickerExternal` established for its
//! previous / next-month buttons. Page-cell sends (`"<i>:<Event>"`),
//! queries and intervene delegate straight to the inner group, so the
//! whole introspect surface AI clients and the view read is the radio
//! group's, plus `can_prev` / `can_next`.

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::radio::{RadioEvent, RadioState};
use crate::widgets::radio_group::RadioGroupExternal;

/// A pagination coordinator: N page cells (a [`RadioGroupExternal`]) plus
/// clamping previous / next stepping. See the module docs.
#[derive(Debug)]
pub struct PaginationExternal {
    pages: RadioGroupExternal,
    count: usize,
}

impl PaginationExternal {
    /// Build a pagination control over `count` pages with `current`
    /// selected (clamped into range). The current page is seeded through
    /// the `KeyboardActivate` edge so the boot frame paints a clean current
    /// cell with no hover / pressed residue (the R728 boot-seed lesson).
    ///
    /// # Panics
    /// Never panics; an out-of-range `current` is clamped.
    #[must_use]
    pub fn new(count: usize, current: usize) -> Self {
        let mut pages = RadioGroupExternal::new(count);
        if count > 0 {
            pages.send(current.min(count - 1), RadioEvent::KeyboardActivate);
        }
        Self { pages, count }
    }

    /// Total page count.
    #[must_use]
    pub fn count(&self) -> usize {
        self.count
    }

    /// The current (selected) page, or `0` when nothing is selected.
    #[must_use]
    pub fn current(&self) -> usize {
        self.pages.selected_index().unwrap_or(0)
    }

    /// `true` when a previous page exists (the current page is not the
    /// first).
    #[must_use]
    pub fn can_prev(&self) -> bool {
        self.current() > 0
    }

    /// `true` when a next page exists (the current page is not the last).
    #[must_use]
    pub fn can_next(&self) -> bool {
        self.count > 0 && self.current() + 1 < self.count
    }

    /// Step the current page by `delta`, **clamping** at the ends (no
    /// wrap-around — unlike the cyclic arrow roving). A step that would
    /// leave the range is a no-op, so a `prev` press on page 0 (or `next`
    /// on the last page) does nothing. The new page is activated through
    /// the `KeyboardActivate` edge, firing the §5.20 `"selected"` intent on
    /// a real change.
    pub fn step(&mut self, delta: i32) {
        if self.count == 0 {
            return;
        }
        let cur = i32::try_from(self.current()).unwrap_or(0);
        let max = i32::try_from(self.count - 1).unwrap_or(0);
        let target = (cur + delta).clamp(0, max);
        if target != cur {
            // `target` is in `0..=max`, so the conversion never fails.
            let target = usize::try_from(target).unwrap_or(0);
            self.pages.send(target, RadioEvent::KeyboardActivate);
        }
    }

    /// Drive a [`RadioEvent`] on page cell `index` (the page-cell pointer
    /// arc). Mirrors [`RadioGroupExternal::send`].
    pub fn send_page(&mut self, index: usize, event: RadioEvent) {
        self.pages.send(index, event);
    }

    /// Page cell `index`'s interaction state.
    #[must_use]
    pub fn state(&self, index: usize) -> RadioState {
        self.pages.state(index)
    }

    /// Whether page cell `index` is the current page.
    #[must_use]
    pub fn is_selected(&self, index: usize) -> bool {
        self.pages.is_selected(index)
    }

    /// The AT-side roving active descendant page, or `None`.
    #[must_use]
    pub fn focused_index(&self) -> Option<usize> {
        self.pages.focused_index()
    }

    fn pages_introspect(&self) -> &dyn ExternalIntrospect {
        self.pages.introspect().expect("RadioGroupExternal always introspects")
    }

    fn pages_introspect_mut(&mut self) -> &mut dyn ExternalIntrospect {
        self.pages.introspect_mut().expect("RadioGroupExternal always introspects")
    }
}

impl External for PaginationExternal {
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
        // Page-selection `"selected"` intents (including those a prev /
        // next step produces) flow through the inner group's emitter.
        self.pages.drain_intents(sink);
    }

    fn is_dirty(&self) -> bool {
        self.pages.is_dirty()
    }
}

impl ExternalIntrospect for PaginationExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("count", "int"),
            ("selected_index", "int"),
            ("focused_index", "int"),
            ("state.<index>", "string"),
            ("selected.<index>", "bool"),
            ("can_prev", "bool"),
            ("can_next", "bool"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "can_prev" => Some(IntrospectValue::Bool(self.can_prev())),
            "can_next" => Some(IntrospectValue::Bool(self.can_next())),
            // count / selected_index / focused_index / state.<i> /
            // selected.<i> are the page group's surface verbatim.
            _ => self.pages_introspect().query(path),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        // selected_index / focused_index restore is the page group's admin
        // surface (no `"selected"` intent).
        self.pages_introspect_mut().intervene(path, value)
    }

    fn invoke(&mut self, path: &str, args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        match path {
            "send" => match args {
                IntrospectValue::Text(s) => {
                    // Composite nav sub-tags: `"{tag}#prev"` / `"{tag}#next"`
                    // arrive as `"prev:<Event>"` / `"next:<Event>"`. Step on
                    // the `PointerUp` edge; the other cycle events
                    // (Enter / Down / Leave) are accepted no-ops so the full
                    // pointer cycle a click produces is not rejected.
                    if let Some(ev) = s.strip_prefix("prev:") {
                        if ev == "PointerUp" {
                            self.step(-1);
                        }
                        return Ok(IntrospectValue::Null);
                    }
                    if let Some(ev) = s.strip_prefix("next:") {
                        if ev == "PointerUp" {
                            self.step(1);
                        }
                        return Ok(IntrospectValue::Null);
                    }
                    // Page cell `"<i>:<Event>"` — delegate to the group.
                    self.pages_introspect_mut().invoke("send", IntrospectValue::Text(s))
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

    fn activate_page(p: &mut PaginationExternal, idx: usize) {
        for ev in [
            RadioEvent::PointerEnter,
            RadioEvent::PointerDown,
            RadioEvent::PointerUp,
            RadioEvent::PointerLeave,
        ] {
            p.send_page(idx, ev);
        }
    }

    #[test]
    fn boots_with_seeded_current_no_residue() {
        let p = PaginationExternal::new(5, 2);
        assert_eq!(p.count(), 5);
        assert_eq!(p.current(), 2, "current seeded");
        assert_eq!(p.state(2), RadioState::Idle, "no hover/pressed residue");
        assert!(p.is_selected(2));
    }

    #[test]
    fn out_of_range_current_is_clamped() {
        let p = PaginationExternal::new(3, 9);
        assert_eq!(p.current(), 2, "clamped to last page");
    }

    #[test]
    fn clicking_a_page_makes_it_current() {
        let mut p = PaginationExternal::new(5, 0);
        activate_page(&mut p, 3);
        assert_eq!(p.current(), 3, "page 3 is now current");
        assert!(!p.is_selected(0), "page 0 no longer current (1-of-N)");
    }

    #[test]
    fn next_steps_forward_and_clamps_at_last() {
        let mut p = PaginationExternal::new(3, 1);
        p.step(1);
        assert_eq!(p.current(), 2, "next -> page 2");
        assert!(!p.can_next(), "no next past the last page");
        p.step(1);
        assert_eq!(p.current(), 2, "next on the last page is a no-op (clamp)");
    }

    #[test]
    fn prev_steps_back_and_clamps_at_first() {
        let mut p = PaginationExternal::new(3, 1);
        p.step(-1);
        assert_eq!(p.current(), 0, "prev -> page 0");
        assert!(!p.can_prev(), "no previous before the first page");
        p.step(-1);
        assert_eq!(p.current(), 0, "prev on the first page is a no-op (clamp)");
    }

    #[test]
    fn can_prev_can_next_track_the_ends() {
        let mut p = PaginationExternal::new(4, 0);
        assert!(!p.can_prev(), "page 0: no previous");
        assert!(p.can_next(), "page 0: has next");
        activate_page(&mut p, 3);
        assert!(p.can_prev(), "page 3: has previous");
        assert!(!p.can_next(), "page 3 (last): no next");
    }

    #[test]
    fn wire_prev_next_step_on_pointer_up_only() {
        let mut p = PaginationExternal::new(5, 2);
        // Down / Enter / Leave are accepted no-ops.
        let _ = p.invoke("send", IntrospectValue::Text("next:PointerEnter".into()));
        let _ = p.invoke("send", IntrospectValue::Text("next:PointerDown".into()));
        assert_eq!(p.current(), 2, "no step before PointerUp");
        let _ = p.invoke("send", IntrospectValue::Text("next:PointerUp".into()));
        assert_eq!(p.current(), 3, "next steps on PointerUp");
        let _ = p.invoke("send", IntrospectValue::Text("prev:PointerUp".into()));
        assert_eq!(p.current(), 2, "prev steps back on PointerUp");
    }

    #[test]
    fn wire_page_cell_delegates_to_group() {
        let mut p = PaginationExternal::new(5, 0);
        for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
            let _ = p.invoke("send", IntrospectValue::Text(format!("4:{ev}")));
        }
        assert_eq!(p.current(), 4, "page-cell wire selects via the group");
    }

    #[test]
    fn query_surface_matches_the_group_plus_can_prev_next() {
        let p = PaginationExternal::new(4, 0);
        assert_eq!(p.query("count"), Some(IntrospectValue::Int(4)));
        assert_eq!(p.query("selected_index"), Some(IntrospectValue::Int(0)));
        assert_eq!(p.query("can_prev"), Some(IntrospectValue::Bool(false)));
        assert_eq!(p.query("can_next"), Some(IntrospectValue::Bool(true)));
        assert_eq!(p.query("selected.0"), Some(IntrospectValue::Bool(true)));
        assert_eq!(p.query("selected.1"), Some(IntrospectValue::Bool(false)));
    }

    #[test]
    fn step_emits_selected_intent_through_the_group() {
        let mut p = PaginationExternal::new(5, 1);
        p.step(1);
        let mut intents: Vec<Intent> = Vec::new();
        p.drain_intents(&mut |i| intents.push(i));
        assert!(
            intents.iter().any(|i| i.tag_str() == "selected"),
            "a clamped next step fires the §5.20 selected intent: {intents:?}",
        );
    }
}
