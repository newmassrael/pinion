//! R1721 §5.40 — **the accessibility half of a chip row, derived from the one
//! word the row declared.**
//!
//! [`Choice`] says how many chips may be on. This module is the only place that
//! turns that into roles, and it does so by *choosing which of the three shapes
//! this crate already builds* — there is no fourth topology here:
//!
//! | rule | builder | group role | member role | the on-ness is |
//! |---|---|---|---|---|
//! | [`Any`](Choice::Any) | [`toggle_button_group_nodes`] | `group` | `button` | `aria-pressed` |
//! | [`AtMostOne`](Choice::AtMostOne) | [`listbox_option_nodes`] | `listbox` | `option` | `aria-selected` |
//! | [`ExactlyOne`](Choice::ExactlyOne) | [`radiogroup_radio_nodes`] | `radiogroup` | `radio` | `aria-checked` |
//!
//! That the three families were already here is the finding, not a convenience:
//! the shapes existed and nothing related them to the rule a row obeys, so
//! three screens each picked one by hand and two picked wrongly. Measured on
//! 2026-08-19 by driving them — a set where only one member can be on was
//! announced as three independent toggle buttons on one screen and as five on
//! another.
//!
//! ## What a caller cannot do
//!
//! Pass a role. [`chip_group_nodes`] takes the row and where focus is, and
//! nothing else; the `match` on [`Choice`] has no wildcard arm, so a rule added
//! later fails to compile here rather than falling through to whichever shape
//! was written first. This is the seam R1720 built for refusals, on the axis
//! R1721 is about.
//!
//! ## The floor, measured rather than read
//!
//! A probe built against the mature toolkit at 6.11.1 and run offscreen: an
//! exclusive set of checkable buttons and an independent one report the **same**
//! member role — the push-button one, in both — and the object that carries the
//! rule has no accessibility node at all, so nothing stands for the set. Every
//! row of the table above is a compile error there.

use crate::listbox::{ListOption, listbox_option_nodes};
use crate::node::AccessNode;
use crate::radiogroup::{RadioCell, radiogroup_radio_nodes};
use crate::role::AriaRole;
use crate::toggle_group::{ToggleSegment, toggle_button_group_nodes};
use pinion_core::widgets::chip_group::{Chip, ChipGroup, ChipPosture, Choice};
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::toggle::ToggleState;

/// The role the row itself carries, derived from its rule.
///
/// Published because a screen that declares its own accessibility census — the
/// analysis screens each carry a table of tag family to expected role — must
/// read the role from the same place the tree builds it, or the census checks a
/// role somebody typed against a role somebody else typed. The test
/// `r1721_the_roles_are_the_ones_the_rule_names` ties this to what
/// [`chip_group_nodes`] actually emits, so the two cannot drift.
#[must_use]
pub const fn group_role(choice: Choice) -> AriaRole {
    match choice {
        Choice::Any => AriaRole::Group,
        Choice::AtMostOne => AriaRole::Listbox,
        Choice::ExactlyOne => AriaRole::RadioGroup,
    }
}

/// The role each chip carries, derived from its row's rule. See [`group_role`].
#[must_use]
pub const fn member_role(choice: Choice) -> AriaRole {
    match choice {
        Choice::Any => AriaRole::Button,
        Choice::AtMostOne => AriaRole::ListBoxOption,
        Choice::ExactlyOne => AriaRole::RadioButton,
    }
}

/// Where the keyboard is, as far as a chip row is concerned.
///
/// Derived from the one tag the shell reports as focused rather than declared,
/// because a row that could be told it has focus while the shell says otherwise
/// is a row that can announce a focus ring nobody can see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipFocus {
    /// Focus is somewhere else entirely.
    Elsewhere,
    /// Focus is on the row itself — the composite case, where the cursor's chip
    /// becomes the active descendant.
    OnTheRow,
    /// Focus is on the chip at this index — the independent case, where each
    /// chip is its own Tab stop.
    OnChip(usize),
}

