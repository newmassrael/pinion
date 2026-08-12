//! R695 §5.38 §5.40 §5.50 — `Tooltip` widget: a hover / keyboard-focus
//! descriptive popup (WCAG 2.2 SC 1.4.13 "Content on Hover or Focus").
//!
//! Phase B widget-catalog entry — the contextual "what does this do?"
//! label every pro DCC / IDE / CAD tool attaches to its icon buttons,
//! and a direct step toward the northern-star "engine-class editor
//! self-hosted in pinion" (every toolbar glyph carries one).
//!
//! ## Descriptive-class, not interactive (the WAI-ARIA classification)
//!
//! A tooltip is **passive descriptive content** — WAI-ARIA 1.2 §3.7
//! `tooltip`. It carries no command (`menuitem`), no selection
//! (`tab` / `option`), and no toggle (`button` + `aria-pressed`); it is
//! never focusable and owns no keyboard model of its own. The trigger
//! element references it through `aria-describedby`
//! (`AccessNode::with_described_by` in the binding) so AT
//! announces "Save, Saves the current file" — the tooltip text becomes
//! the trigger's *description*, not a node the user can land on. This is
//! why [`TooltipExternal`] is its own tiny visibility statechart rather
//! than a reuse of a command / selection widget.
//!
//! ## The WCAG 1.4.13 trigger + dismiss model
//!
//! The tooltip is shown while its trigger is **hovered or focused** and
//! is hidden once both clear — *persistent* (no auto-hide timer). The
//! three SC 1.4.13 obligations land as:
//!
//! - **Hoverable** — the binding paints the overlay carrying the *same*
//!   paint tag as the anchor and places it contiguous with the anchor
//!   rect, so the router never sees a hover-target transition when the
//!   cursor crosses from the trigger onto the tooltip body
//!   (`pinion_widget_paint::tooltip` owns the geometry). The
//!   `hovered` posture therefore stays set across the whole
//!   trigger-plus-body region — the cursor can rest on the tooltip
//!   without it disappearing.
//! - **Dismissible** — `Self::dismiss` (the `dismiss` invoke action,
//!   driven by the shell's `Escape` key handler **and** the RPC
//!   `scene/invoke` action channel — one funnel, §2 invariant #2) sets
//!   a latch that hides the tooltip while hover / focus stays put.
//! - **Persistent** — there is no timer; visibility is a pure function
//!   of the live posture, `Self::visible`.
//!
//! The dismiss latch clears on the trigger-episode's falling edge (the
//! `PointerLeave` that drops the last hover with no focus, or the blur
//! that drops the last focus with no hover) so a *later* hover / focus
//! shows the tooltip again — the WAI-ARIA APG re-display behaviour —
//! without a sticky "dismissed forever" state.
//!
//! ## Wire surface (§5.12)
//!
//! `query("visible" | "hovered" | "focused" | "dismissed")` reads the
//! posture; `invoke("send", "<PointerEvent>")` is the router's hover
//! channel (`PointerEnter` / `PointerLeave`; press events are no-ops —
//! a tooltip is not clickable); `invoke("dismiss", …)` is the WCAG
//! dismiss. The trigger's hover / focus posture (the *other* half of
//! the visibility input) is sourced by the binding from the anchor's
//! own external — here the anchor *is* this tooltip widget, so the
//! `PointerEnter` / [`External::on_focus_change`] arcs the shell already
//! fires drive the statechart directly with no anchor→overlay
//! back-channel (the cross-widget "attach a tooltip to an arbitrary
//! existing widget" primitive is a future axis once a 2nd consumer
//! needs it, per `[[abstraction-needs-second-consumer]]`).

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, ReadRefusal, RepaintOwner, SchemaField,
    ThreadOwnership,
};
use crate::input::PointerWireEvent;
use crate::intent::Intent;

/// R695 §5.38 — a hover / focus tooltip visibility statechart.
///
/// `visible = (hovered || focused) && !dismissed`. See the module docs
/// for the WCAG 1.4.13 hoverable / dismissible / persistent model.
#[derive(Default)]
pub struct TooltipExternal {
    /// The trigger (and contiguous tooltip body) is under the pointer.
    hovered: bool,
    /// The trigger holds the shell keyboard focus (mirrored via
    /// [`External::on_focus_change`]).
    focused: bool,
    /// WCAG 1.4.13 dismiss latch — set by `Self::dismiss`, cleared on
    /// the trigger episode's falling edge so a later hover / focus
    /// re-shows the tooltip.
    dismissed: bool,
}

impl TooltipExternal {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the tooltip is currently shown:
    /// `(hovered || focused) && !dismissed`.
    #[must_use]
    pub fn visible(&self) -> bool {
        (self.hovered || self.focused) && !self.dismissed
    }

