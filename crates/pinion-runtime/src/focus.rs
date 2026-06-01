//! R51.52 §5.39 — Focus state owner.
//!
//! [`FocusManager`] owns the single focused-widget identity for key
//! dispatch and ARIA `:focus` visual indication. The model mirrors the
//! W3C ARIA / WCAG 2.1.1 keyboard-navigation contract: at most one
//! focusable widget is focused at any time; Tab and Shift+Tab traverse
//! the focusable enumeration in paint-scene order; click on a tagged
//! focusable widget aliases [`FocusManager::focus_set`]; Escape and
//! click on non-focusable background alias [`FocusManager::focus_clear`].
//!
//! ## Ownership
//!
//! Held by `pinion-shell::AppShell` adjacent to [`super::InputRouter`]
//! (§5.35). The shell consults the manager on every winit key event
//! before forwarding to `WidgetView::apply_key` (R51.53 wiring). The
//! `tab_order` enumeration is refreshed by the shell every render via
//! [`update_focusable_tags`] (depth-first paint-scene traversal of
//! focusable tags supplied by `WidgetView::focusable_tags`).
//!
//! ## Wrap semantics
//!
//! Tab from the last focusable wraps to the first; Shift+Tab from the
//! first wraps to the last. This matches the single-window Slint /
//! Xilem / iced convention. (HTML's "Tab leaves the document into UA
//! chrome" is browser-specific and not applicable to a standalone
//! framework with no chrome to escape into.)
//!
//! ## Initial focus
//!
//! When `focused` is `None`, the first Tab focuses the first element
//! and the first Shift+Tab focuses the last — the WAI-ARIA Authoring
//! Practices convention browsers use when focus first enters a
//! focusable group.
//!
//! ## Window blur / restore (R51.59 carry)
//!
//! [`FocusManager::save`] snapshots the focused tag; [`FocusManager::restore`]
//! reinstates it. Wiring (`WindowEvent::Focus { focused: bool }` →
//! save / restore) lands in `pinion-shell` per the R51.59 round.
//!
//! ## Modal focus trap (R693 §5.39)
//!
//! [`FocusManager::push_modal_scope`] / [`FocusManager::pop_modal_scope`]
//! implement the WAI-ARIA modal-dialog focus-management contract
//! (`aria-modal` / the HTML `<dialog>.showModal()` top-layer model):
//! while a modal scope is active, Tab / Shift+Tab and
//! [`focus_set`](FocusManager::focus_set) traverse *only* the scope's
//! member tags (the trap), and the wrap stays inside the modal. Opening
//! a scope snapshots the invoker's focus and auto-focuses the modal's
//! first control; closing it restores the invoker.
//!
//! The member list is the scope's own *active focusable enumeration* —
//! it does **not** need to be a subset of the static [`tab_order`]
//! (`update_focusable_tags`). That is deliberate: a dialog's controls
//! (action buttons, form fields) are focusable *only while the dialog is
//! open*, so listing them in the binding's static `focusable_tags` would
//! make them phantom tab stops when closed (the dynamic-focusable gap
//! todomvc's R664 in-place editor flagged). Pushing the members as the
//! active order when the modal opens, and popping back to `tab_order`
//! when it closes, is the textbook resolution of that gap scoped to the
//! modal case.
//!
//! The stack (`Vec<ModalScope>`) models nested modals (a confirm dialog
//! over a settings dialog — the HTML top-layer is likewise a stack):
//! the topmost scope owns traversal, and each scope restores its own
//! saved focus on pop.

use std::mem;

/// One entry on the modal focus-trap stack. Captures the focus to
/// restore when the scope closes plus the active focusable enumeration
/// (the modal's controls, in Tab order) that traversal is confined to
/// while the scope is on top.
#[derive(Debug, Clone)]
struct ModalScope {
    /// Focus owner at the moment the scope opened (the invoker, e.g.
    /// the button that opened the dialog). Restored on pop.
    saved_focus: Option<String>,
    /// The modal's focusable tags in Tab order. While this scope is the
    /// topmost, Tab / Shift+Tab / `focus_set` traverse only these.
    members: Vec<String>,
}

