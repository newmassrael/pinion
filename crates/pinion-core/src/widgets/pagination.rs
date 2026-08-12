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
    IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner, SchemaArg,
    SchemaField, ThreadOwnership,
};
use crate::input::AutoRepeat;
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
    /// R1549 §5.35 — cadence a held chevron repeats at. Holding a pager
    /// arrow walks pages the way holding a scrollbar arrow walks lines;
    /// before this it walked exactly one.
    repeat: AutoRepeat,
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
            repeat: AutoRepeat::desktop(),
        };
        me.sync_enabled();
        me
    }

    /// R1549 §5.35 — override the held-chevron repeat cadence (defaults to
    /// [`AutoRepeat::desktop`]). A pager over thousands of pages wants an
    /// [`AutoRepeat::accelerating`] cadence; one over five wants a slower
    /// fixed one.
    #[must_use]
    pub fn with_auto_repeat(mut self, repeat: AutoRepeat) -> Self {
        self.repeat = repeat;
        self
    }

    /// The declared held-chevron repeat cadence.
    #[must_use]
    pub const fn auto_repeat_policy(&self) -> AutoRepeat {
        self.repeat
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

    /// R1549 §5.35 — a held chevron keeps paging, read off the two chevron
    /// statecharts. The end-of-range gate is already expressed as
    /// `Disabled` by the widget's own `sync_enabled`, and a `Disabled`
    /// button is not `Pressed`, so reaching the last page stops it through
    /// the state machine that was already there — no bound check is
    /// restated here.
    fn auto_repeat(&self) -> Option<AutoRepeat> {
        let held = matches!(self.prev.state(), ButtonState::Pressed)
            || matches!(self.next.state(), ButtonState::Pressed);
        held.then_some(self.repeat)
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
                    SchemaField::new("can_prev", "bool"),
                    SchemaField::new("can_next", "bool"),
                    SchemaField::new("prev.state", "string"),
                    SchemaField::new("next.state", "string"),
                    SchemaField::send("string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        match path {
            "can_prev" => Ok(IntrospectValue::Bool(self.can_prev())),
            "can_next" => Ok(IntrospectValue::Bool(self.can_next())),
            "prev.state" => Ok(IntrospectValue::Text(
                self.prev.state().as_name().to_string(),
            )),
            "next.state" => Ok(IntrospectValue::Text(
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
                    // Composite nav sub-tags: `"{tag}#prev"` / `"{tag}#next"`.
                    // Drive the chevron button (hover / press / capture); step
                    // on its click edge. A `Disabled` button (clamped end)
                    // ignores the events, so no step occurs.
                    //
                    // R1627 — decoded through the `:` grammar's SSOT rather
                    // than by stripping a prefix and treating the remainder as
                    // the event name. That hand-rolled read is what R1619
                    // broke: the wire grew a fourth segment, the remainder
                    // became `"PointerDown::l"`, `ButtonEvent::from_name`
                    // answered `None`, and the chevrons went silently dead for
                    // eight rounds. A named key is now as first-class as a
                    // numeric one, so there is no longer a reason to hand-roll
                    // this.
                    if let Some(payload) = crate::composite_tag::split_send_payload(&s) {
                        let chevron = match payload.key {
                            "prev" => Some((&mut self.prev, -1_i32)),
                            "next" => Some((&mut self.next, 1_i32)),
                            _ => None,
                        };
                        if let Some((button, delta)) = chevron {
                            if Self::drive_chevron(button, payload.event) {
                                self.step(delta);
                            }
                            return Ok(IntrospectValue::Null);
                        }
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

    /// R1627 — the chevrons survive the `:` grammar GROWING.
    ///
    /// This is the test that was missing for eight rounds. `drive_chevron`
    /// read everything after `"prev:"` as the event name, so when R1619 added
    /// the fourth wire segment the remainder became `"PointerDown::l"`,
    /// `ButtonEvent::from_name` answered `None`, and both chevrons went
    /// silently dead — `r754_pagination` caught it in CI and nothing in
    /// `cargo test` did, because every unit test here spells the payload the
    /// pre-R1619 way.
    ///
    /// So the payload is spelled EVERY way the grammar allows, including the
    /// shapes a future segment would produce: a fifth segment must leave the
    /// chevron working, because `split_send_payload` bounds its split at four
    /// and hands the rest back as context.
    #[test]
    fn r1627_a_chevron_click_survives_every_shape_of_the_wire() {
        // Each row is the same click, spelled with more of the grammar. The
        // `::l` form is exactly what R1619's router emits for a press.
        /// One way of spelling the same event: a name, and the suffix the
        /// wire appends for that much context.
        type Spelling = (&'static str, &'static str);
        let shapes: [Spelling; 4] = [
            ("bare", ""),
            ("modifiers", ":"),
            ("buttons", "::l"),
            ("modifiers+buttons", ":sc:l"),
        ];
        for (name, suffix) in shapes {
            let mut p = PaginationExternal::new(5, 2);
            for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
                let payload = format!("prev:{ev}{suffix}");
                let _ = p.invoke("send", IntrospectValue::Text(payload));
            }
            assert_eq!(
                p.current(),
                1,
                "{name}: a prev click steps the page whatever context the wire carries",
            );
        }
    }

    /// R1627 — and the NEGATIVE control: a payload whose event name is
    /// genuinely unknown must still be a no-op, so the test above is not
    /// passing because the parser became permissive.
    #[test]
    fn r1627_an_unknown_event_is_still_a_no_op() {
        let mut p = PaginationExternal::new(5, 2);
        for payload in ["prev:Nonsense", "prev:Nonsense::l", "prev:", "prev"] {
            let _ = p.invoke("send", IntrospectValue::Text(payload.to_string()));
        }
        assert_eq!(p.current(), 2, "nothing that is not a click steps the page");
        // An unknown KEY falls through to the page group, which is what makes
        // `"<i>:<Event>"` work — and it must not be read as a chevron.
        let mut q = PaginationExternal::new(5, 2);
        let _ = q.invoke("send", IntrospectValue::Text("previous:PointerUp".into()));
        assert_eq!(
            q.current(),
            2,
            "a key that merely starts with `prev` is not one"
        );
    }

    /// Drive the full pointer click cycle on a chevron through the wire.
    fn click_chevron(p: &mut PaginationExternal, which: &str) {
        for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
            let _ = p.invoke("send", IntrospectValue::Text(format!("{which}:{ev}")));
        }
    }

    /// Hold a chevron: the arc stops at `PointerDown`.
    fn hold_chevron(p: &mut PaginationExternal, which: &str) {
        for ev in ["PointerEnter", "PointerDown"] {
            let _ = p.invoke("send", IntrospectValue::Text(format!("{which}:{ev}")));
        }
    }

    // ─────────────────────────────────────────────────────────────
    // R1549 §5.35 §5.38 — held-chevron auto-repeat declaration.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn idle_pager_declares_no_repeat() {
        assert_eq!(PaginationExternal::new(5, 0).auto_repeat(), None);
    }

    #[test]
    fn held_chevron_declares_the_desktop_cadence() {
        let mut p = PaginationExternal::new(5, 2);
        hold_chevron(&mut p, "next");
        assert_eq!(p.next_state(), ButtonState::Pressed);
        assert_eq!(p.auto_repeat(), Some(AutoRepeat::desktop()));
    }

    /// The end-of-range stop costs no new code: `sync_enabled` already
    /// spells it `Disabled`, and a `Disabled` chevron is not `Pressed`.
    /// Holding the disabled end therefore declares nothing without any
    /// bound arithmetic in `auto_repeat`.
    #[test]
    fn disabled_chevron_at_the_end_declares_nothing() {
        let mut p = PaginationExternal::new(5, 0);
        assert_eq!(p.prev_state(), ButtonState::Disabled, "first page");
        hold_chevron(&mut p, "prev");
        assert_eq!(p.auto_repeat(), None);
    }

    /// Walking a held chevron INTO the last page goes quiet at exactly
    /// that page — the router's catch-up loop re-asks per fire, so this is
    /// the property that stops a long `scene/tick` from paging past the end.
    #[test]
    fn held_chevron_goes_quiet_on_reaching_the_last_page() {
        let mut p = PaginationExternal::new(4, 1);
        hold_chevron(&mut p, "next");
        let mut fires = 0;
        while p.auto_repeat().is_some() {
            fires += 1;
            for ev in ["PointerUp", "PointerDown"] {
                let _ = p.invoke("send", IntrospectValue::Text(format!("next:{ev}")));
            }
            assert!(fires < 10, "must terminate at the last page");
        }
        assert_eq!(fires, 2, "page 1 -> 2 -> 3 (last of 4), then quiet");
        assert_eq!(p.current(), 3);
    }

    #[test]
    fn cadence_is_declarable_per_pager() {
        let slow = AutoRepeat::new(0.6, 0.4);
        let mut p = PaginationExternal::new(5, 2).with_auto_repeat(slow);
        assert_eq!(p.auto_repeat_policy(), slow);
        hold_chevron(&mut p, "next");
        assert_eq!(p.auto_repeat(), Some(slow));
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
        assert_eq!(p.query("count"), Ok(IntrospectValue::Int(4)));
        assert_eq!(p.query("selected_index"), Ok(IntrospectValue::Int(0)));
        assert_eq!(p.query("can_prev"), Ok(IntrospectValue::Bool(false)));
        assert_eq!(p.query("can_next"), Ok(IntrospectValue::Bool(true)));
        assert_eq!(
            p.query("prev.state"),
            Ok(IntrospectValue::Text("Disabled".into()))
        );
        assert_eq!(
            p.query("next.state"),
            Ok(IntrospectValue::Text("Idle".into()))
        );
        assert_eq!(p.query("selected.0"), Ok(IntrospectValue::Bool(true)));
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
