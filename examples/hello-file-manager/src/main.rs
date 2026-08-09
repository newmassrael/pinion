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
//! northern-star "engine-class editor self-hosted in pinion".
//!
//! ## Inline rename (R791)
//!
//! **Rename** (or `F2`) puts the selected row into edit mode: its
//! [`file_row`](pinion_widget_paint::file_browser::file_row) is replaced by a
//! `TextField` (the R789.1 [`file_browser_pane`]'s new
//! [`EditingRow`] hook),
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

use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, AriaRole, WidgetA11y, windowed_list_nodes_selected,
};
use pinion_core::directory::{Directory, InMemoryDirectory};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::reactive::{Owner, Signal, batch};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::file_browser::{
    DirectoryExternal, DirectoryState, dir_nav_key, use_directory_state,
};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::{TextFieldExternal, TextFieldState};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{DirEntry, Frame, Scene, WidgetCore};
use pinion_platform_clipboard::use_app_clipboard;
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_text::CaretRect;
use pinion_widget_paint::button::{
    ButtonColors, ButtonStyle, button_a11y_state, button_scene, read_button_state,
};
use pinion_widget_paint::file_browser::{EditingRow, FileBrowserMetrics, file_browser_pane};
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
        vec![
            DirEntry::dir("src"),
            DirEntry::dir("assets"),
            DirEntry::file("Cargo.toml"),
            DirEntry::file("README.md"),
        ],
    );
    // `src` carries more rows than the 9-row list viewport so keyboard
    // navigation (End / PageDown) scrolls a never-materialized row into view
    // (R792): main.rs, lib.rs, then mod00..mod19.
    let mut src = vec![DirEntry::file("main.rs"), DirEntry::file("lib.rs")];
    src.extend((0..20).map(|i| DirEntry::file(format!("mod{i:02}.rs"))));
    d.insert("/proj/src", src);
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
    // R791.1 — the selected entry's leaf name is `DirectoryState`'s SSOT.
    let Some(name) = directory().selected_name() else {
        return;
    };
    let edit = rename_state();
    batch(|| {
        edit.seed(name.clone());
        use_renaming().set(true);
    });
    pinion_core::focus_request::request(RENAME_TF_TAG);
}

/// R791 — commit the inline rename: write the (trimmed, non-blank) field
/// text onto the selected entry via the [`Directory`] rename surface, then
/// tear down edit mode. A blank name cancels (no rename). `restore_focus`
/// returns focus to the Rename button (the `Enter` path); the R793
/// commit-on-blur path passes `false` so it does not steal focus back from
/// wherever the click that caused the blur moved it.
fn commit_rename(restore_focus: bool) {
    let name = rename_state().text();
    if !name.trim().is_empty() {
        directory().rename_selected(name.trim());
    }
    end_rename(restore_focus);
}

/// R791 — cancel the inline rename without writing.
fn cancel_rename() {
    end_rename(true);
}

/// R791 — shared teardown: clear the edit flag and wipe the field. When
/// `restore_focus` is set (the `Enter` / `Escape` keyboard paths) focus
/// returns to the Rename button so the next keystroke lands on the toolbar;
/// the R793 blur path leaves focus where the user moved it.
fn end_rename(restore_focus: bool) {
    batch(|| {
        use_renaming().set(false);
        rename_state().set_text(String::new());
    });
    if restore_focus {
        pinion_core::focus_request::request(RENAME_TAG);
    }
}

/// Render one toolbar button via the [`pinion_widget_paint::button`]
/// substrate (the opinionated filled-tonal default).
fn toolbar_button(
    tag: &'static str,
    label: &str,
    state: ButtonState,
    hover_key: &'static str,
    theme: &Theme,
) -> Scene {
    button_scene(
        label,
        state,
        hover_key,
        &ButtonColors::filled_tonal(theme),
        &ButtonStyle::m3_default(tag)
            .with_size(Size::px(BTN_W, BTN_H))
            .with_label_font_size_px(14),
    )
}

