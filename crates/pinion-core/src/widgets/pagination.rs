//! R754 §5.38 — `Pagination`: a single-select page coordinator with
//! interactive, clamping previous / next controls.
//!
//! A pagination control is, at its core, a single-select group of numbered
//! page links (exactly one page is *current*) — so the N page cells reuse
//! the [`RadioGroupExternal`] coordinator verbatim (per-cell interaction
//! state, 1-of-N exclusion, the §5.20 `"selected"` intent, the roving
//! keyboard model, and the `pinion_a11y::navigation_link_nodes` tree),
//! exactly as `hello-breadcrumb` (R731) and `hello-nav-rail` (R751) do.
//!
//! What pagination adds is **previous / next** stepping. Unlike the cyclic
//! arrow roving of a radio group, prev / next *clamp* at the ends (page 0
//! has no previous, the last page has no next), and they are their own
//! pointer targets. Each is a real [`Button`] (R754.1) — so the chevrons
//! show the M3 hover / pressed state-layer, carry pointer-capture
//! (jitter-robust, the R741 rule), and sit in the `Disabled` state at the
//! clamped ends where they ignore pointer input entirely. This wrapper owns
//! the page [`RadioGroupExternal`] plus the two buttons and routes the
//! composite `'#'`-split wire (R51.42): a click on the paint tag
//! `"{tag}#prev"` / `"{tag}#next"` arrives as `"prev:<Event>"` /
//! `"next:<Event>"` and drives that button; on the button's click edge
//! (`Pressed -> Hover`, detected through the button's own
//! [`WidgetTransition::detect`] SSOT) the current page steps. Page-cell
//! sends (`"<i>:<Event>"`), queries and intervene delegate straight to the
//! inner group, so the whole introspect surface AI clients and the view
//! read is the radio group's, plus `can_prev` / `can_next` and the
//! `prev.state` / `next.state` button postures.

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::WidgetTransition;
use crate::widgets::button::{Button, ButtonEvent, ButtonState};
use crate::widgets::radio::{RadioEvent, RadioState};
use crate::widgets::radio_group::RadioGroupExternal;
use crate::{WidgetEventName, WidgetStateName};

/// A pagination coordinator: N page cells (a [`RadioGroupExternal`]) plus
/// two clamping previous / next [`Button`]s. See the module docs.
pub struct PaginationExternal {
    pages: RadioGroupExternal,
    prev: Button,
    next: Button,
    count: usize,
}

// `Button` wraps a non-`Debug` SCXML `Widget`, so derive is unavailable;
// the [`External`] supertrait requires `Debug`, so format the observable
// posture (count / current / chevron states) by hand.
impl core::fmt::Debug for PaginationExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PaginationExternal")
            .field("count", &self.count)
            .field("current", &self.current())
            .field("prev", &self.prev.state())
            .field("next", &self.next.state())
            .finish_non_exhaustive()
    }
}

