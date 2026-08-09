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
//! - `open_path: Vec<usize>` — R985: the chain of [`MenuItem::Submenu`]
//!   item indices open *below* the top dropdown, descending. Empty when
//!   only the top dropdown is open; `[2]` means item 2 of the top menu
//!   (a submenu) is expanded; `[2, 1]` adds a further nested level. The
//!   *current* menu (where the keyboard cursor lives) is the deepest one
//!   reached by following `open` then `open_path`.
//! - `active: Option<usize>` — the highlighted item within the *current*
//!   (deepest open) menu (the WAI-ARIA active descendant; set by hover or
//!   Arrow keys).
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
//! * `invoke("send", "i<path>:<PointerWireEvent>")` — a dropdown **item**
//!   pointer event (composite tag `<bar>#i<path>`). R985 — `<path>` is the
//!   descent **relative to the open dropdown** (the open menu is implicit
//!   because you can only click a *visible* item): `i0` is top item 0,
//!   `i1.2` is item 2 of submenu 1. `PointerEnter` highlights (collapsing
//!   any deeper-open submenus to that level); `PointerUp` activates — a
//!   submenu *opens*, a leaf emits `"command"` and closes.
//! * `invoke("key", "<W3CKeyName>")` — the full WAI-ARIA §3.5 / §3.16
//!   menubar keyboard model in one place (Arrow Left/Right between titles,
//!   Arrow Up/Down + Home/End within an open menu, Arrow Right opens a
//!   submenu / moves to the adjacent menu on a leaf, Arrow Left / Escape
//!   close one submenu level, Enter/Space to activate). Returns
//!   [`IntrospectValue::Bool`] = "did this key do anything".
//!
//! **Addressing asymmetry (deliberate):** the read slots (`item_kind` /
//! `item_count` / `checked` / `enabled`) take an **absolute** `<path>`
//! (`<menu>` then descent, e.g. `item_kind.0.1.2`) because they inspect the
//! *static* structure of any menu; `send` takes an **open-relative** path
//! because a pointer can only reach the *currently visible* cascade (menu
//! implicit). The full open state is itself readable — `open` (top index),
//! `open_path` (dotted submenu descent), `active_path` (the absolute path
//! of the highlighted item) — so an AI client never has to guess.
//!
//! On leaf activation the §5.20 channel emits a `"command"` intent whose
//! payload is [`IntrospectValue::Text`] the **absolute** dotted path
//! (`"<menu>.<item>"`, R985 `"<menu>.<item>.<sub>…"` for a nested leaf); the
//! runtime walk prefixes the scene-side `ExternalNode` tag (e.g.
//! `"menu"` → `"menu.command"`) so widget identity stays decoupled
//! from the UI-chosen tag, exactly like `Button::"click"`.
//!
//! ## Dismiss paths + future axes (per [[abstraction-needs-second-consumer]])
//!
//! An open menu dismisses via **Escape**, **re-clicking the open
//! title**, **activating an item**, or (R715) a **click-outside** on the
//! transparent dismiss barrier. The barrier is the shared light-dismiss
//! layer (`pinion_widget_paint::barrier`); the binding paints it as a
//! `<bar>#barrier` composite node behind the open dropdown and the
//! R51.42 router feeds its `PointerUp` here as `send("barrier:…")`,
//! which closes the menu (R985 — including any open submenus). *Focus-loss
//! dismiss* remains an additive axis once a real consumer surfaces it.
//! Likewise *mouse hover-follow between top-level titles while a menu is
//! open*, *`menuitemradio` stateful entries*, and *accelerator / mnemonic
//! keys* are additive axes once a real consumer needs them.
//!
//! ## R985 — cascading submenus (WAI-ARIA §3.16)
//!
//! A [`MenuItem::Submenu`] opens a nested menu instead of firing a
//! command. The keyboard model follows the WAI-ARIA menubar pattern:
//! **Arrow Right** on a submenu parent opens it (focus its first item);
//! **Arrow Right** on a leaf closes any open submenus and moves to the
//! next top-level menu; **Arrow Left** / **Escape** in a submenu closes
//! just that level and returns focus to its parent item; **Arrow Left**
//! at the top dropdown level moves to the previous top-level menu;
//! **Enter / Space** on a submenu parent opens it, on a leaf activates it.
//! A submenu opens on a **discrete event** (click or Arrow Right), never
//! on a hover timer — hover only *highlights* — so the model stays
//! deterministic / ZERO-FLAKE; hover-open-on-delay is an additive axis.

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaArg, SchemaField,
    ThreadOwnership,
};
use crate::input::PointerWireEvent;
use crate::intent::Intent;
use crate::widgets::IntentEmitter;
use crate::widgets::menu_nav;
use crate::widgets::wire::resolve_index;

/// R805 §5.40 — one dropdown item: the WAI-ARIA 1.2 menu-item taxonomy.
/// A sum type so illegal states are unrepresentable (R805.1 audit: the
/// prior `{ kind, checked, enabled }` struct let a `Separator` carry a
/// `checked` flag, and `intervene("checked", …)` could set it on a
/// non-checkbox — both nonsensical). `checked` now exists *only* on
/// [`Self::Checkbox`]; `enabled` only on the interactive variants; a
/// [`Self::Separator`] carries neither.
///
/// R985 — [`Self::Submenu`] adds nested menus (WAI-ARIA §3.16 menubar
/// cascade). It owns a `Vec<MenuItem>` so the type is recursive, which
/// drops the `Copy` derive (a `Vec` is not `Copy`); every call site
/// constructs fresh or matches by reference, so `Clone` suffices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuItem {
    /// A one-shot command (WAI-ARIA `menuitem`): activating it fires the
    /// `"command"` intent and dismisses the menu. `enabled == false`
    /// greys it and makes it inert.
    Command {
        /// Whether the command is interactive.
        enabled: bool,
    },
    /// A toggle (WAI-ARIA `menuitemcheckbox`): activating it flips
    /// `checked` (which survives close / reopen), fires `"command"`, and
    /// dismisses.
    Checkbox {
        /// Persistent checked state.
        checked: bool,
        /// Whether the toggle is interactive.
        enabled: bool,
    },
    /// A non-interactive divider (WAI-ARIA `separator`): never navigable,
    /// never activatable — it only sections the dropdown visually.
    Separator,
    /// R985 — a nested submenu (WAI-ARIA §3.16 `menuitem` with
    /// `aria-haspopup="menu"`): activating it does *not* fire `"command"`
    /// or dismiss; it *opens* the child menu (the cascade descends a
    /// level). `enabled == false` greys it and makes it inert.
    Submenu {
        /// Whether the submenu can be opened.
        enabled: bool,
        /// The nested item list shown when this submenu is open.
        items: Vec<MenuItem>,
    },
}

impl MenuItem {
    /// A one-shot enabled command item.
    #[must_use]
    pub const fn command() -> Self {
        Self::Command { enabled: true }
    }

    /// An enabled checkbox item with the given initial checked state.
    #[must_use]
    pub const fn checkbox(checked: bool) -> Self {
        Self::Checkbox {
            checked,
            enabled: true,
        }
    }

    /// A non-interactive divider.
    #[must_use]
    pub const fn separator() -> Self {
        Self::Separator
    }

    /// R985 — an enabled submenu carrying the given nested item list.
    #[must_use]
    pub fn submenu(items: Vec<MenuItem>) -> Self {
        Self::Submenu {
            enabled: true,
            items,
        }
    }

    /// Builder: mark this item disabled (greyed, skipped, non-activatable).
    /// A [`Self::Separator`] is already inert, so this is a no-op there.
    #[must_use]
    pub fn disabled(self) -> Self {
        match self {
            Self::Command { .. } => Self::Command { enabled: false },
            Self::Checkbox { checked, .. } => Self::Checkbox {
                checked,
                enabled: false,
            },
            Self::Separator => Self::Separator,
            Self::Submenu { items, .. } => Self::Submenu {
                enabled: false,
                items,
            },
        }
    }

