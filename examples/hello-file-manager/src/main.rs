// R789 §3 §5.15 / R791 §5.15 §5.38 — example bindings tolerate looser
// doc-markdown lints than substrate crates; the narrative carries many
// proper-noun identifiers (DirectoryExternal, WAI-ARIA, TextFieldExternal, …).
#![allow(clippy::doc_markdown)]

//! `hello-file-manager` — R789 §3 §5.15 **own-rendered file manager** + R791
//! §5.15 §5.38 **inline rename**: the R787 file browser plus the [`Directory`]
//! **write surface** (`create_dir` / `create_file` / `remove` / `rename`). A
//! toolbar of **New Folder**, **New File**, **Rename**, and **Delete**
//! mutates the listing in place — the editor-essential filesystem-management
//! gesture every pro DCC / IDE / CAD tool ships, a direct step toward the
//! northern-star "Unreal-class editor self-hosted in pinion".
//!
//! ## Inline rename (R791)
//!
//! **Rename** (or `F2`) puts the selected row into edit mode: its
//! [`file_row`](pinion_widget_paint::file_browser::file_row) is replaced by a
//! `TextField` (the R789.1 [`file_browser_pane`]'s new
//! [`EditingRow`](pinion_widget_paint::file_browser::EditingRow) hook),
//! pre-filled with the current name and focused. `Enter` commits the rename
//! ([`DirectoryState::rename_selected`] → [`Directory::rename`]); `Escape`
//! cancels. The in-place editor is the same extra-external `TextField`
//! shape todomvc's row editor uses (`[[textfield-as-modal-member]]`).
//!
//! ## Why this binding exists
//!
//! R787 landed read (browse + select); R788 composed the open dialog; R789
//! the create/delete write side; R790 the save dialog; R791 closes the write
//! surface with **rename**. Like the rest, the whole flow is AI-first: an
//! agent creates / deletes / renames through `scene/invoke
//! /fb_dir/external/{mkdir, touch, delete, rename}` and reads the resulting
//! listing back as data (`scene/query .../entries`), no pixels and no native
//! file manager required (§2 #2 / #7).
//!
//! ## Architecture (unidirectional)
//!
//! The toolbar's four [`ButtonExternal`]s (`fm_newdir` primary, `fm_newfile`
//! / `fm_rename` / `fm_delete` extras) emit `"<tag>.click"` intents the
//! [`FileManagerView::update`] reducer maps onto the shared
//! [`DirectoryState`]; the rename `TextField` (`fm_rename_tf`) is an extra
//! external whose keys route through `apply_key` while it owns focus. Row
//! clicks route to the [`DirectoryExternal`]; the listing is a reactive
//! `Signal`, so a create / delete / rename repaints. **Rename** + **Delete**
//! are gated on a selection (paint [`ButtonState::Disabled`] + `aria-disabled`
//! until a file/folder is picked, the R788 OK-gate pattern).
//!
//! ## Deterministic backing
//!
//! A seeded [`InMemoryDirectory`] (the `Storage`/`InMemoryStorage`
//! precedent); the real-fs `FsDirectory` write side (`std::fs::create_dir` /
//! `File::create` / `remove_dir_all` / `rename`) is unit-tested in
//! `pinion-platform-storage`.

use pinion_a11y::{windowed_list_nodes_selected, AccessNode, AriaRole, WidgetA11y};
use pinion_core::directory::{Directory, InMemoryDirectory};
use pinion_core::external::External;
use pinion_core::reactive::{batch, Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::file_browser::{use_directory_state, DirectoryExternal, DirectoryState};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::{TextFieldExternal, TextFieldState};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{DirEntry, Frame, Scene, WidgetCore};
use pinion_platform_clipboard::use_app_clipboard;
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_text::CaretRect;
use pinion_widget_paint::button::{
    button_a11y_state, button_scene, read_button_focused, read_button_state, ButtonColors,
    ButtonStyle,
};
use pinion_widget_paint::file_browser::{file_browser_pane, EditingRow, FileBrowserMetrics};
use pinion_widget_paint::text_field as tf_paint;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloFileManagerRenderer, HelloFileManagerRendererError);

const WIN_W: u32 = 480;
const WIN_H: u32 = 540;
const THEME_TAG: &str = "app";