impl ChipFocus {
    /// Resolve the shell's focused tag against `row`.
    #[must_use]
    pub fn of(row: &ChipGroup, focused: Option<&str>) -> Self {
        let Some(tag) = focused else {
            return Self::Elsewhere;
        };
        if tag == row.tag() {
            return Self::OnTheRow;
        }
        row.chips()
            .iter()
            .position(|chip| chip.tag == tag)
            .map_or(Self::Elsewhere, Self::OnChip)
    }

    /// Whether the chip at `index` is the one an assistive technology should
    /// report as focused.
    ///
    /// For a composite the row owns the Tab stop and the chip under the cursor
    /// is the active descendant, so "focused" moves to the seat; for a row of
    /// independent chips it is the chip the shell named.
    fn lands_on(self, index: usize, seat: Option<usize>) -> bool {
        match self {
            Self::Elsewhere => false,
            Self::OnTheRow => seat == Some(index),
            Self::OnChip(focused) => focused == index,
        }
    }
}

/// Convert a chip's posture into the state enum a builder wants.
///
/// Three near-identical conversions rather than one generic, because the three
/// builders take three different widget-state types and a `Locked` chip must
/// reach all three as `Disabled`. Total by construction: every posture maps, and
/// a posture added later fails to compile in all three.
const fn as_toggle(posture: ChipPosture) -> ToggleState {
    match posture {
        ChipPosture::Idle => ToggleState::Idle,
        ChipPosture::Hover => ToggleState::Hover,
        ChipPosture::Pressed => ToggleState::Pressed,
        ChipPosture::Locked => ToggleState::Disabled,
    }
}

const fn as_option(posture: ChipPosture) -> ListboxItemState {
    match posture {
        ChipPosture::Idle => ListboxItemState::Idle,
        ChipPosture::Hover => ListboxItemState::Hover,
        ChipPosture::Pressed => ListboxItemState::Pressed,
        ChipPosture::Locked => ListboxItemState::Disabled,
    }
}

const fn as_radio(posture: ChipPosture) -> RadioState {
    match posture {
        ChipPosture::Idle => RadioState::Idle,
        ChipPosture::Hover => RadioState::Hover,
        ChipPosture::Pressed => RadioState::Pressed,
        ChipPosture::Locked => RadioState::Disabled,
    }
}