/// Focused tag identity for key dispatch + ARIA visual indication.
///
/// The struct stores three pieces of state:
///
/// - `focused`: currently focused widget tag, `None` between Tab
///   traversal boundaries or when no focusable widget exists.
/// - `tab_order`: focusable enumeration in paint-scene order, refilled
///   every render by [`update_focusable_tags`]; Tab advances forward,
///   Shift+Tab backward, both wrap.
/// - `saved`: snapshot for window blur / refocus restore.
#[derive(Debug, Default, Clone)]
pub struct FocusManager {
    focused: Option<String>,
    tab_order: Vec<String>,
    saved: Option<String>,
    modal_stack: Vec<ModalScope>,
}

impl FocusManager {
    /// Empty manager — no focus, no focusable enumeration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Currently focused tag, if any.
    #[must_use]
    pub fn focused(&self) -> Option<&str> {
        self.focused.as_deref()
    }

    /// Focusable enumeration in Tab order. Lent out for the shell /
    /// introspect; mutate via [`update_focusable_tags`].
    #[must_use]
    pub fn tab_order(&self) -> &[String] {
        &self.tab_order
    }

    /// Refresh the focusable enumeration from the latest paint-scene
    /// traversal. If the currently focused tag is no longer in the
    /// new enumeration, focus is dropped — stale focus would dispatch
    /// to a missing target.
    ///
    /// R693 §5.39 — the drop-on-removal guard is suppressed while a
    /// modal scope is active: a modal's members are a *separate* active
    /// enumeration ([`push_modal_scope`](Self::push_modal_scope)) that
    /// is intentionally absent from the static `tab_order`, so checking
    /// the focused member against `tab_order` would spuriously drop the
    /// modal's focus every render. The base `tab_order` is still
    /// refreshed underneath so popping the scope restores a current
    /// enumeration.
    pub fn update_focusable_tags(&mut self, tags: Vec<String>) {
        if self.modal_stack.is_empty() {
            if let Some(f) = self.focused.as_deref() {
                if !tags.iter().any(|t| t == f) {
                    self.focused = None;
                }
            }
        }
        self.tab_order = tags;
    }

    /// The focusable enumeration traversal currently operates over: the
    /// topmost modal scope's members while a modal is open, otherwise
    /// the base [`tab_order`](Self::tab_order).
    fn active_order(&self) -> &[String] {
        match self.modal_stack.last() {
            Some(scope) => &scope.members,
            None => &self.tab_order,
        }
    }

    /// R693 §5.39 — the focusable enumeration RPC `focus/set` /
    /// `focus/get` operate against: the topmost modal scope's members
    /// while a modal is open, otherwise the base
    /// [`tab_order`](Self::tab_order). AI clients addressing a dialog's
    /// controls (focusable only while the trap is up) must target this,
    /// not the static `tab_order` — the trap members are intentionally
    /// absent from the latter.
    #[must_use]
    pub fn active_tab_order(&self) -> &[String] {
        self.active_order()
    }

    /// `true` while at least one modal focus scope is active (Tab is
    /// trapped, Escape dismisses the modal rather than the window).
    #[must_use]
    pub fn is_modal(&self) -> bool {
        !self.modal_stack.is_empty()
    }

    /// Number of stacked modal scopes (nested modals). `0` when no
    /// modal is open.
    #[must_use]
    pub fn modal_depth(&self) -> usize {
        self.modal_stack.len()
    }

