//! R772 §5.53 §5.38 — `ContextMenu`: the right-click command popup.
//!
//! Per R771.1 (§5.53), pinion's own scene-graph menu renderer is the
//! 1st-class menu surface on every platform. A context menu is the
//! second consumer of the R691 command-menu contract: a single floating
//! list of one-shot command items that activating dismisses, emitting a
//! `"command"` intent. The *only* axis a context menu adds over a
//! [`MenuBar`](super::menu) dropdown is the **arbitrary cursor anchor**
//! — a menubar dropdown anchors below its title at a deterministic `x`;
//! a context menu opens wherever the secondary-button press landed.
//!
//! Everything else is shared substrate, reused rather than recopied:
//! the active-item keyboard navigation (`super::menu_nav`, lifted at
//! this second consumer), the floating-panel paint
//! (`pinion_widget_paint::view_menu_dropdown`), and the R715
//! click-outside dismiss barrier (`pinion_widget_paint::barrier`).
//!
//! ## State model
//!
//! - `open_at: Option<(f32, f32)>` — the window-space anchor when open,
//!   `None` when closed. Opening is positional (unlike a menubar, whose
//!   open state is a menu *index*), so the anchor lives in the model and
//!   is queryable (`open` / `open_x` / `open_y`) for AI introspection.
//! - `active: Option<usize>` — the highlighted item (WAI-ARIA active
//!   descendant); `None` right after a pointer open (pointer ergonomics).
//!
//! ## Command class (WAI-ARIA §3.5)
//!
//! Like [`MenuBar`](super::menu), a context-menu item is a `menuitem`
//! one-shot command — neither `aria-selected` nor `aria-checked`.
//! Activation emits a `"command"` intent carrying the item index and
//! dismisses the popup. Stateful entries (`menuitemcheckbox` /
//! `menuitemradio`) and submenus are additive future axes.

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use crate::input::PointerWireEvent;
use crate::intent::Intent;
use crate::widgets::IntentEmitter;
use crate::widgets::menu_nav;
use crate::widgets::wire::resolve_index;

/// R772 §5.38 — a logical right-click command popup. See module docs for
/// the command-vs-selection rationale and the state model.
pub struct ContextMenu {
    /// Number of command items.
    item_count: usize,
    /// Window-space anchor when open, `None` when closed.
    open_at: Option<(f32, f32)>,
    /// Highlighted item (active descendant), or `None`.
    active: Option<usize>,
}

impl ContextMenu {
    /// Construct a closed context menu carrying `item_count` command
    /// items.
    #[must_use]
    pub fn new(item_count: usize) -> Self {
        Self {
            item_count,
            open_at: None,
            active: None,
        }
    }

    /// Number of command items.
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.item_count
    }

    /// `true` while the popup is open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open_at.is_some()
    }

    /// The window-space anchor when open, or `None`.
    #[must_use]
    pub fn open_at(&self) -> Option<(f32, f32)> {
        self.open_at
    }

    /// The highlighted item, or `None`.
    #[must_use]
    pub fn active_item(&self) -> Option<usize> {
        self.active
    }

    /// Open (or re-anchor) the popup at window-space `(x, y)` with no
    /// item pre-highlighted (pointer ergonomics).
    fn open(&mut self, x: f32, y: f32) {
        self.open_at = Some((x, y));
        self.active = None;
    }

    /// Close the popup, clearing the highlight.
    fn close(&mut self) {
        self.open_at = None;
        self.active = None;
    }

    /// Mouse: highlight item `i` (hover). No-op when closed or `i` is
    /// out of range.
    fn hover_item(&mut self, i: usize) {
        if self.is_open() && i < self.item_count {
            self.active = Some(i);
        }
    }

    /// Activate item `i`: returns `Some(i)` and closes the popup when
    /// valid (open and in range), `None` otherwise. The caller (the
    /// [`ContextMenuExternal`] adapter) emits the `"command"` intent.
    fn activate_item(&mut self, i: usize) -> Option<usize> {
        if !self.is_open() || i >= self.item_count {
            return None;
        }
        self.close();
        Some(i)
    }

    /// Keyboard (open): move the highlight by one with wrap; from no
    /// highlight Down lands on the first item and Up on the last.
    fn move_active(&mut self, forward: bool) {
        if self.is_open() {
            self.active = menu_nav::nav_move(self.active, self.item_count, forward);
        }
    }

    /// Keyboard (open): jump the highlight to the first / last item.
    fn active_edge(&mut self, last: bool) {
        if self.is_open() {
            if let Some(a) = menu_nav::nav_edge(self.item_count, last) {
                self.active = Some(a);
            }
        }
    }
}