/// The [`DirectoryExternal`] / a11y `list` tag. Rows are `fb_dir#<i>`, the
/// parent affordance is `fb_dir#up` — all route here (R51.42 composite).
const DIR_TAG: &str = "fb_dir";
/// New-folder button (the primary external).
const NEWDIR_TAG: &str = "fm_newdir";
/// New-file button (extra external).
const NEWFILE_TAG: &str = "fm_newfile";
/// Rename-selected button (extra external) — enters inline edit mode.
const RENAME_TAG: &str = "fm_rename";
/// Delete-selected button (extra external).
const DELETE_TAG: &str = "fm_delete";
/// The inline-rename `TextField` (extra external + phantom tab stop).
const RENAME_TF_TAG: &str = "fm_rename_tf";

const SCROLL_KEY: &str = "fm_scroll";
/// `Owner::cache` key for the `Signal<bool>` inline-edit-mode flag.
const RENAMING_KEY: &str = "hello_file_manager.renaming";
const ROOT_DIR: &str = "/proj";

const NEWDIR_HOVER_KEY: &str = "hello_file_manager.newdir_hover";
const NEWFILE_HOVER_KEY: &str = "hello_file_manager.newfile_hover";
const RENAME_HOVER_KEY: &str = "hello_file_manager.rename_hover";
const DELETE_HOVER_KEY: &str = "hello_file_manager.delete_hover";

const BTN_W: u32 = 92;
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

/// The inline-rename field's reactive text store (shared between the
/// `TextFieldExternal::attach_state` and every read site).
fn rename_state() -> Rc<pinion_core::widgets::text_edit::TextEditState> {
    use_text_edit_state(RENAME_TF_TAG)
}

/// `Owner::cache`-keyed inline-edit-mode flag — `true` while the selected
/// row shows the rename `TextField`.
fn use_renaming() -> Rc<Signal<bool>> {
    Owner::current()
        .expect("use_renaming requires an active Owner scope")
        .cache(RENAMING_KEY, || Signal::new(false))
}

/// The last path component of a `'/'`-path (`"/proj/a.txt"` → `"a.txt"`).
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// A name not already present in `entries`, built from `base` + `ext`.
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

/// R791 — enter inline rename: pre-fill the field with the selected
/// entry's name (caret at end), flip into edit mode, and move focus to the
/// field. A no-op without a selection (the Rename gate). The text + caret +
/// flag writes [`batch`] so the row re-renders once.
fn enter_rename() {
    let Some(selected) = directory().selected() else {
        return;
    };
    let name = basename(&selected).to_owned();
    let edit = rename_state();
    batch(|| {
        edit.set_text(name.clone());
        edit.set_caret(name.len());
        use_renaming().set(true);
    });
    pinion_core::focus_request::request(RENAME_TF_TAG);
}

/// R791 — commit the inline rename: write the (trimmed, non-blank) field
/// text onto the selected entry via the [`Directory`] rename surface, then
/// tear down edit mode. A blank name cancels (no rename).
fn commit_rename() {
    let name = rename_state().text();
    if !name.trim().is_empty() {
        directory().rename_selected(name.trim());
    }
    end_rename();
}

/// R791 — cancel the inline rename without writing.
fn cancel_rename() {
    end_rename();
}

/// R791 — shared teardown: clear the edit flag, wipe the field, and return
/// focus to the Rename button so the next keystroke lands on the toolbar.
fn end_rename() {
    batch(|| {
        use_renaming().set(false);
        rename_state().set_text(String::new());
    });
    pinion_core::focus_request::request(RENAME_TAG);
}

/// Render one toolbar button via the [`pinion_widget_paint::button`]
/// substrate (the opinionated filled-tonal default).
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
        &ButtonStyle::m3_default(tag).with_size(Size::px(BTN_W, BTN_H)).with_label_font_size_px(14),
    )
}

/// The rename field's paint style — the M3 filled `TextField` sized to fill
/// the list row it replaces (`width` × `pitch` from the pane).
fn rename_field_style(width: u32, pitch: u32) -> tf_paint::TextFieldStyle {
    tf_paint::TextFieldStyle { field_w: width, field_h: pitch, ..tf_paint::TextFieldStyle::m3_filled() }
}

