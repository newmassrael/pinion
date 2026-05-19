//! R51.71 §5.40 — AT-side focus declaration with active descendant.
//!
//! ARIA Authoring Practices' canonical composite focus model
//! (roving-tabindex / `aria-activedescendant`): the parent widget
//! owns the tab stop, and a single child within the parent is the
//! "currently active" descendant. Concretely:
//!
//! * `TreeUpdate::focus` = the parent's `NodeId` (or the atomic
//!   widget's own id).
//! * `accesskit::Node::set_active_descendant(child_id)` on the
//!   parent surfaces the within-parent "current item" to AT
//!   clients.
//!
//! [`AccessFocus`] is the typed carrier pinion's `WidgetView` impls
//! return so the shell can apply both pieces — replacing the
//! pre-R51.71 single-tag `Option<String>` (which conflated focus
//! and active descendant by addressing the child directly).

/// AT-side focus target.
///
/// Atomic widgets construct with [`AccessFocus::atomic`]; the
/// emitted `TreeUpdate::focus` resolves to the widget's own
/// `NodeId` and no `active_descendant` is set.
///
/// Composite widgets (`RadioGroup`, future `ListBox` /
/// `MenuButton` / `TreeView`) construct with
/// [`AccessFocus::composite`]; the `TreeUpdate::focus` resolves to
/// the parent's `NodeId` and the parent `accesskit::Node` is
/// decorated with `set_active_descendant(child_id)` at tree-build
/// time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessFocus {
    /// Tag whose `NodeId` becomes `TreeUpdate::focus`. Atomic
    /// widget = own tag; composite = parent tag.
    pub focus_tag: String,
    /// Composite-only: tag of the child element currently
    /// addressed within the focused parent. `None` for atomic
    /// widgets.
    pub active_descendant: Option<String>,
}

impl AccessFocus {
    /// Atomic shorthand — `focus_tag` with no active descendant.
    /// Use when a single-`NodeId` widget (Button, Switch, Slider)
    /// owns the focus.
    #[must_use]
    pub fn atomic(tag: impl Into<String>) -> Self {
        Self {
            focus_tag: tag.into(),
            active_descendant: None,
        }
    }

    /// R51.84 §5.40 — chainable builder that attaches an active
    /// descendant tag to an existing [`AccessFocus`].
    ///
    /// Matches the [`crate::AccessNode::with_name`] /
    /// [`crate::AccessNode::with_value`] builder pattern so widgets
    /// that conditionally attach a descendant can chain on top of
    /// [`Self::atomic`]:
    ///
    /// ```ignore
    /// let mut focus = AccessFocus::atomic(parent_tag);
    /// if let Some(child) = active_child {
    ///     focus = focus.with_active_descendant(child);
    /// }
    /// ```
    ///
    /// Unconditional composite widgets prefer [`Self::composite`] —
    /// the shorthand delegates here.
    #[must_use]
    pub fn with_active_descendant(mut self, child: impl Into<String>) -> Self {
        self.active_descendant = Some(child.into());
        self
    }

    /// Composite shorthand — parent + active descendant. The
    /// AT-side focus lands on `parent`; the parent's
    /// `accesskit::Node` is annotated with
    /// `set_active_descendant(child)`.
    ///
    /// R51.84 §5.40 — implemented as
    /// `Self::atomic(parent).with_active_descendant(child)` so the
    /// builder chain and the shorthand share a single source of
    /// truth for the active-descendant assignment.
    #[must_use]
    pub fn composite(parent: impl Into<String>, child: impl Into<String>) -> Self {
        Self::atomic(parent).with_active_descendant(child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_has_no_active_descendant() {
        let f = AccessFocus::atomic("main_btn");
        assert_eq!(f.focus_tag, "main_btn");
        assert!(f.active_descendant.is_none());
    }

    #[test]
    fn composite_carries_both_tags() {
        let f = AccessFocus::composite("main_group", "main_group#1");
        assert_eq!(f.focus_tag, "main_group");
        assert_eq!(f.active_descendant.as_deref(), Some("main_group#1"));
    }

    #[test]
    fn equality_distinguishes_atomic_from_composite() {
        let a = AccessFocus::atomic("g");
        let c = AccessFocus::composite("g", "g#0");
        assert_ne!(a, c);
    }

    #[test]
    fn r51_84_with_active_descendant_chains_on_atomic() {
        // R51.84 §5.40 — `atomic(...).with_active_descendant(...)`
        // is byte-identical to `composite(...)`. The chainable form
        // is the textbook path for conditional descendant
        // attachment; the `composite` constructor is the unconditional
        // shorthand that delegates to it.
        let chained =
            AccessFocus::atomic("g").with_active_descendant("g#0");
        let shorthand = AccessFocus::composite("g", "g#0");
        assert_eq!(chained, shorthand);
        assert_eq!(chained.focus_tag, "g");
        assert_eq!(chained.active_descendant.as_deref(), Some("g#0"));
    }
}