/// The rename field's paint style — the M3 filled `TextField` sized to fill
/// the list row it replaces (`width` × `pitch` from the pane).
fn rename_field_style(width: u32, pitch: u32) -> tf_paint::TextFieldStyle {
    tf_paint::TextFieldStyle {
        field_w: width,
        field_h: pitch,
        ..tf_paint::TextFieldStyle::m3_filled()
    }
}

/// Cached posture for the paint fn: `[newdir, newfile, rename, delete]`
/// states, then the rename field's interaction + caret.
type FmViewState = (
    ButtonState,
    ButtonState,
    ButtonState,
    ButtonState,
    TextFieldState,
    u32,
);

/// view-fn (§6.3): pure sync mapping `(postures) -> Scene`, reading the
/// shared [`DirectoryState`] + the `renaming` flag so a create / delete /
/// rename / navigate / edit-toggle repaints.
// R1026 — rustfmt's reflow pushed this example view past too_many_lines (100).
#[allow(clippy::trivially_copy_pass_by_ref, clippy::too_many_lines)]
fn view(state: FmViewState, _frame: &Frame) -> Scene {
    let (newdir_state, newfile_state, rename_state_btn, delete_state, tf_interaction, tf_caret) =
        state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let dir = directory();
    let scroll = use_scroll_state(SCROLL_KEY);
    let has_selection = dir.selected().is_some();
    let renaming = use_renaming().get();
    let item_count = dir.entries().len();

    // ── toolbar ──────────────────────────────────────────────────
    let newdir = toolbar_button(
        NEWDIR_TAG,
        "New Folder",
        newdir_state,
        NEWDIR_HOVER_KEY,
        &theme,
    );
    let newfile = toolbar_button(
        NEWFILE_TAG,
        "New File",
        newfile_state,
        NEWFILE_HOVER_KEY,
        &theme,
    );
    // Rename + Delete gated on a selection (the R788 OK-gate pattern).
    let rename_posture = if has_selection {
        rename_state_btn
    } else {
        ButtonState::Disabled
    };
    let rename_btn = toolbar_button(
        RENAME_TAG,
        "Rename",
        rename_posture,
        RENAME_HOVER_KEY,
        &theme,
    );
    let delete_posture = if has_selection {
        delete_state
    } else {
        ButtonState::Disabled
    };
    let delete = toolbar_button(
        DELETE_TAG,
        "Delete",
        delete_posture,
        DELETE_HOVER_KEY,
        &theme,
    );
    let toolbar = Scene::Container(
        ContainerNode::new(vec![newdir, newfile, rename_btn, delete])
            .with_tag("fm_toolbar")
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(6),
            ),
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
    let editing = editing_index.map(|index| EditingRow {
        index,
        build: &build_field,
    });
    let pane = file_browser_pane(
        DIR_TAG,
        &dir,
        &scroll,
        &theme,
        FileBrowserMetrics {
            list_width: LIST_W,
            list_height: LIST_H,
            row_pitch: ROW_PITCH,
            overscan: OVERSCAN,
            focusable: true,
        },
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
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_tag("fm_status"),
    );

    Scene::Container(
        ContainerNode::new(vec![toolbar, pane, status])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(8)
                    .with_padding(Rect::new(12, 12, 12, 12)),
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
                        .attach_clipboard(use_app_clipboard(RENAME_TF_TAG))
                        // R793 — commit the rename when focus leaves the field.
                        .with_blur_intent(),
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
                    commit_rename(true);
                    return true;
                }
                "Escape" => {
                    cancel_rename();
                    return true;
                }
                _ => {
                    return pinion_core::forward_key_to_field(scene, RENAME_TF_TAG, key, modifiers);
                }
            }
        }
        // R792 — while the file list owns focus, arrows move the roving
        // cursor (the shell rings the active row) and Enter activates it
        // (navigate a folder / pick a file). The `dir_nav_key` controller
        // reuses the shared `clamp_nav` policy + scroll-into-view.
        if focused == Some(DIR_TAG) {
            return dir_nav_key(
                &directory(),
                &use_scroll_state(SCROLL_KEY),
                focused,
                DIR_TAG,
                key,
                ROW_PITCH,
            );
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
            // R793 — commit-on-blur: the rename field lost focus (a click
            // elsewhere) while editing → save the rename without restoring
            // focus (the click already moved it). The `renaming` gate makes
            // the post-commit / post-cancel blur (focus restored to the
            // Rename button) a no-op, so only a genuine click-away commits.
            "fm_rename_tf.blur" => {
                if use_renaming().get() {
                    commit_rename(false);
                }
            }
            "fm_delete.click" => {
                dir.delete_selected();
            }
            _ => {}
        }
        Vec::new()
    }

    fn fmt_state_log(state: &FmViewState) -> String {
        format!(
            "newdir={:?} newfile={:?} rename={:?} delete={:?}",
            state.0, state.1, state.2, state.3
        )
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

        let rename_posture = if has_selection {
            state.2
        } else {
            ButtonState::Disabled
        };
        let delete_posture = if has_selection {
            state.3
        } else {
            ButtonState::Disabled
        };
        let mut nodes = vec![
            AccessNode::new(NEWDIR_TAG, AriaRole::Button)
                .with_state(button_a11y_state(state.0, focused == Some(NEWDIR_TAG))),
            AccessNode::new(NEWFILE_TAG, AriaRole::Button)
                .with_state(button_a11y_state(state.1, focused == Some(NEWFILE_TAG))),
            AccessNode::new(RENAME_TAG, AriaRole::Button).with_state(button_a11y_state(
                rename_posture,
                focused == Some(RENAME_TAG),
            )),
            AccessNode::new(DELETE_TAG, AriaRole::Button).with_state(button_a11y_state(
                delete_posture,
                focused == Some(DELETE_TAG),
            )),
        ];
        nodes.extend(windowed_list_nodes_selected(
            DIR_TAG,
            "Files",
            u32::try_from(count).unwrap_or(u32::MAX),
            &window,
            dir.selected_index(),
        ));
        // R792 — when the list owns focus, the roving cursor row is the
        // `aria-activedescendant`; mark its windowed item focused (the
        // datepicker active-cell precedent). `cursor` is the effective row,
        // so it is in-window after a navigation (which scrolls it in).
        if focused == Some(DIR_TAG) {
            if let Some(cursor) = dir.cursor() {
                let tag = format!("{DIR_TAG}#{cursor}");
                if let Some(node) = nodes.iter_mut().find(|n| n.tag == tag) {
                    node.state.focused = true;
                }
            }
        }
        if renaming {
            nodes.push(tf_paint::text_field_a11y_node(
                RENAME_TF_TAG,
                rename_state().text(),
                state.4,
                focused == Some(RENAME_TF_TAG),
            ));
        }
        nodes
    }

    /// R792 §5.40 — composite focus model (the datepicker / listbox roving
    /// precedent). When the file list owns shell focus, focus stays on the
    /// list container (`DIR_TAG`) and the roving cursor row is reported as
    /// the `aria-activedescendant` — so the shell's focus ring frames the
    /// active row, not the whole list. Every other focused tag (a toolbar
    /// button, the rename field) passes through atomically.
    fn access_focus_target(state: &FmViewState, focused: Option<&str>) -> Option<AccessFocus> {
        let _ = state;
        if focused == Some(DIR_TAG) {
            if let Some(cursor) = directory().cursor() {
                return Some(AccessFocus::composite(
                    DIR_TAG,
                    format!("{DIR_TAG}#{cursor}"),
                ));
            }
        }
        focused.map(AccessFocus::atomic)
    }

    /// R792 §5.40 — composite child action dispatch. An AT `Click` /
    /// `Default` on a file row (`"{DIR_TAG}#<i>"`) or the parent affordance
    /// (`"{DIR_TAG}#up"`) routes through the same `DirectoryExternal` "send"
    /// wire a pointer click drives (`"<sub>:KeyboardActivate"`), so AT
    /// activation navigates a folder / picks a file exactly as the mouse and
    /// keyboard do. Other actions decline so the shell keeps its fallback.
    fn access_child_invoke(
        scene: &mut Scene,
        parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        if parent_tag != DIR_TAG {
            return false;
        }
        if !matches!(action, AccessAction::Click | AccessAction::Default) {
            return false;
        }
        let Some(node) = scene.find_external_with_tag_mut(DIR_TAG) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        intro
            .invoke(
                "send",
                IntrospectValue::Text(format!("{sub_tag}:KeyboardActivate")),
            )
            .is_ok()
    }
}

