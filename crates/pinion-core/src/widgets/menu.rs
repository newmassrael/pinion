//! R691 §5.38 §5.40 — `MenuBar` widget: command-class menubar +
//! dropdown menus.
//!
//! Phase B widget-catalog entry — the editor `File` / `Edit` / `View`
//! menubar primitive (every pro DCC / IDE / CAD tool ships one). A
//! `MenuBar` owns N top-level menus, each carrying a flat list of
//! command items; exactly one dropdown is open at a time.
//!
//! ## Command-class, not selection (the R690 audit finding)
//!
//! A menu item is a **one-shot command** — clicking "Save" fires the
//! Save action and closes the menu. WAI-ARIA 1.2 §3.5 models this with
//! the `menuitem` role, which carries *neither* `aria-selected` (the
//! `tab` / `option` selection axis) *nor* `aria-checked` (the `radio` /
//! `checkbox` toggle axis). The stateful menu variants
//! (`menuitemcheckbox` / `menuitemradio`) are separate roles.
//!
//! This is why `MenuBar` does **not** reuse
//! [`RadioGroupExternal`](crate::widgets::radio_group::RadioGroupExternal)
//! the way the R690 `Tabs` substrate does: tabs are "select 1 of N"
//! (a radio group at heart), but a menubar has no persistent selected
//! item — activating an item emits a `"command"` intent and dismisses
//! the dropdown. The state a menubar *does* hold is structural (which
//! dropdown is open, which item the keyboard cursor highlights), not a
//! selection.
//!
//! ## State model
//!
//! - `open: Option<usize>` — the open dropdown's menu index (`None`
//!   when every menu is closed).
//! - `active: Option<usize>` — the highlighted item within the open
//!   menu (the WAI-ARIA active descendant; set by hover or Arrow keys).
//! - `bar_focus: usize` — the focused top-level title for keyboard
//!   navigation. Invariant: `open == Some(m)` ⟹ `bar_focus == m`.
//!
//! Opening via mouse click leaves `active == None` (no item
//! pre-highlighted, matching pointer ergonomics); opening via the
//! keyboard (Arrow Down / Enter / Space) sets `active == Some(0)` (the
//! first item, matching WAI-ARIA §3.5 keyboard activation).
//!
//! ## Wire surface
//!
//! The §5.12 introspect surface drives the menubar identically from
//! the human cursor, the keyboard, and an RPC headless client
//! (§2 invariant #2):
//!
//! * `invoke("send", "t<m>:<PointerEvent>")` — a top-level **title**
//!   pointer event (composite tag `<bar>#t<m>`). Only `PointerUp`
//!   toggles menu `m` open/closed.
//! * `invoke("send", "i<i>:<PointerEvent>")` — a dropdown **item**
//!   pointer event (composite tag `<bar>#i<i>`). `PointerEnter`
//!   highlights item `i`; `PointerUp` activates it (emits `"command"`,
//!   closes).
//! * `invoke("key", "<W3CKeyName>")` — the full WAI-ARIA §3.5 menubar
//!   keyboard model in one place (Arrow Left/Right between titles,
//!   Arrow Up/Down + Home/End within an open menu, Arrow Right/Left to
//!   the adjacent menu, Enter/Space to activate, Escape to close).
//!   Returns [`IntrospectValue::Bool`] = "did this key do anything"
//!   so the binding's `apply_key` reports the right swallow verdict.
//!
//! On item activation the §5.20 channel emits a `"command"` intent
//! whose payload is [`IntrospectValue::Text`] `"<menu>.<item>"`; the
//! runtime walk prefixes the scene-side `ExternalNode` tag (e.g.
//! `"menu"` → `"menu.command"`) so widget identity stays decoupled
//! from the UI-chosen tag, exactly like `Button::"click"`.
//!
//! ## Dismiss paths + future axes (per [[abstraction-needs-second-consumer]])
//!
//! R691 dismisses an open menu via **Escape**, **re-clicking the open
//! title**, or **activating an item**. *Click-outside-to-dismiss* and
//! *focus-loss dismiss* are a cross-widget overlay-dismiss substrate
//! axis (it also serves tooltips / popovers / context menus / combo
//! boxes) — deferred until a 2nd overlay consumer surfaces the need
//! rather than special-cased into the menubar now. Likewise *mouse
//! hover-follow between top-level titles while a menu is open*,
//! *`menuitemcheckbox` / `menuitemradio` stateful entries*, *submenus
//! (nested menus)*, and *accelerator / mnemonic keys* are additive
//! axes once a real consumer needs them.

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::IntentEmitter;

