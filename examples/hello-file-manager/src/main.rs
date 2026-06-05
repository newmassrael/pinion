// R789 §3 §5.15 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the narrative carries many proper-noun identifiers
// (DirectoryExternal, WAI-ARIA, InMemoryDirectory, …).
#![allow(clippy::doc_markdown)]

//! `hello-file-manager` — R789 §3 §5.15 **own-rendered file manager**: the
//! R787 file browser plus the new [`Directory`] **write surface**
//! (`create_dir` / `create_file` / `remove`). A toolbar of **New Folder**,
//! **New File**, and **Delete** actions mutates the listing in place, the
//! editor-essential filesystem-management gesture every pro DCC / IDE / CAD
//! tool ships — a direct step toward the northern-star "Unreal-class editor
//! self-hosted in pinion" (every `File → New / Delete` routes through one).
//!
//! ## Why this binding exists
//!
//! R787 landed the read side (browse + select); R788 composed it into a
//! modal file-open dialog; R789 completes the filesystem hatch with the
//! **write** side. Like the read side, the whole flow is AI-first: an agent
//! creates and deletes through `scene/invoke /fb_dir/external/{mkdir,
//! touch, delete}` and reads the resulting listing back as data
//! (`scene/query .../entries`), no pixels and no native file manager
//! required (§2 #2 / #7).
//!
//! ## The 3rd file-row consumer (the lift this round)
//!
//! This is the third own-rendered file UI (browser R787, open-dialog R788,
//! manager R789), so the per-row paint affordance moved to the shared
//! [`pinion_widget_paint::file_browser::file_row`] SSOT — all three render
//! a directory raised, a file zebra-striped, the selection Accent, tagged
//! `"fb_dir#<i>"`, from one place (the `button_scene` lift cadence).
//!
//! ## Architecture (unidirectional)
//!
//! The toolbar's three real focusable [`ButtonExternal`]s (`fm_newdir`
//! primary, `fm_newfile` / `fm_delete` extras) emit `"<tag>.click"`
//! intents the [`FileManagerView::update`] reducer maps onto the shared
//! [`DirectoryState`] mutations; the [`DirectoryExternal`] (`fb_dir`) owns
//! browse + select + the same mutations over RPC. Row clicks route to the
//! directory external; the listing is a reactive `Signal`, so a create /
//! delete repaints. **Delete** is gated on a selection (paints
//! [`ButtonState::Disabled`] + `aria-disabled` until a file/folder is
//! picked, the R788 OK-gate pattern).
//!
//! ## Deterministic backing
//!
//! A seeded [`InMemoryDirectory`] (the `Storage`/`InMemoryStorage`
//! precedent) so the create/delete flow is deterministic for the demo +
//! tests; the real-fs `FsDirectory` write side (`std::fs::create_dir` /
//! `File::create` / `remove_dir_all`) is unit-tested in
//! `pinion-platform-storage`.

use pinion_a11y::{windowed_list_nodes_selected, AccessNode, AriaRole, WidgetA11y};
use pinion_core::directory::{Directory, InMemoryDirectory};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::widgets::file_browser::{use_directory_state, DirectoryExternal, DirectoryState};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{DirEntry, Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::button::{
    button_a11y_state, button_scene, read_button_focused, read_button_state, ButtonColors,
    ButtonStyle,
};
use pinion_widget_paint::file_browser::{file_browser_pane, FileBrowserMetrics};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloFileManagerRenderer, HelloFileManagerRendererError);

const WIN_W: u32 = 460;
const WIN_H: u32 = 520;
const THEME_TAG: &str = "app";

/// The [`DirectoryExternal`] / a11y `list` tag. Rows are `fb_dir#<i>`, the
/// parent affordance is `fb_dir#up` — all route here (R51.42 composite).
const DIR_TAG: &str = "fb_dir";
/// New-folder button (the primary external).
const NEWDIR_TAG: &str = "fm_newdir";
/// New-file button (extra external).
const NEWFILE_TAG: &str = "fm_newfile";
/// Delete-selected button (extra external).
const DELETE_TAG: &str = "fm_delete";

const SCROLL_KEY: &str = "fm_scroll";
const ROOT_DIR: &str = "/proj";

const NEWDIR_HOVER_KEY: &str = "hello_file_manager.newdir_hover";
const NEWFILE_HOVER_KEY: &str = "hello_file_manager.newfile_hover";
const DELETE_HOVER_KEY: &str = "hello_file_manager.delete_hover";