/// Cached posture for the paint fn: `[newdir, newfile, rename, delete]`
/// states + focus flags, then the rename field's interaction + caret.
type FmViewState =
    (ButtonState, ButtonState, ButtonState, ButtonState, [bool; 4], TextFieldState, u32);

/// view-fn (§6.3): pure sync mapping `(postures) -> Scene`, reading the
/// shared [`DirectoryState`] + the `renaming` flag so a create / delete /
/// rename / navigate / edit-toggle repaints.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: FmViewState, _frame: &Frame) -> Scene {
    let (newdir_state, newfile_state, rename_state_btn, delete_state, focus, tf_interaction, tf_caret) =
        state;
    let [newdir_focused, newfile_focused, rename_focused, delete_focused] = focus;
    let theme = use_theme(THEME_TAG).theme_animated();
    let dir = directory();
    let scroll = use_scroll_state(SCROLL_KEY);
    let has_selection = dir.selected().is_some();
    let renaming = use_renaming().get();
    let item_count = dir.entries().len();

    // ── toolbar ──────────────────────────────────────────────────
    let newdir = toolbar_button(NEWDIR_TAG, "New Folder", newdir_state, newdir_focused, NEWDIR_HOVER_KEY, &theme);
    let newfile = toolbar_button(NEWFILE_TAG, "New File", newfile_state, newfile_focused, NEWFILE_HOVER_KEY, &theme);
    // Rename + Delete gated on a selection (the R788 OK-gate pattern).
    let rename_posture = if has_selection { rename_state_btn } else { ButtonState::Disabled };
    let rename_btn = toolbar_button(RENAME_TAG, "Rename", rename_posture, rename_focused, RENAME_HOVER_KEY, &theme);
    let delete_posture = if has_selection { delete_state } else { ButtonState::Disabled };
    let delete = toolbar_button(DELETE_TAG, "Delete", delete_posture, delete_focused, DELETE_HOVER_KEY, &theme);
    let toolbar = Scene::Container(
        ContainerNode::new(vec![newdir, newfile, rename_btn, delete])
            .with_tag("fm_toolbar")
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row).with_align_items(AlignItems::Center).with_gap(6)),
    );

    // ── the file-browser pane, with the editing row when renaming ──
    // The edit field replaces the selected row's file_row in place.
    let editing_index = if renaming { dir.selected_index() } else { None };
    let build_field = |w: u32, h: u32| {
        tf_paint::view_field(
            RENAME_TF_TAG,
            tf_interaction,
            tf_caret,
            &theme,
            &rename_field_style(w, h),
            "Rename",
        )
    };
    let editing = editing_index.map(|index| EditingRow { index, build: &build_field });
    let pane = file_browser_pane(
        DIR_TAG,
        &dir,
        &scroll,
        &theme,
        FileBrowserMetrics { list_width: LIST_W, list_height: LIST_H, row_pitch: ROW_PITCH, overscan: OVERSCAN },
        editing,
    );

    let status = Scene::Text(
        TextNode::styled(
            match dir.selected() {
                Some(p) if renaming => format!("Renaming: {p}"),
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
            ExtraExternal::new(RENAME_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(DELETE_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(DIR_TAG, Box::new(DirectoryExternal::new(directory()))),
            ExtraExternal::new(
                RENAME_TF_TAG,
                Box::new(
                    TextFieldExternal::new()
                        .attach_state(rename_state())
                        .attach_blink(use_caret_blink(RENAME_TF_TAG))
                        .attach_clipboard(use_app_clipboard(RENAME_TF_TAG)),
                ),
            ),
        ]
    }

    fn tag() -> &'static str {
        NEWDIR_TAG
    }

    fn read_state(scene: &Scene) -> FmViewState {
        let (tf_interaction, tf_caret) = tf_paint::read_text_field_state(scene, RENAME_TF_TAG);
        (
            read_button_state(scene, NEWDIR_TAG),
            read_button_state(scene, NEWFILE_TAG),
            read_button_state(scene, RENAME_TAG),
            read_button_state(scene, DELETE_TAG),
            [
                read_button_focused(scene, NEWDIR_TAG),
                read_button_focused(scene, NEWFILE_TAG),
                read_button_focused(scene, RENAME_TAG),
                read_button_focused(scene, DELETE_TAG),
            ],
            tf_interaction,
            tf_caret,
        )
    }

    fn view(state: FmViewState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-file-manager (R789 write surface + R791 inline rename)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    fn focusable_tags() -> Vec<&'static str> {
        vec![NEWDIR_TAG, NEWFILE_TAG, RENAME_TAG, DELETE_TAG, RENAME_TF_TAG]
    }

    /// R791 §5.38 — `F2` (with a selection, not already editing) starts an
    /// inline rename. While the rename field owns focus, `Enter` commits,
    /// `Escape` cancels, and any other key forwards to the `TextField`
    /// substrate. The toolbar buttons activate through the shared ARIA
    /// helper.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: pinion_core::Modifiers,
    ) -> bool {
        if key == "F2" && !use_renaming().get() && directory().selected().is_some() {
            enter_rename();
            return true;
        }
        if focused == Some(RENAME_TF_TAG) {
            match key {
                "Enter" => {
                    commit_rename();
                    return true;
                }
                "Escape" => {
                    cancel_rename();
                    return true;
                }
                _ => return tf_paint::forward_key_to_field(scene, RENAME_TF_TAG, key, modifiers),
            }
        }
        apply_aria_activate(scene, focused, key, NEWDIR_TAG)
            || apply_aria_activate(scene, focused, key, NEWFILE_TAG)
            || apply_aria_activate(scene, focused, key, RENAME_TAG)
            || apply_aria_activate(scene, focused, key, DELETE_TAG)
    }

    /// R791 §5.13 — route IME composition to the rename field while it owns
    /// focus.
    fn apply_composition(
        scene: &mut Scene,
        focused: Option<&str>,
        event: &pinion_core::CompositionEvent,
    ) -> bool {
        if focused != Some(RENAME_TF_TAG) {
            return false;
        }
        tf_paint::forward_composition_to_field(scene, RENAME_TF_TAG, event)
    }

    /// Bridge the toolbar buttons' clicks onto the [`DirectoryState`]
    /// mutations. New Folder / New File auto-name a fresh entry; Rename
    /// enters inline edit mode; Delete removes the selection (each a no-op
    /// without one — the painted gate). Side-effect-only.
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
            "fm_rename.click" => enter_rename(),
            "fm_delete.click" => {
                dir.delete_selected();
            }
            _ => {}
        }
        Vec::new()
    }

    fn fmt_state_log(state: &FmViewState) -> String {
        format!("newdir={:?} newfile={:?} rename={:?} delete={:?}", state.0, state.1, state.2, state.3)
    }
}