/// Pointer events a menubar title / item accepts over the `send` wire.
/// Mirrors the [`crate::widgets::radio::RadioEvent`] pointer subset so
/// the composite-tag router (`<bar>#t<m>` / `<bar>#i<i>`) feeds the
/// same `PointerEnter → Down → Up → Leave` cycle a real cursor emits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PointerEvent {
    Enter,
    Down,
    Up,
    Leave,
    Cancel,
}

fn parse_pointer_event(name: &str) -> Option<PointerEvent> {
    match name {
        "PointerEnter" => Some(PointerEvent::Enter),
        "PointerDown" => Some(PointerEvent::Down),
        "PointerUp" => Some(PointerEvent::Up),
        "PointerLeave" => Some(PointerEvent::Leave),
        "PointerCancel" => Some(PointerEvent::Cancel),
        _ => None,
    }
}

/// R691 §5.38 — logical menubar with N command menus. See module docs
/// for the command-vs-selection rationale and the state model.
pub struct MenuBar {
    /// Item count per top-level menu. `items_per_menu.len()` is the
    /// number of top-level titles.
    items_per_menu: Vec<usize>,
    /// Open dropdown menu index, or `None` when every menu is closed.
    open: Option<usize>,
    /// Highlighted item within the open menu (the WAI-ARIA active
    /// descendant), or `None`.
    active: Option<usize>,
    /// Focused top-level title for keyboard navigation. Invariant:
    /// `open == Some(m)` ⟹ `bar_focus == m`.
    bar_focus: usize,
}

impl MenuBar {
    /// Construct a menubar whose menu `m` carries `items_per_menu[m]`
    /// command items. All menus start closed.
    #[must_use]
    pub fn new(items_per_menu: Vec<usize>) -> Self {
        Self {
            items_per_menu,
            open: None,
            active: None,
            bar_focus: 0,
        }
    }

    /// Number of top-level menus.
    #[must_use]
    pub fn menu_count(&self) -> usize {
        self.items_per_menu.len()
    }

    /// Item count of menu `m`, or `None` when `m` is out of range.
    #[must_use]
    pub fn item_count(&self, menu: usize) -> Option<usize> {
        self.items_per_menu.get(menu).copied()
    }

    /// The open dropdown's menu index, or `None`.
    #[must_use]
    pub fn open_menu(&self) -> Option<usize> {
        self.open
    }

    /// The highlighted item within the open menu, or `None`.
    #[must_use]
    pub fn active_item(&self) -> Option<usize> {
        self.active
    }

    /// The focused top-level title (keyboard cursor).
    #[must_use]
    pub fn bar_focus(&self) -> usize {
        self.bar_focus
    }

    /// Mouse: toggle menu `m`. Re-toggling the open menu closes it;
    /// toggling a different (or closed) menu opens `m` with no item
    /// pre-highlighted (pointer ergonomics). Out-of-range `m` is a
    /// no-op.
    fn toggle_title(&mut self, m: usize) {
        if m >= self.menu_count() {
            return;
        }
        if self.open == Some(m) {
            self.close();
        } else {
            self.open = Some(m);
            self.active = None;
            self.bar_focus = m;
        }
    }

    /// Mouse: highlight item `i` of the open menu (hover). No-op when
    /// no menu is open or `i` is out of range.
    fn hover_item(&mut self, i: usize) {
        if let Some(m) = self.open {
            if i < self.items_per_menu[m] {
                self.active = Some(i);
            }
        }
    }

    /// Activate item `i` of the open menu: returns `Some((menu, item))`
    /// and closes the menu when valid, `None` otherwise. The caller
    /// (the [`MenuBarExternal`] adapter) emits the `"command"` intent
    /// from the returned coordinates.
    fn activate_item(&mut self, i: usize) -> Option<(usize, usize)> {
        let m = self.open?;
        if i >= self.items_per_menu[m] {
            return None;
        }
        self.close();
        self.bar_focus = m;
        Some((m, i))
    }

    /// Close every menu. `bar_focus` is preserved (Escape returns the
    /// keyboard cursor to the title that was open).
    fn close(&mut self) {
        self.open = None;
        self.active = None;
    }