impl Default for ContextMenu {
    /// Default constructs an empty, closed context menu. Applications
    /// pass a concrete `item_count` to [`ContextMenu::new`]; `Default`
    /// exists for the `IntentEmitter<W: Default>` bound.
    fn default() -> Self {
        Self::new(0)
    }
}

/// R772 §5.15 §5.38 — the [`External`] adapter that exposes a
/// [`ContextMenu`] on the §5.12 RPC plane and emits a `"command"` intent
/// on activation (the §5.20 channel, identical wire-form to
/// [`super::menu::MenuBarExternal`]).
pub struct ContextMenuExternal {
    em: IntentEmitter<ContextMenu>,
}

impl ContextMenuExternal {
    /// Construct with `item_count` command items, closed.
    #[must_use]
    pub fn new(item_count: usize) -> Self {
        Self {
            em: IntentEmitter::new(ContextMenu::new(item_count)),
        }
    }

    /// R888.1 §5.53 — encode the `invoke("open_at", …)` wire payload
    /// for a window-space point: the inverse of
    /// `Self::dispatch_open_at`'s `"<x>,<y>"` parse, kept adjacent
    /// so the separator/format decision has one home (encode/decode
    /// pair completeness — pre-R888.1 the `format!("{x},{y}")` encode
    /// was hand-rolled at two `apply_secondary_click` call sites
    /// against this decoder).
    #[must_use]
    pub fn open_at_args(x: f32, y: f32) -> IntrospectValue {
        IntrospectValue::Text(format!("{x},{y}"))
    }