impl WidgetA11y for FileManagerView {
    /// WAI-ARIA tree: the four toolbar buttons (Rename + Delete
    /// `aria-disabled` until a selection) + the single-select file `list`.
    /// While renaming, the inline-edit `TextField` is announced as a
    /// `textbox` (the R790 `text_field_a11y_node` SSOT). Names enrich from
    /// the paint scene.
    fn access_node(state: &FmViewState, focused: Option<&str>) -> Vec<AccessNode> {
        let dir = directory();
        let scroll = use_scroll_state(SCROLL_KEY);
        let count = dir.entries().len();
        let has_selection = dir.selected().is_some();
        let renaming = use_renaming().get();
        let window = compute_visible_range(scroll.offset_y(), LIST_H, count, ROW_PITCH, OVERSCAN);

        let rename_posture = if has_selection { state.2 } else { ButtonState::Disabled };
        let delete_posture = if has_selection { state.3 } else { ButtonState::Disabled };
        let mut nodes = vec![
            AccessNode::new(NEWDIR_TAG, AriaRole::Button)
                .with_state(button_a11y_state(state.0, focused == Some(NEWDIR_TAG))),
            AccessNode::new(NEWFILE_TAG, AriaRole::Button)
                .with_state(button_a11y_state(state.1, focused == Some(NEWFILE_TAG))),
            AccessNode::new(RENAME_TAG, AriaRole::Button)
                .with_state(button_a11y_state(rename_posture, focused == Some(RENAME_TAG))),
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
        if renaming {
            nodes.push(tf_paint::text_field_a11y_node(
                RENAME_TF_TAG,
                rename_state().text(),
                state.5,
                focused == Some(RENAME_TF_TAG),
            ));
        }
        nodes
    }
}

impl WidgetView for FileManagerView {
    type Renderer = HelloFileManagerRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }

    /// R791 §5.13 — publish the rename field's caret rect to the platform
    /// IME while it owns focus.
    fn ime_caret_rect(
        state: &FmViewState,
        scene: &Scene,
        focused: Option<&str>,
    ) -> Option<CaretRect> {
        if focused != Some(RENAME_TF_TAG) {
            return None;
        }
        let field_rect = pinion_shell::rect_for_tag(scene, RENAME_TF_TAG)?;
        let theme = use_theme(THEME_TAG).theme_animated();
        // The field fills a list row; mirror the paint style's row sizing.
        Some(tf_paint::ime_caret_rect_for(
            RENAME_TF_TAG,
            state.5,
            state.6,
            field_rect,
            &theme,
            &rename_field_style(LIST_W, ROW_PITCH),
        ))
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
        (
            ButtonState::Idle,
            ButtonState::Idle,
            ButtonState::Idle,
            ButtonState::Idle,
            [false; 4],
            TextFieldState::Idle,
            0,
        )
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
            let before = dir.entries().len();
            let _ = FileManagerView::update(idle(), &intent("fm_delete.click"));
            assert_eq!(dir.entries().len(), before, "delete with no selection is a no-op");
        });
    }

    #[test]
    fn r789_rename_delete_disabled_without_selection_enabled_with() {
        Owner::new().run(|| {
            let nodes = FileManagerView::access_node(&idle(), None);
            let del = nodes.iter().find(|n| n.tag == DELETE_TAG).expect("delete node");
            let ren = nodes.iter().find(|n| n.tag == RENAME_TAG).expect("rename node");
            assert!(del.state.disabled, "Delete aria-disabled with no selection");
            assert!(ren.state.disabled, "Rename aria-disabled with no selection");
            directory().select("Cargo.toml");
            let nodes = FileManagerView::access_node(&idle(), None);
            let del = nodes.iter().find(|n| n.tag == DELETE_TAG).expect("delete node");
            let ren = nodes.iter().find(|n| n.tag == RENAME_TAG).expect("rename node");
            assert!(!del.state.disabled, "Delete enabled once an entry is selected");
            assert!(!ren.state.disabled, "Rename enabled once an entry is selected");
        });
    }

    // ----- R791 inline rename -----

    #[test]
    fn r791_rename_click_enters_edit_mode_prefilled_and_focuses() {
        Owner::new().run(|| {
            let _ = pinion_core::focus_request::drain();
            // No selection: Rename is a no-op.
            let _ = FileManagerView::update(idle(), &intent("fm_rename.click"));
            assert!(!use_renaming().get(), "rename with no selection does nothing");
            // Select, then Rename → edit mode, field pre-filled, focus queued.
            directory().select("README.md");
            let _ = FileManagerView::update(idle(), &intent("fm_rename.click"));
            assert!(use_renaming().get(), "Rename enters edit mode");
            assert_eq!(rename_state().text(), "README.md", "field pre-filled with the current name");
            assert_eq!(rename_state().caret(), "README.md".len(), "caret at end");
            assert_eq!(
                pinion_core::focus_request::drain(),
                Some(RENAME_TF_TAG.to_string()),
                "focus moves to the rename field",
            );
        });
    }

    #[test]
    fn r791_enter_commits_rename_and_exits() {
        Owner::new().run(|| {
            let _ = pinion_core::focus_request::drain();
            let dir = directory();
            dir.select("README.md");
            enter_rename();
            rename_state().set_text("NOTES.md".to_owned());
            let _ = pinion_core::focus_request::drain();
            let mut scene = boot_scene();
            assert!(FileManagerView::apply_key(
                &mut scene,
                Some(RENAME_TF_TAG),
                "Enter",
                pinion_core::Modifiers::empty(),
            ));
            assert!(!use_renaming().get(), "Enter exits edit mode");
            assert!(dir.entries().iter().any(|e| e.name == "NOTES.md"), "file renamed");
            assert!(!dir.entries().iter().any(|e| e.name == "README.md"), "old name gone");
            assert_eq!(dir.selected(), Some("/proj/NOTES.md".to_string()), "selection follows");
            assert_eq!(rename_state().text(), "", "field wiped on exit");
            assert_eq!(pinion_core::focus_request::drain(), Some(RENAME_TAG.to_string()), "focus restored");
        });
    }

    #[test]
    fn r791_escape_cancels_rename_without_writing() {
        Owner::new().run(|| {
            let _ = pinion_core::focus_request::drain();
            let dir = directory();
            dir.select("README.md");
            enter_rename();
            rename_state().set_text("WONT.md".to_owned());
            let mut scene = boot_scene();
            assert!(FileManagerView::apply_key(
                &mut scene,
                Some(RENAME_TF_TAG),
                "Escape",
                pinion_core::Modifiers::empty(),
            ));
            assert!(!use_renaming().get(), "Escape exits edit mode");
            assert!(dir.entries().iter().any(|e| e.name == "README.md"), "name unchanged");
            assert!(!dir.entries().iter().any(|e| e.name == "WONT.md"), "nothing renamed");
        });
    }

    #[test]
    fn r791_blank_name_commit_cancels() {
        Owner::new().run(|| {
            let dir = directory();
            dir.select("README.md");
            enter_rename();
            rename_state().set_text("   ".to_owned());
            commit_rename();
            assert!(!use_renaming().get(), "blank commit exits edit mode");
            assert!(dir.entries().iter().any(|e| e.name == "README.md"), "blank name does not rename");
        });
    }

    #[test]
    fn r791_f2_starts_rename_with_a_selection() {
        Owner::new().run(|| {
            let _ = pinion_core::focus_request::drain();
            let mut scene = boot_scene();
            // F2 with no selection is inert.
            assert!(!FileManagerView::apply_key(
                &mut scene,
                Some(NEWDIR_TAG),
                "F2",
                pinion_core::Modifiers::empty(),
            ));
            assert!(!use_renaming().get());
            // F2 with a selection enters rename.
            directory().select("Cargo.toml");
            assert!(FileManagerView::apply_key(
                &mut scene,
                Some(NEWDIR_TAG),
                "F2",
                pinion_core::Modifiers::empty(),
            ));
            assert!(use_renaming().get(), "F2 starts the inline rename");
            assert_eq!(rename_state().text(), "Cargo.toml");
        });
    }

    #[test]
    fn r791_view_shows_edit_field_in_place_of_selected_row() {
        Owner::new().run(|| {
            let dir = directory();
            dir.select("Cargo.toml");
            enter_rename();
            let scene = view(idle(), &Frame::new());
            assert!(scene.contains_tag(RENAME_TF_TAG), "the rename field paints in the row");
            // The selected row's plain file_row is replaced by the field.
            let idx = dir.selected_index().expect("a selection has a row");
            assert!(!scene.contains_tag(&format!("{DIR_TAG}#{idx}")), "selected file_row replaced");
            // Another row stays a plain file_row.
            assert!(scene.contains_tag(&format!("{DIR_TAG}#0")) || idx == 0, "non-edited rows remain");
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
    fn r791_a11y_announces_textbox_while_renaming() {
        Owner::new().run(|| {
            directory().select("Cargo.toml");
            enter_rename();
            let nodes = FileManagerView::access_node(&idle(), Some(RENAME_TF_TAG));
            let tb = nodes.iter().find(|n| n.tag == RENAME_TF_TAG).expect("textbox node");
            assert_eq!(tb.role, AriaRole::TextInput, "the rename field is announced as a textbox");
        });
    }

    #[test]
    fn r789_view_contains_primary_paint_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<FileManagerView>(
            idle(),
            &Frame::default(),
        );
    }

    /// Build the multi-external scene the way the shell does so `apply_key`'s
    /// forwarding has real externals to dispatch against.
    fn boot_scene() -> Scene {
        use pinion_core::scene::ExternalNode;
        let mut children = vec![Scene::External(
            ExternalNode::new(FileManagerView::create_external()).with_tag(NEWDIR_TAG),
        )];
        for extra in FileManagerView::create_extra_externals() {
            children.push(Scene::External(ExternalNode::new(extra.handle).with_tag(extra.tag)));
        }
        Scene::Container(ContainerNode::new(children))
    }
}
