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

use std::mem;

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
    pub fn update_focusable_tags(&mut self, tags: Vec<String>) {
        if let Some(f) = self.focused.as_deref() {
            if !tags.iter().any(|t| t == f) {
                self.focused = None;
            }
        }
        self.tab_order = tags;
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
        if !self.tab_order.iter().any(|t| t == tag) {
            return false;
        }
        if self.focused.as_deref() == Some(tag) {
            return false;
        }
        self.focused = Some(tag.to_owned());
        true
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
        if self.tab_order.is_empty() {
            return false;
        }
        let n = self.tab_order.len();
        let next_idx: usize = match self.focused.as_deref() {
            None => {
                if direction > 0 {
                    0
                } else {
                    n - 1
                }
            }
            Some(t) => {
                let Some(cur) = self.tab_order.iter().position(|x| x == t) else {
                    // Inconsistent state — `update_focusable_tags`
                    // invariant says focused tag is in tab_order.
                    // Defensive recovery: drop focus.
                    self.focused = None;
                    return true;
                };
                let n_i = i64::try_from(n).unwrap_or(i64::MAX);
                let cur_i = i64::try_from(cur).unwrap_or(0);
                let stepped = (cur_i + direction).rem_euclid(n_i);
                usize::try_from(stepped).unwrap_or(0)
            }
        };
        let new_tag = &self.tab_order[next_idx];
        if self.focused.as_deref() == Some(new_tag.as_str()) {
            return false;
        }
        self.focused = Some(new_tag.clone());
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
}
