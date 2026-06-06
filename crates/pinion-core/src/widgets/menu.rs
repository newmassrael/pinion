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
//! * `invoke("send", "t<m>:<PointerWireEvent>")` — a top-level **title**
//!   pointer event (composite tag `<bar>#t<m>`). Only `PointerUp`
//!   toggles menu `m` open/closed.
//! * `invoke("send", "i<i>:<PointerWireEvent>")` — a dropdown **item**
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
//! An open menu dismisses via **Escape**, **re-clicking the open
//! title**, **activating an item**, or (R715) a **click-outside** on the
//! transparent dismiss barrier. The barrier is the shared light-dismiss
//! layer ([`pinion_widget_paint::barrier`]); the binding paints it as a
//! `<bar>#barrier` composite node behind the open dropdown and the
//! R51.42 router feeds its `PointerUp` here as `send("barrier:…")`,
//! which closes the menu. *Focus-loss dismiss* remains an additive axis
//! once a real consumer surfaces it. Likewise *mouse
//! hover-follow between top-level titles while a menu is open*,
//! *`menuitemcheckbox` / `menuitemradio` stateful entries*, *submenus
//! (nested menus)*, and *accelerator / mnemonic keys* are additive
//! axes once a real consumer needs them.

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::menu_nav;
use crate::input::PointerWireEvent;
use crate::widgets::wire::resolve_index;
use crate::widgets::IntentEmitter;

/// R805 §5.40 — the WAI-ARIA 1.2 menu-item taxonomy a dropdown item can
/// carry. The R691 first slice modelled every item as a one-shot
/// [`Self::Command`]; R805 adds the stateful + structural kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItemKind {
    /// A one-shot command (WAI-ARIA `menuitem`): activating it fires the
    /// `"command"` intent and dismisses the menu. Carries no persistent
    /// state.
    Command,
    /// A toggle (WAI-ARIA `menuitemcheckbox`): activating it flips the
    /// item's persistent [`MenuItem::checked`] state, fires `"command"`,
    /// and dismisses. The checked state survives close / reopen.
    Checkbox,
    /// A non-interactive divider (WAI-ARIA `separator`): never navigable,
    /// never activatable — it only sections the dropdown visually.
    Separator,
}

/// R805 §5.40 — one dropdown item: its [`MenuItemKind`], persistent
/// `checked` state (meaningful only for [`MenuItemKind::Checkbox`]), and
/// whether it is `enabled`. A disabled item paints muted, is skipped by
/// keyboard / hover navigation, and cannot be activated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuItem {
    /// The WAI-ARIA item kind.
    pub kind: MenuItemKind,
    /// Persistent checked state ([`MenuItemKind::Checkbox`] only).
    pub checked: bool,
    /// Whether the item is enabled (interactive).
    pub enabled: bool,
}

impl MenuItem {
    /// A one-shot enabled command item.
    #[must_use]
    pub const fn command() -> Self {
        Self { kind: MenuItemKind::Command, checked: false, enabled: true }
    }

    /// An enabled checkbox item with the given initial checked state.
    #[must_use]
    pub const fn checkbox(checked: bool) -> Self {
        Self { kind: MenuItemKind::Checkbox, checked, enabled: true }
    }

    /// A non-interactive divider.
    #[must_use]
    pub const fn separator() -> Self {
        Self { kind: MenuItemKind::Separator, checked: false, enabled: false }
    }

    /// Builder: mark this item disabled (greyed, skipped, non-activatable).
    #[must_use]
    pub const fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Whether keyboard / hover navigation may land on this item, and
    /// whether activation has any effect: every enabled non-separator.
    #[must_use]
    pub const fn is_navigable(&self) -> bool {
        self.enabled && !matches!(self.kind, MenuItemKind::Separator)
    }
}

/// R691 §5.38 — logical menubar with N command menus. See module docs
/// for the command-vs-selection rationale and the state model.
pub struct MenuBar {
    /// Items per top-level menu. `menus.len()` is the number of top-level
    /// titles; `menus[m]` is menu `m`'s ordered item list (R805 — was a
    /// bare `Vec<usize>` count in R691).
    menus: Vec<Vec<MenuItem>>,
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
    /// **command** items (the R691 all-command shape). All menus start
    /// closed. For the stateful R805 taxonomy use [`Self::with_items`].
    #[must_use]
    pub fn new(items_per_menu: Vec<usize>) -> Self {
        let menus = items_per_menu
            .into_iter()
            .map(|count| vec![MenuItem::command(); count])
            .collect();
        Self { menus, open: None, active: None, bar_focus: 0 }
    }