    /// Keyboard (closed): move the top-level focus by `delta` with
    /// wrap-around. No menu opens.
    fn bar_nav(&mut self, forward: bool) {
        let n = self.menu_count();
        if n == 0 {
            return;
        }
        self.bar_focus = step(self.bar_focus, forward, n);
    }

    /// Keyboard (closed): open the focused menu, highlighting its first
    /// item (Arrow Down / Enter / Space on a top-level title).
    fn open_focused(&mut self) {
        let n = self.menu_count();
        if n == 0 {
            return;
        }
        let m = self.bar_focus;
        self.open = Some(m);
        self.active = if self.items_per_menu[m] == 0 {
            None
        } else {
            Some(0)
        };
    }

    /// Keyboard (open): switch to the adjacent menu and open it with
    /// its first item highlighted (Arrow Right/Left inside an open
    /// menu — the WAI-ARIA menubar cross-navigation).
    fn menu_switch(&mut self, forward: bool) {
        let Some(m) = self.open else { return };
        let n = self.menu_count();
        if n == 0 {
            return;
        }
        let next = step(m, forward, n);
        self.open = Some(next);
        self.bar_focus = next;
        self.active = if self.items_per_menu[next] == 0 {
            None
        } else {
            Some(0)
        };
    }

    /// Keyboard (open): move the active item by `delta` with wrap.
    /// With no active item, Down lands on the first item and Up on the
    /// last.
    fn move_active(&mut self, forward: bool) {
        let Some(m) = self.open else { return };
        let cnt = self.items_per_menu[m];
        if cnt == 0 {
            return;
        }
        self.active = Some(match self.active {
            Some(a) => step(a, forward, cnt),
            None => {
                if forward {
                    0
                } else {
                    cnt - 1
                }
            }
        });
    }

    /// Keyboard (open): jump the active item to the first / last entry
    /// (Home / End).
    fn active_edge(&mut self, last: bool) {
        let Some(m) = self.open else { return };
        let cnt = self.items_per_menu[m];
        if cnt == 0 {
            return;
        }
        self.active = Some(if last { cnt - 1 } else { 0 });
    }
}

/// One-step wrap-around over `0..n` (`n > 0`). `forward` advances,
/// `!forward` retreats; both wrap at the ends.
fn step(current: usize, forward: bool, n: usize) -> usize {
    if forward {
        (current + 1) % n
    } else {
        (current + n - 1) % n
    }
}

impl Default for MenuBar {
    /// Default constructs an empty menubar (no menus). Applications
    /// pass a concrete `items_per_menu` to [`MenuBar::new`]; `Default`
    /// exists for the `IntentEmitter<W: Default>` bound.
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

/// R691 §5.38 — `External` adapter wrapping a [`MenuBar`]. Surfaces the
/// menubar to the §5.12 RPC plane and emits a `"command"` intent
/// (payload [`IntrospectValue::Text`] `"<menu>.<item>"`) when an item
/// activates.
pub struct MenuBarExternal {
    em: IntentEmitter<MenuBar>,
}

impl MenuBarExternal {
    /// Construct with `items_per_menu[m]` command items per menu.
    #[must_use]
    pub fn new(items_per_menu: Vec<usize>) -> Self {
        Self {
            em: IntentEmitter::new(MenuBar::new(items_per_menu)),
        }
    }

    /// Number of top-level menus.
    #[must_use]
    pub fn menu_count(&self) -> usize {
        self.em.inner.menu_count()
    }

    /// Item count of menu `m`, or `None` when out of range.
    #[must_use]
    pub fn item_count(&self, menu: usize) -> Option<usize> {
        self.em.inner.item_count(menu)
    }

    /// The open dropdown's menu index, or `None`.
    #[must_use]
    pub fn open_menu(&self) -> Option<usize> {
        self.em.inner.open_menu()
    }

    /// The highlighted item within the open menu, or `None`.
    #[must_use]
    pub fn active_item(&self) -> Option<usize> {
        self.em.inner.active_item()
    }

    /// The focused top-level title (keyboard cursor).
    #[must_use]
    pub fn bar_focus(&self) -> usize {
        self.em.inner.bar_focus()
    }

    /// Drive a title pointer event (`m`, `event`). Only `PointerUp`
    /// toggles the menu.
    fn send_title(&mut self, m: usize, event: PointerEvent) {
        if event == PointerEvent::Up {
            self.em.inner.toggle_title(m);
        }
    }

