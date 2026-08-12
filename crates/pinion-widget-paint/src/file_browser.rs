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

use std::rc::Rc;

use pinion_core::Scene;
use pinion_core::directory::DirEntry;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::file_browser::{DirectoryState, FileDropTarget};
use pinion_core::widgets::scroll::ScrollState;

use crate::virtual_list::view_virtual_list;

/// R794 — the transient highlight of a file row. Mutually exclusive in
/// paint: a live drop target washes *over* a selection during a drag, so
/// the caller resolves the one that wins (drop target first).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RowHighlight {
    /// Resting ink (directory raised / file zebra-striped).
    #[default]
    None,
    /// The picked leaf — a solid [`ColorRole::Accent`] wash.
    Selected,
    /// The live drag drop target — an [`ColorRole::Accent`] outline (the
    /// "drop here" affordance), distinct from the solid selection fill.
    DropTarget,
}

/// R794 — the drop-target outline width in px (the [`RowHighlight::DropTarget`]
/// affordance). Thick enough to read as a deliberate "drop here" frame and to
/// survive a pixel sample along the row edge.
const DROP_TARGET_BORDER_PX: u32 = 3;

/// R789 — paint one file-browser entry row.
///
/// - a **directory** paints on a raised [`ColorRole::SurfaceContainerHigh`]
///   with a trailing `/` (the navigable affordance);
/// - a **file** zebra-stripes ([`ColorRole::SurfaceContainerLow`] /
///   [`ColorRole::SurfaceContainer`] by row parity) — a selectable leaf;
/// - a [`RowHighlight::Selected`] file washes [`ColorRole::Accent`] (its text
///   flips to [`ColorRole::OnAccent`]);
/// - a [`RowHighlight::DropTarget`] row (R794) keeps its resting fill but gains
///   an [`ColorRole::Accent`] outline + Accent label — the OS / M3 "drop here"
///   affordance (an *outline*, not a second fill, because this single-accent
///   palette has no secondary-container tone, the R753 carry).
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
    highlight: RowHighlight,
    theme: &Theme,
    width: u32,
    pitch: u32,
) -> Scene {
    let (fill, fg) = if highlight == RowHighlight::Selected {
        (
            theme.resolve(ColorRole::Accent),
            theme.resolve(ColorRole::OnAccent),
        )
    } else if entry.is_dir {
        (
            theme.resolve(ColorRole::SurfaceContainerHigh),
            theme.resolve(ColorRole::OnSurface),
        )
    } else {
        let stripe = if index % 2 == 0 {
            ColorRole::SurfaceContainerLow
        } else {
            ColorRole::SurfaceContainer
        };
        (theme.resolve(stripe), theme.resolve(ColorRole::OnSurface))
    };
    // R794 — the drop target outlines its resting fill (Accent border + label).
    let (fg, box_style) = if highlight == RowHighlight::DropTarget {
        let accent = theme.resolve(ColorRole::Accent);
        (
            accent,
            BoxStyle::filled(fill).with_border(Border::new(accent, DROP_TARGET_BORDER_PX)),
        )
    } else {
        (fg, BoxStyle::filled(fill))
    };
    let label = if entry.is_dir {
        format!("{}/", entry.name)
    } else {
        entry.name.clone()
    };
    let text = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new().with_size_px(15).with_fg(fg),
    ));
    Scene::Container(
        ContainerNode::new(vec![text])
            .with_tag(format!("{list_tag}#{index}"))
            .with_style(box_style)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(width, pitch))
                    .with_padding(Rect::new(14, 0, 14, 0)),
            ),
    )
}

/// R789.1 — the list dimensions a [`file_browser_pane`] is laid out at.
/// Bundled (the `view_virtual_table` arg-reduction convention) so the pane
/// fn stays within the 7-argument budget; each file UI sizes its list
/// differently, so these stay caller-supplied.
#[derive(Clone, Copy, Debug)]
pub struct FileBrowserMetrics {
    /// Row + list width in logical px.
    pub list_width: u32,
    /// Visible list viewport height in logical px.
    pub list_height: u32,
    /// Per-row vertical slot in logical px.
    pub row_pitch: u32,
    /// Rows rendered beyond the viewport each side (windowing overscan).
    pub overscan: usize,
    /// (R1020 §5.39) Keyboard focus stop. When `true`, [`file_browser_pane`]
    /// marks the `dir_tag` list-region Container `.with_focusable(true)` so
    /// the scene-derived §5.39 enumeration collects the list as a Tab stop
    /// (the file-manager's standalone Files pane). Default `false` (opt-in):
    /// a dialog's file list is focusable only inside its modal trap, not a
    /// base Tab stop, and a display-only browser exposes no stops — they
    /// leave this `false`. Mirrors
    /// [`ButtonStyle::focusable`](crate::button::ButtonStyle::focusable).
    pub focusable: bool,
}

