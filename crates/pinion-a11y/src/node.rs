//! R51.61 §5.40 — Pinion `AccessNode` descriptor.
//!
//! [`AccessNode`] is the pinion-native a11y descriptor returned by
//! `WidgetView::access_node(&Scene, focused_tag) -> Option<AccessNode>`
//! (lands R51.63 wiring). It carries everything an
//! `accesskit::TreeUpdate` consumer needs to expose the widget to
//! Windows UIA / macOS AX / Linux AT-SPI / Android: ARIA role,
//! accessible name, current value, interaction-state flags, hit-test
//! bounds, and (for composites) the tag list of internal children.
//!
//! The struct is intentionally a plain data type — `AccessTreeBuilder`
//! (`tree.rs`) is the only consumer that lowers it into
//! `accesskit::Node`, so a future `accesskit` API change rewrites the
//! builder rather than every widget impl.

use pinion_core::scene::Rect;

use crate::role::AriaRole;

/// Pinion-native a11y descriptor for one widget.
///
/// One [`AccessNode`] per tagged widget in the paint scene. Composite
/// widgets (`RadioGroup`) own their internal children's `AccessNode`s
/// via `children: Vec<String>` (tag references — the tree builder
/// resolves them into `accesskit::NodeId`s at build time).
#[derive(Clone, Debug, PartialEq)]
pub struct AccessNode {
    /// Widget tag (the same identifier used by `InputRouter`, the
    /// focus manager, and the introspect schema).
    pub tag: String,
    /// ARIA role — drives `accesskit::Role` in the emitted node.
    pub role: AriaRole,
    /// Accessible name (`aria-label` equivalent).
    ///
    /// `WidgetView::access_node` impls leave this `None`; the shell
    /// calls [`crate::enrich_names_from_scene`] after layout to
    /// derive the name from the paint scene per WAI-ARIA 1.2 §4.3
    /// precedence: `ContainerNode::aria_label` override first, then
    /// the first descendant `TextNode::content`. Widgets that
    /// fundamentally lack visible text (icon-only without an
    /// `aria_label` modifier) may set this explicitly via
    /// [`AccessNode::with_name`] and the enrichment will respect
    /// that override.
    pub name: Option<String>,
    /// Current widget value (boolean for switch/check/radio, float
    /// for slider). Introspect schema reports the same value, by
    /// design (lockstep, single source of truth).
    pub value: Option<AccessValue>,
    /// Interaction-state flags — mirror §5.39 focus + §5.35 hover
    /// + §5.35 pressed.
    pub state: AccessState,
    /// Hit-test rectangle. Used by AT to overlay focus rings,
    /// magnifiers, and pointer-driven readout.
    pub bounds: Option<Rect>,
    /// Tag references for composite children. Empty for atomic
    /// widgets. The tree builder resolves these into
    /// `accesskit::NodeId`s and attaches them under this node.
    pub children: Vec<String>,
}

impl AccessNode {
    /// Construct a minimal node with no name / value / state /
    /// bounds / children. Builder-style setters (`with_*`) fill
    /// in the rest.
    #[must_use]
    pub fn new(tag: impl Into<String>, role: AriaRole) -> Self {
        Self {
            tag: tag.into(),
            role,
            name: None,
            value: None,
            state: AccessState::default(),
            bounds: None,
            children: Vec::new(),
        }
    }

    /// Set the accessible name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the current value.
    #[must_use]
    pub fn with_value(mut self, value: AccessValue) -> Self {
        self.value = Some(value);
        self
    }

    /// Replace the state flags.
    #[must_use]
    pub fn with_state(mut self, state: AccessState) -> Self {
        self.state = state;
        self
    }

    /// Set the hit-test bounds.
    #[must_use]
    pub fn with_bounds(mut self, bounds: Rect) -> Self {
        self.bounds = Some(bounds);
        self
    }

    /// Append a composite child tag.
    #[must_use]
    pub fn with_child(mut self, child_tag: impl Into<String>) -> Self {
        self.children.push(child_tag.into());
        self
    }
}