    /// R805 — construct a menubar from explicit per-menu [`MenuItem`]
    /// lists (checkbox / separator / disabled entries). All menus start
    /// closed.
    #[must_use]
    pub fn with_items(menus: Vec<Vec<MenuItem>>) -> Self {
        Self { menus, open: None, active: None, bar_focus: 0 }
    }

    /// Number of top-level menus.
    #[must_use]
    pub fn menu_count(&self) -> usize {
        self.menus.len()
    }

    /// Item count of menu `m`, or `None` when `m` is out of range.
    #[must_use]
    pub fn item_count(&self, menu: usize) -> Option<usize> {
        self.menus.get(menu).map(Vec::len)
    }

    /// R805 — the [`MenuItem`] at `(menu, item)`, or `None` when either
    /// index is out of range.
    #[must_use]
    pub fn item(&self, menu: usize, item: usize) -> Option<MenuItem> {
        self.menus.get(menu)?.get(item).copied()
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
    /// no menu is open, `i` is out of range, or item `i` is a separator /
    /// disabled (R805 — hover skips non-navigable rows).
    fn hover_item(&mut self, i: usize) {
        if let Some(m) = self.open {
            if self.menus[m].get(i).is_some_and(MenuItem::is_navigable) {
                self.active = Some(i);
            }
        }
    }

    /// Activate item `i` of the open menu: returns `Some((menu, item))`
    /// and closes the menu when valid, `None` otherwise. A separator /
    /// disabled item is inert (R805). A [`MenuItemKind::Checkbox`] flips
    /// its persistent `checked` state before closing. The caller
    /// (the [`MenuBarExternal`] adapter) emits the `"command"` intent
    /// from the returned coordinates.
    fn activate_item(&mut self, i: usize) -> Option<(usize, usize)> {
        let m = self.open?;
        let item = self.menus[m].get_mut(i)?;
        if !item.is_navigable() {
            return None;
        }
        if item.kind == MenuItemKind::Checkbox {
            item.checked = !item.checked;
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
        self.bar_focus = menu_nav::step(self.bar_focus, forward, n);
    }

    /// Keyboard (closed): open the focused menu, highlighting its first
    /// navigable item (Arrow Down / Enter / Space on a top-level title).
    fn open_focused(&mut self) {
        let n = self.menu_count();
        if n == 0 {
            return;
        }
        let m = self.bar_focus;
        self.open = Some(m);
        self.active = self.first_navigable(m);
    }

    /// Keyboard (open): switch to the adjacent menu and open it with
    /// its first navigable item highlighted (Arrow Right/Left inside an
    /// open menu — the WAI-ARIA menubar cross-navigation).
    fn menu_switch(&mut self, forward: bool) {
        let Some(m) = self.open else { return };
        let n = self.menu_count();
        if n == 0 {
            return;
        }
        let next = menu_nav::step(m, forward, n);
        self.open = Some(next);
        self.bar_focus = next;
        self.active = self.first_navigable(next);
    }

    /// Keyboard (open): move the active item by one with wrap, skipping
    /// separators / disabled rows (R805). With no active item, Down lands
    /// on the first navigable item and Up on the last.
    fn move_active(&mut self, forward: bool) {
        let Some(m) = self.open else { return };
        let items = &self.menus[m];
        self.active = menu_nav::nav_move_skip(self.active, items.len(), forward, |i| {
            items[i].is_navigable()
        });
    }

    /// Keyboard (open): jump the active item to the first / last
    /// **navigable** entry (Home / End).
    fn active_edge(&mut self, last: bool) {
        let Some(m) = self.open else { return };
        let items = &self.menus[m];
        if let Some(a) = menu_nav::nav_edge_skip(items.len(), last, |i| items[i].is_navigable()) {
            self.active = Some(a);
        }
    }

    /// The first navigable item of menu `m` (skipping leading separators /
    /// disabled rows), or `None` when the menu has no navigable item.
    fn first_navigable(&self, m: usize) -> Option<usize> {
        let items = &self.menus[m];
        menu_nav::nav_edge_skip(items.len(), false, |i| items[i].is_navigable())
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

    /// R805 — construct from explicit per-menu [`MenuItem`] lists
    /// (checkbox / separator / disabled entries).
    #[must_use]
    pub fn with_items(menus: Vec<Vec<MenuItem>>) -> Self {
        Self {
            em: IntentEmitter::new(MenuBar::with_items(menus)),
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

    /// R805 — the [`MenuItem`] at `(menu, item)`, or `None` out of range.
    #[must_use]
    pub fn item(&self, menu: usize, item: usize) -> Option<MenuItem> {
        self.em.inner.item(menu, item)
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
    fn send_title(&mut self, m: usize, event: PointerWireEvent) {
        if event == PointerWireEvent::Up {
            self.em.inner.toggle_title(m);
        }
    }

    /// Drive an item pointer event (`i`, `event`). `PointerEnter`
    /// highlights; `PointerUp` activates (and emits `"command"`).
    fn send_item(&mut self, i: usize, event: PointerWireEvent) {
        match event {
            PointerWireEvent::Enter => self.em.inner.hover_item(i),
            PointerWireEvent::Up => {
                if let Some((m, item)) = self.em.inner.activate_item(i) {
                    self.emit_command(m, item);
                }
            }
            PointerWireEvent::Down | PointerWireEvent::Leave | PointerWireEvent::Cancel => {}
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
    /// where `<sub>` is `t<m>` (title), `i<i>` (item), or the literal
    /// `barrier` (R715 click-outside dismiss). Returns the open index as
    /// the round-trip outcome.
    fn dispatch_send(&mut self, payload: &str) -> Result<IntrospectValue, InvokeError> {
        let (sub, event_name) = payload.split_once(':').ok_or(InvokeError::Rejected)?;
        let event = PointerWireEvent::from_wire_name(event_name).ok_or(InvokeError::Rejected)?;
        // R715 §5.16 — the transparent dismiss barrier (`<bar>#barrier`,
        // painted behind an open dropdown over the area below the title
        // strip): a `PointerUp` outside the menu closes it. Other pointer
        // events over the barrier region are inert.
        if sub == "barrier" {
            if event == PointerWireEvent::Up {
                self.em.inner.close();
            }
            return Ok(open_value(self.open_menu()));
        }
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

/// R805 — parse a `<menu>.<item>` per-item slot suffix into its two
/// indices. `None` for a missing dot or a non-numeric component.
fn parse_menu_item(suffix: &str) -> Option<(usize, usize)> {
    let (m, i) = suffix.split_once('.')?;
    Some((m.parse().ok()?, i.parse().ok()?))
}

/// R805 — the WAI-ARIA wire name for a [`MenuItemKind`] (`item_kind`
/// query value): the inverse of how an AI client reads an item's role.
fn item_kind_name(kind: MenuItemKind) -> &'static str {
    match kind {
        MenuItemKind::Command => "command",
        MenuItemKind::Checkbox => "checkbox",
        MenuItemKind::Separator => "separator",
    }
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
            ("item_kind.<menu>.<item>", "string"),
            ("checked.<menu>.<item>", "bool"),
            ("enabled.<menu>.<item>", "bool"),
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
                if let Some(suffix) = path.strip_prefix("item_count.") {
                    let menu: usize = suffix.parse().ok()?;
                    let count = self.item_count(menu)?;
                    return Some(IntrospectValue::Int(
                        i64::try_from(count).expect("item_count fits in i64"),
                    ));
                }
                // R805 — per-item stateful slots, keyed `<menu>.<item>`.
                if let Some(suffix) = path.strip_prefix("item_kind.") {
                    let (m, i) = parse_menu_item(suffix)?;
                    let item = self.item(m, i)?;
                    return Some(IntrospectValue::Text(item_kind_name(item.kind).to_owned()));
                }
                if let Some(suffix) = path.strip_prefix("checked.") {
                    let (m, i) = parse_menu_item(suffix)?;
                    return Some(IntrospectValue::Bool(self.item(m, i)?.checked));
                }
                if let Some(suffix) = path.strip_prefix("enabled.") {
                    let (m, i) = parse_menu_item(suffix)?;
                    return Some(IntrospectValue::Bool(self.item(m, i)?.enabled));
                }
                None
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
                    let item = resolve_index(i, self.em.inner.menus[m].len())?;
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
            // R805 — programmatic checkbox toggle `checked.<menu>.<item>`
            // (RPC restore / form default; fires no `"command"` intent).
            _ => {
                let suffix = path
                    .strip_prefix("checked.")
                    .ok_or(InterveneError::UnknownPath)?;
                let (m, i) = parse_menu_item(suffix).ok_or(InterveneError::UnknownPath)?;
                let item = self
                    .em
                    .inner
                    .menus
                    .get_mut(m)
                    .and_then(|menu| menu.get_mut(i))
                    .ok_or(InterveneError::OutOfRange)?;
                match value {
                    IntrospectValue::Bool(b) => {
                        item.checked = b;
                        Ok(())
                    }
                    _ => Err(InterveneError::TypeMismatch),
                }
            }
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
    fn r715_barrier_pointer_up_closes_open_menu() {
        let mut e = ext();
        click_title(&mut e, 2);
        assert_eq!(e.open_menu(), Some(2), "menu 2 open");
        // A click-outside on the transparent dismiss barrier closes it.
        let out = e
            .invoke("send", IntrospectValue::Text("barrier:PointerUp".to_string()))
            .unwrap();
        assert_eq!(e.open_menu(), None, "barrier PointerUp dismisses");
        assert_eq!(out, IntrospectValue::Null, "send returns the (now None) open index");
        // Barrier dismiss emits no command intent (it is not an activation).
        assert!(!e.is_dirty(), "click-outside fires no command");
    }

    #[test]
    fn r715_barrier_non_up_events_are_inert() {
        let mut e = ext();
        click_title(&mut e, 0);
        for ev in ["PointerEnter", "PointerDown", "PointerLeave", "PointerCancel"] {
            e.invoke("send", IntrospectValue::Text(format!("barrier:{ev}")))
                .unwrap();
            assert_eq!(e.open_menu(), Some(0), "{ev} over barrier does not dismiss");
        }
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
                ("item_kind.<menu>.<item>", "string"),
                ("checked.<menu>.<item>", "bool"),
                ("enabled.<menu>.<item>", "bool"),
                ("send", "string"),
                ("key", "string"),
            ]
        );
    }

    // ----- R805: stateful item taxonomy -----

    /// A menu mixing every R805 item kind:
    /// `[Command, Checkbox(off), Separator, Checkbox(on), Command(disabled)]`.
    fn rich() -> MenuBar {
        MenuBar::with_items(vec![vec![
            MenuItem::command(),
            MenuItem::checkbox(false),
            MenuItem::separator(),
            MenuItem::checkbox(true),
            MenuItem::command().disabled(),
        ]])
    }

    #[test]
    fn r805_new_builds_all_command_items() {
        let m = MenuBar::new(vec![2]);
        assert_eq!(m.item(0, 0), Some(MenuItem::command()));
        assert_eq!(m.item(0, 1), Some(MenuItem::command()));
        assert_eq!(m.item(0, 2), None);
    }

    #[test]
    fn r805_is_navigable_classifies_kinds() {
        assert!(MenuItem::command().is_navigable());
        assert!(MenuItem::checkbox(false).is_navigable());
        assert!(!MenuItem::separator().is_navigable());
        assert!(!MenuItem::command().disabled().is_navigable());
    }

    #[test]
    fn r805_activate_checkbox_toggles_and_persists() {
        let mut m = rich();
        m.toggle_title(0);
        // Activate the unchecked checkbox (item 1) -> toggles on + closes.
        assert_eq!(m.activate_item(1), Some((0, 1)));
        assert!(m.item(0, 1).unwrap().checked, "checkbox toggled on");
        assert_eq!(m.open_menu(), None, "activation closes");
        // Reopen: the checked state survived.
        m.toggle_title(0);
        assert!(m.item(0, 1).unwrap().checked, "checked persists across reopen");
        // Toggle it back off.
        assert_eq!(m.activate_item(1), Some((0, 1)));
        assert!(!m.item(0, 1).unwrap().checked, "checkbox toggled off");
    }

    #[test]
    fn r805_separator_and_disabled_are_inert() {
        let mut m = rich();
        m.toggle_title(0);
        assert_eq!(m.activate_item(2), None, "separator not activatable");
        assert_eq!(m.open_menu(), Some(0), "inert activate leaves menu open");
        assert_eq!(m.activate_item(4), None, "disabled not activatable");
        assert_eq!(m.open_menu(), Some(0));
        // Hover skips both.
        m.hover_item(2);
        assert_eq!(m.active_item(), None, "hover ignores separator");
        m.hover_item(4);
        assert_eq!(m.active_item(), None, "hover ignores disabled");
        m.hover_item(3);
        assert_eq!(m.active_item(), Some(3), "hover lands on the checkbox");
    }

    #[test]
    fn r805_keyboard_nav_skips_separator_and_disabled() {
        let mut m = rich();
        m.toggle_title(0);
        // Down from None -> first navigable (item 0, a command).
        m.move_active(true);
        assert_eq!(m.active_item(), Some(0));
        // Down -> item 1 (checkbox); next Down skips the separator (2) and
        // lands on item 3 (checkbox); the disabled item 4 is never reached,
        // so a further Down wraps back to item 0.
        m.move_active(true);
        assert_eq!(m.active_item(), Some(1));
        m.move_active(true);
        assert_eq!(m.active_item(), Some(3), "skips separator at 2");
        m.move_active(true);
        assert_eq!(m.active_item(), Some(0), "skips disabled 4, wraps to 0");
        // Up from 0 wraps backward past the disabled tail to item 3.
        m.move_active(false);
        assert_eq!(m.active_item(), Some(3), "Up wraps past disabled to last navigable");
    }

    #[test]
    fn r805_active_edge_lands_on_navigable_bounds() {
        let mut m = rich();
        m.toggle_title(0);
        m.active_edge(true);
        assert_eq!(m.active_item(), Some(3), "End -> last navigable (3, not disabled 4)");
        m.active_edge(false);
        assert_eq!(m.active_item(), Some(0), "Home -> first navigable (0)");
    }

    #[test]
    fn r805_open_focused_highlights_first_navigable() {
        // A menu whose first two items are a separator + disabled command.
        let mut m = MenuBar::with_items(vec![vec![
            MenuItem::separator(),
            MenuItem::command().disabled(),
            MenuItem::command(),
        ]]);
        m.open_focused();
        assert_eq!(m.active_item(), Some(2), "first navigable skips leading non-nav rows");
    }

    #[test]
    fn r805_external_query_item_slots() {
        let e = MenuBarExternal::with_items(vec![vec![
            MenuItem::command(),
            MenuItem::checkbox(true),
            MenuItem::separator(),
            MenuItem::command().disabled(),
        ]]);
        assert_eq!(e.query("item_kind.0.0"), Some(IntrospectValue::Text("command".into())));
        assert_eq!(e.query("item_kind.0.1"), Some(IntrospectValue::Text("checkbox".into())));
        assert_eq!(e.query("item_kind.0.2"), Some(IntrospectValue::Text("separator".into())));
        assert_eq!(e.query("checked.0.1"), Some(IntrospectValue::Bool(true)));
        assert_eq!(e.query("checked.0.0"), Some(IntrospectValue::Bool(false)));
        assert_eq!(e.query("enabled.0.0"), Some(IntrospectValue::Bool(true)));
        assert_eq!(e.query("enabled.0.3"), Some(IntrospectValue::Bool(false)));
        assert_eq!(e.query("checked.0.9"), None, "out-of-range item -> None");
        assert_eq!(e.query("checked.9.0"), None, "out-of-range menu -> None");
    }

    #[test]
    fn r805_external_intervene_checked_round_trip() {
        let mut e = MenuBarExternal::with_items(vec![vec![MenuItem::checkbox(false)]]);
        e.intervene("checked.0.0", IntrospectValue::Bool(true)).unwrap();
        assert_eq!(e.query("checked.0.0"), Some(IntrospectValue::Bool(true)));
        assert!(!e.is_dirty(), "intervene fires no command intent");
        assert_eq!(
            e.intervene("checked.0.9", IntrospectValue::Bool(true)),
            Err(InterveneError::OutOfRange)
        );
        assert_eq!(
            e.intervene("checked.0.0", IntrospectValue::Int(1)),
            Err(InterveneError::TypeMismatch)
        );
    }

    #[test]
    fn r805_external_send_checkbox_activation_toggles_and_emits() {
        let mut e = MenuBarExternal::with_items(vec![vec![
            MenuItem::command(),
            MenuItem::checkbox(false),
        ]]);
        e.invoke("send", IntrospectValue::Text("t0:PointerUp".into())).unwrap();
        // Click the checkbox (item 1).
        for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
            e.invoke("send", IntrospectValue::Text(format!("i1:{ev}"))).unwrap();
        }
        assert!(e.item(0, 1).unwrap().checked, "checkbox toggled on via send");
        assert_eq!(e.open_menu(), None, "activation closes");
        let mut got = Vec::new();
        e.drain_intents(&mut |i| got.push(i));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].payload, IntrospectValue::Text("0.1".into()));
    }
}
