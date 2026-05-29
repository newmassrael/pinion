//! R51.61 §5.40 — Pinion ARIA role enum.
//!
//! [`AriaRole`] is a pinion-native subset of `accesskit::Role` that
//! covers the framework's standard widget catalogue (Button / Switch /
//! `CheckBox` / `RadioButton` / `Slider` / `RadioGroup`) plus a `Generic`
//! fallback. The wrapper keeps the public `WidgetView::access_node`
//! return type stable across `accesskit` minor-version bumps — a future
//! `accesskit` upgrade rewrites only `to_accesskit` here, not every
//! widget impl.
//!
//! ARIA role names follow the WAI-ARIA 1.2 specification literal
//! spelling so introspect / RPC consumers see the same identifiers
//! they would in HTML's `role` attribute.

use accesskit::Role;

/// Pinion-native ARIA role enum.
///
/// One-to-one mapping with `accesskit::Role` for the widgets pinion
/// ships in its standard catalogue. The `Generic` variant maps to
/// `accesskit::Role::GenericContainer` and is the default for any
/// scene element that opts into the a11y tree without claiming a
/// specific role.
///
/// `#[non_exhaustive]` is deliberate — additional widgets (`TextInput`,
/// `ScrollBar`, etc.) land additively as new axes ratify.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AriaRole {
    Button,
    Switch,
    CheckBox,
    RadioButton,
    Slider,
    RadioGroup,
    /// R51.96.1 §5.40 — WAI-ARIA 1.2 §4.3.x `listbox` composite
    /// role. Container for [`Self::ListBoxOption`] children; pairs
    /// with the `ListBox` composite primitive
    /// ([`pinion_core::widgets::listbox`]). Distinct from
    /// [`Self::RadioGroup`] in the WAI-ARIA keyboard model: Arrow
    /// keys move focus only, `Space` / `Enter` commits the
    /// selection (vs Arrow-activates-immediately for `RadioGroup`).
    Listbox,
    /// R51.96.1 §5.40 — WAI-ARIA 1.2 `option` role. Single
    /// selectable child of an [`Self::Listbox`] parent. Distinct
    /// from [`Self::RadioButton`] only at the AT-side role surface;
    /// the underlying widget primitive
    /// ([`pinion_core::widgets::listbox_item`]) shares the
    /// button-like statechart with `Radio` / `Toggle` / `Checkbox`.
    ListBoxOption,
    /// R56.1.b.1 §5.40 — WAI-ARIA 1.2 §4.3 `textbox` role
    /// (single-line input). Pairs with the §5.38 `TextField` widget
    /// primitive ([`pinion_core::widgets::text_field`]). The role
    /// name lowers to `accesskit::Role::TextInput` — AccessKit splits
    /// the WAI-ARIA `textbox` role across two enum members
    /// (`TextInput` for single-line, `MultilineTextInput` for the
    /// `aria-multiline=true` variant); pinion's single-line first
    /// slice maps to `TextInput`. A future multiline `TextArea` axis
    /// adds the second variant additively.
    TextInput,
    /// R656 §5.40 — WAI-ARIA 1.2 §5.3.5 `list` role. Container for
    /// [`Self::ListItem`] children carrying a flat sequence of items.
    /// Distinct from [`Self::Listbox`]: a `list` is a passive AT
    /// container with no keyboard model (Tab traversal lands on item
    /// children, not the list root), while a `listbox` owns the
    /// roving-tabindex + Arrow/Space activation model. Pinion's
    /// `todomvc` composed app uses `List` for the user-driven todo
    /// collection — items are added by Enter on a separate text field
    /// (not Arrow-key navigation), and the list itself never receives
    /// focus, matching the WAI-ARIA `list` semantics exactly.
    List,
    /// R656 §5.40 — WAI-ARIA 1.2 §5.3.6 `listitem` role. Single child
    /// of a [`Self::List`] parent. AT presents each item as a
    /// distinct entry with its own name + (optionally) interactive
    /// descendants; AT tools (screen readers, `VoiceOver`, Orca)
    /// announce position-in-list ("item 2 of 3") automatically when
    /// items are nested under a `List` parent. Distinct from
    /// [`Self::ListBoxOption`]: a `listitem` does not participate in
    /// a selection model (no `aria-selected`), while a `listboxoption`
    /// always carries one.
    ListItem,
    /// R673 §5.40 — WAI-ARIA 1.2 §5.3.10 `tree` role. Hierarchical
    /// container of [`Self::TreeItem`] children. Distinct from
    /// [`Self::List`]: a `tree` owns the keyboard model (Arrow
    /// Up/Down navigates, Arrow Right expands, Arrow Left
    /// collapses, Home/End jump to first/last visible row); a
    /// `list` is a passive AT container. Pairs with the §5.50
    /// `pinion_widget_paint::tree_view` substrate (R671) +
    /// the interactive consumer pattern (R673 `hello-tree-view`).
    Tree,
    /// R673 §5.40 — WAI-ARIA 1.2 §5.3.11 `treeitem` role. Single
    /// child of a [`Self::Tree`] parent. Each row in the flat row
    /// sequence the `tree_view` substrate emits carries this role.
    ///
    /// **Authoring requirement** (R674 §5.40 correction): per
    /// WAI-ARIA 1.2 §6.6.8 / §6.6.9 / §6.6.10, `aria-level` /
    /// `aria-posinset` / `aria-setsize` are *required* on custom
    /// widget roles — AT does **not** infer them from DOM nesting
    /// for `role="treeitem"`. Pinion paint scenes are flat row
    /// sequences (the substrate stamps composite tags per row), so
    /// the binding's `access_node` walker is the sole source of
    /// truth for these values. Provide them via
    /// [`AccessNode::with_level`] / [`AccessNode::with_position_in_set`] /
    /// [`AccessNode::with_size_of_set`] when emitting per-row
    /// descriptors. `aria-expanded` is carried by [`AccessState`] on
    /// future axes; today branches encode the expanded glyph in the
    /// paint scene rather than the a11y state.
    ///
    /// [`AccessNode::with_level`]: crate::AccessNode::with_level
    /// [`AccessNode::with_position_in_set`]: crate::AccessNode::with_position_in_set
    /// [`AccessNode::with_size_of_set`]: crate::AccessNode::with_size_of_set
    /// [`AccessState`]: crate::node::AccessState
    TreeItem,
    /// R690 §5.40 — WAI-ARIA 1.2 §3.6 `tablist` role. Container for
    /// [`Self::Tab`] children that owns the roving-tabindex keyboard
    /// model (Arrow Left/Right move between tabs, Home/End jump to
    /// first/last). Distinct from [`Self::RadioGroup`] only at the AT
    /// role surface: the underlying selection substrate is shared
    /// ([`pinion_core::widgets::radio_group::RadioGroupExternal`] —
    /// "select 1 of N" is identical semantics), but a `tablist`
    /// announces "tab list" and its children announce "tab" rather
    /// than "radio button". Pairs with the §5.50
    /// `pinion_widget_paint::tabs` substrate (R690) + the
    /// `hello-tabs` consumer.
    TabList,
    /// R690 §5.40 — WAI-ARIA 1.2 §3.6 `tab` role. Single selectable
    /// child of a [`Self::TabList`] parent. Carries `aria-selected`
    /// (via [`AccessNode::with_selected`]) for the active tab and
    /// the WAI-ARIA 1.2 §6.6.9 / §6.6.10 `aria-posinset` /
    /// `aria-setsize` sibling axes ("tab N of M"). Distinct from
    /// [`Self::RadioButton`]: a tab is *selected*, not *checked* —
    /// the truthy axis is `aria-selected`, matching
    /// [`Self::ListBoxOption`].
    ///
    /// `aria-controls` (the tab → tab-panel relationship) is a future
    /// additive axis once a consumer surfaces multi-panel addressing;
    /// R690 renders a single panel for the active tab, so the
    /// relationship is structurally implicit.
    ///
    /// [`AccessNode::with_selected`]: crate::AccessNode::with_selected
    Tab,
    /// R690 §5.40 — WAI-ARIA 1.2 §3.6 `tabpanel` role. The content
    /// region associated with the active [`Self::Tab`]. Focusable
    /// (Tab key lands on the panel when it has no focusable content)
    /// so it shares the `Focus`-only action set with the other
    /// container roles. Only the active tab's panel is rendered, so
    /// at most one `tabpanel` node exists in the tree at a time.
    TabPanel,
    /// R691 §5.40 — WAI-ARIA 1.2 §3.5 `menubar` role. A horizontal
    /// presentation of a [`Self::Menu`] that usually stays visible
    /// (the editor `File` / `Edit` / `View` bar). Container for
    /// [`Self::MenuItem`] title children; owns the WAI-ARIA §3.5
    /// menubar keyboard model (Arrow Left/Right move between
    /// top-level items, Arrow Down opens the focused item's menu).
    /// Joins the focus-only container action set alongside
    /// [`Self::TabList`] / [`Self::Tree`] / [`Self::Listbox`].
    MenuBar,
    /// R691 §5.40 — WAI-ARIA 1.2 §3.5 `menu` role. The dropdown
    /// container shown when a [`Self::MenuBar`] title opens. Holds the
    /// open menu's [`Self::MenuItem`] children and owns the in-menu
    /// keyboard model (Arrow Up/Down move the active item, Home/End
    /// jump, Escape closes). Focus-only container action set — the
    /// active item is reported as the menu's `aria-activedescendant`,
    /// not as a focus of the container itself.
    Menu,
    /// R691 §5.40 — WAI-ARIA 1.2 §3.5 `menuitem` role. A single
    /// **command** within a [`Self::MenuBar`] (a top-level title) or
    /// a [`Self::Menu`] (a dropdown command). Commit-class atomic at
    /// the AT-action surface (Click activates, Focus moves the AT
    /// cursor) — the action set matches [`Self::Button`] /
    /// [`Self::Tab`].
    ///
    /// **Distinct from selection roles.** A base `menuitem` carries
    /// neither `aria-selected` ([`Self::Tab`] / [`Self::ListBoxOption`])
    /// nor `aria-checked` ([`Self::RadioButton`] / [`Self::CheckBox`]):
    /// activating it fires a one-shot command and (for a dropdown
    /// item) closes the menu. The stateful menu variants
    /// (`menuitemcheckbox` / `menuitemradio`) are separate WAI-ARIA
    /// roles that land additively if a consumer needs a toggled menu
    /// entry. This command-vs-selection split is why the §5.50 menu
    /// substrate (R691) uses a command-class `MenuBarExternal` rather
    /// than reusing the
    /// [`RadioGroupExternal`](pinion_core::widgets::radio_group::RadioGroupExternal)
    /// the way [`Self::Tab`] does.
    ///
    /// `aria-haspopup` (a top-level title that opens a submenu) and
    /// `aria-expanded` (the open/closed state of that submenu) are
    /// future additive `AccessNode` axes; R691 encodes the open menu
    /// structurally (the dropdown `Menu` node is present only while
    /// open) rather than on the title's a11y state.
    MenuItem,
    /// R692 §5.40 — WAI-ARIA 1.2 §3.4 `toolbar` role. A horizontal
    /// grouping of controls (the editor format / command strip every
    /// DCC / IDE / CAD tool ships) that owns the roving-tabindex
    /// keyboard model: the toolbar is a single Tab stop, Arrow
    /// Left/Right move the roving focus between controls, Home/End
    /// jump to first/last. Joins the focus-only container action set
    /// alongside [`Self::MenuBar`] / [`Self::TabList`] / [`Self::Tree`].
    ///
    /// Distinct from [`Self::MenuBar`]: a `toolbar`'s children are
    /// directly-actionable controls (command [`Self::Button`]s +
    /// toggle buttons carrying `aria-pressed`), not titles that open
    /// dropdown [`Self::Menu`]s. A toggle button is a `Button` whose
    /// [`AccessState::checked`](crate::node::AccessState) lowers to the
    /// AccessKit `Toggled` attribute — the WAI-ARIA `aria-pressed`
    /// axis emerges from the `button` role + the toggled state (vs the
    /// `aria-checked` axis a `checkbox` role carries for the same
    /// underlying attribute).
    Toolbar,
    /// R693 §5.40 — WAI-ARIA 1.2 §3.2 `dialog` role. A modal container
    /// (the confirm / form / alert dialog every pro tool ships) that
    /// holds the focus trap while open: focus moves into the dialog on
    /// open, Tab is confined to the dialog's controls, Escape dismisses,
    /// and focus returns to the invoker on close (the
    /// [`crate::node::AccessNode::modal`] flag lowers to the AccessKit
    /// `aria-modal` attribute so AT announces the modality).
    ///
    /// Focus-only container action set (alongside [`Self::MenuBar`] /
    /// [`Self::Toolbar`] / [`Self::Tree`]): the dialog itself is a
    /// grouping node; its action buttons are child [`Self::Button`]s.
    Dialog,
    /// R695 §5.40 — WAI-ARIA 1.2 §3.7 `tooltip` role. A contextual
    /// descriptive popup shown on hover / keyboard focus of a trigger
    /// element (WCAG 2.2 SC 1.4.13 "Content on Hover or Focus"). The
    /// tooltip carries **no interactive semantics** — it is a passive
    /// label region the trigger references through
    /// [`AccessNode::with_described_by`](crate::AccessNode::with_described_by)
    /// (the `aria-describedby` relation), so AT announces the tooltip
    /// text as the trigger's description rather than as a focusable
    /// node. Distinct from [`Self::Dialog`] (a focus container) and
    /// [`Self::Menu`] (a command popup): a tooltip never receives focus
    /// and owns no keyboard model of its own — `Escape` dismisses it
    /// while focus stays on the trigger.
    Tooltip,
    Generic,
}