/// Interaction-state flags exposed to AT.
///
/// Mirrors §5.39 focus + §5.35 hover / pressed; `disabled` is opt-in
/// (default = false, so widgets that ignore the disabled invariant
/// stay AT-active). `checked: Option<bool>` is `None` for widgets
/// without a check semantic (`Button`, `Slider`) and `Some` for
/// `Switch` / `CheckBox` / `Radio`.
///
/// The four flag bools mirror the WAI-ARIA 1.2 state vocabulary
/// (focused / disabled / hovered / pressed) one-to-one. A bitflags
/// refactor would compress storage but obscure the public surface
/// — the textbook ARIA presentation is named fields, so the
/// `clippy::struct_excessive_bools` pedantic threshold is overridden
/// here.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AccessState {
    pub focused: bool,
    pub disabled: bool,
    pub hovered: bool,
    pub pressed: bool,
    pub checked: Option<bool>,
}

/// Numeric / boolean / string value carried by an `AccessNode`.
///
/// Lockstep with the introspect schema (§5.21): a checkbox's
/// `AccessValue::Bool` is the same `bool` the RPC introspect path
/// reports for the `"value"` key, and the slider's
/// `AccessValue::Float` shares min/max/value with the slider's
/// introspect descriptor.
#[derive(Clone, Debug, PartialEq)]
pub enum AccessValue {
    Bool(bool),
    Float { value: f32, min: f32, max: f32 },
    Text(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_empty() {
        let n = AccessNode::new("main_btn", AriaRole::Button);
        assert_eq!(n.tag, "main_btn");
        assert_eq!(n.role, AriaRole::Button);
        assert!(n.name.is_none());
        assert!(n.value.is_none());
        assert!(n.bounds.is_none());
        assert!(n.children.is_empty());
        assert_eq!(n.state, AccessState::default());
    }

    #[test]
    fn with_name_sets_name() {
        let n = AccessNode::new("btn", AriaRole::Button).with_name("Save");
        assert_eq!(n.name.as_deref(), Some("Save"));
    }

    #[test]
    fn with_value_bool() {
        let n = AccessNode::new("cb", AriaRole::CheckBox)
            .with_value(AccessValue::Bool(true));
        assert_eq!(n.value, Some(AccessValue::Bool(true)));
    }

    #[test]
    fn with_value_float() {
        let n = AccessNode::new("sl", AriaRole::Slider).with_value(
            AccessValue::Float { value: 0.5, min: 0.0, max: 1.0 },
        );
        assert!(matches!(
            n.value,
            Some(AccessValue::Float { value, min, max })
                if (value - 0.5).abs() < f32::EPSILON
                    && (min - 0.0).abs() < f32::EPSILON
                    && (max - 1.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn with_state_replaces_default() {
        let state = AccessState {
            focused: true,
            checked: Some(true),
            ..AccessState::default()
        };
        let n = AccessNode::new("cb", AriaRole::CheckBox).with_state(state);
        assert!(n.state.focused);
        assert_eq!(n.state.checked, Some(true));
        assert!(!n.state.disabled);
    }

    #[test]
    fn with_bounds_sets_rect() {
        let n = AccessNode::new("btn", AriaRole::Button)
            .with_bounds(Rect::new(10, 20, 100, 30));
        assert_eq!(n.bounds, Some(Rect::new(10, 20, 100, 30)));
    }

    #[test]
    fn with_child_appends_in_order() {
        let n = AccessNode::new("rg", AriaRole::RadioGroup)
            .with_child("r0")
            .with_child("r1")
            .with_child("r2");
        assert_eq!(n.children, vec!["r0", "r1", "r2"]);
    }

    #[test]
    fn access_state_default_all_false() {
        let s = AccessState::default();
        assert!(!s.focused);
        assert!(!s.disabled);
        assert!(!s.hovered);
        assert!(!s.pressed);
        assert_eq!(s.checked, None);
    }
}