impl WidgetView for FileManagerView {
    type Renderer = HelloFileManagerRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
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
            state.4,
            state.5,
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
            TextFieldState::Idle,
            0,
        )
    }

    fn intent(tag: &str) -> pinion_core::Intent {
        pinion_core::Intent::new_owned(
            tag.to_string(),
            pinion_core::external::IntrospectValue::Null,
        )
    }

    #[test]
    fn r789_unique_name_avoids_collisions() {
        let entries = vec![DirEntry::dir("New Folder"), DirEntry::file("untitled.txt")];
        assert_eq!(unique_name(&entries, "New Folder", ""), "New Folder 2");
        assert_eq!(unique_name(&entries, "untitled", ".txt"), "untitled 2.txt");
        assert_eq!(
            unique_name(&entries, "src", ""),
            "src",
            "free base used verbatim"
        );
    }

    #[test]
    fn r789_new_folder_click_creates_and_lists() {
        Owner::new().run(|| {
            let _ = FileManagerView::update(idle(), &intent("fm_newdir.click"));
            let names: Vec<String> = directory()
                .entries()
                .iter()
                .map(|e| e.name.clone())
                .collect();
            assert!(
                names.contains(&"New Folder".to_string()),
                "new folder appears: {names:?}"
            );
            let _ = FileManagerView::update(idle(), &intent("fm_newdir.click"));
            let names: Vec<String> = directory()
                .entries()
                .iter()
                .map(|e| e.name.clone())
                .collect();
            assert!(
                names.contains(&"New Folder 2".to_string()),
                "second click = New Folder 2"
            );
        });
    }

    #[test]
    fn r789_new_file_click_creates() {
        Owner::new().run(|| {
            let _ = FileManagerView::update(idle(), &intent("fm_newfile.click"));
            assert!(
                directory()
                    .entries()
                    .iter()
                    .any(|e| e.name == "untitled.txt" && !e.is_dir),
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
            assert!(
                !dir.entries().iter().any(|e| e.name == "README.md"),
                "README.md removed"
            );
            assert_eq!(dir.selected(), None, "selection cleared");
            let before = dir.entries().len();
            let _ = FileManagerView::update(idle(), &intent("fm_delete.click"));
            assert_eq!(
                dir.entries().len(),
                before,
                "delete with no selection is a no-op"
            );
        });
    }

    #[test]
    fn r789_rename_delete_disabled_without_selection_enabled_with() {
        Owner::new().run(|| {
            let nodes = FileManagerView::access_node(&idle(), None);
            let del = nodes
                .iter()
                .find(|n| n.tag == DELETE_TAG)
                .expect("delete node");
            let ren = nodes
                .iter()
                .find(|n| n.tag == RENAME_TAG)
                .expect("rename node");
            assert!(del.state.disabled, "Delete aria-disabled with no selection");
            assert!(ren.state.disabled, "Rename aria-disabled with no selection");
            directory().select("Cargo.toml");
            let nodes = FileManagerView::access_node(&idle(), None);
            let del = nodes
                .iter()
                .find(|n| n.tag == DELETE_TAG)
                .expect("delete node");
            let ren = nodes
                .iter()
                .find(|n| n.tag == RENAME_TAG)
                .expect("rename node");
            assert!(
                !del.state.disabled,
                "Delete enabled once an entry is selected"
            );
            assert!(
                !ren.state.disabled,
                "Rename enabled once an entry is selected"
            );
        });
    }

    // ----- R791 inline rename -----

    #[test]
    fn r791_rename_click_enters_edit_mode_prefilled_and_focuses() {
        Owner::new().run(|| {
            let _ = pinion_core::focus_request::drain();
            // No selection: Rename is a no-op.
            let _ = FileManagerView::update(idle(), &intent("fm_rename.click"));
            assert!(
                !use_renaming().get(),
                "rename with no selection does nothing"
            );
            // Select, then Rename → edit mode, field pre-filled, focus queued.
            directory().select("README.md");
            let _ = FileManagerView::update(idle(), &intent("fm_rename.click"));
            assert!(use_renaming().get(), "Rename enters edit mode");
            assert_eq!(
                rename_state().text(),
                "README.md",
                "field pre-filled with the current name"
            );
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
            assert!(
                dir.entries().iter().any(|e| e.name == "NOTES.md"),
                "file renamed"
            );
            assert!(
                !dir.entries().iter().any(|e| e.name == "README.md"),
                "old name gone"
            );
            assert_eq!(
                dir.selected(),
                Some("/proj/NOTES.md".to_string()),
                "selection follows"
            );
            assert_eq!(rename_state().text(), "", "field wiped on exit");
            assert_eq!(
                pinion_core::focus_request::drain(),
                Some(RENAME_TAG.to_string()),
                "focus restored"
            );
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
            assert!(
                dir.entries().iter().any(|e| e.name == "README.md"),
                "name unchanged"
            );
            assert!(
                !dir.entries().iter().any(|e| e.name == "WONT.md"),
                "nothing renamed"
            );
        });
    }

    #[test]
    fn r791_blank_name_commit_cancels() {
        Owner::new().run(|| {
            let dir = directory();
            dir.select("README.md");
            enter_rename();
            rename_state().set_text("   ".to_owned());
            commit_rename(true);
            assert!(!use_renaming().get(), "blank commit exits edit mode");
            assert!(
                dir.entries().iter().any(|e| e.name == "README.md"),
                "blank name does not rename"
            );
        });
    }

    #[test]
    fn r793_blur_commits_rename_without_restoring_focus() {
        Owner::new().run(|| {
            let _ = pinion_core::focus_request::drain();
            let dir = directory();
            dir.select("README.md");
            enter_rename();
            rename_state().set_text("NOTES.md".to_owned());
            let _ = pinion_core::focus_request::drain(); // consume enter_rename's request
            // The rename field lost focus → the "blur" intent commits the edit.
            let _ = FileManagerView::update(idle(), &intent("fm_rename_tf.blur"));
            assert!(!use_renaming().get(), "blur exits edit mode");
            assert!(
                dir.entries().iter().any(|e| e.name == "NOTES.md"),
                "rename committed on blur"
            );
            assert!(
                !dir.entries().iter().any(|e| e.name == "README.md"),
                "old name gone"
            );
            assert_eq!(
                pinion_core::focus_request::drain(),
                None,
                "commit-on-blur does not steal focus back (the click already moved it)",
            );
        });
    }

    #[test]
    fn r793_blur_without_an_edit_in_progress_is_inert() {
        Owner::new().run(|| {
            // The post-commit / post-cancel blur (renaming already false) is a
            // no-op — only a genuine click-away while editing commits.
            let dir = directory();
            let before = dir.entries().len();
            let _ = FileManagerView::update(idle(), &intent("fm_rename_tf.blur"));
            assert!(!use_renaming().get());
            assert_eq!(
                dir.entries().len(),
                before,
                "blur with no edit in progress does nothing"
            );
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
            assert!(
                scene.contains_tag(RENAME_TF_TAG),
                "the rename field paints in the row"
            );
            // The selected row's plain file_row is replaced by the field.
            let idx = dir.selected_index().expect("a selection has a row");
            assert!(
                !scene.contains_tag(&format!("{DIR_TAG}#{idx}")),
                "selected file_row replaced"
            );
            // Another row stays a plain file_row.
            assert!(
                scene.contains_tag(&format!("{DIR_TAG}#0")) || idx == 0,
                "non-edited rows remain"
            );
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
            let tb = nodes
                .iter()
                .find(|n| n.tag == RENAME_TF_TAG)
                .expect("textbox node");
            assert_eq!(
                tb.role,
                AriaRole::TextInput,
                "the rename field is announced as a textbox"
            );
        });
    }

    // ----- R792 keyboard navigation over the file list -----

    #[test]
    fn r792_apply_key_arrows_move_the_roving_cursor() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let dir = directory();
            assert_eq!(
                dir.cursor(),
                Some(0),
                "boot cursor defaults to the first row"
            );
            // A key while a toolbar button owns focus does not move the list.
            assert!(!FileManagerView::apply_key(
                &mut scene,
                Some(NEWDIR_TAG),
                "ArrowDown",
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(dir.cursor(), Some(0), "unfocused list ignores the arrow");
            // Focused on the list → ArrowDown advances the cursor.
            assert!(FileManagerView::apply_key(
                &mut scene,
                Some(DIR_TAG),
                "ArrowDown",
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(dir.cursor(), Some(1));
            assert!(FileManagerView::apply_key(
                &mut scene,
                Some(DIR_TAG),
                "End",
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(dir.cursor(), Some(3), "End jumps to the last /proj row");
        });
    }

    #[test]
    fn r792_enter_activates_the_cursor_row() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            // /proj sorts dirs-first, case-insensitive: row 0 = assets (dir).
            let dir = directory();
            assert!(FileManagerView::apply_key(
                &mut scene,
                Some(DIR_TAG),
                "Enter",
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(
                dir.cwd(),
                "/proj/assets",
                "Enter on a folder row navigates into it"
            );
            // In /proj/assets the cursor defaults to row 0 (logo.png, a file);
            // Enter picks it.
            assert!(FileManagerView::apply_key(
                &mut scene,
                Some(DIR_TAG),
                "Enter",
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(
                dir.selected(),
                Some("/proj/assets/logo.png".to_string()),
                "Enter on a file picks it",
            );
        });
    }

    #[test]
    fn r792_access_focus_target_reports_the_cursor_as_active_descendant() {
        Owner::new().run(|| {
            directory().set_cursor(Some(1));
            let target = FileManagerView::access_focus_target(&idle(), Some(DIR_TAG))
                .expect("the focused list reports a focus target");
            assert_eq!(
                target.focus_tag, DIR_TAG,
                "focus stays on the list container"
            );
            assert_eq!(
                target.active_descendant.as_deref(),
                Some("fb_dir#1"),
                "the cursor row is the aria-activedescendant",
            );
            // A toolbar button passes through atomically (no descendant).
            let btn = FileManagerView::access_focus_target(&idle(), Some(NEWDIR_TAG))
                .expect("button focus target");
            assert_eq!(btn.active_descendant, None);
        });
    }

    #[test]
    fn r792_a11y_marks_the_cursor_row_focused() {
        Owner::new().run(|| {
            directory().set_cursor(Some(2));
            let nodes = FileManagerView::access_node(&idle(), Some(DIR_TAG));
            let cursor_row = nodes
                .iter()
                .find(|n| n.tag == "fb_dir#2")
                .expect("cursor row node");
            assert!(
                cursor_row.state.focused,
                "the cursor row is aria-active (focused)"
            );
            let other = nodes
                .iter()
                .find(|n| n.tag == "fb_dir#0")
                .expect("row 0 node");
            assert!(!other.state.focused, "a non-cursor row is not focused");
            // Unfocused list → no row is marked active.
            let unfocused = FileManagerView::access_node(&idle(), None);
            let row2 = unfocused
                .iter()
                .find(|n| n.tag == "fb_dir#2")
                .expect("row node");
            assert!(
                !row2.state.focused,
                "no active row when the list is not focused"
            );
        });
    }

    #[test]
    fn r792_access_child_invoke_activates_an_at_clicked_row() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            let dir = directory();
            // AT Click on row 0 (assets dir, dirs-first sort) navigates into it.
            assert!(FileManagerView::access_child_invoke(
                &mut scene,
                DIR_TAG,
                "0",
                AccessAction::Click
            ));
            assert_eq!(dir.cwd(), "/proj/assets");
            // A non-list parent declines (shell keeps its fallback chain).
            assert!(!FileManagerView::access_child_invoke(
                &mut scene,
                NEWDIR_TAG,
                "x",
                AccessAction::Click
            ));
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
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag),
            ));
        }
        Scene::Container(ContainerNode::new(children))
    }
}
