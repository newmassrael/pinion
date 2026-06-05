//! `hello-file-browser` — R787 §5.15 §5.16 **own-rendered file browser**.
//!
//! The scene-graph-native peer of an OS file-open dialog. Where
//! `hello-file-dialog` bridges to a native (`rfd`-class) dialog — a window
//! pinion does not own, invisible to `scene/query` and unverifiable on a
//! headless Linux box ([[native-menu-macos-windows-only-verify]]) — this
//! browses a filesystem *inside* pinion's own scene tree. Every directory
//! entry is a paint node; the current directory, its listing, and the
//! selection are reactive `Signal`s an AI agent reads + drives entirely
//! over `scene/query` + `scene/invoke` (§2 #2 / #7).
//!
//! The only escape hatch is the [`Directory`] read trait (the §3
//! External(opaque) seam, peer of R665 `Storage`). This example backs it
//! with a **seeded [`InMemoryDirectory`]** — a synthetic sample project
//! tree — so the browse / select flow is deterministic for the demo +
//! pixel guard; the real-filesystem `FsDirectory` (which calls
//! `std::fs::read_dir`) is the production impl, unit-tested in
//! `pinion-platform-storage`.
//!
//! ## The witness (§2 #7 scene-as-data)
//!
//! `scene/query /fb_dir/external/entries` returns the current listing as
//! data (dirs suffixed `/`); `scene/invoke /fb_dir/external/navigate
//! "src"` descends into a directory and the listing + breadcrumb update;
//! clicking a row (`scene/click` on `fb_dir#<i>`) navigates into a folder
//! or selects a file (the row's `is_dir`); `.../select` picks a file and
//! `.../selected` reports its full path. No pixels required to verify the
//! browse + select flow (see `tools/r787_file_browser.py`).
//!
//! ## a11y (WAI-ARIA single-select list)
//!
//! One `AriaRole::List` over the rendered entry window (the shared
//! `pinion_a11y::windowed_list_nodes_selected`, so the windowed-list
//! topology stays one source of truth), `aria-setsize` = the entry count,
//! each rendered row an `AriaRole::ListItem` with `aria-posinset` and
//! `aria-selected` on the picked file. Keyboard roving over the window is
//! a later additive axis (R746/R786 pointer-then-keyboard precedent).

use pinion_a11y::{windowed_list_nodes_selected, AccessNode, WidgetA11y};
use pinion_core::directory::{Directory, InMemoryDirectory};
use pinion_core::external::{External, StubExternal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{BoxStyle, FlexDirection, LayoutStyle, TextStyle};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::file_browser::{use_directory_state, DirectoryExternal};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{DirEntry, Frame, Scene, WidgetCore};
use pinion_widget_paint::file_browser::{file_browser_pane, FileBrowserMetrics};
use pinion_shell::{vello_renderer_impl, WidgetView};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloFileBrowserRenderer, HelloFileBrowserRendererError);

const WIN_W: u32 = 420;
const WIN_H: u32 = 460;
const THEME_TAG: &str = "app";
/// Paint root + focusable anchor (a no-op [`StubExternal`]).
const ROOT_TAG: &str = "fb";
/// The [`DirectoryExternal`] / a11y `list` tag. Rows are `fb_dir#<i>`, the
/// parent affordance is `fb_dir#up` — all route here (R51.42 composite).
const DIR_TAG: &str = "fb_dir";
/// Vertical body scroll for the virtualized entry list.
const SCROLL_KEY: &str = "fb_scroll";
/// Where the browser opens.
const ROOT_DIR: &str = "/proj";
/// Per-row vertical slot (px) + the entry-list viewport height (10 rows).
const ROW_PITCH: u32 = 34;
const LIST_W: u32 = WIN_W - 24;
const LIST_H: u32 = 10 * ROW_PITCH;
const OVERSCAN: usize = 3;

