//! R51.121 §5.40 — `WidgetA11y` accessibility binding supertrait.
//!
//! Lives between [`pinion_core::WidgetCore`] (backend-free contract)
//! and the backend traits (`pinion_shell::WidgetView` for Vello GUI,
//! `pinion_tui::WidgetViewTui` for ratatui TUI), carrying the methods
//! that return pinion-a11y types ([`AccessNode`], [`AccessFocus`],
//! [`AccessAction`]). Both backend traits supertrait `WidgetA11y` so
//! each widget binding declares its a11y semantic-tree contribution in
//! one place regardless of backend.
//!
//! ## Why a separate trait, not folded into `WidgetCore`
//!
//! `pinion-a11y` depends on `pinion-core` (for `Scene` / `Rect`) so
//! `WidgetCore` cannot reference `AccessNode` without a dependency
//! cycle. The textbook ISP resolution is the supertrait split: the
//! a11y surface lives in this crate, gets `WidgetCore` as its
//! supertrait, and downstream backend traits then supertrait
//! `WidgetA11y` to inherit the whole chain.
//!
//! ## What this trait carries
//!
//! - [`access_node`](WidgetA11y::access_node) — per-binding a11y
//!   semantic-tree contribution. WAI-ARIA 1.2 §4.3 — the role / name /
//!   value / state shape exposed to AT (Narrator / `VoiceOver` / Orca /
//!   `TalkBack` via `accesskit` lowering).
//! - [`access_focus_target`](WidgetA11y::access_focus_target) —
//!   composite focus model (atomic widgets return
//!   `AccessFocus::atomic`, composite widgets return
//!   `AccessFocus::composite(parent, child)`). Drives AccessKit's
//!   `TreeUpdate::focus` + `aria-activedescendant` lowering.
//! - [`access_child_invoke`](WidgetA11y::access_child_invoke) —
//!   composite-side dispatch for AT-driven activation of a sub-child.
//!   Closes WCAG 4.1.2 (Name, Role, **Value**) for composites where
//!   the atomic event channel cannot carry the child identity.
//!
//! All three carry default impls that are correct for atomic widgets;
//! composites override `access_focus_target` + `access_child_invoke`.
//! Bindings without an AT-visible identity may leave all three at
//! their defaults — the widget is then `role="presentation"` to AT
//! consumers.

use pinion_core::{Scene, WidgetCore};

use crate::action::AccessAction;
use crate::focus::AccessFocus;
use crate::node::AccessNode;

/// R51.121 §5.40 — a11y contribution supertrait, between
/// [`pinion_core::WidgetCore`] and the backend traits.
///
/// Every widget binding's `impl WidgetA11y for X` block carries the
/// a11y semantic-tree shape; the backend traits (`WidgetView` /
/// `WidgetViewTui`) supertrait this so the same a11y data flows
/// uniformly to AccessKit (GUI today; PTY-driven TUI screen reader
/// carry-forward).
///
/// Like [`WidgetCore`], every method is an associated function (no
/// `&self`) because widget bindings live on unit types and the trait
/// is used for namespacing.
pub trait WidgetA11y: WidgetCore {
    /// Accessibility semantic tree contribution.
    ///
    /// Return one [`AccessNode`] per AT-visible tag the widget paints
    /// (atomic widgets emit a single node; composite widgets like
    /// `RadioGroup` emit the group node plus one per child radio).
    /// Bounds are filled in by the shell after layout — widgets need
    /// not (and should not) resolve pixel rects here. The `focused`
    /// argument carries the focus manager's currently-focused tag at
    /// emit time so each [`AccessNode`]'s state can set its `focused`
    /// flag without the widget tracking focus state independently.
    ///
    /// Default returns an empty vector — widgets that opt out are
    /// AT-invisible (a deliberate intent declaration; see WAI-ARIA
    /// Authoring Practices on `role="presentation"`).
    #[must_use]
    fn access_node(_state: &Self::State, _focused: Option<&str>) -> Vec<AccessNode> {
        Vec::new()
    }

    /// Composite focus model for AccessKit.
    ///
    /// AccessKit's `TreeUpdate::focus` points at a single `NodeId`,
    /// and ARIA Authoring Practices' roving-tabindex pattern adds a
    /// second piece: the focused composite parent's
    /// `aria-activedescendant` (lowered as
    /// `accesskit::Node::set_active_descendant`) names the currently
    /// addressed child within that parent.
    ///
    /// * Atomic widgets (Button, Switch, Slider) return
    ///   `Some(AccessFocus::atomic(tag))` — own `NodeId`, no
    ///   descendant.
    /// * Composite widgets (`RadioGroup`, `ListBox`) return
    ///   `Some(AccessFocus::composite(parent_tag, child_tag))` —
    ///   parent `NodeId` becomes `TreeUpdate::focus`, parent
    ///   `accesskit::Node` is annotated via
    ///   `set_active_descendant(child_id)`.
    ///
    /// Default wraps `focused` as [`AccessFocus::atomic`]. Returning
    /// `None` leaves AccessKit's focus on the synthetic window root.
    #[must_use]
    fn access_focus_target(_state: &Self::State, focused: Option<&str>) -> Option<AccessFocus> {
        focused.map(AccessFocus::atomic)
    }

    /// Composite-side dispatch for an AT-driven action targeting a
    /// sub-child by the segment after `#` in the widget tag.
    ///
    /// AccessKit's `ActionRequest` delivers `Click` / `Default` /
    /// `Focus` / `Increment` / `Decrement` against a `NodeId`; the
    /// shell recovers the widget tag and, for composite widgets,
    /// splits it at `#` (e.g. `"main_group#1"` → `("main_group",
    /// "1")`). The shell focuses the parent tag uniformly and then
    /// calls this hook so composite widgets can wire the activation
    /// through their existing wire-format invocation path.
    ///
    /// R662 §5.40 — the `parent_tag` argument disambiguates multi-
    /// composite bindings (e.g. todomvc carries both `todo_filter#<i>`
    /// and `todo_item#<id>` children; the `sub_tag` alone is ambiguous
    /// because filter indices `1`/`2` collide with `TodoItem` id `1`/`2`).
    /// Single-composite bindings (hello-radio-group, hello-listbox)
    /// can ignore the argument.
    ///
    /// Returns `true` if the action was handled — the shell then
    /// bumps the §5.34 revision, refreshes cached state, and drains
    /// pending intents. Returns `false` to let the shell fall through
    /// to the atomic-widget chain (`focus_set` + `apply_key("Enter")`).
    ///
    /// Default returns `false` — atomic widgets receive no
    /// composite-child requests because their [`access_node`] impls
    /// never expose `#`-suffixed tags.
    ///
    /// WAI-ARIA / WCAG 4.1.2 (Name, Role, **Value**) coverage:
    /// without this hook, an AT issuing `Click` on a composite child
    /// cannot programmatically set the underlying value — the §5.40
    /// substrate prior to R51.70 only logged a carry. The hook closes
    /// the write-path gap for composites.
    fn access_child_invoke(
        _scene: &mut Scene,
        _parent_tag: &str,
        _sub_tag: &str,
        _action: AccessAction,
    ) -> bool {
        false
    }
}