const BTN_W: u32 = 120;
const BTN_H: u32 = 44;
const ROW_PITCH: u32 = 34;
const LIST_W: u32 = WIN_W - 24;
const LIST_H: u32 = 9 * ROW_PITCH;
const OVERSCAN: usize = 3;

/// Seed the synthetic sample-project tree the manager walks (the
/// `InMemoryDirectory` deterministic backing).
fn seed_directory() -> Rc<dyn Directory> {
    let d = InMemoryDirectory::new();
    d.insert(
        "/proj",
        vec![DirEntry::dir("src"), DirEntry::dir("assets"), DirEntry::file("Cargo.toml"), DirEntry::file("README.md")],
    );
    d.insert("/proj/src", vec![DirEntry::file("main.rs"), DirEntry::file("lib.rs")]);
    d.insert("/proj/assets", vec![DirEntry::file("logo.png")]);
    Rc::new(d)
}

/// The shared [`DirectoryState`] (one `Rc` via the cache key `DIR_TAG`).
fn directory() -> Rc<DirectoryState> {
    use_directory_state(DIR_TAG, seed_directory, || ROOT_DIR.to_string())
}

/// A name not already present in `entries`, built from `base` + `ext`
/// (`ext` includes the leading dot, or `""` for a folder): `"New Folder"`,
/// then `"New Folder 2"`, `"New Folder 3"`, … so a repeated click always
/// creates (rather than silently failing the duplicate-name rejection).
fn unique_name(entries: &[DirEntry], base: &str, ext: &str) -> String {
    let taken = |n: &str| entries.iter().any(|e| e.name == n);
    let first = format!("{base}{ext}");
    if !taken(&first) {
        return first;
    }
    for n in 2u32.. {
        let candidate = format!("{base} {n}{ext}");
        if !taken(&candidate) {
            return candidate;
        }
    }
    unreachable!("u32 range yields a free name long before exhaustion")
}

/// Render one toolbar button via the [`pinion_widget_paint::button`]
/// substrate (the opinionated filled-tonal default; the mechanical hover +
/// view_button pairing is the lifted core).
fn toolbar_button(
    tag: &'static str,
    label: &str,
    state: ButtonState,
    focused: bool,
    hover_key: &'static str,
    theme: &Theme,
) -> Scene {
    button_scene(
        label,
        state,
        focused,
        hover_key,
        &ButtonColors::filled_tonal(theme),
        &ButtonStyle::m3_default(tag).with_size(Size::px(BTN_W, BTN_H)).with_label_font_size_px(15),
    )
}

/// Cached posture for the paint fn: `[newdir, newfile, delete]` states +
/// focus flags. Browse state lives in the `DirectoryState` signals.
type FmViewState = (ButtonState, ButtonState, ButtonState, [bool; 3]);

/// view-fn (§6.3): pure sync mapping `(button postures) -> Scene`, reading
/// the shared [`DirectoryState`] so a create / delete / navigate repaints.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: FmViewState, _frame: &Frame) -> Scene {
    let (newdir_state, newfile_state, delete_state, focus) = state;
    let [newdir_focused, newfile_focused, delete_focused] = focus;
    let theme = use_theme(THEME_TAG).theme_animated();
    let dir = directory();
    let scroll = use_scroll_state(SCROLL_KEY);
    let has_selection = dir.selected().is_some();
    let item_count = dir.entries().len();

    // ── toolbar ──────────────────────────────────────────────────
    let newdir = toolbar_button(NEWDIR_TAG, "New Folder", newdir_state, newdir_focused, NEWDIR_HOVER_KEY, &theme);
    let newfile = toolbar_button(NEWFILE_TAG, "New File", newfile_state, newfile_focused, NEWFILE_HOVER_KEY, &theme);
    // Delete gated on a selection (the R788 OK-gate pattern).
    let delete_posture = if has_selection { delete_state } else { ButtonState::Disabled };
    let delete = toolbar_button(DELETE_TAG, "Delete", delete_posture, delete_focused, DELETE_HOVER_KEY, &theme);
    let toolbar = Scene::Container(
        ContainerNode::new(vec![newdir, newfile, delete])
            .with_tag("fm_toolbar")
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row).with_align_items(AlignItems::Center).with_gap(8)),
    );

    // ── the lifted R789.1 file-browser pane (breadcrumb + list) ──
    let pane = file_browser_pane(
        DIR_TAG,
        &dir,
        &scroll,
        &theme,
        FileBrowserMetrics { list_width: LIST_W, list_height: LIST_H, row_pitch: ROW_PITCH, overscan: OVERSCAN },
    );

    let status = Scene::Text(
        TextNode::styled(
            match dir.selected() {
                Some(p) => format!("Selected: {p}"),
                None => format!("{item_count} items"),
            },
            Rect::default(),
            TextStyle::new().with_size_px(14).with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_tag("fm_status"),
    );

    Scene::Container(
        ContainerNode::new(vec![toolbar, pane, status])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new().flex(FlexDirection::Column).with_gap(8).with_padding(Rect::new(12, 12, 12, 12)),
            ),
    )
}