/// Seed the synthetic sample-project tree the browser walks. The same
/// factory is handed to every `use_directory_state` call site so the
/// Owner-cached `Rc<DirectoryState>` builds it exactly once (the demo +
/// pixel guard then see a fixed, deterministic tree).
fn seed_directory() -> Rc<dyn Directory> {
    let d = InMemoryDirectory::new();
    d.insert(
        "/proj",
        vec![
            DirEntry::dir("src"),
            DirEntry::dir("assets"),
            DirEntry::file("Cargo.toml"),
            DirEntry::file("README.md"),
            DirEntry::file(".gitignore"),
        ],
    );
    d.insert(
        "/proj/src",
        vec![DirEntry::dir("ui"), DirEntry::file("main.rs"), DirEntry::file("lib.rs")],
    );
    d.insert(
        "/proj/src/ui",
        vec![DirEntry::file("button.rs"), DirEntry::file("list.rs")],
    );
    d.insert(
        "/proj/assets",
        vec![DirEntry::file("logo.png"), DirEntry::file("icon.svg")],
    );
    Rc::new(d)
}

/// view-fn (§6.3): pure sync `() -> Scene`. Reads the shared
/// [`DirectoryState`] (the same `Rc` the [`DirectoryExternal`] mutates) so
/// a navigate / select repaints; the listing is virtualized over the
/// current directory's entries.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let dir = use_directory_state(DIR_TAG, seed_directory, || ROOT_DIR.to_string());
    let scroll = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let selected = dir.selected();

    // The lifted R789.1 file-browser pane (breadcrumb + virtualized list).
    let pane = file_browser_pane(
        DIR_TAG,
        &dir,
        &scroll,
        &theme,
        FileBrowserMetrics { list_width: LIST_W, list_height: LIST_H, row_pitch: ROW_PITCH, overscan: OVERSCAN },
    );

    let footer = Scene::Text(TextNode::styled(
        match &selected {
            Some(p) => format!("Selected: {p}"),
            None => "Selected: (none)".to_string(),
        },
        Rect::default(),
        TextStyle::new().with_size_px(14).with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    Scene::Container(
        ContainerNode::new(vec![pane, footer])
            .with_tag(ROOT_TAG.to_string())
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(8)
                    .with_padding(Rect::new(12, 12, 12, 12)),
            ),
    )
}

struct FileBrowserView;

impl WidgetCore for FileBrowserView {
    type State = ();
    type Event = ();

    /// Display-driven: the addressable anchor is the no-op
    /// [`StubExternal`] at [`ROOT_TAG`]. The browse / select logic lives
    /// in the [`DirectoryExternal`] extra (rows route to it); repaints flow
    /// from the [`DirectoryState`] reactive `Signal`s the view reads.
    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    /// The [`DirectoryExternal`] over the **same** shared [`DirectoryState`]
    /// the view reads via [`use_directory_state`]. Registered at [`DIR_TAG`]
    /// so the `fb_dir#<i>` row clicks + `fb_dir#up` parent affordance route
    /// to its `send` handler (navigate / select), and `scene/invoke
    /// /fb_dir/external/...` drives the same model.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let dir = use_directory_state(DIR_TAG, seed_directory, || ROOT_DIR.to_string());
        vec![ExtraExternal::new(DIR_TAG, Box::new(DirectoryExternal::new(dir)))]
    }

    fn tag() -> &'static str {
        ROOT_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
    }

    fn title() -> &'static str {
        "pinion hello-file-browser (R787 §5.15 own-rendered file browser)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display-only (browse state in DirectoryExternal)".to_string()
    }
}

