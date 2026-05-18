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
        assert_eq!(AriaRole::Generic.aria_name(), "generic");
    }
}