struct FileManagerView;

impl WidgetCore for FileManagerView {
    type State = FmViewState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(NEWFILE_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(DELETE_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(DIR_TAG, Box::new(DirectoryExternal::new(directory()))),
        ]
    }

    fn tag() -> &'static str {
        NEWDIR_TAG
    }

    fn read_state(scene: &Scene) -> FmViewState {
        (
            read_button_state(scene, NEWDIR_TAG),
            read_button_state(scene, NEWFILE_TAG),
            read_button_state(scene, DELETE_TAG),
            [
                read_button_focused(scene, NEWDIR_TAG),
                read_button_focused(scene, NEWFILE_TAG),
                read_button_focused(scene, DELETE_TAG),
            ],
        )
    }

    fn view(state: FmViewState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-file-manager (R789 §3 §5.15 Directory write surface)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    fn focusable_tags() -> Vec<&'static str> {
        vec![NEWDIR_TAG, NEWFILE_TAG, DELETE_TAG]
    }

    /// Enter / Space on a focused toolbar button activate it through the
    /// shared ARIA helper, which emits the button's `"click"` intent.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        apply_aria_activate(scene, focused, key, NEWDIR_TAG)
            || apply_aria_activate(scene, focused, key, NEWFILE_TAG)
            || apply_aria_activate(scene, focused, key, DELETE_TAG)
    }

    /// Bridge the toolbar buttons' clicks onto the [`DirectoryState`]
    /// mutations. New Folder / New File auto-name a fresh entry in the
    /// current directory; Delete removes the selection (a no-op without
    /// one — the painted gate). Side-effect-only.
    fn update(
        _state: FmViewState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        let dir = directory();
        match intent.tag_str() {
            "fm_newdir.click" => {
                let name = unique_name(&dir.entries(), "New Folder", "");
                dir.create_dir(&name);
            }
            "fm_newfile.click" => {
                let name = unique_name(&dir.entries(), "untitled", ".txt");
                dir.create_file(&name);
            }
            "fm_delete.click" => {
                dir.delete_selected();
            }
            _ => {}
        }
        Vec::new()
    }

    fn fmt_state_log(state: &FmViewState) -> String {
        format!("newdir={:?} newfile={:?} delete={:?}", state.0, state.1, state.2)
    }
}

impl WidgetA11y for FileManagerView {
    /// WAI-ARIA tree: the three toolbar buttons (Delete `aria-disabled`
    /// until a selection) + the single-select file `list` (the shared
    /// `windowed_list_nodes_selected`, `aria-selected` on the picked
    /// entry). Names enrich from the paint scene.
    fn access_node(state: &FmViewState, focused: Option<&str>) -> Vec<AccessNode> {
        let dir = directory();
        let scroll = use_scroll_state(SCROLL_KEY);
        let count = dir.entries().len();
        let has_selection = dir.selected().is_some();
        let window = compute_visible_range(scroll.offset_y(), LIST_H, count, ROW_PITCH, OVERSCAN);

        let delete_posture = if has_selection { state.2 } else { ButtonState::Disabled };
        let mut nodes = vec![
            AccessNode::new(NEWDIR_TAG, AriaRole::Button)
                .with_state(button_a11y_state(state.0, focused == Some(NEWDIR_TAG))),
            AccessNode::new(NEWFILE_TAG, AriaRole::Button)
                .with_state(button_a11y_state(state.1, focused == Some(NEWFILE_TAG))),
            AccessNode::new(DELETE_TAG, AriaRole::Button)
                .with_state(button_a11y_state(delete_posture, focused == Some(DELETE_TAG))),
        ];
        nodes.extend(windowed_list_nodes_selected(
            DIR_TAG,
            "Files",
            u32::try_from(count).unwrap_or(u32::MAX),
            &window,
            dir.selected_index(),
        ));
        nodes
    }
}