/// Build the row's accessibility subtree: the group node first, then one node
/// per chip, in the flat-list convention the tree builder resolves.
///
/// `focused` is the tag the shell reports as focused, or `None`. Everything
/// else — the group role, the member role, which attribute carries on-ness, the
/// cursor the group publishes, and where focus lands — is derived from the row.
#[must_use]
pub fn chip_group_nodes(row: &ChipGroup, focused: Option<&str>) -> Vec<AccessNode> {
    let focus = ChipFocus::of(row, focused);
    let seat = row.seat();
    let mut nodes = match row.choice() {
        Choice::Any => {
            let segments: Vec<ToggleSegment<'_>> = row
                .chips()
                .iter()
                .map(|chip: &Chip| ToggleSegment {
                    tag: &chip.tag,
                    label: &chip.label,
                    state: as_toggle(chip.posture),
                    on: chip.on,
                })
                .collect();
            let named = match focus {
                ChipFocus::OnChip(index) => row.chips().get(index).map(|chip| chip.tag.as_str()),
                ChipFocus::Elsewhere | ChipFocus::OnTheRow => None,
            };
            toggle_button_group_nodes(row.tag(), row.name(), &segments, named)
        }
        Choice::AtMostOne => {
            let options: Vec<ListOption<'_>> = row
                .chips()
                .iter()
                .enumerate()
                .map(|(index, chip)| ListOption {
                    tag: &chip.tag,
                    label: Some(&chip.label),
                    state: as_option(chip.posture),
                    selected: chip.on,
                    focused: focus.lands_on(index, seat),
                })
                .collect();
            // Never multi-selectable: the row said at most one, and a container
            // that advertised otherwise would contradict its own rule.
            listbox_option_nodes(row.tag(), row.name(), false, &options)
        }
        Choice::ExactlyOne => {
            let cells: Vec<RadioCell<'_>> = row
                .chips()
                .iter()
                .enumerate()
                .map(|(index, chip)| RadioCell {
                    tag: &chip.tag,
                    label: Some(&chip.label),
                    state: as_radio(chip.posture),
                    selected: chip.on,
                    focused: focus.lands_on(index, seat),
                })
                .collect();
            radiogroup_radio_nodes(row.tag(), row.name(), &cells)
        }
    };
    // ★★★★★ The composite half. A row that is one Tab stop has to publish the
    // cursor inside it, or the stop is a room with a door and no floor (R1698);
    // a row of independent chips has no cursor to publish, and saying it did
    // would promise arrows that move nothing.
    if let Some(cursor) = row.cursor()
        && let Some(group) = nodes.first_mut()
    {
        *group = group.clone().with_navigation(&cursor);
        if focus == ChipFocus::OnTheRow {
            *group = group.clone().with_focused(true);
        }
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::widgets::roving::Activation;

    fn row(choice: Choice) -> ChipGroup {
        ChipGroup::new(
            "row",
            "Saved filters",
            vec![
                Chip::new("row.0", "units only", false),
                Chip::new("row.1", "shared memory", true),
                Chip::new("row.2", "reassembly failed", false),
            ],
            choice,
        )
    }

    /// ★★★★★ The headline: changing only the rule changes what a screen reader
    /// is told, in every one of the four ways the table names. Two rules that
    /// produced the same tree would be a declaration nobody could act on.
    #[test]
    fn r1721_each_rule_gives_the_row_a_different_shape() {
        let shapes: Vec<_> = Choice::ALL
            .into_iter()
            .map(|choice| {
                let nodes = chip_group_nodes(&row(choice), None);
                assert_eq!(nodes.len(), 4, "{}: one group + three chips", choice.wire());
                (
                    choice.wire(),
                    nodes[0].role,
                    nodes[1].role,
                    // Which attribute carries on-ness: `checked` for the two
                    // that use it, `selected` for the listbox option.
                    nodes[2].state.checked,
                    nodes[2].selected,
                )
            })
            .collect();
        for (i, left) in shapes.iter().enumerate() {
            for right in &shapes[i + 1..] {
                assert_ne!(left, right, "two rules build the same tree");
            }
        }
    }

    /// The role table in the module documentation, asserted rather than written
    /// down: a doc comment that drifted from the code would be the R1720.1 class.
    ///
    /// ★★★★ It also ties [`group_role`] / [`member_role`] — which the screens'
    /// own censuses read — to what the tree **actually** emits. Those two
    /// functions are a second spelling of the three builders' hardcoded roles,
    /// and a second spelling with nothing binding it to the first is exactly how
    /// a census ends up checking one person's typing against another's.
    #[test]
    fn r1721_the_roles_are_the_ones_the_rule_names() {
        let want = [
            (Choice::Any, AriaRole::Group, AriaRole::Button),
            (
                Choice::AtMostOne,
                AriaRole::Listbox,
                AriaRole::ListBoxOption,
            ),
            (
                Choice::ExactlyOne,
                AriaRole::RadioGroup,
                AriaRole::RadioButton,
            ),
        ];
        for (choice, group, member) in want {
            let nodes = chip_group_nodes(&row(choice), None);
            assert_eq!(nodes[0].role, group, "{}", choice.wire());
            assert_eq!(group_role(choice), group, "{}", choice.wire());
            for node in &nodes[1..] {
                assert_eq!(node.role, member, "{}", choice.wire());
            }
            assert_eq!(member_role(choice), member, "{}", choice.wire());
        }
    }

    /// ★★★★ A one-stop row publishes the cursor inside it, with the keys and
    /// the ends the rule derived; a row of independent chips publishes none.
    #[test]
    fn r1721_a_composite_row_publishes_its_cursor_and_the_others_do_not() {
        for choice in [Choice::AtMostOne, Choice::ExactlyOne] {
            let nodes = chip_group_nodes(&row(choice), None);
            let nav = nodes[0]
                .navigation
                .as_ref()
                .unwrap_or_else(|| panic!("{}: the row publishes its cursor", choice.wire()));
            assert_eq!(nav.members().len(), 3, "{}", choice.wire());
            assert_eq!(
                nav.cursor_tag(),
                Some("row.1"),
                "{}: the cursor rests on the chip that is on",
                choice.wire()
            );
            assert_eq!(
                nav.spec().activation,
                if choice == Choice::ExactlyOne {
                    Activation::Follows
                } else {
                    Activation::Explicit
                },
                "{}",
                choice.wire()
            );
        }
        assert!(
            chip_group_nodes(&row(Choice::Any), None)[0]
                .navigation
                .is_none(),
            "a row of independent switches promises no arrows"
        );
    }

    /// Focus on the row lands on the seat; focus on a chip lands on that chip;
    /// and a tag from somewhere else lands nowhere.
    #[test]
    fn r1721_focus_lands_where_the_rule_puts_it() {
        let composite = chip_group_nodes(&row(Choice::AtMostOne), Some("row"));
        assert!(composite[0].state.focused, "the row owns the stop");
        assert!(
            composite[2].state.focused,
            "and the chip under the cursor is the active descendant"
        );
        let independent = chip_group_nodes(&row(Choice::Any), Some("row.2"));
        assert!(!independent[0].state.focused, "the group is not a stop");
        assert!(independent[3].state.focused, "the chip the shell named is");
        for choice in Choice::ALL {
            let nodes = chip_group_nodes(&row(choice), Some("somewhere.else"));
            assert!(
                nodes.iter().all(|node| !node.state.focused),
                "{}: a tag from another widget focuses nothing here",
                choice.wire()
            );
        }
    }

    /// A locked chip reaches every one of the three builders as disabled, and is
    /// still announced — the whole point of letting the cursor rest on it.
    #[test]
    fn r1721_a_locked_chip_is_announced_as_locked_under_every_rule() {
        for choice in Choice::ALL {
            let mut locked = row(choice);
            let chips = locked
                .chips()
                .iter()
                .map(|chip| {
                    if chip.tag == "row.0" {
                        chip.clone().with_posture(ChipPosture::Locked)
                    } else {
                        chip.clone()
                    }
                })
                .collect();
            locked = ChipGroup::new(locked.tag(), locked.name(), chips, choice);
            let nodes = chip_group_nodes(&locked, None);
            assert!(
                nodes[1].state.disabled,
                "{}: the locked chip says so",
                choice.wire()
            );
            assert_eq!(
                nodes[1].name.as_deref(),
                Some("units only"),
                "{}: and is still named",
                choice.wire()
            );
        }
    }

    /// An empty row still has a group node, because "the filter matched nothing"
    /// is a state a reader is told about rather than a container announced over
    /// nothing.
    #[test]
    fn r1721_an_empty_row_is_still_a_node() {
        for choice in Choice::ALL {
            let empty = ChipGroup::new("row", "Saved filters", Vec::new(), choice);
            let nodes = chip_group_nodes(&empty, None);
            assert_eq!(nodes.len(), 1, "{}", choice.wire());
            assert_eq!(nodes[0].name.as_deref(), Some("Saved filters"));
            assert!(
                nodes[0].navigation.is_none(),
                "{}: an empty row seats no cursor",
                choice.wire()
            );
        }
    }
}
