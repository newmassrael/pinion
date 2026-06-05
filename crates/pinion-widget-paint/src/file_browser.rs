//! R789 §5.15 §5.16 — file-browser **row paint**: the lifted entry-row
//! affordance shared by every own-rendered file UI.
//!
//! ## Why this is lifted now (the 3rd consumer)
//!
//! `hello-file-browser` (R787) and `hello-file-open-dialog` (R788) each
//! grew a byte-identical `build_row`: a directory paints raised with a
//! trailing `/` (navigable), a file zebra-stripes (selectable), the
//! selected file washes [`ColorRole::Accent`], and the container is tagged
//! `"{list_tag}#{index}"` so a click routes to the `DirectoryExternal`.
//! Two copies were the deferred `button_scene` cadence (presentation, lift
//! at the 3rd identical consumer); `hello-file-manager` (R789) is that 3rd
//! consumer, so the row affordance lives here once — the inks + tag + zebra
//! parity are a `divergence-is-a-bug` if hand-rolled a third time (an AT
//! that reads `aria-selected` off a row whose paint disagrees, a click that
//! misses because one binding tagged differently).
//!
//! The opinionated *dimensions* (row width / pitch) stay caller-supplied
//! (each UI sizes its list differently); only the ink + structure are the
//! shared SSOT.

use pinion_core::directory::DirEntry;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::Scene;

/// R789 — paint one file-browser entry row.
///
/// - a **directory** paints on a raised [`ColorRole::SurfaceContainerHigh`]
///   with a trailing `/` (the navigable affordance);
/// - a **file** zebra-stripes ([`ColorRole::SurfaceContainerLow`] /
///   [`ColorRole::SurfaceContainer`] by row parity) — a selectable leaf;
/// - the **selected** file washes [`ColorRole::Accent`] (its text flips to
///   [`ColorRole::OnAccent`]).
///
/// The container is tagged `"{list_tag}#{index}"` so a pointer click /
/// `scene/click` routes to the binding's `DirectoryExternal` (navigate a
/// folder / select a file on the row's `is_dir`). `width` / `pitch` are
/// the caller's list dimensions.
#[must_use]
pub fn file_row(
    list_tag: &str,
    index: usize,
    entry: &DirEntry,
    selected: bool,
    theme: &Theme,
    width: u32,
    pitch: u32,
) -> Scene {
    let (fill, fg) = if selected {
        (theme.resolve(ColorRole::Accent), theme.resolve(ColorRole::OnAccent))
    } else if entry.is_dir {
        (theme.resolve(ColorRole::SurfaceContainerHigh), theme.resolve(ColorRole::OnSurface))
    } else {
        let stripe = if index % 2 == 0 {
            ColorRole::SurfaceContainerLow
        } else {
            ColorRole::SurfaceContainer
        };
        (theme.resolve(stripe), theme.resolve(ColorRole::OnSurface))
    };
    let label = if entry.is_dir { format!("{}/", entry.name) } else { entry.name.clone() };
    let text = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new().with_size_px(15).with_fg(fg),
    ));
    Scene::Container(
        ContainerNode::new(vec![text])
            .with_tag(format!("{list_tag}#{index}"))
            .with_style(BoxStyle::filled(fill))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(width, pitch))
                    .with_padding(Rect::new(14, 0, 14, 0)),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill_of(scene: &Scene) -> pinion_core::style::Color {
        match scene {
            Scene::Container(c) => c.style.fill,
            _ => panic!("file_row is a Container"),
        }
    }

    #[test]
    fn r789_dir_file_selected_paint_distinct_inks() {
        let theme = Theme::light();
        let dir = file_row("fb", 0, &DirEntry::dir("src"), false, &theme, 300, 32);
        let file = file_row("fb", 1, &DirEntry::file("a.rs"), false, &theme, 300, 32);
        let sel = file_row("fb", 2, &DirEntry::file("b.rs"), true, &theme, 300, 32);
        let accent = theme.resolve(ColorRole::Accent);
        assert_ne!(fill_of(&dir), fill_of(&file), "dir row != file row");
        assert_eq!(fill_of(&sel), accent, "selected file row washes Accent");
        assert_ne!(fill_of(&dir), accent, "dir row is not Accent");
    }

    #[test]
    fn r789_row_tag_is_list_tag_indexed() {
        let theme = Theme::light();
        let row = file_row("picker", 3, &DirEntry::file("x"), false, &theme, 200, 30);
        let Scene::Container(c) = &row else { panic!("container") };
        assert_eq!(c.tag.as_deref(), Some("picker#3"), "row tagged {{list_tag}}#{{index}}");
    }

    #[test]
    fn r789_directory_label_carries_trailing_slash() {
        let theme = Theme::light();
        let row = file_row("fb", 0, &DirEntry::dir("assets"), false, &theme, 200, 30);
        let Scene::Container(c) = &row else { panic!("container") };
        let Scene::Text(t) = &c.children[0] else { panic!("text child") };
        assert_eq!(t.content, "assets/", "directory label gets the navigable trailing slash");
    }
}