impl AriaRole {
    /// Lower into `accesskit::Role` for `TreeUpdate` construction.
    ///
    /// `Generic` lowers to `Role::GenericContainer` per the WAI-ARIA
    /// authoring practices convention (it carries no implicit
    /// semantics; AT presents it as a passive container).
    #[must_use]
    pub const fn to_accesskit(self) -> Role {
        match self {
            Self::Button => Role::Button,
            Self::Switch => Role::Switch,
            Self::CheckBox => Role::CheckBox,
            Self::RadioButton => Role::RadioButton,
            Self::Slider => Role::Slider,
            Self::RadioGroup => Role::RadioGroup,
            Self::Listbox => Role::ListBox,
            Self::ListBoxOption => Role::ListBoxOption,
            Self::TextInput => Role::TextInput,
            Self::List => Role::List,
            Self::ListItem => Role::ListItem,
            Self::Tree => Role::Tree,
            Self::TreeItem => Role::TreeItem,
            Self::TabList => Role::TabList,
            Self::Tab => Role::Tab,
            Self::TabPanel => Role::TabPanel,
            Self::MenuBar => Role::MenuBar,
            Self::Menu => Role::Menu,
            Self::MenuItem => Role::MenuItem,
            Self::Toolbar => Role::Toolbar,
            Self::Dialog => Role::Dialog,
            Self::Tooltip => Role::Tooltip,
            Self::Generic => Role::GenericContainer,
        }
    }

