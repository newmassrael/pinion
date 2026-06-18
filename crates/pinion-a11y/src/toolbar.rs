//! AccessKit tree builder for a **WAI-ARIA 1.2 §3.4 toolbar** (R692
//! §5.40, lifted R800): a labelled [`AriaRole::Toolbar`] parent holding one
//! [`AriaRole::Button`] per control, each carrying its roving-tabindex set
//! position ([`AccessNode::with_position_in_set`] / `with_size_of_set`) and,
//! for a toggle control, **`aria-pressed`** ([`AccessState::checked`] =
//! `Some(on)` on a `button` role).
//!
//! The `toolbar` role (a single Tab stop with an internal roving cursor) is
//! distinct from the passive [`AriaRole::Group`] a
//! [`toggle_button_group`](crate::toggle_group) wraps and from the
//! single-select [`radiogroup`](crate::radiogroup); see [`AriaRole::Toolbar`]
//! for the role rationale.
//!
//! ## Two consumers, two name sources
//!
//! - `hello-toolbar` (R692) labels its controls with painted glyphs
//!   (`B` / `I` / `Undo`), so it leaves [`ToolbarControl::name`] `None` and
//!   lets [`enrich_names_from_scene`](crate::enrich_names_from_scene) fill
//!   the accessible name from the paint.
//! - `hello-textarea`'s formatting toolbar (R799) mixes glyph toggles
//!   (`B` / `I`) with glyph-less colour swatches, so it passes an
//!   **explicit** name for every control (`"Bold"`, `"Red"`, …) — a swatch
//!   has no `TextNode` to enrich from, and an explicit name is the more
//!   robust choice regardless (it cannot be corrupted by a stray glyph).
//!
//! The interaction half (roving cursor / command-toggle activation /
//! introspect) lives in [`pinion_core::widgets::toolbar`]; this module is
//! the a11y half, mirroring the [`toggle_group`](crate::toggle_group) /
//! [`navigation`](crate::navigation) / [`radiogroup`](crate::radiogroup)
//! family lifts.

use crate::node::{AccessNode, AccessState};
use crate::role::AriaRole;

/// One toolbar control's identity and live state for
/// [`toolbar_button_nodes`].
#[derive(Clone, Copy, Debug)]
pub struct ToolbarControl<'a> {
    /// Dispatch tag — the control's composite tag
    /// (`pinion_widget_paint::toolbar::composite_item_tag`), matching the
    /// painted node so `enrich_names_from_scene` and the router agree.
    pub tag: &'a str,
    /// Explicit accessible name, or `None` to enrich it from the painted
    /// label. Glyph-less controls (e.g. a colour swatch) **must** name
    /// themselves — there is no `TextNode` to enrich from.
    pub name: Option<&'a str>,
    /// `aria-pressed` for a toggle control (`Some(on)`); `None` for a
    /// one-shot command. For a *reflective* formatting toggle this is read
    /// from the document (the selection's style), not an owned bit.
    pub checked: Option<bool>,
    /// `aria-disabled` — `true` for a control that is currently inoperable
    /// (R989). Like [`checked`](Self::checked) this is *reflective*: a
    /// contextual selection toolbar recomputes it each frame from the
    /// selection (e.g. "Delete" is disabled while nothing is selected). A
    /// disabled control stays in the tree (AT announces it as unavailable);
    /// its activation is a no-op the binding's reducer gates.
    pub disabled: bool,
}