    /// Number of command items.
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.em.inner.item_count()
    }

    /// `true` while the popup is open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.em.inner.is_open()
    }

    /// The window-space anchor when open, or `None`.
    #[must_use]
    pub fn open_at(&self) -> Option<(f32, f32)> {
        self.em.inner.open_at()
    }

    /// The highlighted item, or `None`.
    #[must_use]
    pub fn active_item(&self) -> Option<usize> {
        self.em.inner.active_item()
    }

    /// Drive an item pointer event. `PointerEnter` highlights;
    /// `PointerUp` activates (emitting `"command"`).
    fn send_item(&mut self, i: usize, event: PointerWireEvent) {
        match event {
            PointerWireEvent::Enter => self.em.inner.hover_item(i),
            PointerWireEvent::Up => {
                if let Some(item) = self.em.inner.activate_item(i) {
                    self.emit_command(item);
                }
            }
            PointerWireEvent::Down | PointerWireEvent::Leave | PointerWireEvent::Cancel => {}
        }
    }

    /// Apply a W3C key name through the WAI-ARIA §3.5 menu keyboard
    /// model. Returns `true` when handled so the binding reports the
    /// right swallow verdict; a closed popup ignores every key.
    fn apply_key(&mut self, key: &str) -> bool {
        if !self.is_open() {
            return false;
        }
        match key {
            "ArrowDown" => {
                self.em.inner.move_active(true);
                true
            }
            "ArrowUp" => {
                self.em.inner.move_active(false);
                true
            }
            "Home" => {
                self.em.inner.active_edge(false);
                true
            }
            "End" => {
                self.em.inner.active_edge(true);
                true
            }
            "Enter" | "Space" => {
                if let Some(i) = self.em.inner.active_item() {
                    if let Some(item) = self.em.inner.activate_item(i) {
                        self.emit_command(item);
                    }
                }
                true
            }
            "Escape" => {
                self.em.inner.close();
                true
            }
            _ => false,
        }
    }

    /// Push the `"command"` intent for an activated item (payload =
    /// the item index, mirroring the menubar's `"command"` channel).
    fn emit_command(&mut self, item: usize) {
        self.em.push(Intent::new_static(
            "command",
            IntrospectValue::Text(item.to_string()),
        ));
    }

    /// Shared parse for the `send` wire payload `"<sub>:<EventName>"`
    /// where `<sub>` is `i<i>` (item) or the literal `barrier` (R715
    /// click-outside dismiss). Returns the open flag as the round-trip
    /// outcome.
    fn dispatch_send(&mut self, payload: &str) -> Result<IntrospectValue, InvokeError> {
        // R880.1 — decode via the `split_send_payload` `:` grammar SSOT so a
        // modifier-held release ("barrier:PointerUp:c") still parses and a
        // Ctrl+click outside the popup dismisses it (the hand-rolled
        // split_once read "PointerUp:c" as the event name).
        let (sub, event_name, _mods) =
            crate::composite_tag::require_send_payload("context_menu.send", payload)?;
        let event = PointerWireEvent::from_wire_name(event_name).ok_or_else(|| {
            InvokeError::rejected(format!(
                "context_menu.send: {event_name:?} is not a pointer event name"
            ))
        })?;
        // R715 §5.16 — the transparent dismiss barrier painted behind the
        // popup: a `PointerUp` outside the panel closes it. Other pointer
        // events over the barrier are inert.
        if sub == "barrier" {
            if event == PointerWireEvent::Up {
                self.em.inner.close();
            }
            return Ok(IntrospectValue::Bool(self.is_open()));
        }
        let mut chars = sub.chars();
        let kind = chars.next().ok_or_else(|| {
            InvokeError::rejected(
                "context_menu.send: empty target (expected \"i<index>\" or \"barrier\")",
            )
        })?;
        let rest = chars.as_str();
        let idx: usize = rest.parse().map_err(|_| {
            InvokeError::rejected(format!(
                "context_menu.send: target {sub:?} carries no item index ({rest:?} is not a number)"
            ))
        })?;
        match kind {
            'i' => self.send_item(idx, event),
            _ => {
                return Err(InvokeError::rejected(format!(
                    "context_menu.send: target {sub:?} names no sub-element \
                     (expected \"i<index>\" or \"barrier\")"
                )));
            }
        }
        Ok(IntrospectValue::Bool(self.is_open()))
    }

    /// Parse the `open_at` wire payload `"<x>,<y>"` (window-space floats)
    /// and open the popup there. Returns the open flag.
    fn dispatch_open_at(&mut self, payload: &str) -> Result<IntrospectValue, InvokeError> {
        let (xs, ys) = payload.split_once(',').ok_or_else(|| {
            InvokeError::rejected(format!(
                "context_menu.open_at: malformed argument {payload:?} (expected \"<x>,<y>\")"
            ))
        })?;
        let x: f32 = xs.trim().parse().map_err(|_| {
            InvokeError::rejected(format!("context_menu.open_at: x {xs:?} is not a number"))
        })?;
        let y: f32 = ys.trim().parse().map_err(|_| {
            InvokeError::rejected(format!("context_menu.open_at: y {ys:?} is not a number"))
        })?;
        self.em.inner.open(x, y);
        Ok(IntrospectValue::Bool(self.is_open()))
    }
}

impl Default for ContextMenuExternal {
    fn default() -> Self {
        Self::new(0)
    }
}

impl core::fmt::Debug for ContextMenuExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ContextMenuExternal")
            .field("item_count", &self.item_count())
            .field("open_at", &self.open_at())
            .field("active", &self.active_item())
            .finish()
    }
}