    /// Whether keyboard / hover navigation may land on this item, and
    /// whether activation has any effect: every enabled non-separator
    /// (R985 — an enabled [`Self::Submenu`] is navigable; landing on it
    /// then activating opens it).
    #[must_use]
    pub const fn is_navigable(&self) -> bool {
        matches!(
            self,
            Self::Command { enabled: true }
                | Self::Checkbox { enabled: true, .. }
                | Self::Submenu { enabled: true, .. }
        )
    }

    /// R985 — whether this item opens a nested submenu.
    #[must_use]
    pub const fn is_submenu(&self) -> bool {
        matches!(self, Self::Submenu { .. })
    }

    /// R985 — the nested item list for a [`Self::Submenu`], else `None`.
    #[must_use]
    pub fn submenu_items(&self) -> Option<&[MenuItem]> {
        match self {
            Self::Submenu { items, .. } => Some(items),
            _ => None,
        }
    }

    /// The persistent checked state (`false` for non-checkbox items).
    #[must_use]
    pub const fn checked(&self) -> bool {
        matches!(self, Self::Checkbox { checked: true, .. })
    }

    /// Whether the item is enabled (`false` for a separator).
    #[must_use]
    pub const fn enabled(&self) -> bool {
        match self {
            Self::Command { enabled }
            | Self::Checkbox { enabled, .. }
            | Self::Submenu { enabled, .. } => *enabled,
            Self::Separator => false,
        }
    }