    /// Pointer-hover posture (trigger or contiguous tooltip body).
    #[must_use]
    pub fn hovered(&self) -> bool {
        self.hovered
    }

    /// Keyboard-focus posture of the trigger.
    #[must_use]
    pub fn focused(&self) -> bool {
        self.focused
    }

    /// Whether the WCAG dismiss latch is currently held.
    #[must_use]
    pub fn dismissed(&self) -> bool {
        self.dismissed
    }

    /// Pointer entered the trigger / contiguous tooltip body.
    fn pointer_enter(&mut self) {
        self.hovered = true;
    }

    /// Pointer left the trigger + tooltip body. Ends the hover; if no
    /// focus independently holds the tooltip open, this is the
    /// trigger-episode falling edge, so clear the dismiss latch (the
    /// next hover / focus shows the tooltip again).
    fn pointer_leave(&mut self) {
        self.hovered = false;
        if !self.focused {
            self.dismissed = false;
        }
    }

    /// WCAG 1.4.13 dismiss — hide the tooltip while hover / focus stays
    /// put. A **no-op when nothing is shown**: "dismiss" means "hide the
    /// currently-shown tooltip", so a stray dismiss (e.g. an RPC
    /// `scene/invoke dismiss` against a hidden tooltip) must not arm the
    /// latch and suppress the *next* hover / focus show. Idempotent.
    fn dismiss(&mut self) {
        if self.visible() {
            self.dismissed = true;
        }
    }

    /// Drive a pointer event by W3C name. `PointerEnter` / `PointerLeave`
    /// move the hover posture; press / cancel events are accepted but
    /// inert (a tooltip is descriptive, not clickable). Unknown names
    /// are rejected so a malformed wire payload is visible to the
    /// caller.
    ///
    /// R51.42 composite-tag aware: when the binding paints the tooltip
    /// body with a `#`-sub-tag of the anchor (so the overlay is hover-
    /// routed to *this* external yet stays independently locatable), the
    /// router rewrites the wire payload to `"<sub>:<EventName>"`. Both
    /// the trigger (`"PointerEnter"`) and the body (`"pop:PointerEnter"`)
    /// are the same hover surface, so the sub-index is dropped — the
    /// tooltip has a single hover posture regardless of which region the
    /// pointer is over.
    fn send(&mut self, name: &str) -> Result<(), InvokeError> {
        // R880.1 — strip the sub-index (and any held-modifier third
        // segment) via the `split_send_payload` `:` grammar SSOT; the
        // hand-rolled split_once would have read "PointerUp:c" as the
        // event name (inert here since only Enter/Leave act, but every
        // send decoder shares one grammar).
        let name = crate::composite_tag::split_send_payload(name).map_or(name, |sent| sent.event);
        match PointerWireEvent::from_wire_name(name) {
            Some(PointerWireEvent::Enter) => self.pointer_enter(),
            Some(PointerWireEvent::Leave) => self.pointer_leave(),
            // Press / release / cancel are inert on a descriptive
            // tooltip but must not error — the router forwards them when
            // the cursor presses on the shared-tag overlay body.
            Some(PointerWireEvent::Down | PointerWireEvent::Up | PointerWireEvent::Cancel) => {}
            None => {
                return Err(InvokeError::rejected(format!(
                    "tooltip.send: {name:?} is not a pointer event name"
                )));
            }
        }
        Ok(())
    }
}

impl core::fmt::Debug for TooltipExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TooltipExternal")
            .field("hovered", &self.hovered)
            .field("focused", &self.focused)
            .field("dismissed", &self.dismissed)
            .field("visible", &self.visible())
            .finish()
    }
}

impl External for TooltipExternal {
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

    /// A tooltip emits no §5.20 intents — it carries no command /
    /// selection / value. Visibility is observed through the introspect
    /// slots, not the intent channel.
    fn drain_intents(&mut self, _sink: &mut dyn FnMut(Intent)) {}

    fn is_dirty(&self) -> bool {
        false
    }

    /// R695 §5.39 — mirror the trigger's keyboard-focus posture. Gaining
    /// focus shows the tooltip (the WCAG 1.4.13 focus trigger); losing
    /// focus with no hover left is the trigger-episode falling edge, so
    /// clear the dismiss latch.
    fn on_focus_change(&mut self, focused: bool) {
        self.focused = focused;
        if !focused && !self.hovered {
            self.dismissed = false;
        }
    }
}