    /// Open a modal focus trap over `members` (the modal's controls in
    /// Tab order). Snapshots the current focus owner for restore on
    /// [`pop_modal_scope`](Self::pop_modal_scope) and auto-focuses the
    /// first member (WAI-ARIA "focus moves into the dialog on open").
    /// Returns `true` if the focused tag changed.
    ///
    /// `members` becomes the active focusable enumeration while this
    /// scope is topmost; it need not intersect the static `tab_order`
    /// (see the module-level modal-trap note). An empty `members`
    /// clears focus (a modal with no focusable controls).
    pub fn push_modal_scope(&mut self, members: Vec<String>) -> bool {
        let saved_focus = self.focused.clone();
        let first = members.first().cloned();
        self.modal_stack.push(ModalScope {
            saved_focus,
            members,
        });
        match first {
            Some(tag) => {
                let changed = self.focused.as_deref() != Some(tag.as_str());
                self.focused = Some(tag);
                changed
            }
            None => self.focused.take().is_some(),
        }
    }

    /// Close the topmost modal focus trap and restore the focus owner
    /// captured when it opened (the invoker), if that tag is still
    /// focusable in the now-active enumeration. Returns `true` if the
    /// focused tag changed. No-op (returns `false`) when no modal scope
    /// is active.
    pub fn pop_modal_scope(&mut self) -> bool {
        let Some(scope) = self.modal_stack.pop() else {
            return false;
        };
        let before = self.focused.take();
        self.focused = scope
            .saved_focus
            .filter(|t| self.active_order().iter().any(|x| x == t));
        before != self.focused
    }

    /// Move focus to the next focusable widget. Returns `true` if
    /// focus changed (so the shell can repaint the focus ring).
    /// Wraps end → start.
    pub fn focus_next(&mut self) -> bool {
        self.advance(1)
    }

    /// Move focus to the previous focusable widget. Returns `true`
    /// if focus changed. Wraps start → end.
    pub fn focus_prev(&mut self) -> bool {
        self.advance(-1)
    }

    /// Programmatic focus to `tag`. Returns `true` if focus changed;
    /// `false` if `tag` is absent from `tab_order` (caller error) or
    /// is already the focused tag (no-op).
    pub fn focus_set(&mut self, tag: &str) -> bool {
        if !self.active_order().iter().any(|t| t == tag) {
            return false;
        }
        if self.focused.as_deref() == Some(tag) {
            return false;
        }
        self.focused = Some(tag.to_owned());
        true
    }

    /// R742.3 §5.39 — resolve a (possibly composite `group#i`) tag to the
    /// focusable tag a *click* on it should land on:
    ///
    /// * the tag itself when it is registered focusable — per-sub-tab
    ///   composites (`hello-accordion-single` registers every header
    ///   `accordion_single#i`);
    /// * else its primary half (`group`) when THAT is focusable — the
    ///   single-tab-stop composites (`ListBox` / `RadioGroup` / `Table` /
    ///   the reorder list) that register only the primary, where a click
    ///   resolves to a sub-tag (`group#2`) the bare exact-match
    ///   [`focus_set`](Self::focus_set) would reject;
    /// * else `None` — a tagged-but-non-focusable decoration the click
    ///   should leave focus unchanged for (the W3C HTML convention).
    ///
    /// Pre-R742.3 click-to-focus only matched the exact hover tag, so
    /// clicking any primary-focusable composite never focused it — the
    /// gap was masked because the example demos focus via RPC
    /// `focus/set` (which passes the primary tag explicitly).
    #[must_use]
    pub fn resolve_focusable(&self, tag: &str) -> Option<String> {
        let order = self.active_order();
        if order.iter().any(|t| t == tag) {
            return Some(tag.to_owned());
        }
        // Composite sub-tag (`group#i`) → fall back to the primary half
        // via the shared `#` SSOT (R742.4).
        if let (primary, Some(_)) = pinion_core::composite_tag::split_subindex(tag) {
            if order.iter().any(|t| t == primary) {
                return Some(primary.to_owned());
            }
        }
        None
    }

    /// Clear focus. Returns `true` if focus changed (`focused` was
    /// `Some`).
    pub fn focus_clear(&mut self) -> bool {
        self.focused.take().is_some()
    }