impl WidgetA11y for FileBrowserView {
    /// WAI-ARIA single-select `list` over the rendered entry window — the
    /// shared `windowed_list_nodes_selected` (the windowed-list a11y SSOT,
    /// R776/R746), `aria-setsize` = entry count, `aria-selected` on the
    /// picked file's row.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let dir = use_directory_state(DIR_TAG, seed_directory, || ROOT_DIR.to_string());
        let scroll = use_scroll_state(SCROLL_KEY);
        let count = dir.entries().len();
        let window = compute_visible_range(scroll.offset_y(), LIST_H, count, ROW_PITCH, OVERSCAN);
        windowed_list_nodes_selected(
            DIR_TAG,
            "File browser",
            u32::try_from(count).unwrap_or(u32::MAX),
            &window,
            dir.selected_index(),
        )
    }
}

impl WidgetView for FileBrowserView {
    type Renderer = HelloFileBrowserRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<FileBrowserView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;
    use pinion_core::theme::Theme;
    use pinion_core::widgets::file_browser::DirectoryState;
    use pinion_core::Owner;

    #[test]
    fn seed_tree_lists_proj_root() {
        let dir = seed_directory();
        let names: Vec<String> =
            dir.read_dir("/proj").expect("root lists").iter().map(|e| e.name.clone()).collect();
        // Dirs first (assets, src), then files alpha (.gitignore, Cargo.toml, README.md).
        assert_eq!(names, ["assets", "src", ".gitignore", "Cargo.toml", "README.md"]);
    }

    #[test]
    fn selected_index_maps_path_to_row() {
        let st = DirectoryState::new(seed_directory(), "/proj");
        assert_eq!(st.selected_index(), None);
        st.select("README.md");
        // README.md is the last row (dirs first, then files alpha).
        assert_eq!(st.selected_index(), Some(4));
    }

    /// Find the `fb_dir#<i>` row container's fill color.
    fn row_fill(scene: &Scene, index: usize) -> Option<pinion_core::style::Color> {
        fn walk(scene: &Scene, want: &str) -> Option<pinion_core::style::Color> {
            match scene {
                Scene::Container(c) => {
                    if c.tag.as_deref() == Some(want) {
                        return Some(c.style.fill);
                    }
                    c.children.iter().find_map(|ch| walk(ch, want))
                }
                Scene::Scroll(s) => walk(s.content.as_ref(), want),
                _ => None,
            }
        }
        walk(scene, &format!("{DIR_TAG}#{index}"))
    }

    #[test]
    fn selected_file_row_paints_accent() {
        let accent = Theme::light().resolve(ColorRole::Accent);
        let scene = Owner::new().run(|| {
            let dir = use_directory_state(DIR_TAG, seed_directory, || ROOT_DIR.to_string());
            dir.select("Cargo.toml"); // row 3
            view((), &Frame::default())
        });
        assert_eq!(row_fill(&scene, 3), Some(accent), "selected file row is Accent");
        assert_ne!(row_fill(&scene, 0), Some(accent), "directory row is not Accent");
        assert_ne!(row_fill(&scene, 4), Some(accent), "unselected file row is not Accent");
    }

    #[test]
    fn dir_and_file_rows_paint_distinct_fills() {
        // The dir-vs-file affordance: a directory row (raised surface) and a
        // file row (zebra) render in distinct inks (the Phase-2 pixel witness's
        // structural twin).
        let scene = Owner::new().run(|| view((), &Frame::default()));
        assert_ne!(row_fill(&scene, 0), row_fill(&scene, 2), "dir row != file row fill");
    }

    #[test]
    fn a11y_list_marks_selected_file() {
        let nodes = Owner::new().run(|| {
            let dir = use_directory_state(DIR_TAG, seed_directory, || ROOT_DIR.to_string());
            dir.select("Cargo.toml");
            FileBrowserView::access_node(&(), None)
        });
        assert_eq!(nodes[0].role, AriaRole::List);
        assert_eq!(nodes[0].size_of_set, Some(5), "five entries in /proj");
        // Cargo.toml is row 3 (assets, src, .gitignore, Cargo.toml).
        let item = nodes[1..]
            .iter()
            .find(|n| n.position_in_set == Some(4))
            .expect("listitem for row 3");
        assert_eq!(item.selected, Some(true), "picked file carries aria-selected");
    }
}