impl PaginationExternal {
    /// Build a pagination control over `count` pages with `current`
    /// selected (clamped into range). The current page is seeded through
    /// the `KeyboardActivate` edge so the boot frame paints a clean current
    /// cell with no hover / pressed residue (the R728 boot-seed lesson).
    /// The previous / next buttons are immediately synced to the clamped
    /// ends (page 0 → prev `Disabled`).
    ///
    /// # Panics
    /// Never panics; an out-of-range `current` is clamped.
    #[must_use]
    pub fn new(count: usize, current: usize) -> Self {
        let mut pages = RadioGroupExternal::new(count);
        if count > 0 {
            pages.send(current.min(count - 1), RadioEvent::KeyboardActivate);
        }
        let mut me = Self {
            pages,
            prev: Button::new(),
            next: Button::new(),
            count,
        };
        me.sync_enabled();
        me
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

    /// The previous button's interaction posture (drives the chevron's
    /// state-layer overlay; `Disabled` at the first page).
    #[must_use]
    pub fn prev_state(&self) -> ButtonState {
        self.prev.state()
    }

    /// The next button's interaction posture.
    #[must_use]
    pub fn next_state(&self) -> ButtonState {
        self.next.state()
    }

    /// Step the current page by `delta`, **clamping** at the ends (no
    /// wrap-around — unlike the cyclic arrow roving). A step that would
    /// leave the range is a no-op. The new page is activated through the
    /// `KeyboardActivate` edge, firing the §5.20 `"selected"` intent on a
    /// real change, then the prev / next enabled state is re-synced.
    pub fn step(&mut self, delta: i32) {
        if self.count == 0 {
            return;
        }
        let cur = i32::try_from(self.current()).unwrap_or(0);
        let max = i32::try_from(self.count - 1).unwrap_or(0);
        let target = (cur + delta).clamp(0, max);
        if target != cur {
            let target = usize::try_from(target).unwrap_or(0);
            self.pages.send(target, RadioEvent::KeyboardActivate);
        }
        self.sync_enabled();
    }

    /// Drive a [`RadioEvent`] on page cell `index` (the page-cell pointer
    /// arc). Mirrors [`RadioGroupExternal::send`]; re-syncs prev / next.
    pub fn send_page(&mut self, index: usize, event: RadioEvent) {
        self.pages.send(index, event);
        self.sync_enabled();
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

    /// Re-sync each chevron's `Disabled` posture to the clamped ends. A
    /// disabled [`Button`] ignores pointer input (no hover, no click), so
    /// the clamp is enforced at the interaction layer too, not only by
    /// [`Self::step`]'s arithmetic.
    fn sync_enabled(&mut self) {
        let (can_prev, can_next) = (self.can_prev(), self.can_next());
        Self::sync_button(&mut self.prev, can_prev);
        Self::sync_button(&mut self.next, can_next);
    }

    fn sync_button(button: &mut Button, enabled: bool) {
        let disabled = matches!(button.state(), ButtonState::Disabled);
        if enabled && disabled {
            button.send(ButtonEvent::Enable);
        } else if !enabled && !disabled {
            button.send(ButtonEvent::Disable);
        }
    }

    /// Drive a chevron button with one wire event; step on the click edge.
    /// Returns the (unused) outcome shape for the wire.
    fn drive_chevron(button: &mut Button, event_name: &str) -> bool {
        let Some(ev) = ButtonEvent::from_name(event_name) else {
            return false;
        };
        let before = button.state();
        button.send(ev);
        let after = button.state();
        // Reuse the Button's own click rule (Pressed -> Hover ⇒ "click")
        // rather than re-deriving it — the WidgetTransition SSOT.
        !<Button as WidgetTransition>::detect(before, ev, after).is_empty()
    }

    fn pages_introspect(&self) -> &dyn ExternalIntrospect {
        self.pages
            .introspect()
            .expect("RadioGroupExternal always introspects")
    }

    fn pages_introspect_mut(&mut self) -> &mut dyn ExternalIntrospect {
        self.pages
            .introspect_mut()
            .expect("RadioGroupExternal always introspects")
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
        // next step produces) flow through the inner group's emitter. The
        // chevron buttons are plain logical widgets (no intent buffer); a
        // chevron click surfaces only as the resulting page `"selected"`.
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
            ("prev.state", "string"),
            ("next.state", "string"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "can_prev" => Some(IntrospectValue::Bool(self.can_prev())),
            "can_next" => Some(IntrospectValue::Bool(self.can_next())),
            "prev.state" => Some(IntrospectValue::Text(
                self.prev.state().as_name().to_string(),
            )),
            "next.state" => Some(IntrospectValue::Text(
                self.next.state().as_name().to_string(),
            )),
            // count / selected_index / focused_index / state.<i> /
            // selected.<i> are the page group's surface verbatim.
            _ => self.pages_introspect().query(path),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        // selected_index / focused_index restore is the page group's admin
        // surface (no `"selected"` intent). Re-sync the chevrons afterward.
        let outcome = self.pages_introspect_mut().intervene(path, value);
        if outcome.is_ok() {
            self.sync_enabled();
        }
        outcome
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "send" => match args {
                IntrospectValue::Text(s) => {
                    // Composite nav sub-tags: `"{tag}#prev"` / `"{tag}#next"`
                    // arrive as `"prev:<Event>"` / `"next:<Event>"`. Drive
                    // the chevron button (hover / press / capture); step on
                    // its click edge. A `Disabled` button (clamped end)
                    // ignores the events, so no step occurs.
                    if let Some(ev) = s.strip_prefix("prev:") {
                        if Self::drive_chevron(&mut self.prev, ev) {
                            self.step(-1);
                        }
                        return Ok(IntrospectValue::Null);
                    }
                    if let Some(ev) = s.strip_prefix("next:") {
                        if Self::drive_chevron(&mut self.next, ev) {
                            self.step(1);
                        }
                        return Ok(IntrospectValue::Null);
                    }
                    // Page cell `"<i>:<Event>"` — delegate to the group,
                    // then re-sync the chevrons (the current page may have
                    // moved to / from an end).
                    let outcome = self
                        .pages_introspect_mut()
                        .invoke("send", IntrospectValue::Text(s));
                    self.sync_enabled();
                    outcome
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

    /// Drive the full pointer click cycle on a chevron through the wire.
    fn click_chevron(p: &mut PaginationExternal, which: &str) {
        for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
            let _ = p.invoke("send", IntrospectValue::Text(format!("{which}:{ev}")));
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
    fn wire_chevron_click_steps_the_page() {
        let mut p = PaginationExternal::new(5, 2);
        click_chevron(&mut p, "next");
        assert_eq!(p.current(), 3, "next chevron click steps forward");
        click_chevron(&mut p, "prev");
        assert_eq!(p.current(), 2, "prev chevron click steps back");
    }

    #[test]
    fn chevron_hover_and_press_track_button_state() {
        let mut p = PaginationExternal::new(5, 2);
        let _ = p.invoke("send", IntrospectValue::Text("next:PointerEnter".into()));
        assert_eq!(p.next_state(), ButtonState::Hover, "next chevron hovers");
        let _ = p.invoke("send", IntrospectValue::Text("next:PointerDown".into()));
        assert_eq!(p.next_state(), ButtonState::Pressed, "next chevron presses");
        assert_eq!(p.current(), 2, "no step until the click edge (PointerUp)");
        let _ = p.invoke("send", IntrospectValue::Text("next:PointerUp".into()));
        assert_eq!(p.current(), 3, "click edge steps");
    }

    #[test]
    fn disabled_chevron_ignores_pointer_no_hover_no_step() {
        // Page 0: prev is at the clamped end -> Disabled.
        let mut p = PaginationExternal::new(5, 0);
        assert_eq!(
            p.prev_state(),
            ButtonState::Disabled,
            "prev disabled on page 0"
        );
        let _ = p.invoke("send", IntrospectValue::Text("prev:PointerEnter".into()));
        assert_eq!(
            p.prev_state(),
            ButtonState::Disabled,
            "disabled prev does not hover"
        );
        click_chevron(&mut p, "prev");
        assert_eq!(p.current(), 0, "disabled prev does not step");
    }

    #[test]
    fn stepping_to_an_end_disables_that_chevron() {
        let mut p = PaginationExternal::new(3, 1);
        assert_eq!(p.next_state(), ButtonState::Idle, "next enabled mid-range");
        click_chevron(&mut p, "next"); // -> page 2 (last)
        assert_eq!(p.current(), 2);
        assert_eq!(
            p.next_state(),
            ButtonState::Disabled,
            "next disabled at the last page"
        );
        assert_ne!(p.prev_state(), ButtonState::Disabled, "prev now enabled");
    }

    #[test]
    fn query_surface_matches_the_group_plus_can_prev_next_and_button_states() {
        let p = PaginationExternal::new(4, 0);
        assert_eq!(p.query("count"), Some(IntrospectValue::Int(4)));
        assert_eq!(p.query("selected_index"), Some(IntrospectValue::Int(0)));
        assert_eq!(p.query("can_prev"), Some(IntrospectValue::Bool(false)));
        assert_eq!(p.query("can_next"), Some(IntrospectValue::Bool(true)));
        assert_eq!(
            p.query("prev.state"),
            Some(IntrospectValue::Text("Disabled".into()))
        );
        assert_eq!(
            p.query("next.state"),
            Some(IntrospectValue::Text("Idle".into()))
        );
        assert_eq!(p.query("selected.0"), Some(IntrospectValue::Bool(true)));
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
