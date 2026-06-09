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
//! header skin (twisty glyph, the `"  {label}  ({detail})"` format, the
//! `SurfaceContainerHigh` fill, the centred flex row, the 10 px horizontal
//! padding) lifts here; the consumer owns only what genuinely differs — the
//! composite `tag`, the `label`, the parenthesized `detail` string (a bare
//! count, or count + aggregate), and the row's `width` / `height`.
//!
//! The twisty glyphs are the disclosure / tree-branch SSOT (`U+25B6` collapsed /
//! `U+25BC` expanded), so a grouped header reads with the same affordance as a
//! [`crate::disclosure`] section and a `tree_view` branch.

use crate::glyph::{DISCLOSURE_COLLAPSED as GLYPH_COLLAPSED, DISCLOSURE_EXPANDED as GLYPH_EXPANDED};
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
/// Renders `"{twisty}  {label}  ({detail})"` (the twisty resolving from
/// `collapsed`) as an [`OnSurface`](ColorRole::OnSurface) [`HEADER_FONT_PX`]
/// label in a [`SurfaceContainerHigh`](ColorRole::SurfaceContainerHigh) flex row
/// of `(width, height)`, padded [`HEADER_PAD_X`] each side and tagged `tag` (the
/// group's composite tag, e.g. `"{group_prefix}#{group}"`, so a click routes to
/// the collapse coordinator). An empty `detail` omits the parenthesized suffix
/// (just `"{twisty}  {label}"`).
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
    let chevron = if collapsed { GLYPH_COLLAPSED } else { GLYPH_EXPANDED };
    let text = if detail.is_empty() {
        format!("{chevron}  {label}")
    } else {
        format!("{chevron}  {label}  ({detail})")
    };
    let label_node = Scene::Text(TextNode::styled(
        text,
        Rect::default(),
        TextStyle::new().with_size_px(HEADER_FONT_PX).with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label_node])
            .with_tag(tag)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh)))
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

    /// Pull the header label text out of the built row (the single Text child).
    fn header_text(scene: &Scene) -> &str {
        let Scene::Container(root) = scene else { panic!("group header is a Container") };
        let Scene::Text(t) = &root.children[0] else { panic!("header has a Text child") };
        &t.content
    }

    #[test]
    fn expanded_header_shows_down_twisty_label_and_detail() {
        let theme = Theme::light();
        let row = group_header_row("pg#0".to_string(), "Transform", "2", false, &theme, 400, 28);
        assert_eq!(header_text(&row), "\u{25BC}  Transform  (2)");
    }

    #[test]
    fn collapsed_header_shows_right_twisty() {
        let theme = Theme::light();
        let row = group_header_row("pg#1".to_string(), "Appearance", "4", true, &theme, 400, 28);
        assert_eq!(header_text(&row), "\u{25B6}  Appearance  (4)");
    }

    #[test]
    fn empty_detail_omits_parentheses() {
        let theme = Theme::light();
        let row = group_header_row("pg#2".to_string(), "Physics", "", false, &theme, 400, 28);
        assert_eq!(header_text(&row), "\u{25BC}  Physics");
    }

    #[test]
    fn header_row_carries_its_composite_tag() {
        let theme = Theme::light();
        let row = group_header_row("pg#3".to_string(), "Stats", "1", false, &theme, 400, 28);
        assert_eq!(row.tag(), Some("pg#3"));
    }
}