    /// Drive an item pointer event (`i`, `event`). `PointerEnter`
    /// highlights; `PointerUp` activates (and emits `"command"`).
    fn send_item(&mut self, i: usize, event: PointerEvent) {
        match event {
            PointerEvent::Enter => self.em.inner.hover_item(i),
            PointerEvent::Up => {
                if let Some((m, item)) = self.em.inner.activate_item(i) {
                    self.emit_command(m, item);
                }
            }
            PointerEvent::Down | PointerEvent::Leave | PointerEvent::Cancel => {}
        }
    }

    /// Apply a W3C key name through the WAI-ARIA §3.5 menubar keyboard
    /// model. Returns `true` when the key was handled (drove a state
    /// change or activation), `false` for keys the menubar ignores so
    /// the shell's swallow contract can fall through (e.g. `Tab`).
    fn apply_key(&mut self, key: &str) -> bool {
        match self.em.inner.open_menu() {
            None => match key {
                "ArrowRight" => {
                    self.em.inner.bar_nav(true);
                    true
                }
                "ArrowLeft" => {
                    self.em.inner.bar_nav(false);
                    true
                }
                "ArrowDown" | "Enter" | "Space" => {
                    self.em.inner.open_focused();
                    true
                }
                "Home" => {
                    set_bar_focus(&mut self.em.inner, false);
                    true
                }
                "End" => {
                    set_bar_focus(&mut self.em.inner, true);
                    true
                }
                _ => false,
            },
            Some(_) => match key {
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
                "ArrowRight" => {
                    self.em.inner.menu_switch(true);
                    true
                }
                "ArrowLeft" => {
                    self.em.inner.menu_switch(false);
                    true
                }
                "Enter" | "Space" => {
                    if let Some(i) = self.em.inner.active_item() {
                        if let Some((m, item)) = self.em.inner.activate_item(i) {
                            self.emit_command(m, item);
                        }
                    }
                    true
                }
                "Escape" => {
                    self.em.inner.close();
                    true
                }
                _ => false,
            },
        }
    }

    /// Push the `"command"` intent for an activated item.
    fn emit_command(&mut self, menu: usize, item: usize) {
        self.em.push(Intent::new_static(
            "command",
            IntrospectValue::Text(format!("{menu}.{item}")),
        ));
    }

    /// Shared parse for the `send` wire payload `"<sub>:<EventName>"`
    /// where `<sub>` is `t<m>` (title) or `i<i>` (item). Returns the
    /// open index as the round-trip outcome.
    fn dispatch_send(&mut self, payload: &str) -> Result<IntrospectValue, InvokeError> {
        let (sub, event_name) = payload.split_once(':').ok_or(InvokeError::Rejected)?;
        let event = parse_pointer_event(event_name).ok_or(InvokeError::Rejected)?;
        let mut chars = sub.chars();
        let kind = chars.next().ok_or(InvokeError::Rejected)?;
        let idx: usize = chars.as_str().parse().map_err(|_| InvokeError::Rejected)?;
        match kind {
            't' => self.send_title(idx, event),
            'i' => self.send_item(idx, event),
            _ => return Err(InvokeError::Rejected),
        }
        Ok(open_value(self.open_menu()))
    }
}

/// Set `bar_focus` to the first (`!last`) or last (`last`) top-level
/// title, guarding the empty-menubar case.
fn set_bar_focus(menu: &mut MenuBar, last: bool) {
    let n = menu.menu_count();
    if n == 0 {
        return;
    }
    menu.bar_focus = if last { n - 1 } else { 0 };
}

/// `Some(i)` → `Int(i)`, `None` → `Null` — shared optional-index
/// lowering for `query` / the `send` return value.
fn open_value(idx: Option<usize>) -> IntrospectValue {
    match idx {
        Some(i) => IntrospectValue::Int(i64::try_from(i).expect("menu index fits in i64")),
        None => IntrospectValue::Null,
    }
}

impl Default for MenuBarExternal {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl core::fmt::Debug for MenuBarExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MenuBarExternal")
            .field("menu_count", &self.menu_count())
            .field("open", &self.open_menu())
            .field("active", &self.active_item())
            .field("bar_focus", &self.bar_focus())
            .finish()
    }
}

