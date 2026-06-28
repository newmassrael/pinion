//! §5.51.1 R1127 — Dock-panel float/redock lifecycle widget statechart.
//!
//! The discrete `docked ↔ floating` lifecycle (`dock_panel.scxml` →
//! [`DockPanelPolicy`]), bringing the last interactive widget onto the §5.38
//! per-widget-SCXML canon and retiring the imperative `DockPanelExternal` +
//! binding-reducer lifecycle (the §2 #5 drift).
//!
//! Per the Slider "SCXML owns the interaction state; the binding owns the typed
//! value" split (the textbook "null datamodel + typed Rust sidecar"), this module
//! exposes ONLY the statechart vocabulary — the state / event ⇄ SCXML-name
//! mappings. The home-anchor, target panel, drop zone, dock topology, and the
//! floating `WindowSpec` are typed Rust data the BINDING owns (the
//! `pinion-widget-paint` `DockPanelExternal`, a [`Widget<DockPanelPolicy>`]
//! consumer), NOT this chart — the §5.51.1 review found no advanced SCXML feature
//! (`<data>`/`cond`/`<parallel>`/`<history>`/`<invoke>`/event-data) is needed.
//!
//! Events in (the binding sends §5.51 drag-substrate outcomes): `escaped` (a
//! docked drag left the dock), `dropped` (a floater released over a zone),
//! `dock_back` (an explicit keyboard / cancel-to-home dock-back). Events out
//! (`<raise>`d, the binding performs the topology IO via the §5.51.1 data
//! primitives): `dock_panel.float` / `dock_panel.redock` / `dock_panel.restore`.

#[allow(
    non_snake_case,
    unused_imports,
    dead_code,
    unused_variables,
    unused_mut,
    unused_labels,
    unreachable_patterns,
    unreachable_code,
    unused_assignments,
    clippy::style,
    clippy::complexity,
    clippy::pedantic,
    clippy::all
)]
mod sm {
    include!(concat!(env!("OUT_DIR"), "/dock_panel_sm.rs"));
}

pub use sm::DockPanelPolicy;
pub use sm::{DockPanelEvent, DockPanelState};

// §5.16 — DockPanelState ⇄ SCXML-id mapping through the R643 `WidgetStateName`
// SSOT primitive (the External `state().as_name()` introspect + a binding's
// `from_name_or_default` read share one variant list).
// R1137 — `RedockArmed` is a floating SUB-mode (the floater is over a dock zone,
// the binding paints the preview); `is_floating()` is true for both Floating and
// RedockArmed (the panel is floating in both).
crate::widget_state_name!(
    DockPanelState,
    default = Docked,
    [Docked, Floating, RedockArmed,]
);

// §5.16 — DockPanelEvent ⇄ SCXML-name mapping through the `WidgetEventName` SSOT
// primitive. `external` = events the binding SENDS (drag-substrate outcomes;
// R1137 adds `OverZone`/`LeaveZone`, the floater's live enter/leave of a dock
// zone); `internal` = the `<raise>`d semantic outputs + the W3C eventless `Null`.
crate::widget_event_name!(
    DockPanelEvent,
    external = [Escaped, OverZone, LeaveZone, Dropped, DockBack,],
    internal = [DockPanelFloat, DockPanelRedock, DockPanelRestore, Null,],
);

#[cfg(test)]
mod tests {
    use super::{DockPanelEvent, DockPanelPolicy, DockPanelState};
    use crate::widgets::Widget;

    #[test]
    fn r1127_dock_panel_lifecycle_transitions() {
        let mut w: Widget<DockPanelPolicy> = Widget::new();
        assert!(
            matches!(w.state(), DockPanelState::Docked),
            "initial = docked"
        );
        // escaped: a docked drag left the dock surface → floating (tear-off).
        w.send(DockPanelEvent::Escaped);
        assert!(matches!(w.state(), DockPanelState::Floating));
        // dropped: the floater released over a zone → docked (redock-at-zone).
        w.send(DockPanelEvent::Dropped);
        assert!(matches!(w.state(), DockPanelState::Docked));
        // escaped again, then dock_back: floating → docked (restore-home).
        w.send(DockPanelEvent::Escaped);
        assert!(matches!(w.state(), DockPanelState::Floating));
        w.send(DockPanelEvent::DockBack);
        assert!(matches!(w.state(), DockPanelState::Docked));
    }

    #[test]
    fn r1127_redock_events_are_inert_while_docked() {
        // A docked panel has nothing to redock: `dropped` / `dock_back` (the
        // floating-only transitions) leave it docked — the chart enforces the
        // valid lifecycle, so the binding cannot redock a panel that never floated.
        let mut w: Widget<DockPanelPolicy> = Widget::new();
        w.send(DockPanelEvent::Dropped);
        assert!(matches!(w.state(), DockPanelState::Docked));
        w.send(DockPanelEvent::DockBack);
        assert!(matches!(w.state(), DockPanelState::Docked));
    }

    #[test]
    fn r1137_floater_arms_over_a_zone_and_redocks_only_while_armed() {
        // R1137 — the holistic redesign: a floater ARMS over a dock zone and
        // DISARMS when it leaves; a `dropped` while armed re-docks; a floater that
        // is `floating` (free) and never armed stays floating on a stray move.
        let mut w: Widget<DockPanelPolicy> = Widget::new();
        w.send(DockPanelEvent::Escaped); // docked → floating
        assert!(matches!(w.state(), DockPanelState::Floating));
        // over a dock zone → armed.
        w.send(DockPanelEvent::OverZone);
        assert!(
            matches!(w.state(), DockPanelState::RedockArmed),
            "over a zone arms"
        );
        // left the zone → free floating again.
        w.send(DockPanelEvent::LeaveZone);
        assert!(
            matches!(w.state(), DockPanelState::Floating),
            "leaving a zone disarms"
        );
        // re-arm, then drop while armed → docked (redock).
        w.send(DockPanelEvent::OverZone);
        w.send(DockPanelEvent::Dropped);
        assert!(
            matches!(w.state(), DockPanelState::Docked),
            "drop while armed redocks"
        );
        // arm-events are inert while docked (the chart enforces the lifecycle).
        w.send(DockPanelEvent::OverZone);
        assert!(
            matches!(w.state(), DockPanelState::Docked),
            "over_zone inert while docked"
        );
    }

    #[test]
    fn r1137_dock_back_while_armed_restores_home() {
        // An explicit dock-back (cancel) while armed returns the floater home.
        let mut w: Widget<DockPanelPolicy> = Widget::new();
        w.send(DockPanelEvent::Escaped);
        w.send(DockPanelEvent::OverZone);
        assert!(matches!(w.state(), DockPanelState::RedockArmed));
        w.send(DockPanelEvent::DockBack);
        assert!(
            matches!(w.state(), DockPanelState::Docked),
            "dock_back while armed restores"
        );
    }
}
