//! R867 §5.16 §5.40 — the popup-listbox **option-row** paint substrate: one
//! cell of a combobox / select dropdown — a leading check-mark column (drawn
//! when the option is selected), the option label, a state-layer-tinted fill,
//! and the 2 px accent border that marks the WAI-ARIA active descendant.
//!
//! ## Why this module exists (3rd-consumer lift)
//!
//! `hello-combobox` (R714) was the **1st** option-row painter and built the
//! cell inline (the inline-first rule). `hello-combobox-editable` (R717) was
//! the **2nd** — a byte-identical cell modulo its panel width and the
//! `active` cursor's representation. R867 `hello-property-grid` adds an
//! enum/choice cell whose popup is the **3rd** identical option painter, so
//! the opinionated cell skin lifts here (the Rule-of-Three gate for *style*
//! paint — mechanical wiring lifts at 3, opinionated paint at the 3rd
//! *identical* consumer). Each callsite still owns what genuinely diverges:
//! its own option *labels*, the popup *width* (trigger-wide vs cell-wide),
//! and the panel container the rows sit in.
//!
//! What is shared (this module): the fill-by-[`ListboxItemState`] chooser, the
//! leading check-mark column convention, the label ink, the active-descendant
//! accent border, and the cell flex layout / radius / gap / padding tokens.

use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_core::Scene;

/// The check-mark glyph drawn in the leading column of the selected option.
const CHECK_MARK: &str = "\u{2713}";
/// Option label + check-mark text size (px).
const OPTION_TEXT_PX: u32 = 15;

/// One popup-listbox option's per-cell data — the bits that vary per option
/// (its routing tag, its label, and the three interaction / selection flags).
/// The geometry (width / height) and theme are passed alongside because they
/// are uniform across a panel's rows.
pub struct OptionRow<'a> {
    /// The cell's routing tag — callers pass the composite `"{listbox}#{i}"`
    /// so the `InputRouter` `'#'`-split reaches the `ListBoxExternal` slot.
    pub tag: String,
    /// The visible option label.
    pub label: &'a str,
    /// The option's interaction posture (drives the hover / pressed fill).
    pub state: ListboxItemState,
    /// Whether this is the active descendant (the roving keyboard cursor) —
    /// draws the 2 px accent border.
    pub active: bool,
    /// Whether this option is the committed selection — draws the check mark.
    pub selected: bool,
}

/// Paint one popup-listbox option cell. `width` / `height` are the panel's
/// inner option dimensions (the caller subtracts its own panel padding).
#[must_use]
pub fn view_option(row: &OptionRow<'_>, width: u32, height: u32, theme: &Theme) -> Scene {
    let fill = match row.state {
        ListboxItemState::Hover | ListboxItemState::Pressed => {
            theme.resolve(ColorRole::SurfaceContainerHighest)
        }
        _ if row.active => theme.resolve(ColorRole::SurfaceContainerHigh),
        _ => theme.resolve(ColorRole::SurfaceContainer),
    };
    // Leading check mark column (selected) keeps every label left-aligned.
    let mark = Scene::Text(TextNode::styled(
        if row.selected { CHECK_MARK } else { " " },
        Rect::default(),
        TextStyle::new().with_size_px(OPTION_TEXT_PX).with_fg(theme.resolve(ColorRole::Accent)),
    ));
    let label = Scene::Text(TextNode::styled(
        row.label,
        Rect::default(),
        TextStyle::new().with_size_px(OPTION_TEXT_PX).with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let mut style = BoxStyle::filled(fill).with_corner_radius(4);
    if row.active {
        // 2-px accent — the WAI-ARIA active-descendant hint (no focus ring
        // substrate yet; mirrors hello-listbox's focused-row cue).
        style = style.with_border(Border::new(theme.resolve(ColorRole::Accent), 2));
    }
    Scene::Container(
        ContainerNode::new(vec![mark, label])
            .with_tag(row.tag.clone())
            .with_style(style)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(width, height))
                    .with_gap(8)
                    .with_padding(Rect::new(10, 0, 10, 0)),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option(active: bool, selected: bool) -> Scene {
        view_option(
            &OptionRow {
                tag: "opt#1".to_owned(),
                label: "Additive",
                state: ListboxItemState::Idle,
                active,
                selected,
            },
            180,
            36,
            &Theme::light(),
        )
    }

    #[test]
    fn paints_tagged_cell_with_mark_and_label() {
        let scene = option(false, false);
        let Scene::Container(node) = &scene else { panic!("option is a container") };
        assert_eq!(node.tag.as_deref(), Some("opt#1"), "cell carries its routing tag");
        assert_eq!(node.children.len(), 2, "leading mark column + label");
    }

    #[test]
    fn selected_option_draws_the_check_mark() {
        let Scene::Container(node) = option(false, true) else { panic!("container") };
        let Scene::Text(mark) = &node.children[0] else { panic!("leading mark is text") };
        assert_eq!(mark.content, CHECK_MARK, "selected option shows the check");
        let Scene::Container(node) = option(false, false) else { panic!("container") };
        let Scene::Text(mark) = &node.children[0] else { panic!("leading mark is text") };
        assert_eq!(mark.content, " ", "unselected keeps the blank column");
    }

    #[test]
    fn active_option_gets_the_accent_border() {
        let Scene::Container(active) = option(true, false) else { panic!("container") };
        assert!(active.style.border.is_some(), "active descendant has the accent ring");
        let Scene::Container(idle) = option(false, false) else { panic!("container") };
        assert!(idle.style.border.is_none(), "idle option has no ring");
    }
}