impl FileBrowserMetrics {
    /// (R1020 §5.39) Mark the file list a keyboard focus stop (default
    /// `false`). See [`Self::focusable`].
    #[must_use]
    pub const fn with_focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }
}

/// R791 — an in-place editing-row override for [`file_browser_pane`]: the
/// visual row `index` whose [`file_row`] is replaced by `build(width,
/// pitch)`, the file manager's inline-rename `TextField`. `None` (the
/// common case) renders every row as a plain [`file_row`].
///
/// `build` is a closure rather than a pre-built [`Scene`] because the pane
/// only renders the editing row when it falls inside the scroll window,
/// and a [`Scene`] is not `Clone` (it may carry an `ExternalNode`); the
/// closure is invoked at most once, when the window reaches `index`.
#[derive(Clone, Copy)]
pub struct EditingRow<'a> {
    /// The visual row index currently in edit mode.
    pub index: usize,
    /// Builds the edit-field scene for that row, given `(width, pitch)`.
    pub build: &'a dyn Fn(u32, u32) -> Scene,
}

/// R789.1 — the **file-browser pane**: a breadcrumb bar (the clickable
/// `../` parent affordance + the current path) above the virtualized entry
/// list (one [`file_row`] per visible entry). The recognizable own-rendered
/// file-UI body every browser / picker / manager shares.
///
/// This is the lifted SSOT the R787-R789 examples each hand-rolled: the
/// `../` affordance tagged `"{dir_tag}#up"`, the cwd label, and the
/// `view_virtual_list` over [`file_row`] were byte-identical across
/// `hello-file-browser`, `hello-file-open-dialog`, and
/// `hello-file-manager` (a 3rd-consumer mechanical duplication —
/// `divergence-is-a-bug`: a breadcrumb that tagged `#up` differently, or a
/// row windowed on a different pitch, would silently break click routing).
/// The caller wraps the pane with its own chrome (a dialog's actions, a
/// manager's toolbar, a footer status line).
///
/// Reads the shared [`DirectoryState`] (cwd / entries / selection), so a
/// navigate / select / create / delete repaints. The [`scroll`](ScrollState)
/// drives the windowing.
#[must_use]
pub fn file_browser_pane(
    dir_tag: &str,
    dir: &DirectoryState,
    scroll: &Rc<ScrollState>,
    theme: &Theme,
    metrics: FileBrowserMetrics,
    editing: Option<EditingRow<'_>>,
) -> Scene {
    let cwd = dir.cwd();
    let entries = dir.entries();
    let sel_idx = dir.selected_index();
    // R794 — the live drag drop target (subscribes: a `drag_to` repaints).
    let drop = dir.drop_target();
    let FileBrowserMetrics {
        list_width,
        list_height,
        row_pitch,
        overscan,
        focusable,
    } = metrics;

    // Breadcrumb: the clickable `../` parent affordance + the cwd path. R794 —
    // the `../` affordance is itself a drop target (drag an entry onto it to
    // move it up to the parent), outlined Accent while it is the live target.
    let up_drop = drop == Some(FileDropTarget::Up);
    let up_accent = theme.resolve(ColorRole::Accent);
    let up_fg = if up_drop {
        up_accent
    } else {
        theme.resolve(ColorRole::OnSurface)
    };
    let up_style = {
        let s = BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh));
        if up_drop {
            s.with_border(Border::new(up_accent, DROP_TARGET_BORDER_PX))
        } else {
            s
        }
    };
    let up = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            "../".to_string(),
            Rect::default(),
            TextStyle::new().with_size_px(15).with_fg(up_fg),
        ))])
        .with_tag(format!("{dir_tag}#up"))
        .with_style(up_style)
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(48, row_pitch))
                .with_padding(Rect::new(10, 0, 10, 0)),
        ),
    );
    let path_label = Scene::Text(TextNode::styled(
        cwd,
        Rect::default(),
        TextStyle::new()
            .with_size_px(15)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    let breadcrumb = Scene::Container(
        ContainerNode::new(vec![up, path_label]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_gap(12),
        ),
    );

    let list = view_virtual_list(
        scroll,
        Rect::new(0, 0, list_width, list_height),
        entries.len(),
        row_pitch,
        overscan,
        |index| {
            // R791 — the editing row renders the caller's inline field in
            // place of its `file_row` (the file manager's rename in place).
            if let Some(e) = &editing {
                if e.index == index {
                    return (e.build)(list_width, row_pitch);
                }
            }
            // R794 — a live drop target outlines its row; otherwise the picked
            // selection washes it. Drop target wins (transient over resting).
            let highlight = if drop == Some(FileDropTarget::Row(index)) {
                RowHighlight::DropTarget
            } else if sel_idx == Some(index) {
                RowHighlight::Selected
            } else {
                RowHighlight::None
            };
            file_row(
                dir_tag,
                index,
                &entries[index],
                highlight,
                theme,
                list_width,
                row_pitch,
            )
        },
    );

    // R792 — tag the list region with `dir_tag` so the file list is a
    // focusable single tab stop (the `focus/set` target). Routing of the
    // `{dir_tag}#{index}` row clicks is registry-based and unaffected; this
    // Container is the focus / `aria-activedescendant` anchor, the
    // `hello-virtual-nav` list-container precedent. The windowed rows are the
    // composite children whose active member the shell rings.
    let list_region = Scene::Container(
        ContainerNode::new(vec![list])
            .with_tag(dir_tag.to_string())
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(list_width, list_height))
                    .with_focusable(focusable),
            ),
    );

    Scene::Container(
        ContainerNode::new(vec![breadcrumb, list_region])
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column).with_gap(8)),
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

    fn border_of(scene: &Scene) -> Option<pinion_core::style::Border> {
        match scene {
            Scene::Container(c) => c.style.border,
            _ => panic!("file_row is a Container"),
        }
    }

    #[test]
    fn r789_dir_file_selected_paint_distinct_inks() {
        let theme = Theme::light();
        let dir = file_row(
            "fb",
            0,
            &DirEntry::dir("src"),
            RowHighlight::None,
            &theme,
            300,
            32,
        );
        let file = file_row(
            "fb",
            1,
            &DirEntry::file("a.rs"),
            RowHighlight::None,
            &theme,
            300,
            32,
        );
        let sel = file_row(
            "fb",
            2,
            &DirEntry::file("b.rs"),
            RowHighlight::Selected,
            &theme,
            300,
            32,
        );
        let accent = theme.resolve(ColorRole::Accent);
        assert_ne!(fill_of(&dir), fill_of(&file), "dir row != file row");
        assert_eq!(fill_of(&sel), accent, "selected file row washes Accent");
        assert_ne!(fill_of(&dir), accent, "dir row is not Accent");
    }

    #[test]
    fn r794_drop_target_outlines_distinct_from_selection() {
        let theme = Theme::light();
        let accent = theme.resolve(ColorRole::Accent);
        // A drop target keeps its resting (directory) fill but gains an Accent
        // outline — distinct from the solid-Accent selection fill.
        let drop = file_row(
            "fb",
            0,
            &DirEntry::dir("src"),
            RowHighlight::DropTarget,
            &theme,
            300,
            32,
        );
        let dir = file_row(
            "fb",
            0,
            &DirEntry::dir("src"),
            RowHighlight::None,
            &theme,
            300,
            32,
        );
        let sel = file_row(
            "fb",
            2,
            &DirEntry::file("b.rs"),
            RowHighlight::Selected,
            &theme,
            300,
            32,
        );
        assert_eq!(
            fill_of(&drop),
            fill_of(&dir),
            "drop target keeps the resting dir fill"
        );
        assert_eq!(
            border_of(&drop).map(|b| b.color),
            Some(accent),
            "drop target gains an Accent outline"
        );
        assert_eq!(
            border_of(&sel),
            None,
            "the selection is a fill, not an outline"
        );
        assert_ne!(
            fill_of(&drop),
            fill_of(&sel),
            "outline (drop) != fill (selection)"
        );
    }

    #[test]
    fn r789_row_tag_is_list_tag_indexed() {
        let theme = Theme::light();
        let row = file_row(
            "picker",
            3,
            &DirEntry::file("x"),
            RowHighlight::None,
            &theme,
            200,
            30,
        );
        let Scene::Container(c) = &row else {
            panic!("container")
        };
        assert_eq!(
            c.tag.as_deref(),
            Some("picker#3"),
            "row tagged {{list_tag}}#{{index}}"
        );
    }

    #[test]
    fn r789_directory_label_carries_trailing_slash() {
        let theme = Theme::light();
        let row = file_row(
            "fb",
            0,
            &DirEntry::dir("assets"),
            RowHighlight::None,
            &theme,
            200,
            30,
        );
        let Scene::Container(c) = &row else {
            panic!("container")
        };
        let Scene::Text(t) = &c.children[0] else {
            panic!("text child")
        };
        assert_eq!(
            t.content, "assets/",
            "directory label gets the navigable trailing slash"
        );
    }

    #[test]
    fn r789_1_pane_carries_breadcrumb_affordance_and_windowed_rows() {
        use pinion_core::directory::InMemoryDirectory;
        use pinion_core::reactive::Owner;
        use pinion_core::widgets::scroll::use_scroll_state;

        Owner::new().run(|| {
            let d = InMemoryDirectory::new();
            d.insert("/p", vec![DirEntry::dir("a"), DirEntry::file("b.txt")]);
            let dir = DirectoryState::new(Rc::new(d), "/p");
            let scroll = use_scroll_state("pane_test_scroll");
            let pane = file_browser_pane(
                "fb",
                &dir,
                &scroll,
                &Theme::light(),
                FileBrowserMetrics {
                    list_width: 300,
                    list_height: 200,
                    row_pitch: 32,
                    overscan: 2,
                    focusable: false,
                },
                None,
            );
            assert!(
                pane.contains_tag("fb#up"),
                "pane carries the parent affordance"
            );
            assert!(pane.contains_tag("fb#0"), "row 0 windowed into the pane");
            assert!(pane.contains_tag("fb#1"), "row 1 windowed into the pane");
        });
    }

    #[test]
    fn r791_editing_row_replaces_file_row_at_index() {
        use pinion_core::directory::InMemoryDirectory;
        use pinion_core::reactive::Owner;
        use pinion_core::widgets::scroll::use_scroll_state;

        Owner::new().run(|| {
            let d = InMemoryDirectory::new();
            d.insert("/p", vec![DirEntry::file("a.txt"), DirEntry::file("b.txt")]);
            let dir = DirectoryState::new(Rc::new(d), "/p");
            let scroll = use_scroll_state("pane_edit_scroll");
            // Edit row 1: its file_row is replaced by a tagged stand-in.
            let build = |w: u32, h: u32| {
                Scene::Container(
                    ContainerNode::new(vec![])
                        .with_tag("edit_field")
                        .with_layout(LayoutStyle::new().with_size(Size::px(w, h))),
                )
            };
            let pane = file_browser_pane(
                "fb",
                &dir,
                &scroll,
                &Theme::light(),
                FileBrowserMetrics {
                    list_width: 300,
                    list_height: 200,
                    row_pitch: 32,
                    overscan: 2,
                    focusable: false,
                },
                Some(EditingRow {
                    index: 1,
                    build: &build,
                }),
            );
            assert!(pane.contains_tag("fb#0"), "row 0 stays a plain file_row");
            assert!(
                pane.contains_tag("edit_field"),
                "row 1 became the edit field"
            );
            assert!(!pane.contains_tag("fb#1"), "row 1's file_row is replaced");
        });
    }

    /// Whether the first container tagged `tag` carries an Accent drop
    /// outline. Walks `Container.children` + `Scroll.content` (the windowed
    /// rows live inside a `Scene::Scroll`), mirroring `Scene::contains_tag`.
    fn outlined(scene: &Scene, tag: &str, accent: pinion_core::style::Color) -> bool {
        match scene {
            Scene::Container(c) if c.tag.as_deref() == Some(tag) => {
                c.style.border.is_some_and(|b| b.color == accent)
            }
            Scene::Container(c) => c.children.iter().any(|ch| outlined(ch, tag, accent)),
            Scene::Scroll(n) => outlined(&n.content, tag, accent),
            _ => false,
        }
    }

    #[test]
    fn r794_pane_outlines_the_drop_target_row() {
        use pinion_core::directory::InMemoryDirectory;
        use pinion_core::external::DropPoint;
        use pinion_core::reactive::Owner;
        use pinion_core::widgets::scroll::use_scroll_state;

        Owner::new().run(|| {
            let d = InMemoryDirectory::new();
            // entries (sorted): dst/ (dir), moveme.txt (file).
            d.insert(
                "/p",
                vec![DirEntry::dir("dst"), DirEntry::file("moveme.txt")],
            );
            d.insert("/p/dst", vec![]);
            let dir = DirectoryState::new(Rc::new(d), "/p");
            let scroll = use_scroll_state("pane_drop_scroll");
            let metrics = FileBrowserMetrics {
                list_width: 300,
                list_height: 200,
                row_pitch: 32,
                overscan: 2,
                focusable: false,
            };
            let theme = Theme::light();
            let accent = theme.resolve(ColorRole::Accent);

            // Arm a drag from the file (row 1), hover the folder (row 0).
            dir.press(1);
            dir.drag_over(Some(&pinion_core::external::DropPoint {
                tag: "fb#0".into(),
                x_rel: 0.5,
                y_rel: 0.5,
            }));
            assert_eq!(
                dir.drop_target(),
                Some(FileDropTarget::Row(0)),
                "state armed the row target"
            );
            let pane = file_browser_pane("fb", &dir, &scroll, &theme, metrics, None);
            assert!(
                outlined(&pane, "fb#0", accent),
                "the hovered folder row paints the Accent drop outline"
            );
            assert!(
                !outlined(&pane, "fb#1", accent),
                "the dragged file row is not outlined"
            );

            // Hover the `../` breadcrumb instead: it becomes the drop target.
            dir.drag_over(Some(&DropPoint {
                tag: "fb#up".into(),
                x_rel: 0.5,
                y_rel: 0.5,
            }));
            let pane = file_browser_pane("fb", &dir, &scroll, &theme, metrics, None);
            assert!(
                outlined(&pane, "fb#up", accent),
                "the `../` breadcrumb outlines when it is the drop target"
            );
            assert!(
                !outlined(&pane, "fb#0", accent),
                "the folder row is no longer the target"
            );
        });
    }

    /// ★★ R1674 — a row highlighted as a drop target keeps its name and icon
    /// inside the accent border that highlight strokes. The crate gate
    /// ([`crate::frame_gate`]).
    ///
    /// The drop-target arm specifically, because that is the arm that HAS a
    /// border: a resting row draws none, so a gate run against an idle pane
    /// would be asking about a widget with no frame.
    #[test]
    fn r1674_a_drop_target_row_keeps_its_ink_inside_its_accent_border() {
        use pinion_core::directory::InMemoryDirectory;
        use pinion_core::reactive::Owner;
        use pinion_core::widgets::scroll::use_scroll_state;

        Owner::new().run(|| {
            let d = InMemoryDirectory::new();
            d.insert(
                "/p",
                vec![DirEntry::dir("destination"), DirEntry::file("moveme.txt")],
            );
            d.insert("/p/destination", vec![]);
            let dir = DirectoryState::new(Rc::new(d), "/p");
            let scroll = use_scroll_state("r1674_frame_gate_scroll");
            dir.press(1);
            dir.drag_over(Some(&pinion_core::external::DropPoint {
                tag: "fb#0".into(),
                x_rel: 0.5,
                y_rel: 0.5,
            }));
            crate::frame_gate::assert_frame_contained("file browser drop target", &mut |w, h| {
                file_browser_pane(
                    "fb",
                    &dir,
                    &scroll,
                    &Theme::light(),
                    FileBrowserMetrics {
                        list_width: w,
                        list_height: h,
                        row_pitch: 32,
                        overscan: 2,
                        focusable: false,
                    },
                    None,
                )
            });
        });
    }
}