impl External for MenuBarExternal {
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

impl ExternalIntrospect for MenuBarExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("menu_count", "int"),
            ("open", "int"),
            ("active", "int"),
            ("bar_focus", "int"),
            ("item_count.<menu>", "int"),
            ("send", "string"),
            ("key", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "menu_count" => Some(IntrospectValue::Int(
                i64::try_from(self.menu_count()).expect("menu_count fits in i64"),
            )),
            "open" => Some(open_value(self.open_menu())),
            "active" => Some(open_value(self.active_item())),
            "bar_focus" => Some(IntrospectValue::Int(
                i64::try_from(self.bar_focus()).expect("bar_focus fits in i64"),
            )),
            _ => {
                let suffix = path.strip_prefix("item_count.")?;
                let menu: usize = suffix.parse().ok()?;
                let count = self.item_count(menu)?;
                Some(IntrospectValue::Int(
                    i64::try_from(count).expect("item_count fits in i64"),
                ))
            }
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "menu_count" => Err(InterveneError::ReadOnly),
            // Programmatic open/close — slot assignment without firing
            // the `"command"` intent (RPC restore / form default).
            "open" => match value {
                IntrospectValue::Int(i) => {
                    let m = resolve_index(i, self.menu_count())?;
                    self.em.inner.open = Some(m);
                    self.em.inner.active = None;
                    self.em.inner.bar_focus = m;
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.em.inner.close();
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "active" => match value {
                IntrospectValue::Int(i) => {
                    let m = self.open_menu().ok_or(InterveneError::OutOfRange)?;
                    let item = resolve_index(i, self.em.inner.items_per_menu[m])?;
                    self.em.inner.active = Some(item);
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.em.inner.active = None;
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "bar_focus" => match value {
                IntrospectValue::Int(i) => {
                    let m = resolve_index(i, self.menu_count())?;
                    self.em.inner.bar_focus = m;
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(&mut self, path: &str, args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Mouse: "t<m>:<Event>" / "i<i>:<Event>". Returns open index.
            "send" => match args {
                IntrospectValue::Text(ref s) => self.dispatch_send(s),
                _ => Err(InvokeError::TypeMismatch),
            },
            // Keyboard: a W3C key name through the WAI-ARIA menubar
            // model. Returns Bool(handled) so the binding reports the
            // right swallow verdict.
            "key" => match args {
                IntrospectValue::Text(ref name) => Ok(IntrospectValue::Bool(self.apply_key(name))),
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// Validate a signed intervene index against `[0, count)`. Negative,
/// overflowing, and `>= count` all map to [`InterveneError::OutOfRange`]
/// (mirrors `RadioGroupExternal::resolve_index_intervene`).
fn resolve_index(i: i64, count: usize) -> Result<usize, InterveneError> {
    if i < 0 {
        return Err(InterveneError::OutOfRange);
    }
    let idx = usize::try_from(i).map_err(|_| InterveneError::OutOfRange)?;
    if idx >= count {
        return Err(InterveneError::OutOfRange);
    }
    Ok(idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar() -> MenuBar {
        // File(3) / Edit(5) / View(2).
        MenuBar::new(vec![3, 5, 2])
    }

    fn ext() -> MenuBarExternal {
        MenuBarExternal::new(vec![3, 5, 2])
    }

    // ----- model: initial state -----

    #[test]
    fn new_menubar_is_closed() {
        let m = bar();
        assert_eq!(m.menu_count(), 3);
        assert_eq!(m.open_menu(), None);
        assert_eq!(m.active_item(), None);
        assert_eq!(m.bar_focus(), 0);
        assert_eq!(m.item_count(0), Some(3));
        assert_eq!(m.item_count(1), Some(5));
        assert_eq!(m.item_count(2), Some(2));
        assert_eq!(m.item_count(3), None);
    }

    // ----- model: mouse toggle -----

    #[test]
    fn toggle_title_opens_then_closes() {
        let mut m = bar();
        m.toggle_title(1);
        assert_eq!(m.open_menu(), Some(1));
        assert_eq!(m.active_item(), None, "mouse open pre-highlights nothing");
        assert_eq!(m.bar_focus(), 1);
        m.toggle_title(1);
        assert_eq!(m.open_menu(), None, "re-toggle closes");
    }

    #[test]
    fn toggle_different_title_switches() {
        let mut m = bar();
        m.toggle_title(0);
        m.toggle_title(2);
        assert_eq!(m.open_menu(), Some(2));
        assert_eq!(m.bar_focus(), 2);
    }

    #[test]
    fn toggle_out_of_range_is_noop() {
        let mut m = bar();
        m.toggle_title(9);
        assert_eq!(m.open_menu(), None);
    }

    // ----- model: hover + activate -----

    #[test]
    fn hover_item_highlights_only_when_open() {
        let mut m = bar();
        m.hover_item(1);
        assert_eq!(m.active_item(), None, "no hover effect while closed");
        m.toggle_title(0);
        m.hover_item(2);
        assert_eq!(m.active_item(), Some(2));
        m.hover_item(9);
        assert_eq!(m.active_item(), Some(2), "out-of-range hover ignored");
    }

    #[test]
    fn activate_item_returns_coords_and_closes() {
        let mut m = bar();
        m.toggle_title(1);
        assert_eq!(m.activate_item(3), Some((1, 3)));
        assert_eq!(m.open_menu(), None, "activation dismisses the menu");
        assert_eq!(m.bar_focus(), 1);
    }

    #[test]
    fn activate_item_out_of_range_returns_none() {
        let mut m = bar();
        m.toggle_title(2);
        assert_eq!(m.activate_item(9), None);
        assert_eq!(m.open_menu(), Some(2), "invalid activate leaves menu open");
    }

    #[test]
    fn activate_item_while_closed_returns_none() {
        let mut m = bar();
        assert_eq!(m.activate_item(0), None);
    }

    // ----- model: keyboard (closed) -----

    #[test]
    fn bar_nav_wraps_both_directions() {
        let mut m = bar();
        m.bar_nav(true);
        assert_eq!(m.bar_focus(), 1);
        m.bar_nav(true);
        m.bar_nav(true);
        assert_eq!(m.bar_focus(), 0, "ArrowRight wraps 2 -> 0");
        m.bar_nav(false);
        assert_eq!(m.bar_focus(), 2, "ArrowLeft wraps 0 -> 2");
        assert_eq!(m.open_menu(), None, "navigation while closed never opens");
    }

    #[test]
    fn open_focused_highlights_first_item() {
        let mut m = bar();
        m.bar_nav(true); // focus menu 1
        m.open_focused();
        assert_eq!(m.open_menu(), Some(1));
        assert_eq!(m.active_item(), Some(0));
    }

    // ----- model: keyboard (open) -----

    #[test]
    fn move_active_wraps_within_open_menu() {
        let mut m = bar();
        m.toggle_title(2); // View, 2 items, active None
        m.move_active(true);
        assert_eq!(m.active_item(), Some(0), "Down from None -> first");
        m.move_active(true);
        assert_eq!(m.active_item(), Some(1));
        m.move_active(true);
        assert_eq!(m.active_item(), Some(0), "Down wraps 1 -> 0");
        m.move_active(false);
        assert_eq!(m.active_item(), Some(1), "Up wraps 0 -> 1");
    }

    #[test]
    fn move_active_up_from_none_lands_last() {
        let mut m = bar();
        m.toggle_title(0); // File, 3 items
        m.move_active(false);
        assert_eq!(m.active_item(), Some(2));
    }

    #[test]
    fn active_edge_jumps_first_and_last() {
        let mut m = bar();
        m.toggle_title(1); // Edit, 5 items
        m.active_edge(true);
        assert_eq!(m.active_item(), Some(4));
        m.active_edge(false);
        assert_eq!(m.active_item(), Some(0));
    }

    #[test]
    fn menu_switch_moves_to_adjacent_open_menu() {
        let mut m = bar();
        m.toggle_title(0);
        m.menu_switch(true);
        assert_eq!(m.open_menu(), Some(1), "ArrowRight opens next menu");
        assert_eq!(m.active_item(), Some(0), "switched menu highlights first");
        assert_eq!(m.bar_focus(), 1);
        m.menu_switch(false);
        assert_eq!(m.open_menu(), Some(0));
        m.menu_switch(false);
        assert_eq!(m.open_menu(), Some(2), "ArrowLeft wraps 0 -> 2");
    }

    // ----- External: query / intervene -----

    #[test]
    fn external_query_initial() {
        let e = ext();
        assert_eq!(e.query("menu_count"), Some(IntrospectValue::Int(3)));
        assert_eq!(e.query("open"), Some(IntrospectValue::Null));
        assert_eq!(e.query("active"), Some(IntrospectValue::Null));
        assert_eq!(e.query("bar_focus"), Some(IntrospectValue::Int(0)));
        assert_eq!(e.query("item_count.1"), Some(IntrospectValue::Int(5)));
        assert_eq!(e.query("item_count.9"), None);
        assert_eq!(e.query("nope"), None);
    }

    #[test]
    fn external_intervene_open_and_close() {
        let mut e = ext();
        e.intervene("open", IntrospectValue::Int(2)).unwrap();
        assert_eq!(e.open_menu(), Some(2));
        assert_eq!(e.bar_focus(), 2);
        assert!(!e.is_dirty(), "intervene fires no command intent");
        e.intervene("open", IntrospectValue::Null).unwrap();
        assert_eq!(e.open_menu(), None);
    }

    #[test]
    fn external_intervene_open_out_of_range() {
        let mut e = ext();
        assert_eq!(
            e.intervene("open", IntrospectValue::Int(9)),
            Err(InterveneError::OutOfRange)
        );
        assert_eq!(
            e.intervene("open", IntrospectValue::Int(-1)),
            Err(InterveneError::OutOfRange)
        );
    }

    #[test]
    fn external_intervene_active_requires_open_menu() {
        let mut e = ext();
        assert_eq!(
            e.intervene("active", IntrospectValue::Int(0)),
            Err(InterveneError::OutOfRange),
            "no open menu -> no active slot"
        );
        e.intervene("open", IntrospectValue::Int(1)).unwrap();
        e.intervene("active", IntrospectValue::Int(4)).unwrap();
        assert_eq!(e.active_item(), Some(4));
        assert_eq!(
            e.intervene("active", IntrospectValue::Int(5)),
            Err(InterveneError::OutOfRange)
        );
    }

    #[test]
    fn external_intervene_menu_count_read_only() {
        let mut e = ext();
        assert_eq!(
            e.intervene("menu_count", IntrospectValue::Int(2)),
            Err(InterveneError::ReadOnly)
        );
    }

    #[test]
    fn external_intervene_wrong_variant_is_type_mismatch() {
        let mut e = ext();
        assert_eq!(
            e.intervene("open", IntrospectValue::Bool(true)),
            Err(InterveneError::TypeMismatch)
        );
    }

    // ----- External: send (mouse) -----

    fn click_title(e: &mut MenuBarExternal, m: usize) {
        for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
            e.invoke("send", IntrospectValue::Text(format!("t{m}:{ev}")))
                .unwrap();
        }
    }

    fn click_item(e: &mut MenuBarExternal, i: usize) {
        for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
            e.invoke("send", IntrospectValue::Text(format!("i{i}:{ev}")))
                .unwrap();
        }
    }

    #[test]
    fn external_send_title_click_toggles() {
        let mut e = ext();
        click_title(&mut e, 1);
        assert_eq!(e.open_menu(), Some(1));
        click_title(&mut e, 1);
        assert_eq!(e.open_menu(), None);
    }

    #[test]
    fn external_send_item_enter_highlights_up_activates() {
        let mut e = ext();
        click_title(&mut e, 0);
        // Hover item 2 without activating.
        e.invoke("send", IntrospectValue::Text("i2:PointerEnter".to_string()))
            .unwrap();
        assert_eq!(e.active_item(), Some(2));
        assert!(!e.is_dirty(), "hover fires no command");
        // Full click activates item 1 -> command "0.1" + close.
        click_item(&mut e, 1);
        assert_eq!(e.open_menu(), None);
        let mut got = Vec::new();
        e.drain_intents(&mut |i| got.push(i));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].tag_str(), "command");
        assert_eq!(got[0].payload, IntrospectValue::Text("0.1".to_string()));
    }

    #[test]
    fn external_send_item_while_closed_no_command() {
        let mut e = ext();
        click_item(&mut e, 0);
        assert!(!e.is_dirty());
        assert_eq!(e.open_menu(), None);
    }

    #[test]
    fn external_send_malformed_rejected() {
        let mut e = ext();
        assert_eq!(
            e.invoke("send", IntrospectValue::Text("no_colon".to_string())),
            Err(InvokeError::Rejected)
        );
        assert_eq!(
            e.invoke("send", IntrospectValue::Text("x0:PointerUp".to_string())),
            Err(InvokeError::Rejected),
            "unknown sub-kind"
        );
        assert_eq!(
            e.invoke("send", IntrospectValue::Text("t0:Teleport".to_string())),
            Err(InvokeError::Rejected),
            "unknown pointer event"
        );
        assert_eq!(
            e.invoke("send", IntrospectValue::Text("tA:PointerUp".to_string())),
            Err(InvokeError::Rejected),
            "non-numeric index"
        );
    }

    #[test]
    fn external_send_returns_open_index() {
        let mut e = ext();
        let out = e
            .invoke("send", IntrospectValue::Text("t1:PointerUp".to_string()))
            .unwrap();
        assert_eq!(out, IntrospectValue::Int(1));
    }

    // ----- External: key (keyboard) -----

    fn key(e: &mut MenuBarExternal, name: &str) -> bool {
        match e
            .invoke("key", IntrospectValue::Text(name.to_string()))
            .unwrap()
        {
            IntrospectValue::Bool(b) => b,
            other => panic!("key invoke must return Bool, got {other:?}"),
        }
    }

    #[test]
    fn external_key_closed_arrow_navigates_titles() {
        let mut e = ext();
        assert!(key(&mut e, "ArrowRight"));
        assert_eq!(e.bar_focus(), 1);
        assert_eq!(e.open_menu(), None);
    }

    #[test]
    fn external_key_arrow_down_opens_focused_first_item() {
        let mut e = ext();
        key(&mut e, "ArrowRight"); // focus menu 1
        key(&mut e, "ArrowDown"); // open
        assert_eq!(e.open_menu(), Some(1));
        assert_eq!(e.active_item(), Some(0));
    }

    #[test]
    fn external_key_in_menu_navigation_and_activate() {
        let mut e = ext();
        key(&mut e, "ArrowDown"); // open menu 0 (File), active 0
        key(&mut e, "ArrowDown"); // active 1
        assert_eq!(e.active_item(), Some(1));
        assert!(key(&mut e, "Enter")); // activate item 1
        assert_eq!(e.open_menu(), None, "activation closes");
        let mut got = Vec::new();
        e.drain_intents(&mut |i| got.push(i));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].payload, IntrospectValue::Text("0.1".to_string()));
    }

    #[test]
    fn external_key_escape_closes() {
        let mut e = ext();
        key(&mut e, "ArrowDown");
        assert_eq!(e.open_menu(), Some(0));
        assert!(key(&mut e, "Escape"));
        assert_eq!(e.open_menu(), None);
    }

    #[test]
    fn external_key_right_left_switch_open_menu() {
        let mut e = ext();
        key(&mut e, "ArrowDown"); // open menu 0
        key(&mut e, "ArrowRight"); // -> menu 1
        assert_eq!(e.open_menu(), Some(1));
        assert_eq!(e.active_item(), Some(0));
        key(&mut e, "ArrowLeft"); // -> menu 0
        assert_eq!(e.open_menu(), Some(0));
    }

    #[test]
    fn external_key_home_end() {
        let mut e = ext();
        // Closed: Home/End move bar_focus across titles.
        assert!(key(&mut e, "End"));
        assert_eq!(e.bar_focus(), 2);
        assert!(key(&mut e, "Home"));
        assert_eq!(e.bar_focus(), 0);
        // Open: Home/End move the active item.
        key(&mut e, "ArrowDown"); // open menu 0, active 0
        key(&mut e, "End");
        assert_eq!(e.active_item(), Some(2));
        key(&mut e, "Home");
        assert_eq!(e.active_item(), Some(0));
    }

    #[test]
    fn external_key_space_activates_like_enter() {
        let mut e = ext();
        key(&mut e, "ArrowDown"); // open menu 0, active 0
        assert!(key(&mut e, "Space"));
        assert_eq!(e.open_menu(), None);
        let mut got = Vec::new();
        e.drain_intents(&mut |i| got.push(i));
        assert_eq!(got[0].payload, IntrospectValue::Text("0.0".to_string()));
    }

    #[test]
    fn external_key_unhandled_returns_false() {
        let mut e = ext();
        assert!(!key(&mut e, "Tab"), "Tab is not a menubar key when closed");
        key(&mut e, "ArrowDown");
        assert!(!key(&mut e, "Tab"), "Tab is not a menubar key when open");
    }

    #[test]
    fn external_invoke_unknown_path() {
        let mut e = ext();
        assert_eq!(
            e.invoke("nope", IntrospectValue::Null),
            Err(InvokeError::UnknownPath)
        );
    }

    #[test]
    fn external_schema_declares_slots() {
        let e = ext();
        assert_eq!(
            e.schema().fields,
            &[
                ("menu_count", "int"),
                ("open", "int"),
                ("active", "int"),
                ("bar_focus", "int"),
                ("item_count.<menu>", "int"),
                ("send", "string"),
                ("key", "string"),
            ]
        );
    }
}