    /// ARIA literal name as it would appear in an HTML `role`
    /// attribute. Used by introspect schema so the RPC surface and
    /// the AT surface report identical role identifiers.
    #[must_use]
    pub const fn aria_name(self) -> &'static str {
        match self {
            Self::Button => "button",
            Self::Switch => "switch",
            Self::CheckBox => "checkbox",
            Self::RadioButton => "radio",
            Self::Slider => "slider",
            Self::RadioGroup => "radiogroup",
            Self::Listbox => "listbox",
            Self::ListBoxOption => "option",
            // WAI-ARIA 1.2 spec literal — the single-line text input
            // role is `textbox` regardless of AccessKit's internal
            // single/multiline split.
            Self::TextInput => "textbox",
            Self::List => "list",
            Self::ListItem => "listitem",
            Self::Tree => "tree",
            Self::TreeItem => "treeitem",
            Self::TabList => "tablist",
            Self::Tab => "tab",
            Self::TabPanel => "tabpanel",
            Self::MenuBar => "menubar",
            Self::Menu => "menu",
            Self::MenuItem => "menuitem",
            Self::Toolbar => "toolbar",
            Self::Dialog => "dialog",
            Self::Tooltip => "tooltip",
            Self::Generic => "generic",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_lowers_to_accesskit_button() {
        assert_eq!(AriaRole::Button.to_accesskit(), Role::Button);
    }

    #[test]
    fn switch_lowers_to_accesskit_switch() {
        assert_eq!(AriaRole::Switch.to_accesskit(), Role::Switch);
    }

    #[test]
    fn radio_group_lowers_to_accesskit_radio_group() {
        assert_eq!(AriaRole::RadioGroup.to_accesskit(), Role::RadioGroup);
    }

    #[test]
    fn slider_lowers_to_accesskit_slider() {
        assert_eq!(AriaRole::Slider.to_accesskit(), Role::Slider);
    }

    #[test]
    fn generic_lowers_to_generic_container() {
        assert_eq!(AriaRole::Generic.to_accesskit(), Role::GenericContainer);
    }

    #[test]
    fn r693_dialog_lowers_to_accesskit_dialog() {
        assert_eq!(AriaRole::Dialog.to_accesskit(), Role::Dialog);
        assert_eq!(AriaRole::Dialog.aria_name(), "dialog");
    }

    #[test]
    fn aria_names_match_wai_aria_literals() {
        assert_eq!(AriaRole::Button.aria_name(), "button");
        assert_eq!(AriaRole::Switch.aria_name(), "switch");
        assert_eq!(AriaRole::CheckBox.aria_name(), "checkbox");
        assert_eq!(AriaRole::RadioButton.aria_name(), "radio");
        assert_eq!(AriaRole::Slider.aria_name(), "slider");
        assert_eq!(AriaRole::RadioGroup.aria_name(), "radiogroup");
        assert_eq!(AriaRole::Listbox.aria_name(), "listbox");
        assert_eq!(AriaRole::ListBoxOption.aria_name(), "option");
        assert_eq!(AriaRole::TextInput.aria_name(), "textbox");
        assert_eq!(AriaRole::List.aria_name(), "list");
        assert_eq!(AriaRole::ListItem.aria_name(), "listitem");
        assert_eq!(AriaRole::Generic.aria_name(), "generic");
    }

    // R51.96.1 §5.40 — Listbox / Option role lowering.

    #[test]
    fn listbox_lowers_to_accesskit_listbox() {
        assert_eq!(AriaRole::Listbox.to_accesskit(), Role::ListBox);
    }

    #[test]
    fn listbox_option_lowers_to_accesskit_listbox_option() {
        assert_eq!(
            AriaRole::ListBoxOption.to_accesskit(),
            Role::ListBoxOption
        );
    }

    // R56.1.b.1 §5.40 — TextInput role lowering.

    #[test]
    fn text_input_lowers_to_accesskit_text_input() {
        assert_eq!(AriaRole::TextInput.to_accesskit(), Role::TextInput);
    }

    // R656 §5.40 — List / ListItem role lowering.

    #[test]
    fn list_lowers_to_accesskit_list() {
        assert_eq!(AriaRole::List.to_accesskit(), Role::List);
    }

    #[test]
    fn list_item_lowers_to_accesskit_list_item() {
        assert_eq!(AriaRole::ListItem.to_accesskit(), Role::ListItem);
    }

    // R690 §5.40 — Tab / TabList / TabPanel role lowering + names.

    #[test]
    fn tab_list_lowers_to_accesskit_tab_list() {
        assert_eq!(AriaRole::TabList.to_accesskit(), Role::TabList);
    }

    #[test]
    fn tab_lowers_to_accesskit_tab() {
        assert_eq!(AriaRole::Tab.to_accesskit(), Role::Tab);
    }

    #[test]
    fn tab_panel_lowers_to_accesskit_tab_panel() {
        assert_eq!(AriaRole::TabPanel.to_accesskit(), Role::TabPanel);
    }

    #[test]
    fn tab_roles_aria_names_match_wai_aria_literals() {
        assert_eq!(AriaRole::TabList.aria_name(), "tablist");
        assert_eq!(AriaRole::Tab.aria_name(), "tab");
        assert_eq!(AriaRole::TabPanel.aria_name(), "tabpanel");
    }

    // R691 §5.40 — Menu / MenuBar / MenuItem role lowering + names.

    #[test]
    fn menu_bar_lowers_to_accesskit_menu_bar() {
        assert_eq!(AriaRole::MenuBar.to_accesskit(), Role::MenuBar);
    }

    #[test]
    fn menu_lowers_to_accesskit_menu() {
        assert_eq!(AriaRole::Menu.to_accesskit(), Role::Menu);
    }

    #[test]
    fn menu_item_lowers_to_accesskit_menu_item() {
        assert_eq!(AriaRole::MenuItem.to_accesskit(), Role::MenuItem);
    }

    #[test]
    fn menu_roles_aria_names_match_wai_aria_literals() {
        assert_eq!(AriaRole::MenuBar.aria_name(), "menubar");
        assert_eq!(AriaRole::Menu.aria_name(), "menu");
        assert_eq!(AriaRole::MenuItem.aria_name(), "menuitem");
    }

    // R692 §5.40 — Toolbar role lowering + name.

    #[test]
    fn toolbar_lowers_to_accesskit_toolbar() {
        assert_eq!(AriaRole::Toolbar.to_accesskit(), Role::Toolbar);
    }

    #[test]
    fn toolbar_aria_name_matches_wai_aria_literal() {
        assert_eq!(AriaRole::Toolbar.aria_name(), "toolbar");
    }

    // R695 §5.40 — Tooltip role lowering + name.

    #[test]
    fn r695_tooltip_lowers_to_accesskit_tooltip() {
        assert_eq!(AriaRole::Tooltip.to_accesskit(), Role::Tooltip);
        assert_eq!(AriaRole::Tooltip.aria_name(), "tooltip");
    }
}
