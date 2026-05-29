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
    /// R51.98 §5.40 — WAI-ARIA `aria-selected` per WAI-ARIA 1.2
    /// §6.6.7. `Some(true)` lowers to `accesskit::Node::set_selected`,
    /// `Some(false)` to `clear_selected` (explicit-unselected for AT
    /// awareness in multi-select containers), `None` omits the
    /// attribute (the default for roles without a selected semantic
    /// — `Button`, `Slider`, atomic `Switch`/`CheckBox`/`RadioButton`
    /// that already carry `aria-checked` instead).
    ///
    /// Axis distinction from `state.checked`: `aria-checked` is the
    /// truthy axis for two-state widgets (`Switch`, `CheckBox`,
    /// `RadioButton`); `aria-selected` is the truthy axis for
    /// container-membership widgets (`ListBoxOption`, `Tab`,
    /// `MenuItemRadio`, future grid cells). WAI-ARIA APG explicitly
    /// distinguishes them — a `Listbox` option is *selected*, not
    /// *checked*, regardless of the visual rendering. The R51.97
    /// `hello-listbox` emitted `aria-checked` via `state.checked` for
    /// `ListBox` options; R51.98 corrects that.
    pub selected: Option<bool>,
    /// R51.98 §5.40 — WAI-ARIA `aria-multiselectable` per WAI-ARIA
    /// 1.2 §6.6.6. `true` lowers to
    /// `accesskit::Node::set_multiselectable` (the AT then announces
    /// the container as "list, multi-selectable" instead of "list").
    /// Default `false` omits the attribute. Only meaningful on
    /// container roles that own a selection set (`Listbox`,
    /// future `Grid`/`Tree`/`TabList`); atomic roles ignore the flag.
    pub multiselectable: bool,
    /// R674 §5.40 — WAI-ARIA `aria-level` per WAI-ARIA 1.2 §6.6.8.
    /// One-based depth in the hierarchy. Required on per-item
    /// descriptors inside roles that own a hierarchical structure
    /// ([`AriaRole::TreeItem`] today; future `Heading` / `ListItem`
    /// nested under a `List`). The root of the hierarchy is
    /// `Some(1)`; each level of nesting adds one. `None` omits the
    /// attribute (the default for roles without a hierarchical
    /// semantic).
    ///
    /// **Authoring requirement** (WAI-ARIA 1.2 §6.6.8): for
    /// custom-widget roles without implicit native semantics
    /// (`role="treeitem"`, etc.) AT does **not** infer hierarchical
    /// depth from DOM nesting. Pinion paint scenes are flat row
    /// sequences (the substrate stamps composite tags per row), so
    /// the binding is the sole source of truth for the depth value.
    pub level: Option<u32>,
    /// R674 §5.40 — WAI-ARIA `aria-posinset` per WAI-ARIA 1.2
    /// §6.6.9. One-based position of this item within the parent's
    /// (visible) set. Pairs with [`Self::size_of_set`] so the AT
    /// can announce "item N of M".
    ///
    /// **Authoring requirement**: like [`Self::level`], the binding
    /// is the sole source of truth for custom-widget roles.
    /// `Some(1)` is the first sibling, `Some(2)` the second, etc.;
    /// `None` omits the attribute.
    pub position_in_set: Option<u32>,
    /// R674 §5.40 — WAI-ARIA `aria-setsize` per WAI-ARIA 1.2
    /// §6.6.10. Total count of (visible) items in this item's
    /// parent set. Pairs with [`Self::position_in_set`].
    ///
    /// **Authoring requirement**: when a tree / list owns a virtual
    /// or expandable set whose total count is unknown to the AT
    /// (collapsed branches, lazy-loaded children), the binding
    /// provides the visible-or-known total here. `None` omits the
    /// attribute.
    pub size_of_set: Option<u32>,
    /// R693 §5.40 — WAI-ARIA `aria-modal` per WAI-ARIA 1.2 §6.6.1.
    /// `true` lowers to `accesskit::Node::set_modal` so AT announces the
    /// node as a modal boundary and confines its virtual cursor to the
    /// subtree (the AT-side mirror of the [`crate::focus`]-trap the
    /// shell installs). Meaningful on [`AriaRole::Dialog`]; default
    /// `false` omits the attribute.
    pub modal: bool,
    /// R695 §5.40 — WAI-ARIA `aria-describedby` per WAI-ARIA 1.2
    /// §6.6.2. The tag of another [`AccessNode`] whose accessible name
    /// supplies *this* node's description (announced after the name).
    /// The tree builder resolves the tag into the target's
    /// `accesskit::NodeId` and lowers it via
    /// `accesskit::Node::set_described_by`. `None` omits the relation.
    ///
    /// The canonical use is the WCAG 2.2 SC 1.4.13 tooltip pattern: a
    /// trigger widget points its `described_by` at the
    /// [`AriaRole::Tooltip`] node so AT reads "Save, Saves the current
    /// file" — the tooltip text becomes the trigger's description, not
    /// a separately-focusable node. Single tag (not a list): pinion's
    /// one-description-source widgets need no multi-target relation
    /// until a 2nd consumer surfaces one
    /// (`[[abstraction-needs-second-consumer]]`).
    pub described_by: Option<String>,
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
            selected: None,
            multiselectable: false,
            level: None,
            position_in_set: None,
            size_of_set: None,
            modal: false,
            described_by: None,
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

    /// R51.98 §5.40 — set the WAI-ARIA `aria-selected` attribute.
    /// Use `true` for "this option is currently in the container's
    /// selection set", `false` for "explicitly not selected"
    /// (announced distinctly by AT in multi-select containers), or
    /// omit (leave `selected = None`) for roles that don't carry a
    /// selected axis.
    #[must_use]
    pub fn with_selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }

    /// R51.98 §5.40 — declare this container exposes
    /// `aria-multiselectable="true"`. Only meaningful on `Listbox`,
    /// future `Grid` / `Tree` / `TabList` parents; atomic roles
    /// ignore it at lowering.
    #[must_use]
    pub fn with_multiselectable(mut self) -> Self {
        self.multiselectable = true;
        self
    }

    /// R693 §5.40 — declare this node exposes `aria-modal="true"`. Used
    /// on the [`AriaRole::Dialog`] root while the dialog is open so AT
    /// confines its virtual cursor to the dialog subtree, mirroring the
    /// shell-side focus trap. See [`Self::modal`].
    #[must_use]
    pub fn with_modal(mut self) -> Self {
        self.modal = true;
        self
    }

    /// R674 §5.40 — set the WAI-ARIA `aria-level` attribute. See
    /// [`Self::level`] for the semantic axis and authoring contract.
    /// One-based: the root of the hierarchy is `1`.
    #[must_use]
    pub fn with_level(mut self, level: u32) -> Self {
        self.level = Some(level);
        self
    }

    /// R674 §5.40 — set the WAI-ARIA `aria-posinset` attribute. See
    /// [`Self::position_in_set`] for the semantic axis. One-based.
    #[must_use]
    pub fn with_position_in_set(mut self, position: u32) -> Self {
        self.position_in_set = Some(position);
        self
    }

    /// R674 §5.40 — set the WAI-ARIA `aria-setsize` attribute. See
    /// [`Self::size_of_set`] for the semantic axis.
    #[must_use]
    pub fn with_size_of_set(mut self, size: u32) -> Self {
        self.size_of_set = Some(size);
        self
    }

    /// R695 §5.40 — set the WAI-ARIA `aria-describedby` relation to the
    /// node tagged `tag`. See [`Self::described_by`] for the semantic
    /// axis (the tooltip-description pattern).
    #[must_use]
    pub fn with_described_by(mut self, tag: impl Into<String>) -> Self {
        self.described_by = Some(tag.into());
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
        // R674 §5.40 — hierarchical axes default to absent so non-
        // tree/list roles continue to omit the attributes.
        assert!(n.level.is_none());
        assert!(n.position_in_set.is_none());
        assert!(n.size_of_set.is_none());
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

    #[test]
    fn r51_98_new_omits_selected_and_multiselectable() {
        let n = AccessNode::new("opt", AriaRole::ListBoxOption);
        assert_eq!(n.selected, None);
        assert!(!n.multiselectable);
    }

    #[test]
    fn r51_98_with_selected_records_axis() {
        let n = AccessNode::new("opt", AriaRole::ListBoxOption).with_selected(true);
        assert_eq!(n.selected, Some(true));
        let n2 = AccessNode::new("opt", AriaRole::ListBoxOption).with_selected(false);
        assert_eq!(n2.selected, Some(false));
    }

    #[test]
    fn r51_98_with_multiselectable_marks_container() {
        let n = AccessNode::new("list", AriaRole::Listbox).with_multiselectable();
        assert!(n.multiselectable);
    }

    // R674 §5.40 — WAI-ARIA hierarchical axes (level / posinset /
    // setsize) builder + default-omission regression tests.

    #[test]
    fn r674_with_level_sets_aria_level() {
        let n = AccessNode::new("row", AriaRole::TreeItem).with_level(1);
        assert_eq!(n.level, Some(1));
        let n2 = AccessNode::new("row", AriaRole::TreeItem).with_level(3);
        assert_eq!(n2.level, Some(3));
    }

    #[test]
    fn r674_with_position_in_set_sets_aria_posinset() {
        let n = AccessNode::new("row", AriaRole::TreeItem)
            .with_position_in_set(2);
        assert_eq!(n.position_in_set, Some(2));
    }

    #[test]
    fn r674_with_size_of_set_sets_aria_setsize() {
        let n = AccessNode::new("row", AriaRole::TreeItem)
            .with_size_of_set(5);
        assert_eq!(n.size_of_set, Some(5));
    }

    // R695 §5.40 — aria-describedby builder + default omission.

    #[test]
    fn r695_new_omits_described_by() {
        let n = AccessNode::new("save_btn", AriaRole::Button);
        assert!(n.described_by.is_none());
    }

    #[test]
    fn r695_with_described_by_records_relation() {
        let n = AccessNode::new("save_btn", AriaRole::Button).with_described_by("save_tip");
        assert_eq!(n.described_by.as_deref(), Some("save_tip"));
    }

    #[test]
    fn r674_hierarchical_axes_compose() {
        // A treeitem typically carries all three axes together —
        // the canonical "item N of M at depth D" announcement
        // requires every value present.
        let n = AccessNode::new("row", AriaRole::TreeItem)
            .with_level(2)
            .with_position_in_set(3)
            .with_size_of_set(7);
        assert_eq!(n.level, Some(2));
        assert_eq!(n.position_in_set, Some(3));
        assert_eq!(n.size_of_set, Some(7));
    }
}