/// Build the `toolbar` parent + one `button` per control.
///
/// The toolbar carries an explicit `bar_name` and references every control
/// tag as a child; each control button carries its name (explicit or
/// enriched), its `aria-pressed` ([`AccessState::checked`]), its
/// roving-set position, and `focused` when its index equals
/// `focused_control`. Pass `Some(cursor)` only while the strip owns the
/// shell focus (so the ring shows on the roving control); pass `None` for a
/// non-focusable strip (e.g. an editor's `NoFocus` formatting toolbar). The
/// returned vector is `[toolbar, button_0, button_1, ...]` — the parent
/// first, mirroring the flat-list convention `lower_access_node` resolves.
#[must_use]
pub fn toolbar_button_nodes(
    bar_tag: &str,
    bar_name: &str,
    controls: &[ToolbarControl<'_>],
    focused_control: Option<usize>,
) -> Vec<AccessNode> {
    let mut nodes: Vec<AccessNode> = Vec::with_capacity(controls.len() + 1);
    let mut bar = AccessNode::new(bar_tag, AriaRole::Toolbar).with_name(bar_name);
    for c in controls {
        bar = bar.with_child(c.tag);
    }
    nodes.push(bar);

    for (i, c) in controls.iter().enumerate() {
        let mut node = AccessNode::new(c.tag, AriaRole::Button)
            .with_state(AccessState {
                focused: focused_control == Some(i),
                checked: c.checked,
                disabled: c.disabled,
                ..AccessState::default()
            })
            .with_set_position(i, controls.len());
        if let Some(name) = c.name {
            node = node.with_name(name);
        }
        nodes.push(node);
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controls() -> [ToolbarControl<'static>; 4] {
        [
            ToolbarControl { tag: "bar#0", name: Some("Bold"), checked: Some(true), disabled: false },
            ToolbarControl { tag: "bar#1", name: Some("Italic"), checked: Some(false), disabled: false },
            ToolbarControl { tag: "bar#2", name: Some("Red"), checked: None, disabled: false },
            ToolbarControl { tag: "bar#3", name: None, checked: None, disabled: false },
        ]
    }

    #[test]
    fn emits_toolbar_parent_plus_n_buttons() {
        let c = controls();
        let nodes = toolbar_button_nodes("bar", "Formatting", &c, None);
        assert_eq!(nodes.len(), c.len() + 1, "one toolbar + N controls");
        assert_eq!(nodes[0].role, AriaRole::Toolbar);
        assert_eq!(nodes[0].name.as_deref(), Some("Formatting"));
        assert_eq!(nodes[0].children.len(), c.len(), "toolbar references every control");
        for node in &nodes[1..] {
            assert_eq!(node.role, AriaRole::Button, "control is a button");
        }
    }

    #[test]
    fn aria_pressed_only_on_toggle_controls() {
        let nodes = toolbar_button_nodes("bar", "Formatting", &controls(), None);
        assert_eq!(nodes[1].state.checked, Some(true), "Bold reflects aria-pressed = true");
        assert_eq!(nodes[2].state.checked, Some(false), "Italic reflects aria-pressed = false");
        assert_eq!(nodes[3].state.checked, None, "a command swatch carries no aria-pressed");
    }

    #[test]
    fn explicit_name_set_else_left_for_enrichment() {
        let nodes = toolbar_button_nodes("bar", "Formatting", &controls(), None);
        assert_eq!(nodes[1].name.as_deref(), Some("Bold"), "explicit name is set");
        assert_eq!(nodes[4].name, None, "a None name is left for scene enrichment");
    }

    #[test]
    fn roving_set_position_and_focus() {
        let c = controls();
        let nodes = toolbar_button_nodes("bar", "Formatting", &c, Some(2));
        for (i, node) in nodes[1..].iter().enumerate() {
            assert_eq!(node.position_in_set, Some(u32::try_from(i + 1).unwrap()));
            assert_eq!(node.size_of_set, Some(4));
            assert_eq!(node.state.focused, i == 2, "only the roving cursor is focused");
        }
    }

    #[test]
    fn no_focus_when_strip_unfocused() {
        let nodes = toolbar_button_nodes("bar", "Formatting", &controls(), None);
        for node in &nodes[1..] {
            assert!(!node.state.focused, "a non-focusable strip rings no control");
        }
    }

    #[test]
    fn r989_disabled_control_lowers_aria_disabled() {
        // Controls 0 and 2 disabled, 1 and 3 operable.
        let c = [
            ToolbarControl { tag: "a#0", name: Some("Delete"), checked: None, disabled: true },
            ToolbarControl { tag: "a#1", name: Some("Clear"), checked: None, disabled: false },
            ToolbarControl { tag: "a#2", name: Some("Tag"), checked: None, disabled: true },
            ToolbarControl { tag: "a#3", name: Some("Select all"), checked: None, disabled: false },
        ];
        let nodes = toolbar_button_nodes("a", "Selection actions", &c, None);
        assert!(nodes[1].state.disabled, "Delete lowers aria-disabled");
        assert!(!nodes[2].state.disabled, "Clear stays operable");
        assert!(nodes[3].state.disabled, "Tag lowers aria-disabled");
        assert!(!nodes[4].state.disabled, "Select all stays operable");
        // A disabled control stays in the tree (AT announces it as unavailable).
        assert_eq!(nodes.len(), c.len() + 1, "disabled controls are not pruned");
    }
}