impl WidgetView for FileManagerView {
    type Renderer = HelloFileManagerRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<FileManagerView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::reactive::Owner;

    fn idle() -> FmViewState {
        (ButtonState::Idle, ButtonState::Idle, ButtonState::Idle, [false; 3])
    }

    fn intent(tag: &str) -> pinion_core::Intent {
        pinion_core::Intent::new_owned(tag.to_string(), pinion_core::external::IntrospectValue::Null)
    }

    #[test]
    fn r789_unique_name_avoids_collisions() {
        let entries = vec![DirEntry::dir("New Folder"), DirEntry::file("untitled.txt")];
        assert_eq!(unique_name(&entries, "New Folder", ""), "New Folder 2");
        assert_eq!(unique_name(&entries, "untitled", ".txt"), "untitled 2.txt");
        assert_eq!(unique_name(&entries, "src", ""), "src", "free base used verbatim");
    }

    #[test]
    fn r789_new_folder_click_creates_and_lists() {
        Owner::new().run(|| {
            let _ = FileManagerView::update(idle(), &intent("fm_newdir.click"));
            let names: Vec<String> = directory().entries().iter().map(|e| e.name.clone()).collect();
            assert!(names.contains(&"New Folder".to_string()), "new folder appears: {names:?}");
            // A second click auto-names rather than failing the duplicate.
            let _ = FileManagerView::update(idle(), &intent("fm_newdir.click"));
            let names: Vec<String> = directory().entries().iter().map(|e| e.name.clone()).collect();
            assert!(names.contains(&"New Folder 2".to_string()), "second click = New Folder 2");
        });
    }

    #[test]
    fn r789_new_file_click_creates() {
        Owner::new().run(|| {
            let _ = FileManagerView::update(idle(), &intent("fm_newfile.click"));
            assert!(
                directory().entries().iter().any(|e| e.name == "untitled.txt" && !e.is_dir),
                "new file appears",
            );
        });
    }

    #[test]
    fn r789_delete_click_removes_selection() {
        Owner::new().run(|| {
            let dir = directory();
            dir.select("README.md");
            let _ = FileManagerView::update(idle(), &intent("fm_delete.click"));
            assert!(!dir.entries().iter().any(|e| e.name == "README.md"), "README.md removed");
            assert_eq!(dir.selected(), None, "selection cleared");
            // Delete with no selection is a no-op (the gate).
            let before = dir.entries().len();
            let _ = FileManagerView::update(idle(), &intent("fm_delete.click"));
            assert_eq!(dir.entries().len(), before, "delete with no selection is a no-op");
        });
    }

    #[test]
    fn r789_delete_disabled_without_selection_enabled_with() {
        Owner::new().run(|| {
            // a11y: Delete disabled with no selection.
            let nodes = FileManagerView::access_node(&idle(), None);
            let del = nodes.iter().find(|n| n.tag == DELETE_TAG).expect("delete node");
            assert!(del.state.disabled, "Delete aria-disabled with no selection");
            // After selecting, Delete enables.
            directory().select("Cargo.toml");
            let nodes = FileManagerView::access_node(&idle(), None);
            let del = nodes.iter().find(|n| n.tag == DELETE_TAG).expect("delete node");
            assert!(!del.state.disabled, "Delete enabled once an entry is selected");
        });
    }

    #[test]
    fn r789_a11y_lists_buttons_and_files() {
        Owner::new().run(|| {
            let nodes = FileManagerView::access_node(&idle(), None);
            assert_eq!(nodes[0].tag, NEWDIR_TAG);
            assert_eq!(nodes[0].role, AriaRole::Button);
            let list = nodes.iter().find(|n| n.tag == DIR_TAG).expect("file list");
            assert_eq!(list.role, AriaRole::List);
            assert_eq!(list.size_of_set, Some(4), "four entries in /proj");
        });
    }

    #[test]
    fn r789_view_contains_primary_paint_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<FileManagerView>(
            idle(),
            &Frame::default(),
        );
    }
}