impl External for ContextMenuExternal {
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

impl ExternalIntrospect for ContextMenuExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("item_count", "int"),
                    SchemaField::new("open", "bool"),
                    SchemaField::new("open_x", "float"),
                    SchemaField::new("open_y", "float"),
                    SchemaField::new("active", "int"),
                    SchemaField::new("send", "string"),
                    SchemaField::new("key", "string"),
                    SchemaField::new("open_at", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "item_count" => Some(IntrospectValue::Int(
                i64::try_from(self.item_count()).expect("item_count fits in i64"),
            )),
            "open" => Some(IntrospectValue::Bool(self.is_open())),
            "open_x" => Some(self.open_at().map_or(IntrospectValue::Null, |(x, _)| {
                IntrospectValue::Float(f64::from(x))
            })),
            "open_y" => Some(self.open_at().map_or(IntrospectValue::Null, |(_, y)| {
                IntrospectValue::Float(f64::from(y))
            })),
            "active" => Some(self.active_item().map_or(IntrospectValue::Null, |a| {
                IntrospectValue::Int(i64::try_from(a).expect("active index fits in i64"))
            })),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "item_count" | "open_x" | "open_y" => Err(InterveneError::ReadOnly),
            // Programmatic close (RPC restore / dismiss) without firing a
            // `"command"` intent. Opening needs an anchor — use the
            // `open_at` invoke, which carries the coordinates.
            "open" => match value {
                IntrospectValue::Bool(false) => {
                    self.em.inner.close();
                    Ok(())
                }
                IntrospectValue::Bool(true) => Err(InterveneError::out_of_range(
                    "a context menu opens at a POINT, so `open` can only be \
                     written false; use invoke open_at \"<x>,<y>\"",
                )),
                _ => Err(InterveneError::TypeMismatch),
            },
            "active" => match value {
                IntrospectValue::Int(i) => {
                    if !self.is_open() {
                        return Err(InterveneError::out_of_range(
                            "no item is highlightable while the popup is closed",
                        ));
                    }
                    let item = resolve_index("item", i, self.item_count())?;
                    self.em.inner.active = Some(item);
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.em.inner.active = None;
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
            // Mouse: "i<i>:<Event>" / "barrier:<Event>". Returns open flag.
            "send" => match args {
                IntrospectValue::Text(ref s) => self.dispatch_send(s),
                _ => Err(InvokeError::TypeMismatch),
            },
            // Keyboard: a W3C key name through the WAI-ARIA menu model.
            // Returns Bool(handled) for the binding's swallow verdict.
            "key" => match args {
                IntrospectValue::Text(ref name) => Ok(IntrospectValue::Bool(self.apply_key(name))),
                _ => Err(InvokeError::TypeMismatch),
            },
            // Open at a window-space point: "<x>,<y>". Returns open flag.
            "open_at" => match args {
                IntrospectValue::Text(ref s) => self.dispatch_open_at(s),
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// R986 §5.53 — project a [`ContextMenuExternal`]'s public UI state — the
/// window-space open anchor and the active (highlighted) item — off its
/// introspect handle. The canonical reader both the first consumer
/// (`hello-contextmenu`, where the menu is the *primary* external reached
/// through `Scene::External`) and a host that composes the menu as an
/// *extra* external (`hello-grid-header-menu`, reached through
/// [`Scene::find_external_with_tag`](crate::scene::Scene::find_external_with_tag))
/// share — the menu peer of
/// [`read_selected`](super::virtual_select::read_selected). `open` gates
/// the anchor (a closed menu reports `None`); `active` is the highlight,
/// `None` when nothing is highlighted.
#[must_use]
pub fn read_open_state(intro: &dyn ExternalIntrospect) -> (Option<(f32, f32)>, Option<usize>) {
    let open_at = if matches!(intro.query("open"), Some(IntrospectValue::Bool(true))) {
        Some((
            query_anchor_axis(intro, "open_x"),
            query_anchor_axis(intro, "open_y"),
        ))
    } else {
        None
    };
    let active = match intro.query("active") {
        Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
        _ => None,
    };
    (open_at, active)
}

/// Read one window-space anchor axis (`open_x` / `open_y`) back to `f32`.
/// The slot was lowered from an `f32` anchor (`f64::from(x)` in
/// [`ContextMenuExternal`]'s `query`), so the narrowing back is lossless.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the slot round-trips an f32 anchor, so f64 -> f32 is exact"
)]
fn query_anchor_axis(intro: &dyn ExternalIntrospect, path: &str) -> f32 {
    match intro.query(path) {
        Some(IntrospectValue::Float(v)) => v as f32,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::assert_out_of_range_saying;

    fn menu() -> ContextMenu {
        ContextMenu::new(4)
    }

    fn ext() -> ContextMenuExternal {
        ContextMenuExternal::new(4)
    }

    // ----- model: initial / open / close -----

    #[test]
    fn new_context_menu_is_closed() {
        let m = menu();
        assert_eq!(m.item_count(), 4);
        assert!(!m.is_open());
        assert_eq!(m.open_at(), None);
        assert_eq!(m.active_item(), None);
    }

    #[test]
    fn open_sets_anchor_no_highlight() {
        let mut m = menu();
        m.open(120.0, 64.0);
        assert!(m.is_open());
        assert_eq!(m.open_at(), Some((120.0, 64.0)));
        assert_eq!(m.active_item(), None, "pointer open pre-highlights nothing");
    }

    #[test]
    fn reopen_reanchors_and_clears_highlight() {
        let mut m = menu();
        m.open(10.0, 10.0);
        m.hover_item(2);
        m.open(200.0, 50.0);
        assert_eq!(m.open_at(), Some((200.0, 50.0)));
        assert_eq!(m.active_item(), None);
    }

    #[test]
    fn close_clears_anchor_and_highlight() {
        let mut m = menu();
        m.open(10.0, 10.0);
        m.hover_item(1);
        m.close();
        assert!(!m.is_open());
        assert_eq!(m.active_item(), None);
    }

    // ----- model: hover + activate -----

    #[test]
    fn hover_when_closed_is_noop() {
        let mut m = menu();
        m.hover_item(2);
        assert_eq!(m.active_item(), None);
    }

    #[test]
    fn hover_out_of_range_is_noop() {
        let mut m = menu();
        m.open(0.0, 0.0);
        m.hover_item(9);
        assert_eq!(m.active_item(), None);
    }

    #[test]
    fn activate_closes_and_returns_index() {
        let mut m = menu();
        m.open(0.0, 0.0);
        assert_eq!(m.activate_item(2), Some(2));
        assert!(!m.is_open(), "activation dismisses the popup");
    }

    #[test]
    fn activate_when_closed_is_none() {
        let mut m = menu();
        assert_eq!(m.activate_item(0), None);
    }

    #[test]
    fn activate_out_of_range_is_none() {
        let mut m = menu();
        m.open(0.0, 0.0);
        assert_eq!(m.activate_item(9), None);
        assert!(m.is_open(), "an invalid activation leaves the popup open");
    }

    // ----- model: keyboard navigation -----

    #[test]
    fn arrow_down_from_none_lands_first_then_wraps() {
        let mut m = menu();
        m.open(0.0, 0.0);
        m.move_active(true);
        assert_eq!(m.active_item(), Some(0));
        for _ in 0..3 {
            m.move_active(true);
        }
        assert_eq!(m.active_item(), Some(3));
        m.move_active(true);
        assert_eq!(m.active_item(), Some(0), "Down wraps past the last item");
    }

    #[test]
    fn arrow_up_from_none_lands_last() {
        let mut m = menu();
        m.open(0.0, 0.0);
        m.move_active(false);
        assert_eq!(m.active_item(), Some(3));
    }

    #[test]
    fn home_end_jump_to_edges() {
        let mut m = menu();
        m.open(0.0, 0.0);
        m.active_edge(true);
        assert_eq!(m.active_item(), Some(3));
        m.active_edge(false);
        assert_eq!(m.active_item(), Some(0));
    }

    #[test]
    fn keyboard_nav_when_closed_is_noop() {
        let mut m = menu();
        m.move_active(true);
        m.active_edge(true);
        assert_eq!(m.active_item(), None);
    }

    // ----- external: command intent + wire -----

    fn drain(ext: &mut ContextMenuExternal) -> Vec<Intent> {
        let mut out = Vec::new();
        ext.drain_intents(&mut |i| out.push(i));
        out
    }

    #[test]
    fn pointer_activate_emits_command_intent() {
        let mut e = ext();
        e.dispatch_open_at("100,80").unwrap();
        e.send_item(2, PointerWireEvent::Enter);
        assert_eq!(e.active_item(), Some(2));
        e.send_item(2, PointerWireEvent::Up);
        let intents = drain(&mut e);
        assert_eq!(intents.len(), 1, "exactly one command intent");
        assert_eq!(intents[0].tag_str(), "command");
        assert_eq!(intents[0].payload, IntrospectValue::Text("2".to_string()));
        assert!(!e.is_open(), "activation dismissed the popup");
    }

    #[test]
    fn keyboard_activate_emits_command_intent() {
        let mut e = ext();
        e.dispatch_open_at("0,0").unwrap();
        assert!(e.apply_key("ArrowDown"));
        assert!(e.apply_key("Enter"));
        let intents = drain(&mut e);
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].payload, IntrospectValue::Text("0".to_string()));
    }

    #[test]
    fn escape_closes_without_command() {
        let mut e = ext();
        e.dispatch_open_at("0,0").unwrap();
        assert!(e.apply_key("Escape"));
        assert!(!e.is_open());
        assert!(drain(&mut e).is_empty(), "Escape fires no command");
    }

    #[test]
    fn barrier_pointer_up_dismisses() {
        let mut e = ext();
        e.dispatch_open_at("0,0").unwrap();
        let r = e.dispatch_send("barrier:PointerUp").unwrap();
        assert_eq!(r, IntrospectValue::Bool(false));
        assert!(!e.is_open());
    }

    #[test]
    fn r880_1_barrier_dismiss_survives_a_held_modifier() {
        // "barrier:PointerUp:c" (the R781 modifier segment) must still
        // dismiss — the pre-R880.1 hand-rolled split read "PointerUp:c" as
        // the event name and a Ctrl+click outside the popup left it open.
        let mut e = ext();
        e.dispatch_open_at("0,0").unwrap();
        let r = e.dispatch_send("barrier:PointerUp:c").unwrap();
        assert_eq!(r, IntrospectValue::Bool(false));
        assert!(!e.is_open());
    }

    #[test]
    fn key_when_closed_reports_unhandled() {
        let mut e = ext();
        assert!(!e.apply_key("ArrowDown"), "closed popup swallows nothing");
    }

    // ----- external: introspection -----

    #[test]
    fn query_reports_open_anchor_and_active() {
        let mut e = ext();
        assert_eq!(e.query("open"), Some(IntrospectValue::Bool(false)));
        assert_eq!(e.query("open_x"), Some(IntrospectValue::Null));
        e.dispatch_open_at("150,90").unwrap();
        assert_eq!(e.query("open"), Some(IntrospectValue::Bool(true)));
        assert_eq!(e.query("open_x"), Some(IntrospectValue::Float(150.0)));
        assert_eq!(e.query("open_y"), Some(IntrospectValue::Float(90.0)));
        assert_eq!(e.query("item_count"), Some(IntrospectValue::Int(4)));
        assert_eq!(e.query("active"), Some(IntrospectValue::Null));
    }

    #[test]
    fn intervene_active_highlights_without_command() {
        let mut e = ext();
        e.dispatch_open_at("0,0").unwrap();
        e.intervene("active", IntrospectValue::Int(3)).unwrap();
        assert_eq!(e.active_item(), Some(3));
        assert!(
            drain(&mut e).is_empty(),
            "intervene restore fires no command"
        );
    }

    #[test]
    fn intervene_active_out_of_range_rejected() {
        let mut e = ext();
        e.dispatch_open_at("0,0").unwrap();
        assert_out_of_range_saying(
            &e.intervene("active", IntrospectValue::Int(9)),
            "no item 9 here",
        );
    }

    #[test]
    fn intervene_open_false_closes() {
        let mut e = ext();
        e.dispatch_open_at("0,0").unwrap();
        e.intervene("open", IntrospectValue::Bool(false)).unwrap();
        assert!(!e.is_open());
    }

    #[test]
    fn intervene_open_true_rejected_needs_anchor() {
        let mut e = ext();
        assert_out_of_range_saying(
            &e.intervene("open", IntrospectValue::Bool(true)),
            "a context menu opens at a POINT",
        );
    }

    #[test]
    fn open_at_invoke_round_trips() {
        let mut e = ext();
        let r = e.invoke("open_at", IntrospectValue::Text("42,24".to_string()));
        assert_eq!(r, Ok(IntrospectValue::Bool(true)));
        assert_eq!(e.open_at(), Some((42.0, 24.0)));
    }

    #[test]
    fn read_open_state_projects_anchor_and_active() {
        let mut e = ext();
        assert_eq!(
            read_open_state(&e),
            (None, None),
            "closed -> no anchor, no active"
        );
        e.dispatch_open_at("150,90").unwrap();
        assert_eq!(
            read_open_state(&e),
            (Some((150.0, 90.0)), None),
            "open with no highlight",
        );
        e.intervene("active", IntrospectValue::Int(2)).unwrap();
        assert_eq!(
            read_open_state(&e),
            (Some((150.0, 90.0)), Some(2)),
            "open + active"
        );
    }
}