    /// The WAI-ARIA wire name for this item's role (`item_kind` query).
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Command { .. } => "command",
            Self::Checkbox { .. } => "checkbox",
            Self::Separator => "separator",
            Self::Submenu { .. } => "submenu",
        }
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
    /// R985 — the [`MenuItem::Submenu`] item indices open below the top
    /// dropdown, descending (empty when only the top dropdown is open).
    /// See the module-level state model.
    open_path: Vec<usize>,
    /// Highlighted item within the *current* (deepest open) menu (the
    /// WAI-ARIA active descendant), or `None`.
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
        Self {
            menus,
            open: None,
            open_path: Vec::new(),
            active: None,
            bar_focus: 0,
        }
    }

    /// R805 — construct a menubar from explicit per-menu [`MenuItem`]
    /// lists (checkbox / separator / disabled entries). All menus start
    /// closed.
    #[must_use]
    pub fn with_items(menus: Vec<Vec<MenuItem>>) -> Self {
        Self {
            menus,
            open: None,
            open_path: Vec::new(),
            active: None,
            bar_focus: 0,
        }
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

    /// R805 — the [`MenuItem`] at `(menu, item)` of a *top-level* menu, or
    /// `None` when either index is out of range. R985 — for nested items use
    /// [`Self::item_at_path`]; this two-index accessor is the original
    /// top-level convenience (clones since [`MenuItem`] is no longer `Copy`).
    #[must_use]
    pub fn item(&self, menu: usize, item: usize) -> Option<MenuItem> {
        self.menus.get(menu)?.get(item).cloned()
    }

    /// R985 — the item list of the menu reached by `path`: `[m]` is top
    /// menu `m`; each further index descends into a [`MenuItem::Submenu`].
    /// `None` when any index is out of range or a non-submenu is descended.
    #[must_use]
    pub fn menu_items_at(&self, path: &[usize]) -> Option<&[MenuItem]> {
        let (&menu, rest) = path.split_first()?;
        walk_submenus(self.menus.get(menu)?, rest)
    }

    /// R985 — the [`MenuItem`] at an absolute `path` (`[menu, item, …]`,
    /// length ≥ 2), or `None` when the path is too short or out of range.
    #[must_use]
    pub fn item_at_path(&self, path: &[usize]) -> Option<&MenuItem> {
        let (&last, parent) = path.split_last()?;
        self.menu_items_at(parent)?.get(last)
    }

    /// R985 — mutable [`MenuItem`] at an absolute `path` (`[menu, item, …]`).
    fn item_at_path_mut(&mut self, path: &[usize]) -> Option<&mut MenuItem> {
        let (&last, parent) = path.split_last()?;
        let (&menu, rest) = parent.split_first()?;
        walk_submenus_mut(self.menus.get_mut(menu)?, rest)?.get_mut(last)
    }

    /// R985 — the item slice of the *current* (deepest open) menu, or
    /// `None` when no menu is open / the open path is inconsistent.
    fn current_items(&self) -> Option<&[MenuItem]> {
        self.rel_items_at(&self.open_path)
    }

    /// R985 — the item slice reached from the open top menu by `rel_prefix`
    /// (a descent *relative to* the open dropdown, the wire `i<path>` form).
    fn rel_items_at(&self, rel_prefix: &[usize]) -> Option<&[MenuItem]> {
        walk_submenus(self.menus.get(self.open?)?, rel_prefix)
    }

    /// The open dropdown's menu index, or `None`.
    #[must_use]
    pub fn open_menu(&self) -> Option<usize> {
        self.open
    }

    /// R985 — the open submenu descent below the top dropdown (empty when
    /// only the top dropdown is open).
    #[must_use]
    pub fn open_path(&self) -> &[usize] {
        &self.open_path
    }

    /// R985 — whether the cursor is inside a nested submenu (the open path
    /// is non-empty).
    #[must_use]
    pub fn in_submenu(&self) -> bool {
        !self.open_path.is_empty()
    }

    /// The highlighted item within the current (deepest open) menu, or `None`.
    #[must_use]
    pub fn active_item(&self) -> Option<usize> {
        self.active
    }

    /// R985 — the absolute path of the active item (`[menu, …open_path…,
    /// active]`), or `None` when nothing is open / highlighted.
    #[must_use]
    pub fn active_path(&self) -> Option<Vec<usize>> {
        let m = self.open?;
        let a = self.active?;
        Some(self.abs_path(m, a))
    }

    /// The focused top-level title (keyboard cursor).
    #[must_use]
    pub fn bar_focus(&self) -> usize {
        self.bar_focus
    }

    /// R985 — whether the current active item opens a submenu.
    fn active_is_submenu(&self) -> bool {
        self.active
            .and_then(|a| self.current_items()?.get(a))
            .is_some_and(MenuItem::is_submenu)
    }

    /// R985 — build the absolute path `[menu, …open_path…, item]`.
    fn abs_path(&self, menu: usize, item: usize) -> Vec<usize> {
        let mut p = Vec::with_capacity(self.open_path.len() + 2);
        p.push(menu);
        p.extend_from_slice(&self.open_path);
        p.push(item);
        p
    }

    /// Mouse: toggle menu `m`. Re-toggling the open menu closes it;
    /// toggling a different (or closed) menu opens `m` with no item
    /// pre-highlighted (pointer ergonomics). Out-of-range `m` is a
    /// no-op. R985 — opening resets the submenu descent.
    fn toggle_title(&mut self, m: usize) {
        if m >= self.menu_count() {
            return;
        }
        if self.open == Some(m) {
            self.close();
        } else {
            self.open = Some(m);
            self.open_path.clear();
            self.active = None;
            self.bar_focus = m;
        }
    }

    /// R1543 §5.39 — **keyboard** activation of top-level title `m`: the
    /// WAI-ARIA §3.5 menubar rule that <kbd>Enter</kbd> / <kbd>Space</kbd> on a
    /// menubar item opens its menu *and moves focus to the first item*, reached
    /// here from a mnemonic (<kbd>Alt</kbd>+F) rather than from the already-focused
    /// title.
    ///
    /// Distinct from [`Self::toggle_title`] — the pointer path — in exactly the way the APG
    /// distinguishes them: a mouse open pre-highlights nothing (the pointer is
    /// the cursor), a keyboard open highlights the first navigable item (the
    /// keyboard needs one). Re-activating the open menu closes it, which is
    /// both the pointer path's behaviour and the toolkit's.
    fn activate_title(&mut self, m: usize) {
        if m >= self.menu_count() {
            return;
        }
        if self.open == Some(m) {
            self.close();
            return;
        }
        self.bar_focus = m;
        self.open_focused();
    }

    /// R985 — mouse: point at the item reached by `rel_path` (a descent
    /// relative to the open dropdown). Collapses any deeper-open submenus
    /// to this level (moving back to a shallower item closes the ones
    /// below it) and highlights the target if navigable. Inert for an
    /// unreachable / non-navigable target.
    fn point_to(&mut self, rel_path: &[usize]) {
        let Some((&last, prefix)) = rel_path.split_last() else {
            return;
        };
        let nav = self
            .rel_items_at(prefix)
            .is_some_and(|items| items.get(last).is_some_and(MenuItem::is_navigable));
        if !nav {
            return;
        }
        self.open_path = prefix.to_vec();
        self.active = Some(last);
    }

    /// Activate item `i` of the *current* menu. R985 — a [`MenuItem::Submenu`]
    /// opens (descends a level) and returns `None` (no command); a leaf
    /// command / checkbox flips its `checked` state, closes the whole menu,
    /// and returns `Some(absolute_path)` so the caller emits `"command"`.
    /// A separator / disabled / out-of-range item is inert (`None`).
    fn activate_item(&mut self, i: usize) -> Option<Vec<usize>> {
        let m = self.open?;
        let (navigable, is_sub) = {
            let item = self.current_items()?.get(i)?;
            (item.is_navigable(), item.is_submenu())
        };
        if !navigable {
            return None;
        }
        if is_sub {
            self.descend_into(i);
            return None;
        }
        let path = self.abs_path(m, i);
        if let Some(MenuItem::Checkbox { checked, .. }) =
            self.current_items_mut().and_then(|items| items.get_mut(i))
        {
            *checked = !*checked;
        }
        self.close();
        self.bar_focus = m;
        Some(path)
    }

    /// R985 — mutable item slice of the current (deepest open) menu.
    fn current_items_mut(&mut self) -> Option<&mut Vec<MenuItem>> {
        let m = self.open?;
        let path = self.open_path.clone();
        walk_submenus_mut(self.menus.get_mut(m)?, &path)
    }

    /// R985 — mouse: activate the item reached by `rel_path` (relative to
    /// the open dropdown). Collapses to the target's level first, then
    /// activates it (a submenu opens, a leaf fires `"command"`). Returns
    /// the activated leaf's absolute path, or `None` (submenu / inert).
    fn activate_rel(&mut self, rel_path: &[usize]) -> Option<Vec<usize>> {
        let (&last, prefix) = rel_path.split_last()?;
        // R985.1 — validate the WHOLE target (prefix reachable AND `last` a
        // navigable item) BEFORE mutating `open_path`. A malformed nested send
        // (e.g. `i1.99` for an out-of-range item) must not collapse the cascade
        // to `prefix` as a side-effect (validate-before-mutate invariant).
        let navigable = self
            .rel_items_at(prefix)?
            .get(last)
            .is_some_and(MenuItem::is_navigable);
        if !navigable {
            return None;
        }
        self.open_path = prefix.to_vec();
        self.activate_item(last)
    }

    /// R985 — keyboard: activate the current active item (Enter / Space).
    /// Delegates to [`Self::activate_item`] (a submenu opens, a leaf fires).
    fn activate_active(&mut self) -> Option<Vec<usize>> {
        let a = self.active?;
        self.activate_item(a)
    }

    /// R985 — open the [`MenuItem::Submenu`] at current-menu index `i`,
    /// descending a level and highlighting the submenu's first navigable
    /// item. No-op if `i` is not a submenu. R985.1 — single-pass: the outer
    /// `Some` of `first` IS the "`i` is a submenu" verdict (its `submenu_items`
    /// returned `Some`), so no separate `is_submenu` re-walk is needed.
    fn descend_into(&mut self, i: usize) {
        let first = self
            .current_items()
            .and_then(|items| items.get(i))
            .and_then(MenuItem::submenu_items)
            .map(|sub| menu_nav::nav_edge_skip(sub.len(), false, |k| sub[k].is_navigable()));
        if let Some(active) = first {
            self.open_path.push(i);
            self.active = active;
        }
    }

    /// R985 — keyboard: open the active submenu (Arrow Right on a parent).
    fn descend_active(&mut self) {
        if let Some(a) = self.active {
            if self.active_is_submenu() {
                self.descend_into(a);
            }
        }
    }

    /// R985 — close the deepest open submenu, returning focus to its parent
    /// item (Arrow Left / Escape inside a submenu). `true` when a level was
    /// closed.
    fn ascend(&mut self) -> bool {
        if let Some(parent) = self.open_path.pop() {
            self.active = Some(parent);
            true
        } else {
            false
        }
    }

    /// Close every menu (and any open submenus). `bar_focus` is preserved
    /// (Escape returns the keyboard cursor to the title that was open).
    fn close(&mut self) {
        self.open = None;
        self.open_path.clear();
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
        self.open_path.clear();
        self.active = self.first_navigable(m);
    }

    /// Keyboard (open): switch to the adjacent top-level menu and open it
    /// with its first navigable item highlighted (Arrow Right/Left at the
    /// top dropdown level — the WAI-ARIA menubar cross-navigation). R985 —
    /// closes any open submenus first.
    fn menu_switch(&mut self, forward: bool) {
        let Some(m) = self.open else { return };
        let n = self.menu_count();
        if n == 0 {
            return;
        }
        let next = menu_nav::step(m, forward, n);
        self.open = Some(next);
        self.open_path.clear();
        self.bar_focus = next;
        self.active = self.first_navigable(next);
    }

    /// Keyboard (open): move the active item by one with wrap, skipping
    /// separators / disabled rows (R805). With no active item, Down lands
    /// on the first navigable item and Up on the last. R985 — operates on
    /// the current (deepest open) menu.
    fn move_active(&mut self, forward: bool) {
        let next = {
            let Some(items) = self.current_items() else {
                return;
            };
            menu_nav::nav_move_skip(self.active, items.len(), forward, |i| {
                items[i].is_navigable()
            })
        };
        self.active = next;
    }

    /// Keyboard (open): jump the active item to the first / last
    /// **navigable** entry of the current menu (Home / End).
    fn active_edge(&mut self, last: bool) {
        let edge = {
            let Some(items) = self.current_items() else {
                return;
            };
            menu_nav::nav_edge_skip(items.len(), last, |i| items[i].is_navigable())
        };
        if let Some(a) = edge {
            self.active = Some(a);
        }
    }

    /// The first navigable item of top menu `m` (skipping leading
    /// separators / disabled rows), or `None` when none is navigable.
    fn first_navigable(&self, m: usize) -> Option<usize> {
        let items = &self.menus[m];
        menu_nav::nav_edge_skip(items.len(), false, |i| items[i].is_navigable())
    }

    /// R985 — validate a candidate `open_path`: every index must address an
    /// enabled [`MenuItem::Submenu`] reachable from the open top menu.
    fn is_valid_open_path(&self, path: &[usize]) -> bool {
        let Some(m) = self.open else { return false };
        let mut items: &[MenuItem] = match self.menus.get(m) {
            Some(i) => i,
            None => return false,
        };
        for &idx in path {
            match items.get(idx) {
                Some(MenuItem::Submenu {
                    enabled: true,
                    items: sub,
                }) => items = sub,
                _ => return false,
            }
        }
        true
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

    /// R985 — item count of the menu reached by `path` (`[m]` = top menu,
    /// further indices descend into submenus), or `None` out of range.
    #[must_use]
    pub fn item_count_at(&self, path: &[usize]) -> Option<usize> {
        self.em.inner.menu_items_at(path).map(<[MenuItem]>::len)
    }

    /// R805 — the [`MenuItem`] at `(menu, item)`, or `None` out of range.
    #[must_use]
    pub fn item(&self, menu: usize, item: usize) -> Option<MenuItem> {
        self.em.inner.item(menu, item)
    }

    /// R985 — the [`MenuItem`] at an absolute `path` (`[menu, item, …]`).
    #[must_use]
    pub fn item_at_path(&self, path: &[usize]) -> Option<&MenuItem> {
        self.em.inner.item_at_path(path)
    }

    /// The open dropdown's menu index, or `None`.
    #[must_use]
    pub fn open_menu(&self) -> Option<usize> {
        self.em.inner.open_menu()
    }

    /// R985 — the open submenu descent below the top dropdown.
    #[must_use]
    pub fn open_path(&self) -> &[usize] {
        self.em.inner.open_path()
    }

    /// The highlighted item within the current (deepest open) menu, or `None`.
    #[must_use]
    pub fn active_item(&self) -> Option<usize> {
        self.em.inner.active_item()
    }

    /// R985 — the absolute path of the active item, or `None`.
    #[must_use]
    pub fn active_path(&self) -> Option<Vec<usize>> {
        self.em.inner.active_path()
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

    /// Drive an item pointer event (`rel_path`, `event`) — R985: `rel_path`
    /// is the descent relative to the open dropdown (`[i]` for a top item,
    /// `[i, j]` for item `j` of submenu `i`). `PointerEnter` highlights /
    /// collapses to that level; `PointerUp` activates (a submenu opens, a
    /// leaf emits `"command"`).
    fn send_item(&mut self, rel_path: &[usize], event: PointerWireEvent) {
        match event {
            PointerWireEvent::Enter => self.em.inner.point_to(rel_path),
            PointerWireEvent::Up => {
                if let Some(path) = self.em.inner.activate_rel(rel_path) {
                    self.emit_command(&path);
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
                // R985 — Arrow Right opens the active submenu; on a leaf it
                // closes any submenus and moves to the next top-level menu
                // (WAI-ARIA §3.16).
                "ArrowRight" => {
                    if self.em.inner.active_is_submenu() {
                        self.em.inner.descend_active();
                    } else {
                        self.em.inner.menu_switch(true);
                    }
                    true
                }
                // R985 — Arrow Left inside a submenu closes that level
                // (focus returns to its parent); at the top dropdown level it
                // moves to the previous top-level menu.
                "ArrowLeft" => {
                    if self.em.inner.in_submenu() {
                        self.em.inner.ascend();
                    } else {
                        self.em.inner.menu_switch(false);
                    }
                    true
                }
                "Enter" | "Space" => {
                    if let Some(path) = self.em.inner.activate_active() {
                        self.emit_command(&path);
                    }
                    true
                }
                // R985 — Escape closes one level: a nested submenu collapses
                // to its parent; the top dropdown closes entirely.
                "Escape" => {
                    if self.em.inner.in_submenu() {
                        self.em.inner.ascend();
                    } else {
                        self.em.inner.close();
                    }
                    true
                }
                _ => false,
            },
        }
    }

    /// Push the `"command"` intent for an activated leaf item, payload the
    /// dot-joined absolute path (`"<menu>.<item>"`, R985 `"<menu>.<item>.<sub>"`
    /// for a nested leaf).
    fn emit_command(&mut self, path: &[usize]) {
        self.em.push(Intent::new_static(
            "command",
            IntrospectValue::Text(path_text(path)),
        ));
    }

    /// Shared parse for the `send` wire payload `"<sub>:<EventName>"`
    /// where `<sub>` is `t<m>` (title), `i<i>` (item), or the literal
    /// `barrier` (R715 click-outside dismiss). Returns the open index as
    /// the round-trip outcome.
    fn dispatch_send(&mut self, payload: &str) -> Result<IntrospectValue, InvokeError> {
        // R880.1 — decode via the `split_send_payload` `:` grammar SSOT so a
        // modifier-held release ("barrier:PointerUp:c") still parses (the
        // hand-rolled split_once read "PointerUp:c" as the event name and a
        // Ctrl+click outside the dropdown failed to dismiss it).
        let (sub, event_name, _mods) =
            crate::composite_tag::require_send_payload("menu.send", payload)?;
        // R1543 §5.39 — `KeyboardActivate` is not a pointer event and must be
        // decoded before the pointer vocabulary, which would reject it. It is
        // the wire a MNEMONIC arrives on: the shell resolves `Alt+F` to the
        // paint tag `<bar>#t0` and re-composes it through the same R51.42
        // grammar the router uses for a click, so the accelerator reaches this
        // widget by the door every other activation uses. Titles only — a
        // dropdown item is reachable only while the menu is open, and the
        // WAI-ARIA §3.5 keyboard model already owns that traversal.
        //
        // Both a title (`t<m>`) and a dropdown item (`i<path>`) answer it: an
        // item's mnemonic is declared on a label that is only painted while its
        // dropdown is open, so the accelerator exists exactly when the item
        // does. Nothing has to register or unregister as menus open and close,
        // which is the whole reason the registry is derived from the paint
        // scene.
        if event_name == "KeyboardActivate" {
            let mut chars = sub.chars();
            match (chars.next(), chars.as_str()) {
                (Some('t'), rest) => {
                    let index: usize = rest.parse().map_err(|_| {
                        InvokeError::rejected(format!(
                            "menu.send: title target {sub:?} carries no index \
                             ({rest:?} is not a number)"
                        ))
                    })?;
                    self.em.inner.activate_title(index);
                }
                (Some('i'), rest) => {
                    let path = parse_path(rest).ok_or_else(|| {
                        InvokeError::rejected(format!(
                            "menu.send: item target {sub:?} is not a dotted descent path"
                        ))
                    })?;
                    if let Some(full) = self.em.inner.activate_rel(&path) {
                        self.emit_command(&full);
                    }
                }
                _ => {
                    return Err(InvokeError::rejected(format!(
                        "menu.send: KeyboardActivate target {sub:?} names no sub-element \
                         (expected \"t<index>\" or \"i<path>\")"
                    )));
                }
            }
            return Ok(open_value(self.open_menu()));
        }
        let event = PointerWireEvent::from_wire_name(event_name).ok_or_else(|| {
            InvokeError::rejected(format!(
                "menu.send: {event_name:?} is neither a pointer event name nor KeyboardActivate"
            ))
        })?;
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
        let kind = chars.next().ok_or_else(|| {
            InvokeError::rejected(
                "menu.send: empty target (expected \"t<index>\", \"i<path>\" or \"barrier\")",
            )
        })?;
        let rest = chars.as_str();
        match kind {
            // Title: a single top-level menu index.
            't' => {
                let idx: usize = rest.parse().map_err(|_| {
                    InvokeError::rejected(format!(
                        "menu.send: title target {sub:?} carries no index \
                         ({rest:?} is not a number)"
                    ))
                })?;
                self.send_title(idx, event);
            }
            // R985 — item: a dotted descent path relative to the open
            // dropdown (`i2` top item, `i2.0` item 0 of submenu 2, …).
            'i' => {
                let path = parse_path(rest).ok_or_else(|| {
                    InvokeError::rejected(format!(
                        "menu.send: item target {sub:?} is not a dotted descent path"
                    ))
                })?;
                self.send_item(&path, event);
            }
            _ => {
                return Err(InvokeError::rejected(format!(
                    "menu.send: target {sub:?} names no sub-element \
                     (expected \"t<index>\", \"i<path>\" or \"barrier\")"
                )));
            }
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

/// R985 — descend a submenu chain: from `start`, follow each `descent`
/// index into the [`MenuItem::Submenu`] it addresses, returning the item
/// slice reached. `None` if any index is out of range or a non-submenu is
/// descended. The shared inner walk behind [`MenuBar::menu_items_at`] /
/// `rel_items_at` / `current_items` (one home for the cascade descent).
fn walk_submenus<'a>(start: &'a [MenuItem], descent: &[usize]) -> Option<&'a [MenuItem]> {
    let mut items = start;
    for &p in descent {
        items = items.get(p)?.submenu_items()?;
    }
    Some(items)
}

/// R985.1 — the mutable mirror of [`walk_submenus`]: descend a submenu chain
/// by `descent`, returning the deepest item list `&mut`. Lifted at the 2nd
/// consumer ([`MenuBar::item_at_path_mut`] / `current_items_mut`) so the
/// mutable descent cannot diverge from the immutable one (a divergence would
/// be a navigation bug, not a style choice — the R743.1 lift class).
fn walk_submenus_mut<'a>(
    start: &'a mut Vec<MenuItem>,
    descent: &[usize],
) -> Option<&'a mut Vec<MenuItem>> {
    let mut items = start;
    for &p in descent {
        items = match items.get_mut(p)? {
            MenuItem::Submenu { items: sub, .. } => sub,
            _ => return None,
        };
    }
    Some(items)
}

/// R985 / R985.1 — the **decode** half of the dotted item-path wire codec:
/// parse `"0"` / `"0.2"` / `"0.2.1"` into its component indices. `None` for an
/// empty string or a non-numeric component. `pub` so wire consumers (a
/// binding, an RPC client adapter) read the path format from its one home
/// rather than re-rolling the split — the inverse of [`path_text`]
/// (`decode == inverse(encode)`, [[wire-form-read-write-symmetry]]).
/// Generalises the R805 two-index `parse_menu_item` to arbitrary depth.
#[must_use]
pub fn parse_path(suffix: &str) -> Option<Vec<usize>> {
    if suffix.is_empty() {
        return None;
    }
    suffix.split('.').map(|p| p.parse::<usize>().ok()).collect()
}

/// `Some(i)` → `Int(i)`, `None` → `Null` — shared optional-index
/// lowering for `query` / the `send` return value.
fn open_value(idx: Option<usize>) -> IntrospectValue {
    match idx {
        Some(i) => IntrospectValue::Int(i64::try_from(i).expect("menu index fits in i64")),
        None => IntrospectValue::Null,
    }
}

/// R985 / R985.1 — the **encode** half of the dotted item-path wire codec:
/// lower a `&[usize]` path (`open_path` / `active_path` / a `"command"`
/// payload / a composite item tag) to its dotted `Text` form (empty slice →
/// `""`). `pub` so the one encode home is shared across crates (the paint
/// composite-tag builder, a binding) — the inverse of [`parse_path`].
#[must_use]
pub fn path_text(path: &[usize]) -> String {
    path.iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(".")
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
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("menu_count", "int"),
                    SchemaField::new("open", "int"),
                    // R985 — the open submenu descent + the full active path (dotted),
                    // so an AI client reads the whole cascade state, not just the top.
                    SchemaField::new("open_path", "string"),
                    SchemaField::new("active", "int"),
                    SchemaField::new("active_path", "string"),
                    SchemaField::new("bar_focus", "int"),
                    // R985 — `<path>` is a dotted index path (`<menu>` top, `<menu>.<item>`,
                    // `<menu>.<item>.<sub>` …), so the same slots address nested items.
                    // (R1353.1) `path` is a DOTTED INDEX PATH (`"0.1"` = item 1 of menu 0),
                    // not a scalar: its domain is hierarchical — each segment bounded by the
                    // item count of the level above. `ArgDomain` cannot express that, so this
                    // says nothing rather than something false. A second hierarchical family
                    // is what should force the variant.
                    SchemaField::parametric(
                        "item_count.<path>",
                        "int",
                        const { &[SchemaArg::open("path", "string")] },
                    ),
                    SchemaField::parametric(
                        "item_kind.<path>",
                        "string",
                        const { &[SchemaArg::open("path", "string")] },
                    ),
                    SchemaField::parametric(
                        "checked.<path>",
                        "bool",
                        const { &[SchemaArg::open("path", "string")] },
                    ),
                    SchemaField::parametric(
                        "enabled.<path>",
                        "bool",
                        const { &[SchemaArg::open("path", "string")] },
                    ),
                    SchemaField::action("send", "string"),
                    SchemaField::action("key", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "menu_count" => Some(IntrospectValue::Int(
                i64::try_from(self.menu_count()).expect("menu_count fits in i64"),
            )),
            "open" => Some(open_value(self.open_menu())),
            // R985 — dotted descent below the top dropdown (empty when none).
            "open_path" => Some(IntrospectValue::Text(path_text(self.open_path()))),
            "active" => Some(open_value(self.active_item())),
            // R985 — full absolute active path, or Null when nothing highlighted.
            "active_path" => Some(match self.active_path() {
                Some(p) => IntrospectValue::Text(path_text(&p)),
                None => IntrospectValue::Null,
            }),
            "bar_focus" => Some(IntrospectValue::Int(
                i64::try_from(self.bar_focus()).expect("bar_focus fits in i64"),
            )),
            _ => {
                // R985 — `item_count.<path>` addresses a *menu* (path of length
                // ≥1: `<menu>` = top, deeper = a submenu's item list).
                if let Some(suffix) = path.strip_prefix("item_count.") {
                    let p = parse_path(suffix)?;
                    let count = self.item_count_at(&p)?;
                    return Some(IntrospectValue::Int(
                        i64::try_from(count).expect("item_count fits in i64"),
                    ));
                }
                // R805 / R985 — per-item stateful slots, keyed by an absolute
                // item `<path>` (length ≥2: `<menu>.<item>[.<sub>…]`).
                if let Some(suffix) = path.strip_prefix("item_kind.") {
                    let p = parse_path(suffix)?;
                    return Some(IntrospectValue::Text(
                        self.item_at_path(&p)?.kind_name().to_owned(),
                    ));
                }
                if let Some(suffix) = path.strip_prefix("checked.") {
                    let p = parse_path(suffix)?;
                    return Some(IntrospectValue::Bool(self.item_at_path(&p)?.checked()));
                }
                if let Some(suffix) = path.strip_prefix("enabled.") {
                    let p = parse_path(suffix)?;
                    return Some(IntrospectValue::Bool(self.item_at_path(&p)?.enabled()));
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
                    let m = resolve_index("menu", i, self.menu_count())?;
                    self.em.inner.open = Some(m);
                    // R985 — opening a top menu resets the submenu descent.
                    self.em.inner.open_path.clear();
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
            // R985 — programmatic submenu descent (RPC restore / form default;
            // fires no `"command"` intent). Each index must address an enabled
            // submenu reachable from the open top menu; `""` collapses to the
            // top dropdown.
            "open_path" => match value {
                IntrospectValue::Text(ref s) => {
                    if s.is_empty() {
                        self.em.inner.open_path.clear();
                        self.em.inner.active = None;
                        return Ok(());
                    }
                    let p = parse_path(s).ok_or(InterveneError::TypeMismatch)?;
                    if !self.em.inner.is_valid_open_path(&p) {
                        return Err(InterveneError::out_of_range(format!(
                            "{s:?} is not a path this menubar can be open at \
                             (each step must name a submenu of the one before it)"
                        )));
                    }
                    self.em.inner.open_path = p;
                    self.em.inner.active = None;
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "active" => match value {
                IntrospectValue::Int(i) => {
                    // R985 — resolve against the *current* (deepest open) menu.
                    let len = self
                        .em
                        .inner
                        .current_items()
                        .ok_or_else(|| {
                            InterveneError::out_of_range(
                                "no menu is open, so no item is highlightable",
                            )
                        })?
                        .len();
                    let item = resolve_index("item", i, len)?;
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
                    let m = resolve_index("menu", i, self.menu_count())?;
                    self.em.inner.bar_focus = m;
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // R805 / R985 — programmatic checkbox toggle `checked.<path>`
            // (RPC restore / form default; fires no `"command"` intent).
            // R805.1 — only a [`MenuItem::Checkbox`] has a writable checked
            // slot; a command / separator / submenu rejects (`ReadOnly`), so the
            // RPC surface cannot reach the nonsensical "checked command" state.
            _ => {
                let suffix = path
                    .strip_prefix("checked.")
                    .ok_or(InterveneError::UnknownPath)?;
                let p = parse_path(suffix).ok_or(InterveneError::UnknownPath)?;
                let item = self.em.inner.item_at_path_mut(&p).ok_or_else(|| {
                    InterveneError::out_of_range(format!("no menu item at path {suffix:?}"))
                })?;
                match (value, item) {
                    (IntrospectValue::Bool(b), MenuItem::Checkbox { checked, .. }) => {
                        *checked = b;
                        Ok(())
                    }
                    (IntrospectValue::Bool(_), _) => Err(InterveneError::ReadOnly),
                    _ => Err(InterveneError::TypeMismatch),
                }
            }
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
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
    use crate::test_fixtures::assert_out_of_range_saying;
    use crate::test_fixtures::assert_refused_saying;

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
    fn r1543_keyboard_activate_opens_with_the_first_item_highlighted() {
        // The WAI-ARIA §3.5 difference between the pointer open and the
        // keyboard one: a mnemonic user has no cursor, so the dropdown must
        // arrive with a highlight to arrow from. `toggle_title` (the pointer
        // path, asserted above) leaves `active_item()` None.
        let mut m = bar();
        m.activate_title(1);
        assert_eq!(m.open_menu(), Some(1));
        assert_eq!(m.bar_focus(), 1);
        assert!(
            m.active_item().is_some(),
            "keyboard open highlights the first navigable item"
        );
    }

    #[test]
    fn r1543_keyboard_activate_on_the_open_menu_closes_it() {
        let mut m = bar();
        m.activate_title(0);
        assert_eq!(m.open_menu(), Some(0));
        m.activate_title(0);
        assert_eq!(
            m.open_menu(),
            None,
            "Alt+F twice closes, as the toolkit does"
        );
    }

    #[test]
    fn r1543_keyboard_activate_out_of_range_is_inert() {
        let mut m = bar();
        m.activate_title(99);
        assert_eq!(m.open_menu(), None);
    }

    #[test]
    fn r1543_a_mnemonic_reaches_the_title_over_the_send_wire() {
        // The whole mnemonic path in one assertion: the composite paint tag
        // `<bar>#t1` re-composed through the R51.42 grammar reaches this
        // widget and opens menu 1. Pre-R1543 the pointer-event decode rejected
        // it, so an accelerator could not address a menubar at all.
        let mut e = ext();
        assert!(
            e.invoke(
                "send",
                IntrospectValue::Text("t1:KeyboardActivate".to_string())
            )
            .is_ok()
        );
        assert_eq!(e.open_menu(), Some(1));
    }

    #[test]
    fn r1543_a_mnemonic_reaches_a_dropdown_item_too() {
        // An item's mnemonic exists only while its dropdown is painted, so the
        // activation wire has to accept the item form as well — otherwise
        // `&New` inside an open File menu would be undeliverable.
        let mut e = ext();
        e.invoke(
            "send",
            IntrospectValue::Text("t0:KeyboardActivate".to_string()),
        )
        .expect("title opens");
        e.invoke(
            "send",
            IntrospectValue::Text("i1:KeyboardActivate".to_string()),
        )
        .expect("item activates");
        let mut got = Vec::new();
        e.drain_intents(&mut |i| got.push(i));
        assert_eq!(got.len(), 1, "one command, exactly as a click emits");
        assert_eq!(got[0].tag_str(), "command");
        assert_eq!(
            got[0].payload,
            IntrospectValue::Text("0.1".to_string()),
            "the emitted path is absolute (menu 0, item 1), not the relative one"
        );
    }

    #[test]
    fn r1543_a_malformed_activation_target_is_rejected() {
        let mut e = ext();
        // R1564 — "a non-numeric title index is rejected, not silently index 0"
        // and "the dismiss barrier has no activation" were two *comments* on two
        // identical refusals. They are now two different sentences on the wire.
        assert_refused_saying(
            &e.invoke(
                "send",
                IntrospectValue::Text("tx:KeyboardActivate".to_string()),
            ),
            "title target \"tx\" carries no index",
        );
        assert_refused_saying(
            &e.invoke(
                "send",
                IntrospectValue::Text("barrier:KeyboardActivate".to_string()),
            ),
            "KeyboardActivate target \"barrier\" names no sub-element",
        );
        assert_eq!(e.open_menu(), None);
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
        // R985 — hover routes through `point_to` (rel descent within the open
        // dropdown); `[i]` highlights top item `i`, mirroring the old hover.
        let mut m = bar();
        m.point_to(&[1]);
        assert_eq!(m.active_item(), None, "no hover effect while closed");
        m.toggle_title(0);
        m.point_to(&[2]);
        assert_eq!(m.active_item(), Some(2));
        m.point_to(&[9]);
        assert_eq!(m.active_item(), Some(2), "out-of-range hover ignored");
    }

    #[test]
    fn activate_item_returns_coords_and_closes() {
        let mut m = bar();
        m.toggle_title(1);
        assert_eq!(m.activate_item(3), Some(vec![1, 3]));
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
        assert_out_of_range_saying(
            &e.intervene("open", IntrospectValue::Int(9)),
            "no menu 9 here",
        );
        assert_out_of_range_saying(
            &e.intervene("open", IntrospectValue::Int(-1)),
            "-1 is not a menu index",
        );
    }

    #[test]
    fn external_intervene_active_requires_open_menu() {
        let mut e = ext();
        assert_out_of_range_saying(
            &e.intervene("active", IntrospectValue::Int(0)),
            "no menu is open, so no item is highlightable",
        );
        e.intervene("open", IntrospectValue::Int(1)).unwrap();
        e.intervene("active", IntrospectValue::Int(4)).unwrap();
        assert_eq!(e.active_item(), Some(4));
        // R1565 — "no menu open" and "no such item in the open menu" were the
        // same value; the comment above used to be the only thing telling them
        // apart.
        assert_out_of_range_saying(
            &e.intervene("active", IntrospectValue::Int(5)),
            "no item 5 here",
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
            .invoke(
                "send",
                IntrospectValue::Text("barrier:PointerUp".to_string()),
            )
            .unwrap();
        assert_eq!(e.open_menu(), None, "barrier PointerUp dismisses");
        assert_eq!(
            out,
            IntrospectValue::Null,
            "send returns the (now None) open index"
        );
        // Barrier dismiss emits no command intent (it is not an activation).
        assert!(!e.is_dirty(), "click-outside fires no command");
    }

    #[test]
    fn r715_barrier_non_up_events_are_inert() {
        let mut e = ext();
        click_title(&mut e, 0);
        for ev in [
            "PointerEnter",
            "PointerDown",
            "PointerLeave",
            "PointerCancel",
        ] {
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
        assert_refused_saying(
            &e.invoke("send", IntrospectValue::Text("no_colon".to_string())),
            "malformed send payload \"no_colon\"",
        );
        assert_refused_saying(
            &e.invoke("send", IntrospectValue::Text("x0:PointerUp".to_string())),
            "target \"x0\" names no sub-element",
        );
        assert_refused_saying(
            &e.invoke("send", IntrospectValue::Text("t0:Teleport".to_string())),
            "\"Teleport\" is neither a pointer event name nor KeyboardActivate",
        );
        assert_refused_saying(
            &e.invoke("send", IntrospectValue::Text("tA:PointerUp".to_string())),
            "title target \"tA\" carries no index",
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
                SchemaField::new("menu_count", "int"),
                SchemaField::new("open", "int"),
                SchemaField::new("open_path", "string"),
                SchemaField::new("active", "int"),
                SchemaField::new("active_path", "string"),
                SchemaField::new("bar_focus", "int"),
                SchemaField::parametric(
                    "item_count.<path>",
                    "int",
                    const { &[SchemaArg::open("path", "string")] }
                ),
                SchemaField::parametric(
                    "item_kind.<path>",
                    "string",
                    const { &[SchemaArg::open("path", "string")] }
                ),
                SchemaField::parametric(
                    "checked.<path>",
                    "bool",
                    const { &[SchemaArg::open("path", "string")] }
                ),
                SchemaField::parametric(
                    "enabled.<path>",
                    "bool",
                    const { &[SchemaArg::open("path", "string")] }
                ),
                SchemaField::action("send", "string"),
                SchemaField::action("key", "string"),
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
        assert_eq!(m.activate_item(1), Some(vec![0, 1]));
        assert!(m.item(0, 1).unwrap().checked(), "checkbox toggled on");
        assert_eq!(m.open_menu(), None, "activation closes");
        // Reopen: the checked state survived.
        m.toggle_title(0);
        assert!(
            m.item(0, 1).unwrap().checked(),
            "checked persists across reopen"
        );
        // Toggle it back off.
        assert_eq!(m.activate_item(1), Some(vec![0, 1]));
        assert!(!m.item(0, 1).unwrap().checked(), "checkbox toggled off");
    }

    #[test]
    fn r805_separator_and_disabled_are_inert() {
        let mut m = rich();
        m.toggle_title(0);
        assert_eq!(m.activate_item(2), None, "separator not activatable");
        assert_eq!(m.open_menu(), Some(0), "inert activate leaves menu open");
        assert_eq!(m.activate_item(4), None, "disabled not activatable");
        assert_eq!(m.open_menu(), Some(0));
        // Hover (point_to) skips both.
        m.point_to(&[2]);
        assert_eq!(m.active_item(), None, "hover ignores separator");
        m.point_to(&[4]);
        assert_eq!(m.active_item(), None, "hover ignores disabled");
        m.point_to(&[3]);
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
        assert_eq!(
            m.active_item(),
            Some(3),
            "Up wraps past disabled to last navigable"
        );
    }

    #[test]
    fn r805_active_edge_lands_on_navigable_bounds() {
        let mut m = rich();
        m.toggle_title(0);
        m.active_edge(true);
        assert_eq!(
            m.active_item(),
            Some(3),
            "End -> last navigable (3, not disabled 4)"
        );
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
        assert_eq!(
            m.active_item(),
            Some(2),
            "first navigable skips leading non-nav rows"
        );
    }

    #[test]
    fn r805_external_query_item_slots() {
        let e = MenuBarExternal::with_items(vec![vec![
            MenuItem::command(),
            MenuItem::checkbox(true),
            MenuItem::separator(),
            MenuItem::command().disabled(),
        ]]);
        assert_eq!(
            e.query("item_kind.0.0"),
            Some(IntrospectValue::Text("command".into()))
        );
        assert_eq!(
            e.query("item_kind.0.1"),
            Some(IntrospectValue::Text("checkbox".into()))
        );
        assert_eq!(
            e.query("item_kind.0.2"),
            Some(IntrospectValue::Text("separator".into()))
        );
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
        e.intervene("checked.0.0", IntrospectValue::Bool(true))
            .unwrap();
        assert_eq!(e.query("checked.0.0"), Some(IntrospectValue::Bool(true)));
        assert!(!e.is_dirty(), "intervene fires no command intent");
        assert_out_of_range_saying(
            &e.intervene("checked.0.9", IntrospectValue::Bool(true)),
            r#"no menu item at path "0.9""#,
        );
        assert_eq!(
            e.intervene("checked.0.0", IntrospectValue::Int(1)),
            Err(InterveneError::TypeMismatch)
        );
    }

    #[test]
    fn r805_1_intervene_checked_rejects_non_checkbox() {
        // R805.1 — only a Checkbox has a writable checked slot. A command
        // or separator rejects (ReadOnly), so the RPC surface cannot reach
        // the nonsensical "checked command / separator" state the sum-type
        // model already makes unrepresentable in code.
        let mut e = MenuBarExternal::with_items(vec![vec![
            MenuItem::command(),
            MenuItem::separator(),
            MenuItem::checkbox(false),
        ]]);
        assert_eq!(
            e.intervene("checked.0.0", IntrospectValue::Bool(true)),
            Err(InterveneError::ReadOnly),
            "command has no writable checked slot"
        );
        assert_eq!(
            e.intervene("checked.0.1", IntrospectValue::Bool(true)),
            Err(InterveneError::ReadOnly),
            "separator has no writable checked slot"
        );
        // The command / separator still report checked = false.
        assert_eq!(e.query("checked.0.0"), Some(IntrospectValue::Bool(false)));
        assert_eq!(e.query("checked.0.1"), Some(IntrospectValue::Bool(false)));
        // The checkbox accepts it.
        e.intervene("checked.0.2", IntrospectValue::Bool(true))
            .unwrap();
        assert_eq!(e.query("checked.0.2"), Some(IntrospectValue::Bool(true)));
    }

    #[test]
    fn r805_external_send_checkbox_activation_toggles_and_emits() {
        let mut e =
            MenuBarExternal::with_items(vec![vec![MenuItem::command(), MenuItem::checkbox(false)]]);
        e.invoke("send", IntrospectValue::Text("t0:PointerUp".into()))
            .unwrap();
        // Click the checkbox (item 1).
        for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
            e.invoke("send", IntrospectValue::Text(format!("i1:{ev}")))
                .unwrap();
        }
        assert!(
            e.item(0, 1).unwrap().checked(),
            "checkbox toggled on via send"
        );
        assert_eq!(e.open_menu(), None, "activation closes");
        let mut got = Vec::new();
        e.drain_intents(&mut |i| got.push(i));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].payload, IntrospectValue::Text("0.1".into()));
    }

    #[test]
    fn r880_1_barrier_dismiss_survives_a_held_modifier() {
        // The router emits "barrier:PointerUp:<token>" when modifiers are
        // held (R781); the pre-R880.1 hand-rolled split read "PointerUp:c"
        // as the event name, so a Ctrl+click outside the open dropdown
        // failed to dismiss it.
        let mut e = MenuBarExternal::with_items(vec![vec![MenuItem::command()]]);
        e.invoke("send", IntrospectValue::Text("t0:PointerUp".into()))
            .unwrap();
        assert_eq!(e.open_menu(), Some(0), "dropdown open");
        e.invoke("send", IntrospectValue::Text("barrier:PointerUp:c".into()))
            .unwrap();
        assert_eq!(e.open_menu(), None, "Ctrl+click outside dismisses");
    }

    // ----- R985: cascading submenus -----

    /// A menubar whose menu 0 ("File") holds:
    /// `[Command "New", Submenu "Open Recent" [Command, Command], Command "Save"]`
    /// and a flat menu 1 ("Edit") with 2 commands.
    fn nested() -> MenuBar {
        MenuBar::with_items(vec![
            vec![
                MenuItem::command(),
                MenuItem::submenu(vec![MenuItem::command(), MenuItem::command()]),
                MenuItem::command(),
            ],
            vec![MenuItem::command(), MenuItem::command()],
        ])
    }

    fn nested_ext() -> MenuBarExternal {
        MenuBarExternal::with_items(vec![
            vec![
                MenuItem::command(),
                MenuItem::submenu(vec![MenuItem::command(), MenuItem::checkbox(false)]),
                MenuItem::command(),
            ],
            vec![MenuItem::command(), MenuItem::command()],
        ])
    }

    #[test]
    fn r985_submenu_item_taxonomy() {
        let sub = MenuItem::submenu(vec![MenuItem::command()]);
        assert!(sub.is_submenu());
        assert!(sub.is_navigable(), "an enabled submenu parent is navigable");
        assert_eq!(sub.kind_name(), "submenu");
        assert_eq!(sub.submenu_items().map(<[_]>::len), Some(1));
        let disabled = MenuItem::submenu(vec![MenuItem::command()]).disabled();
        assert!(!disabled.is_navigable(), "a disabled submenu is inert");
        assert!(!disabled.enabled());
        assert!(!MenuItem::command().is_submenu());
        assert_eq!(MenuItem::command().submenu_items(), None);
    }

    #[test]
    fn r985_descend_opens_submenu_and_highlights_first() {
        let mut m = nested();
        m.toggle_title(0);
        m.move_active(true); // active -> item 0 (New)
        m.move_active(true); // active -> item 1 (Open Recent submenu)
        assert_eq!(m.active_item(), Some(1));
        assert!(m.active_is_submenu());
        m.descend_active();
        assert_eq!(m.open_path(), &[1], "descended into submenu 1");
        assert_eq!(
            m.active_item(),
            Some(0),
            "submenu highlights its first item"
        );
        assert!(m.in_submenu());
    }

    #[test]
    fn r985_ascend_closes_one_level_returns_to_parent() {
        let mut m = nested();
        m.toggle_title(0);
        m.point_to(&[1]); // highlight the submenu parent
        m.descend_active();
        assert_eq!(m.open_path(), &[1]);
        assert!(m.ascend(), "Arrow Left / Escape closes the submenu level");
        assert!(m.open_path().is_empty(), "back to the top dropdown");
        assert_eq!(m.active_item(), Some(1), "focus returns to the parent item");
        assert_eq!(m.open_menu(), Some(0), "top menu stays open");
        assert!(!m.ascend(), "no further level to close at the top");
    }

    #[test]
    fn r985_descend_then_activate_leaf_emits_full_path() {
        let mut e = nested_ext();
        e.invoke("send", IntrospectValue::Text("t0:PointerUp".into()))
            .unwrap();
        // Open submenu 1 and activate its leaf 0 via the keyboard.
        e.invoke("key", IntrospectValue::Text("ArrowDown".into()))
            .unwrap(); // active 0
        e.invoke("key", IntrospectValue::Text("ArrowDown".into()))
            .unwrap(); // active 1 (submenu)
        e.invoke("key", IntrospectValue::Text("ArrowRight".into()))
            .unwrap(); // descend
        assert_eq!(e.open_path(), &[1]);
        assert_eq!(e.active_item(), Some(0));
        assert!(matches!(
            e.invoke("key", IntrospectValue::Text("Enter".into()))
                .unwrap(),
            IntrospectValue::Bool(true)
        ));
        assert_eq!(
            e.open_menu(),
            None,
            "activating a nested leaf closes the whole menu"
        );
        let mut got = Vec::new();
        e.drain_intents(&mut |i| got.push(i));
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].payload,
            IntrospectValue::Text("0.1.0".into()),
            "the command payload is the full absolute path",
        );
    }

    #[test]
    fn r985_arrow_right_on_leaf_jumps_to_next_top_menu() {
        let mut m = nested();
        m.toggle_title(0);
        m.point_to(&[0]); // leaf "New"
        // Arrow Right on a leaf at the top level moves to the next top menu.
        assert!(!m.active_is_submenu());
        m.menu_switch(true);
        assert_eq!(m.open_menu(), Some(1));
        assert!(m.open_path().is_empty());
        assert_eq!(
            m.active_item(),
            Some(0),
            "next menu highlights its first item"
        );
    }

    #[test]
    fn r985_escape_collapses_one_level_at_a_time() {
        let mut e = nested_ext();
        e.invoke("send", IntrospectValue::Text("t0:PointerUp".into()))
            .unwrap();
        e.invoke("send", IntrospectValue::Text("i1:PointerUp".into()))
            .unwrap(); // open submenu 1
        assert_eq!(e.open_path(), &[1]);
        // Escape inside the submenu collapses just that level.
        e.invoke("key", IntrospectValue::Text("Escape".into()))
            .unwrap();
        assert!(e.open_path().is_empty(), "submenu collapsed");
        assert_eq!(e.open_menu(), Some(0), "top dropdown still open");
        // A second Escape closes the top dropdown.
        e.invoke("key", IntrospectValue::Text("Escape".into()))
            .unwrap();
        assert_eq!(e.open_menu(), None);
    }

    #[test]
    fn r985_pointer_open_and_collapse_via_send_path() {
        let mut e = nested_ext();
        e.invoke("send", IntrospectValue::Text("t0:PointerUp".into()))
            .unwrap();
        // Click the submenu parent (rel path [1]) opens it.
        e.invoke("send", IntrospectValue::Text("i1:PointerUp".into()))
            .unwrap();
        assert_eq!(e.open_path(), &[1], "click opened the submenu");
        // Hover a nested item (rel path [1, 1]) highlights it, keeping it open.
        e.invoke("send", IntrospectValue::Text("i1.1:PointerEnter".into()))
            .unwrap();
        assert_eq!(e.open_path(), &[1]);
        assert_eq!(e.active_item(), Some(1));
        // Hover a top-level item (rel path [0]) collapses back to the top.
        e.invoke("send", IntrospectValue::Text("i0:PointerEnter".into()))
            .unwrap();
        assert!(
            e.open_path().is_empty(),
            "moving to a top item collapsed the submenu"
        );
        assert_eq!(e.active_item(), Some(0));
    }

    #[test]
    fn r985_query_nested_structure() {
        let e = nested_ext();
        // Top-level structure.
        assert_eq!(e.query("item_count.0"), Some(IntrospectValue::Int(3)));
        assert_eq!(
            e.query("item_kind.0.1"),
            Some(IntrospectValue::Text("submenu".into())),
            "item 1 of menu 0 is a submenu",
        );
        // Nested submenu structure (path descends into the submenu).
        assert_eq!(e.query("item_count.0.1"), Some(IntrospectValue::Int(2)));
        assert_eq!(
            e.query("item_kind.0.1.0"),
            Some(IntrospectValue::Text("command".into()))
        );
        assert_eq!(
            e.query("item_kind.0.1.1"),
            Some(IntrospectValue::Text("checkbox".into()))
        );
        assert_eq!(e.query("checked.0.1.1"), Some(IntrospectValue::Bool(false)));
        assert_eq!(e.query("item_kind.0.9"), None, "out-of-range item");
        assert_eq!(
            e.query("item_count.0.0"),
            None,
            "a non-submenu has no item list"
        );
    }

    #[test]
    fn r985_query_open_path_and_active_path() {
        let mut e = nested_ext();
        assert_eq!(
            e.query("open_path"),
            Some(IntrospectValue::Text(String::new()))
        );
        assert_eq!(e.query("active_path"), Some(IntrospectValue::Null));
        e.invoke("send", IntrospectValue::Text("t0:PointerUp".into()))
            .unwrap();
        e.invoke("send", IntrospectValue::Text("i1:PointerUp".into()))
            .unwrap(); // open submenu 1
        assert_eq!(
            e.query("open_path"),
            Some(IntrospectValue::Text("1".into()))
        );
        assert_eq!(
            e.query("active_path"),
            Some(IntrospectValue::Text("0.1.0".into())),
            "deepest active = submenu first item",
        );
    }

    #[test]
    fn r985_intervene_open_path_round_trip() {
        let mut e = nested_ext();
        e.intervene("open", IntrospectValue::Int(0)).unwrap();
        e.intervene("open_path", IntrospectValue::Text("1".into()))
            .unwrap();
        assert_eq!(e.open_path(), &[1]);
        assert!(!e.is_dirty(), "intervene fires no command intent");
        // An index that is not a submenu is rejected.
        assert_out_of_range_saying(
            &e.intervene("open_path", IntrospectValue::Text("0".into())),
            "each step must name a submenu",
        );
        // Empty string collapses to the top dropdown.
        e.intervene("open_path", IntrospectValue::Text(String::new()))
            .unwrap();
        assert!(e.open_path().is_empty());
    }

    #[test]
    fn r985_intervene_nested_checkbox() {
        let mut e = nested_ext();
        e.intervene("checked.0.1.1", IntrospectValue::Bool(true))
            .unwrap();
        assert_eq!(e.query("checked.0.1.1"), Some(IntrospectValue::Bool(true)));
        // A nested command has no writable checked slot.
        assert_eq!(
            e.intervene("checked.0.1.0", IntrospectValue::Bool(true)),
            Err(InterveneError::ReadOnly),
        );
        // A submenu parent has no writable checked slot either.
        assert_eq!(
            e.intervene("checked.0.1", IntrospectValue::Bool(true)),
            Err(InterveneError::ReadOnly),
        );
    }

    #[test]
    fn r985_activating_submenu_parent_opens_not_commands() {
        let mut e = nested_ext();
        e.invoke("send", IntrospectValue::Text("t0:PointerUp".into()))
            .unwrap();
        // PointerUp on the submenu parent opens it and emits NO command.
        e.invoke("send", IntrospectValue::Text("i1:PointerUp".into()))
            .unwrap();
        assert_eq!(e.open_path(), &[1]);
        assert_eq!(
            e.open_menu(),
            Some(0),
            "menu stays open (submenu, not a command)"
        );
        assert!(!e.is_dirty(), "opening a submenu fires no command");
    }

    #[test]
    fn r985_1_malformed_nested_send_does_not_mutate_state() {
        // R985.1 (session-review): activate_rel must validate the WHOLE target
        // before mutating open_path. A send to an out-of-range nested item
        // (`i1.99`, when Open Recent has 2 items) must be inert — NOT collapse
        // the cascade to `[1]` as a side-effect (validate-before-mutate).
        let mut e = nested_ext();
        e.invoke("send", IntrospectValue::Text("t0:PointerUp".into()))
            .unwrap();
        e.invoke("send", IntrospectValue::Text("i0:PointerEnter".into()))
            .unwrap(); // highlight top item 0
        let open_before = e.open_path().to_vec();
        let active_before = e.active_item();
        e.invoke("send", IntrospectValue::Text("i1.99:PointerUp".into()))
            .unwrap();
        assert_eq!(
            e.open_path(),
            open_before.as_slice(),
            "malformed send left open_path untouched"
        );
        assert_eq!(
            e.active_item(),
            active_before,
            "malformed send left active untouched"
        );
        assert_eq!(e.open_menu(), Some(0), "the top menu is still open");
        assert!(!e.is_dirty(), "a malformed send fires no command");
    }
}