    /// Snapshot the current focused tag for window-blur restore.
    /// Overwrites any prior snapshot — only the most recent blur
    /// matters because `focus_lost` → `focus_gained` is sequential.
    pub fn save(&mut self) {
        self.saved = self.focused.clone();
    }

    /// Restore the focused tag saved by [`save`]. Returns `true` if
    /// focus changed. No-op if no save was made or the saved tag is
    /// no longer in `tab_order` (the view-fn removed that widget
    /// while the window was unfocused).
    pub fn restore(&mut self) -> bool {
        let Some(saved) = mem::take(&mut self.saved) else {
            return false;
        };
        self.focus_set(&saved)
    }

    /// Step focus by `direction` (`1` = next, `-1` = prev). Wraps.
    /// No-op when `tab_order` is empty.
    ///
    /// When `focused = None`, the first Tab focuses the first
    /// element and the first Shift+Tab focuses the last (ARIA
    /// Authoring Practices convention).
    fn advance(&mut self, direction: i64) -> bool {
        let order = self.active_order();
        if order.is_empty() {
            return false;
        }
        let n = order.len();
        let next_idx: usize = match self.focused.as_deref() {
            None => {
                if direction > 0 {
                    0
                } else {
                    n - 1
                }
            }
            Some(t) => match order.iter().position(|x| x == t) {
                Some(cur) => {
                    let n_i = i64::try_from(n).unwrap_or(i64::MAX);
                    let cur_i = i64::try_from(cur).unwrap_or(0);
                    let stepped = (cur_i + direction).rem_euclid(n_i);
                    usize::try_from(stepped).unwrap_or(0)
                }
                None => {
                    // Focused tag is not in the active enumeration — a
                    // stale or cross-scope focus. Recover by entering at
                    // the leading edge for the direction instead of
                    // dropping focus, so a trapped modal never lets the
                    // cursor escape to "no focus".
                    if direction > 0 {
                        0
                    } else {
                        n - 1
                    }
                }
            },
        };
        let new_tag = order[next_idx].clone();
        if self.focused.as_deref() == Some(new_tag.as_str()) {
            return false;
        }
        self.focused = Some(new_tag);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::FocusManager;

    fn tags(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn new_is_empty() {
        let m = FocusManager::new();
        assert!(m.focused().is_none());
        assert!(m.tab_order().is_empty());
    }

    #[test]
    fn first_tab_focuses_first() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["a", "b", "c"]));
        assert!(m.focus_next());
        assert_eq!(m.focused(), Some("a"));
    }

    #[test]
    fn first_shift_tab_focuses_last() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["a", "b", "c"]));
        assert!(m.focus_prev());
        assert_eq!(m.focused(), Some("c"));
    }

    #[test]
    fn focus_next_steps_through() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["a", "b", "c"]));
        m.focus_next();
        assert_eq!(m.focused(), Some("a"));
        assert!(m.focus_next());
        assert_eq!(m.focused(), Some("b"));
        assert!(m.focus_next());
        assert_eq!(m.focused(), Some("c"));
    }

    #[test]
    fn focus_next_wraps_end_to_start() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["a", "b", "c"]));
        m.focus_set("c");
        assert!(m.focus_next());
        assert_eq!(m.focused(), Some("a"));
    }

    #[test]
    fn focus_prev_wraps_start_to_end() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["a", "b", "c"]));
        m.focus_set("a");
        assert!(m.focus_prev());
        assert_eq!(m.focused(), Some("c"));
    }

    #[test]
    fn focus_set_existing_tag_changes_focus() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["a", "b", "c"]));
        assert!(m.focus_set("b"));
        assert_eq!(m.focused(), Some("b"));
    }

    #[test]
    fn focus_set_missing_tag_returns_false() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["a", "b", "c"]));
        assert!(!m.focus_set("z"));
        assert_eq!(m.focused(), None);
    }

    #[test]
    fn focus_set_already_focused_returns_false() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["a", "b"]));
        m.focus_set("a");
        assert!(!m.focus_set("a"));
    }

    #[test]
    fn resolve_focusable_handles_composite_click_targets() {
        let mut m = FocusManager::new();
        // `dnd` is a single-tab-stop composite (only the primary is
        // focusable); `acc#0` / `acc#1` are a per-sub-tab composite.
        m.update_focusable_tags(tags(&["dnd", "acc#0", "acc#1"]));
        // A click resolving to a sub-tag of a primary-focusable composite
        // lands on the primary (the pre-R742.3 bug: this was `None`).
        assert_eq!(m.resolve_focusable("dnd#2").as_deref(), Some("dnd"));
        // An exactly-registered sub-tag focuses itself (not its primary).
        assert_eq!(m.resolve_focusable("acc#1").as_deref(), Some("acc#1"));
        // A single tag focuses itself.
        assert_eq!(m.resolve_focusable("dnd").as_deref(), Some("dnd"));
        // A tag whose neither form is focusable (decoration) is `None`.
        assert_eq!(m.resolve_focusable("deco#3"), None);
        assert_eq!(m.resolve_focusable("nope"), None);
    }

    #[test]
    fn focus_clear_from_some_returns_true() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["a"]));
        m.focus_set("a");
        assert!(m.focus_clear());
        assert_eq!(m.focused(), None);
    }

    #[test]
    fn focus_clear_from_none_returns_false() {
        let mut m = FocusManager::new();
        assert!(!m.focus_clear());
    }

    #[test]
    fn update_drops_focused_when_removed() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["a", "b"]));
        m.focus_set("a");
        m.update_focusable_tags(tags(&["b", "c"]));
        assert_eq!(m.focused(), None);
    }

    #[test]
    fn update_preserves_focused_when_kept() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["a", "b"]));
        m.focus_set("b");
        m.update_focusable_tags(tags(&["b", "c", "d"]));
        assert_eq!(m.focused(), Some("b"));
    }

    #[test]
    fn save_then_restore_reinstates_focus() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["a", "b"]));
        m.focus_set("b");
        m.save();
        m.focus_clear();
        assert_eq!(m.focused(), None);
        assert!(m.restore());
        assert_eq!(m.focused(), Some("b"));
    }

    #[test]
    fn restore_without_save_returns_false() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["a"]));
        assert!(!m.restore());
    }

    #[test]
    fn restore_after_save_target_removed_returns_false() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["a", "b"]));
        m.focus_set("b");
        m.save();
        m.update_focusable_tags(tags(&["a", "c"])); // b gone
        m.focus_clear();
        assert!(!m.restore());
        assert_eq!(m.focused(), None);
    }

    #[test]
    fn focus_next_no_op_on_empty_enumeration() {
        let mut m = FocusManager::new();
        assert!(!m.focus_next());
        assert_eq!(m.focused(), None);
    }

    // ----- R693 §5.39: modal focus trap -----

    #[test]
    fn push_modal_scope_auto_focuses_first_member() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["open_btn"]));
        m.focus_set("open_btn");
        assert!(m.push_modal_scope(tags(&["ok", "cancel"])));
        assert!(m.is_modal());
        assert_eq!(m.modal_depth(), 1);
        assert_eq!(m.focused(), Some("ok"), "focus moves into the modal");
    }

    #[test]
    fn modal_tab_traps_within_members_and_wraps() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["open_btn"]));
        m.push_modal_scope(tags(&["ok", "cancel"]));
        assert_eq!(m.focused(), Some("ok"));
        assert!(m.focus_next());
        assert_eq!(m.focused(), Some("cancel"));
        assert!(m.focus_next());
        assert_eq!(m.focused(), Some("ok"), "Tab wraps inside the trap");
        assert!(m.focus_prev());
        assert_eq!(m.focused(), Some("cancel"), "Shift+Tab wraps backward");
    }

    #[test]
    fn modal_focus_set_rejects_tags_outside_scope() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["open_btn"]));
        m.push_modal_scope(tags(&["ok", "cancel"]));
        // The invoker (in tab_order, NOT in the modal members) cannot be
        // focused while the trap is active.
        assert!(!m.focus_set("open_btn"));
        assert_eq!(m.focused(), Some("ok"));
        assert!(m.focus_set("cancel"));
        assert_eq!(m.focused(), Some("cancel"));
    }

    #[test]
    fn pop_modal_scope_restores_invoker_focus() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["open_btn", "other"]));
        m.focus_set("open_btn");
        m.push_modal_scope(tags(&["ok", "cancel"]));
        m.focus_next(); // -> cancel, inside the trap
        assert!(m.pop_modal_scope());
        assert!(!m.is_modal());
        assert_eq!(m.focused(), Some("open_btn"), "invoker focus restored");
    }

    #[test]
    fn modal_members_need_not_be_in_tab_order() {
        // The dynamic-focusable case: a dialog's controls are focusable
        // only while open, so they are intentionally absent from the
        // static tab_order. The trap still traverses them.
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["open_btn"]));
        m.push_modal_scope(tags(&["ok", "cancel"]));
        assert!(m.focus_set("cancel"));
        assert_eq!(m.focused(), Some("cancel"));
    }

    #[test]
    fn update_focusable_tags_does_not_drop_modal_focus() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["open_btn"]));
        m.push_modal_scope(tags(&["ok", "cancel"]));
        // A render refreshes the static enumeration; the modal member
        // "ok" is not in it but must survive (it lives in the scope).
        m.update_focusable_tags(tags(&["open_btn"]));
        assert_eq!(m.focused(), Some("ok"));
    }

    #[test]
    fn nested_modal_scopes_stack_and_unwind() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["root"]));
        m.focus_set("root");
        m.push_modal_scope(tags(&["a", "b"])); // settings dialog
        assert_eq!(m.focused(), Some("a"));
        m.focus_next(); // -> b, the invoker for the nested modal
        m.push_modal_scope(tags(&["yes", "no"])); // confirm over settings
        assert_eq!(m.modal_depth(), 2);
        assert_eq!(m.focused(), Some("yes"));
        // Inner trap confines to its own members.
        m.focus_next();
        assert_eq!(m.focused(), Some("no"));
        assert!(!m.focus_set("a"), "outer member unreachable from inner trap");
        // Pop inner -> restores the settings dialog's saved member.
        m.pop_modal_scope();
        assert_eq!(m.modal_depth(), 1);
        assert_eq!(m.focused(), Some("b"));
        // Pop outer -> restores the root invoker.
        m.pop_modal_scope();
        assert!(!m.is_modal());
        assert_eq!(m.focused(), Some("root"));
    }

    #[test]
    fn pop_without_push_is_noop() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["a"]));
        m.focus_set("a");
        assert!(!m.pop_modal_scope());
        assert_eq!(m.focused(), Some("a"));
    }

    #[test]
    fn push_empty_modal_scope_clears_focus() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["open_btn"]));
        m.focus_set("open_btn");
        assert!(m.push_modal_scope(Vec::new()));
        assert!(m.is_modal());
        assert_eq!(m.focused(), None, "a modal with no controls has no focus");
    }

    #[test]
    fn pop_drops_focus_when_saved_invoker_gone() {
        let mut m = FocusManager::new();
        m.update_focusable_tags(tags(&["open_btn"]));
        m.focus_set("open_btn");
        m.push_modal_scope(tags(&["ok"]));
        // The invoker was removed from the enumeration while the modal
        // was up (its view branch went away); restore finds nothing.
        m.update_focusable_tags(tags(&["other"]));
        assert!(m.pop_modal_scope());
        assert_eq!(m.focused(), None);
    }
}
