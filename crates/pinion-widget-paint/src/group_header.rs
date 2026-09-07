//! R871 §5.50 §5.27 — the **group-header row** paint of a grouped collapsible
//! collection (the R843
//! [`GroupRow::Header`](pinion_core::widgets::group_order::GroupRow)): a
//! disclosure twisty + the group label + a parenthesized detail summary,
//! filling the collection's row pitch and tagged with the group's composite tag
//! (so a pointer click routes to the
//! [`GroupOrderExternal`](pinion_core::widgets::group_order::GroupOrderExternal)
//! collapse coordinator).
//!
//! ## Why this module exists (lift at the 4th consumer)
//!
//! Four shipped grouped collections built this header row by hand —
//! `hello-grouped-list`, `hello-grouped-grid` and `hello-grouped-sort` were
//! **byte-identical** (a chevron + `"  {label}  ({count})"` text in a
//! `SurfaceContainerHigh` flex row, differing only in the width constant they
//! filled), and `hello-grouped-grid-sort` differed by *one thing*: it appended a
//! per-group aggregate (`"({count}, {total} B)"`, the R854 carry) to the same
//! parenthesized detail. That is three mechanical copies of one paint decision
//! plus a fourth that varies only the *content* inside the parens — the
//! Rule-of-Three lift threshold the R758 self-grep mandate names, and the same
//! inline-first → lift-at-the-Nth-consumer path [`crate::chip`] took. The
//! header skin (the twisty, the `"{label}  ({detail})"` format, the
//! `SurfaceContainerHigh` fill, the centred flex row, the 10 px horizontal
//! padding) lifts here; the consumer owns only what genuinely differs — the
//! composite `tag`, the `label`, the parenthesized `detail` string (a bare
//! count, or count + aggregate), and the row's `width` / `height`.
//!
//! ★ R2057 — the twisty is the disclosure / tree-branch mark
//! ([`crate::indicator::Indicator::Disclosure`]), so a grouped header reads
//! with the same affordance as a [`crate::disclosure`] section and a
//! `tree_view` branch. It used to be the characters `U+25B6` / `U+25BC`, which
//! the one face this tree renders through does not carry — the same mark, but
//! as a drawn path, which is a shape rather than a promise the face must keep.

use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, Theme};

/// Header label font size — 14 px (the grouped-collection header value the four
/// consumers shared).
const HEADER_FONT_PX: u32 = 14;

/// Horizontal padding inside a group-header row — 10 px each side.
const HEADER_PAD_X: u32 = 10;

/// R871 §5.50 §5.27 — build one group-header row of a grouped collapsible
/// collection.
///
/// Renders the twisty (drawn, resolving from `collapsed`) beside
/// `"{label}  ({detail})"` as an [`OnSurface`](ColorRole::OnSurface)
/// `HEADER_FONT_PX`
/// label in a [`SurfaceContainerHigh`](ColorRole::SurfaceContainerHigh) flex row
/// of `(width, height)`, padded `HEADER_PAD_X` each side and tagged `tag` (the
/// group's composite tag, e.g. `"{group_prefix}#{group}"`, so a click routes to
/// the collapse coordinator). An empty `detail` omits the parenthesized suffix
/// (just the label).
#[must_use]
pub fn group_header_row(
    tag: String,
    label: &str,
    detail: &str,
    collapsed: bool,
    theme: &Theme,
    width: u32,
    height: u32,
) -> Scene {
    // ★★★★★ R2057 — the twisty leaves the SENTENCE and becomes a mark beside it.
    //
    // It was concatenated into the label — `format!("{chevron}  {label}")` —
    // with `U+25B6` / `U+25BC`, neither of which the one face this tree renders
    // through carries: every group header read as a `.notdef` box, two spaces,
    // then its name. Putting the mark in the words also put it in the header's
    // accessible NAME, so a reader who cannot see the drawing was told a
    // character instead of a state.
    //
    // Drawn beside the text, the mark is decorative and says so, and the words
    // are only words.
    let twisty = crate::indicator::inline(
        crate::indicator::Indicator::Disclosure { open: !collapsed },
        HEADER_FONT_PX,
        theme.resolve(ColorRole::OnSurface),
        "the twisty for this group; the header beside it names the group",
    );
    let text = if detail.is_empty() {
        label.to_owned()
    } else {
        format!("{label}  ({detail})")
    };
    let label_node = Scene::Text(TextNode::styled(
        text,
        Rect::default(),
        TextStyle::new()
            .with_size_px(HEADER_FONT_PX)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![twisty, label_node])
            .with_tag(tag)
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerHigh),
            ))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(width, height))
                    .with_padding(Rect::new(HEADER_PAD_X, 0, HEADER_PAD_X, 0)),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::group_header_row;
    use pinion_core::scene::Scene;
    use pinion_core::theme::Theme;

    /// Pull the header label text out of the built row.
    ///
    /// ★ R2057 — the LAST child, because the twisty is a drawn mark in front of
    /// it now rather than two characters inside it.
    fn header_text(scene: &Scene) -> &str {
        let Scene::Container(root) = scene else {
            panic!("group header is a Container")
        };
        let Some(Scene::Text(t)) = root.children.last() else {
            panic!("header has a Text child")
        };
        &t.content
    }

    /// ★★★★★ R2057 — the twisty is a MARK, and the words are only words.
    ///
    /// These three compared the label against a string that began with
    /// `U+25BC` / `U+25B6`. Both characters are absent from the one face this
    /// tree renders through, so the assertions were true while every group
    /// header on screen began with a `.notdef` box — and, because the mark was
    /// inside the sentence, a reader who cannot see the drawing was read a
    /// character where a state belonged.
    #[test]
    fn r2057_the_twisty_is_a_mark_beside_the_words_not_inside_them() {
        let theme = Theme::light();
        for (collapsed, open) in [(false, true), (true, false)] {
            let row = group_header_row(
                "pg#0".to_string(),
                "Transform",
                "2",
                collapsed,
                &theme,
                400,
                28,
            );
            assert_eq!(
                crate::indicator::marks_in(&row),
                vec![crate::indicator::Indicator::Disclosure { open }],
                "a {} group points at where its rows are",
                if open { "shown" } else { "folded" },
            );
            assert_eq!(
                header_text(&row),
                "Transform  (2)",
                "and the words carry no mark at all",
            );
        }
    }

    #[test]
    fn empty_detail_omits_parentheses() {
        let theme = Theme::light();
        let row = group_header_row("pg#2".to_string(), "Physics", "", false, &theme, 400, 28);
        assert_eq!(header_text(&row), "Physics");
    }

    #[test]
    fn header_row_carries_its_composite_tag() {
        let theme = Theme::light();
        let row = group_header_row("pg#3".to_string(), "Stats", "1", false, &theme, 400, 28);
        assert_eq!(row.tag(), Some("pg#3"));
    }
}