impl ExternalIntrospect for TooltipExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("visible", "bool"),
                    SchemaField::new("hovered", "bool"),
                    SchemaField::new("focused", "bool"),
                    SchemaField::new("dismissed", "bool"),
                    SchemaField::send("string"),
                    SchemaField::action("dismiss", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        match path {
            "visible" => Ok(IntrospectValue::Bool(self.visible())),
            "hovered" => Ok(IntrospectValue::Bool(self.hovered)),
            "focused" => Ok(IntrospectValue::Bool(self.focused)),
            "dismissed" => Ok(IntrospectValue::Bool(self.dismissed)),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        // Posture is observed-only via intervene; the show/hide inputs
        // flow through the `send` (hover) + `dismiss` invoke actions.
        match path {
            "visible" | "hovered" | "focused" | "dismissed" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Hover channel: the router forwards `PointerEnter` /
            // `PointerLeave` here. Returns the resulting `visible`
            // so a caller sees the show/hide outcome in one round-trip.
            "send" => match args {
                IntrospectValue::Text(ref name) => {
                    self.send(name)?;
                    Ok(IntrospectValue::Bool(self.visible()))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // WCAG 1.4.13 dismiss — the shell's `Escape` handler and the
            // RPC `scene/invoke` action both land here (one funnel).
            // Args are ignored; returns the resulting `visible` (false).
            "dismiss" => {
                self.dismiss();
                Ok(IntrospectValue::Bool(self.visible()))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external::External;
    use crate::test_fixtures::assert_refused_saying;

    fn send(t: &mut TooltipExternal, name: &str) {
        t.invoke("send", IntrospectValue::Text(name.to_string()))
            .expect("known pointer event");
    }

    #[test]
    fn new_tooltip_is_hidden() {
        let t = TooltipExternal::new();
        assert!(!t.visible());
        assert!(!t.hovered());
        assert!(!t.focused());
        assert!(!t.dismissed());
    }

    #[test]
    fn hover_shows_then_leave_hides() {
        let mut t = TooltipExternal::new();
        send(&mut t, "PointerEnter");
        assert!(t.hovered());
        assert!(t.visible(), "hover shows the tooltip");
        send(&mut t, "PointerLeave");
        assert!(!t.hovered());
        assert!(!t.visible(), "leaving hides it");
    }

    #[test]
    fn focus_shows_then_blur_hides() {
        let mut t = TooltipExternal::new();
        t.on_focus_change(true);
        assert!(t.focused());
        assert!(
            t.visible(),
            "keyboard focus shows the tooltip (WCAG 1.4.13)"
        );
        t.on_focus_change(false);
        assert!(!t.visible());
    }

    #[test]
    fn hover_and_focus_are_orthogonal_or_visibility() {
        // Either trigger alone shows it; dropping one keeps it visible
        // while the other holds.
        let mut t = TooltipExternal::new();
        send(&mut t, "PointerEnter");
        t.on_focus_change(true);
        assert!(t.visible());
        send(&mut t, "PointerLeave");
        assert!(t.visible(), "focus still holds it open after hover leaves");
        t.on_focus_change(false);
        assert!(!t.visible());
    }

    #[test]
    fn dismiss_hides_while_hover_stays() {
        // WCAG 1.4.13 dismissible: Escape hides without moving the
        // pointer.
        let mut t = TooltipExternal::new();
        send(&mut t, "PointerEnter");
        assert!(t.visible());
        t.invoke("dismiss", IntrospectValue::Null).unwrap();
        assert!(t.dismissed());
        assert!(!t.visible(), "dismiss hides while still hovered");
    }

    #[test]
    fn dismiss_latch_clears_on_leave_so_rehover_reshows() {
        let mut t = TooltipExternal::new();
        send(&mut t, "PointerEnter");
        t.invoke("dismiss", IntrospectValue::Null).unwrap();
        assert!(!t.visible());
        // Leave (episode falling edge) clears the latch.
        send(&mut t, "PointerLeave");
        assert!(
            !t.dismissed(),
            "leaving the trigger clears the dismiss latch"
        );
        // Re-hover shows again.
        send(&mut t, "PointerEnter");
        assert!(t.visible(), "re-hover after dismiss + leave re-shows");
    }

    #[test]
    fn dismiss_latch_clears_on_blur_when_not_hovered() {
        let mut t = TooltipExternal::new();
        t.on_focus_change(true);
        t.invoke("dismiss", IntrospectValue::Null).unwrap();
        assert!(!t.visible());
        t.on_focus_change(false);
        assert!(
            !t.dismissed(),
            "blur with no hover clears the dismiss latch"
        );
        t.on_focus_change(true);
        assert!(t.visible(), "re-focus after dismiss + blur re-shows");
    }

    #[test]
    fn dismiss_latch_persists_while_focus_holds_after_hover_leave() {
        // Hover + focus, dismiss, then drop hover only: focus still
        // holds the episode, so the latch must NOT clear (the tooltip
        // stays dismissed until both triggers release).
        let mut t = TooltipExternal::new();
        send(&mut t, "PointerEnter");
        t.on_focus_change(true);
        t.invoke("dismiss", IntrospectValue::Null).unwrap();
        send(&mut t, "PointerLeave");
        assert!(
            t.dismissed(),
            "focus still holds the episode -> latch persists"
        );
        assert!(!t.visible());
    }

    #[test]
    fn dismiss_while_hidden_is_a_noop_not_a_latent_latch() {
        // A stray dismiss with nothing shown must not arm the latch —
        // that would suppress the next hover / focus show. "Dismiss"
        // means "hide the currently-shown tooltip".
        let mut t = TooltipExternal::new();
        t.invoke("dismiss", IntrospectValue::Null).unwrap();
        assert!(!t.dismissed(), "dismiss with nothing shown is a no-op");
        send(&mut t, "PointerEnter");
        assert!(t.visible(), "the next hover still shows the tooltip");
    }

    #[test]
    fn press_events_are_inert_no_op() {
        let mut t = TooltipExternal::new();
        send(&mut t, "PointerEnter");
        for ev in ["PointerDown", "PointerUp", "PointerCancel"] {
            send(&mut t, ev);
            assert!(t.visible(), "press {ev} does not change tooltip visibility");
        }
    }

    #[test]
    fn composite_sub_tag_send_routes_to_same_hover_posture() {
        // R51.42 — the tooltip body is painted with a `#pop` sub-tag of
        // the anchor; the router rewrites the wire to "pop:PointerEnter".
        // Trigger and body share one hover posture, so the sub-index is
        // dropped.
        let mut t = TooltipExternal::new();
        send(&mut t, "pop:PointerEnter");
        assert!(t.hovered(), "composite-tag enter sets the hover posture");
        assert!(t.visible());
        send(&mut t, "pop:PointerLeave");
        assert!(!t.hovered());
    }

    #[test]
    fn send_unknown_event_is_rejected() {
        let mut t = TooltipExternal::new();
        let r = t.invoke("send", IntrospectValue::Text("Teleport".to_string()));
        assert_refused_saying(&r, "\"Teleport\" is not a pointer event name");
    }

    #[test]
    fn send_wrong_arg_type_is_type_mismatch() {
        let mut t = TooltipExternal::new();
        let r = t.invoke("send", IntrospectValue::Int(1));
        assert_eq!(r, Err(InvokeError::TypeMismatch));
    }

    #[test]
    fn invoke_returns_resulting_visibility() {
        let mut t = TooltipExternal::new();
        assert_eq!(
            t.invoke("send", IntrospectValue::Text("PointerEnter".to_string())),
            Ok(IntrospectValue::Bool(true)),
        );
        assert_eq!(
            t.invoke("dismiss", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(false)),
        );
    }

    #[test]
    fn invoke_unknown_path_is_unknown_path() {
        let mut t = TooltipExternal::new();
        assert_eq!(
            t.invoke("nope", IntrospectValue::Null),
            Err(InvokeError::UnknownPath),
        );
    }

    #[test]
    fn query_reports_every_slot() {
        let mut t = TooltipExternal::new();
        send(&mut t, "PointerEnter");
        assert_eq!(t.query("visible"), Ok(IntrospectValue::Bool(true)));
        assert_eq!(t.query("hovered"), Ok(IntrospectValue::Bool(true)));
        assert_eq!(t.query("focused"), Ok(IntrospectValue::Bool(false)));
        assert_eq!(t.query("dismissed"), Ok(IntrospectValue::Bool(false)));
        assert_eq!(t.query("nope"), Err(ReadRefusal::UnknownPath));
    }

    #[test]
    fn intervene_posture_is_read_only() {
        let mut t = TooltipExternal::new();
        assert_eq!(
            t.intervene("visible", IntrospectValue::Bool(true)),
            Err(InterveneError::ReadOnly),
        );
        assert_eq!(
            t.intervene("nope", IntrospectValue::Null),
            Err(InterveneError::UnknownPath),
        );
    }

    #[test]
    fn emits_no_intents() {
        let mut t = TooltipExternal::new();
        send(&mut t, "PointerEnter");
        assert!(!t.is_dirty());
        let mut got: Vec<Intent> = Vec::new();
        t.drain_intents(&mut |i| got.push(i));
        assert!(got.is_empty());
    }

    #[test]
    fn schema_declares_slots_and_actions() {
        let t = TooltipExternal::new();
        assert_eq!(
            t.schema().fields,
            &[
                SchemaField::new("visible", "bool"),
                SchemaField::new("hovered", "bool"),
                SchemaField::new("focused", "bool"),
                SchemaField::new("dismissed", "bool"),
                SchemaField::send("string"),
                SchemaField::action("dismiss", "string"),
            ]
        );
    }
}
