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
}
