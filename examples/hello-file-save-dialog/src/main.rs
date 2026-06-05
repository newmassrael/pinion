// R790 §5.15 §5.16 §5.38 §5.39 — example bindings tolerate looser
// doc-markdown lints than substrate crates; the narrative carries many
// proper-noun identifiers (TextFieldExternal, DirectoryExternal,
// ModalState, WAI-ARIA, …).
#![allow(clippy::doc_markdown)]

//! `hello-file-save-dialog` — R790 §5.15 §5.16 §5.38 §5.39 **modal
//! file-save dialog**: the scene-graph-native, AI-introspectable peer of
//! an OS file-save ("Save As") dialog, assembled from four substrates
//! that already exist —
//!
//! - the R787 **own-rendered file browser** ([`DirectoryExternal`] over
//!   the [`Directory`] trait): browse into the target folder inside
//!   pinion's own scene tree, every entry a paint node;
//! - the R56.1 **`TextField`** ([`TextFieldExternal`] +
//!   [`use_text_edit_state`] / [`use_caret_blink`] / `use_app_clipboard`):
//!   the **filename** field — a *modal member*, so the focus trap Tab-
//!   cycles into it and it carries a blinking caret, click-to-position,
//!   IME, and Ctrl/Cmd+V paste like any other text input;
//! - the R693 **modal dialog chrome** ([`view_dialog`]) carrying the
//!   browser + filename field in its R788 [`DialogContent::body`] slot;
//!   and
//! - the R789 **`Directory` write surface** ([`DirectoryState::create_file`]):
//!   **Save** writes the typed name into the current directory.
//!
//! ## Why this binding exists
//!
//! The save flow is the other half of "Open a file" (R788). Every pro
//! tool's `File → Save As` routes through one own-rendered picker — a
//! direct step toward the northern-star "Unreal-class editor self-hosted
//! in pinion". Because the picker is a pinion scene (not a native window
//! pinion does not own — invisible to `scene/query`, unverifiable on a
//! headless Linux box, see `[[native-menu-macos-windows-only-verify]]`),
//! an AI agent can drive the entire save flow over RPC: browse to a
//! folder, type a filename through the field's `invoke("key", …)`
//! channel, and confirm, then read the written path back as data
//! (§2 #2 / #7).
//!
//! ## The filename field is a modal member (the round's core work)
//!
//! `TextField` has, until now, only ever been a *primary* widget (its
//! own [`WidgetView`] with all the shell hooks). Here it is an **extra
//! external** living inside the modal (the same shape todomvc's in-place
//! row editor uses): the dialog's [`apply_key`](FileSaveView::apply_key)
//! / [`apply_composition`](FileSaveView::apply_composition) /
//! [`ime_caret_rect`](FileSaveView::ime_caret_rect) route to the
//! filename field whenever it owns focus, the focus trap lists it as a
//! member (auto-focused first on open so you can type immediately), and
//! the field paints a live caret via its attached [`use_caret_blink`].
//!
//! ## Save gated on a name; clicking a file populates it
//!
//! A save dialog's confirm button is disabled until a filename is typed,
//! so the view paints **Save** with the [`ButtonState::Disabled`] posture
//! (and `aria-disabled`) while the (trimmed) field is empty, and the
//! reducer no-ops a Save with no name. Clicking an **existing file** in
//! the browser mirrors its basename into the filename field (the
//! macOS / GNOME "click a file to overwrite it" convention) — driven by
//! a reactive [`Effect`] subscribed to the directory selection, the
//! shared-state bridge `[[reactive-holder-for-shared-external-view-state]]`
//! endorses.
//!
//! ## Confirm / dismiss paths
//!
//! - **Save** — `"savefile_save.click"` (or `Enter` in the name field)
//!   with a non-blank name: [`create_file`](DirectoryState::create_file)
//!   in the current folder, record the written `'/'`-path as `saved`,
//!   and close;
//! - **Cancel** — `"savefile_cancel.click"`: close without writing;
//! - **Escape** — routed to [`apply_key`](FileSaveView) while the trap is
//!   active: dismisses as a cancel.
//!
//! Each open resets the browser to the root and pre-fills the field with
//! a default name (`untitled.txt`, caret at end), keeping the flow
//! deterministic.

use pinion_a11y::{windowed_list_nodes_selected, AccessNode, AriaRole, WidgetA11y};
use pinion_core::directory::{Directory, InMemoryDirectory};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::reactive::{batch, Effect, Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::widgets::caret_blink::use_caret_blink;
use pinion_core::widgets::file_browser::{use_directory_state, DirectoryExternal, DirectoryState};
use pinion_core::widgets::modal::{use_modal, ModalState};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::text_edit::use_text_edit_state;
use pinion_core::widgets::text_field::{TextFieldExternal, TextFieldState};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{DirEntry, Frame, Scene, WidgetCore, WidgetStateName};
use pinion_platform_clipboard::use_app_clipboard;
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_text::CaretRect;
use pinion_widget_paint::button::{
    button_a11y_state, button_scene, read_button_focused, read_button_state, ButtonColors,
    ButtonStyle,
};
use pinion_widget_paint::dialog::{view_dialog, DialogContent, DialogStyle};
use pinion_widget_paint::file_browser::{file_browser_pane, FileBrowserMetrics};
use pinion_widget_paint::text_field as tf_paint;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloFileSaveDialogRenderer, HelloFileSaveDialogRendererError);

const WIN_W: u32 = 600;
const WIN_H: u32 = 640;
const THEME_TAG: &str = "app";

/// Trigger button (the primary external). Opens the save dialog.
const TRIGGER_TAG: &str = "save_file";
/// The [`DirectoryExternal`] / a11y `list` tag. Rows are `fb_dir#<i>`, the
/// parent affordance is `fb_dir#up` — all route here (R51.42 composite).
const DIR_TAG: &str = "fb_dir";
/// The filename [`TextField`] — an extra external + modal member.
const FILENAME_TAG: &str = "savefile_name";
/// Affirmative action button — extra external. Writes the file.
const SAVE_TAG: &str = "savefile_save";
/// Cancel action button — extra external.
const CANCEL_TAG: &str = "savefile_cancel";
/// Backdrop tag (no external — blocks + swallows background clicks).
const SCRIM_TAG: &str = "savefile_scrim";
/// Dialog panel tag (queryable via `scene/snapshot`).
const PANEL_TAG: &str = "savefile_panel";

/// `Owner::cache` key for the shared [`ModalState`] open-lifecycle.
const MODAL_KEY: &str = "hello_file_save.modal";
/// `Owner::cache` key for the `Signal<Option<String>>` saved file path.
const SAVED_KEY: &str = "hello_file_save.saved";
/// `Owner::cache` key for the filename-selection sync [`Effect`].
const SYNC_KEY: &str = "hello_file_save.filename_sync";
/// Vertical body scroll for the virtualized entry list.
const SCROLL_KEY: &str = "savefile_scroll";
/// Where the picker opens each time.
const ROOT_DIR: &str = "/proj";
/// The default filename pre-filled on each open.
const DEFAULT_NAME: &str = "untitled.txt";

const TRIGGER_HOVER_KEY: &str = "hello_file_save.trigger_hover";
const SAVE_HOVER_KEY: &str = "hello_file_save.save_hover";
const CANCEL_HOVER_KEY: &str = "hello_file_save.cancel_hover";

const TRIGGER_W: u32 = 200;
const TRIGGER_H: u32 = 56;
const ACTION_W: u32 = 110;
const ACTION_H: u32 = 44;

/// Dialog panel width; the browser list + filename field sit inside its
/// padding.
const PANEL_W: u32 = 460;
/// Per-row vertical slot (px) + the entry-list viewport height (7 rows;
/// the filename field takes the space the open-dialog gave to an 8th).
const ROW_PITCH: u32 = 32;
const LIST_W: u32 = PANEL_W - 48; // panel_width − 2 × m3 panel_padding (24)
const LIST_H: u32 = 7 * ROW_PITCH;
const OVERSCAN: usize = 3;

/// The dialog's focusable controls, in Tab order. The filename field is
/// first so it is auto-focused on open (type the name immediately); Tab
/// moves Name → Cancel → Save and wraps. The file rows are pointer/RPC-
/// driven (R787), not Tab stops.
fn dialog_members() -> Vec<String> {
    vec![FILENAME_TAG.to_string(), CANCEL_TAG.to_string(), SAVE_TAG.to_string()]
}

/// The R788 modal panel style: the M3 default widened to hold the body.
fn dialog_style() -> DialogStyle {
    DialogStyle { panel_width: PANEL_W, ..DialogStyle::m3_default() }
}

/// The filename field paint style — the M3 filled `TextField` widened to
/// span the dialog body so it aligns with the browser pane above it.
fn filename_style() -> tf_paint::TextFieldStyle {
    tf_paint::TextFieldStyle { field_w: LIST_W, ..tf_paint::TextFieldStyle::m3_filled() }
}

/// The shared [`ModalState`] (R788 lifted open-lifecycle SSOT). One `Rc`
/// across the view fn, the reducer, and `access_node`.
fn modal() -> Rc<ModalState> {
    use_modal(MODAL_KEY)
}

/// `Owner::cache`-keyed hook: the saved file path — `None` until the user
/// confirms a write with Save, then its full `'/'`-path.
fn use_saved() -> Rc<Signal<Option<String>>> {
    Owner::current()
        .expect("use_saved requires an active Owner scope")
        .cache(SAVED_KEY, || Signal::new(None::<String>))
}

/// The shared [`DirectoryState`] the browser pane reads and the
/// [`DirectoryExternal`] mutates (one `Rc` via the cache key `DIR_TAG`).
fn directory() -> Rc<DirectoryState> {
    use_directory_state(DIR_TAG, seed_directory, || ROOT_DIR.to_string())
}

/// The filename field's reactive text store (shared between the
/// `TextFieldExternal::attach_state` and every read site via the
/// `FILENAME_TAG` cache key).
fn filename_state() -> Rc<pinion_core::widgets::text_edit::TextEditState> {
    use_text_edit_state(FILENAME_TAG)
}

/// The last path component of a `'/'`-path (`"/proj/Cargo.toml"` →
/// `"Cargo.toml"`). Directories are never selected, so this only ever
/// runs on a selected file's path.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Seed the synthetic sample-project tree the picker walks. Deterministic
/// [`InMemoryDirectory`] backing (the `Storage`/`InMemoryStorage`
/// precedent) so the demo + pixel guard see a fixed tree; the real-fs
/// `FsDirectory` is unit-tested in `pinion-platform-storage`.
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
    d.insert("/proj/src", vec![DirEntry::file("main.rs"), DirEntry::file("lib.rs")]);
    d.insert("/proj/assets", vec![DirEntry::file("logo.png"), DirEntry::file("icon.svg")]);
    Rc::new(d)
}

/// Marker holding the filename-selection sync [`Effect`] for the
/// application lifetime (R665 effect-retention).
struct FilenameSyncMarker {
    _effect: Effect,
}

/// Install (once per `Owner`) the reactive bridge that mirrors the
/// browser's *file* selection into the filename field. Subscribed only to
/// the directory selection (never the field text — reading the text here
/// would make the user's own typing re-trigger the write), so it fires
/// exactly when a row click selects a file, writing its basename + caret-
/// to-end. A directory navigation clears the selection (`None`) and is
/// skipped, leaving whatever the user typed intact.
fn use_filename_sync() -> Rc<FilenameSyncMarker> {
    // Pre-resolve dependent cache slots before entering the cache factory
    // (the `Owner::cache` borrow guard forbids nested resolution —
    // [[owner-cache-no-nested-factory]]).
    let dir = directory();
    let edit = filename_state();
    let owner = Owner::current().expect("use_filename_sync requires an active Owner scope");
    let owner_for_effect = owner.clone();
    owner.cache(SYNC_KEY, move || {
        let effect = Effect::new(&owner_for_effect, move || {
            if let Some(path) = dir.selected() {
                let name = basename(&path).to_owned();
                batch(|| {
                    edit.set_text(name.clone());
                    edit.set_caret(name.len());
                });
            }
        });
        FilenameSyncMarker { _effect: effect }
    })
}

/// Open the dialog: reset the browser to the root, pre-fill the default
/// filename (caret at end), and install the modal trap.
fn open_dialog() {
    directory().open_dir(ROOT_DIR);
    let edit = filename_state();
    batch(|| {
        edit.set_text(DEFAULT_NAME.to_owned());
        edit.set_caret(DEFAULT_NAME.len());
    });
    modal().open(dialog_members());
}

/// Confirm the save: write the (trimmed, non-blank) filename into the
/// current directory, record the resulting `'/'`-path as `saved`, and
/// close. A no-op when the name is blank (the Save gate — the button
/// paints disabled, but a keyboard activation is guarded here too) or
/// when the backing rejects the write (read-only). The outcome write +
/// modal close [`batch`] so the view re-renders once.
fn confirm() {
    let name = filename_state().text();
    let name = name.trim().to_owned();
    if name.is_empty() {
        return;
    }
    let dir = directory();
    // Create the file in the current folder (a no-op when the name is
    // already taken — the Save-As overwrite case). `select` then returns
    // its full path via the `DirectoryState` join SSOT; `None` only when
    // the backing could neither create nor list it (read-only).
    dir.create_file(&name);
    let Some(path) = dir.select(&name) else {
        return;
    };
    let m = modal();
    batch(|| {
        use_saved().set(Some(path));
        m.close();
    });
}

/// The browser pane that fills the upper dialog body: the lifted R789.1
/// [`file_browser_pane`] (breadcrumb + virtualized `file_row` list) sized
/// to the dialog. Reads the shared [`DirectoryState`] so a navigate /
/// select repaints.
fn browser_pane(
    dir: &DirectoryState,
    scroll: &Rc<pinion_core::widgets::scroll::ScrollState>,
    theme: &Theme,
) -> Scene {
    file_browser_pane(
        DIR_TAG,
        dir,
        scroll,
        theme,
        FileBrowserMetrics { list_width: LIST_W, list_height: LIST_H, row_pitch: ROW_PITCH, overscan: OVERSCAN },
        None,
    )
}

/// The lower dialog body: a `"Save as:"` label over the filename
/// [`TextField`] (the R56.1 [`tf_paint::view_field`] substrate, sized to
/// span the pane). The field carries `FILENAME_TAG`, so the input router,
/// the IME caret-rect walk, and the focus ring all resolve it.
fn filename_section(interaction: TextFieldState, caret_byte: u32, theme: &Theme) -> Scene {
    let label = Scene::Text(TextNode::styled(
        "Save as:",
        Rect::default(),
        TextStyle::new().with_size_px(14).with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let field = tf_paint::view_field(
        FILENAME_TAG,
        interaction,
        caret_byte,
        theme,
        &filename_style(),
        "File name",
    );
    Scene::Container(
        ContainerNode::new(vec![label, field]).with_layout(
            LayoutStyle::new().flex(FlexDirection::Column).with_align_items(AlignItems::Start).with_gap(4),
        ),
    )
}

/// Render one action button via the [`pinion_widget_paint::button`]
/// substrate (the 2-consumer-with-hello-dialog opinionated default; the
/// mechanical hover + view_button pairing is the lifted core).
fn action_button(
    tag: &'static str,
    label: &str,
    state: ButtonState,
    focused: bool,
    hover_key: &'static str,
    colors: &ButtonColors,
) -> Scene {
    button_scene(
        label,
        state,
        focused,
        hover_key,
        colors,
        &ButtonStyle::m3_default(tag)
            .with_size(Size::px(ACTION_W, ACTION_H))
            .with_label_font_size_px(16),
    )
}

/// Cached posture for the paint fn: the three button [`ButtonState`]s
/// plus their keyboard-focus flags (`[trigger, save, cancel]`), then the
/// filename field's interaction state + caret byte offset. `open` /
/// `saved` / browse state / field text live in signals the owner-scoped
/// view + access_node read directly.
type FileSaveViewState = (ButtonState, ButtonState, ButtonState, [bool; 3], TextFieldState, u32);

/// view-fn (§6.3): pure sync mapping `(postures) -> Scene`, reading the
/// reactive `modal` open flag, `saved` path, browse state, and filename
/// text. When open, the dialog overlay (scrim + panel + browser pane +
/// filename field) is pushed **last** so it paints over the trigger.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: FileSaveViewState, _frame: &Frame) -> Scene {
    let (trigger_state, save_state, cancel_state, focus, fname_interaction, fname_caret) = state;
    let [trigger_focused, save_focused, cancel_focused] = focus;
    let theme = use_theme(THEME_TAG).theme_animated();

    let trigger = button_scene(
        "Save file\u{2026}",
        trigger_state,
        trigger_focused,
        TRIGGER_HOVER_KEY,
        &ButtonColors::filled_tonal(&theme),
        &ButtonStyle::m3_default(TRIGGER_TAG)
            .with_size(Size::px(TRIGGER_W, TRIGGER_H))
            .with_label_font_size_px(16),
    );
    let saved = use_saved().get();
    let status = Scene::Text(TextNode::styled(
        match &saved {
            Some(p) => format!("Saved: {p}"),
            None => "Saved: (none)".to_string(),
        },
        Rect::default(),
        TextStyle::new().with_size_px(14).with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    let content = Scene::Container(
        ContainerNode::new(vec![trigger, status]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                .with_flex_grow(1.0)
                .with_gap(16),
        ),
    );

    let mut children = vec![content];
    if modal().is_open() {
        let dir = directory();
        let scroll = use_scroll_state(SCROLL_KEY);
        // Save gated on a non-blank filename (subscribes the view to the
        // field text, so typing toggles the posture live).
        let has_name = !filename_state().text().trim().is_empty();
        let pane = browser_pane(&dir, &scroll, &theme);
        let name_section = filename_section(fname_interaction, fname_caret, &theme);
        let body = Scene::Container(
            ContainerNode::new(vec![pane, name_section])
                .with_layout(LayoutStyle::new().flex(FlexDirection::Column).with_gap(12)),
        );

        // Action order matches `dialog_members()` (Cancel, then Save).
        let cancel = action_button(
            CANCEL_TAG,
            "Cancel",
            cancel_state,
            cancel_focused,
            CANCEL_HOVER_KEY,
            &ButtonColors::filled_tonal(&theme),
        );
        let save_posture = if has_name { save_state } else { ButtonState::Disabled };
        let save = action_button(
            SAVE_TAG,
            "Save",
            save_posture,
            save_focused,
            SAVE_HOVER_KEY,
            &ButtonColors::accent(&theme),
        );

        children.push(view_dialog(
            SCRIM_TAG,
            PANEL_TAG,
            DialogContent { title: "Save file", message: "", body: Some(body) },
            vec![cancel, save],
            (WIN_W, WIN_H),
            &theme,
            &dialog_style(),
        ));
    }

    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Start),
            ),
    )
}

struct FileSaveView;

impl WidgetCore for FileSaveView {
    type State = FileSaveViewState;
    // The trigger + action buttons are driven by the pointer router +
    // ARIA Enter/Space; the filename field's keys flow through
    // `apply_key`. No keybinding-channel enum events reach `event_name`.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    /// The [`DirectoryExternal`] (browse + select), the filename
    /// [`TextFieldExternal`] (its full interaction state machine over the
    /// shared `FILENAME_TAG` stores), and the two action
    /// [`ButtonExternal`]s. The filename-selection sync [`Effect`] is
    /// installed here too (resolved before the externals so its
    /// dependent caches pre-exist).
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let _sync = use_filename_sync();
        vec![
            ExtraExternal::new(DIR_TAG, Box::new(DirectoryExternal::new(directory()))),
            ExtraExternal::new(
                FILENAME_TAG,
                Box::new(
                    TextFieldExternal::new()
                        .attach_state(filename_state())
                        .attach_blink(use_caret_blink(FILENAME_TAG))
                        .attach_clipboard(use_app_clipboard(FILENAME_TAG)),
                ),
            ),
            ExtraExternal::new(SAVE_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(CANCEL_TAG, Box::new(ButtonExternal::new())),
        ]
    }

    fn tag() -> &'static str {
        TRIGGER_TAG
    }

    fn read_state(scene: &Scene) -> FileSaveViewState {
        let (fname_interaction, fname_caret) =
            tf_paint::read_text_field_state(scene, FILENAME_TAG);
        (
            read_button_state(scene, TRIGGER_TAG),
            read_button_state(scene, SAVE_TAG),
            read_button_state(scene, CANCEL_TAG),
            [
                read_button_focused(scene, TRIGGER_TAG),
                read_button_focused(scene, SAVE_TAG),
                read_button_focused(scene, CANCEL_TAG),
            ],
            fname_interaction,
            fname_caret,
        )
    }

    fn view(state: FileSaveViewState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-file-save-dialog (R790 §5.15 §5.16 §5.38 §5.39 modal save dialog)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// Only the trigger is a *static* tab stop. The filename field + the
    /// action buttons become focusable solely while the modal scope is up
    /// (`dialog_members()`).
    fn focusable_tags() -> Vec<&'static str> {
        vec![TRIGGER_TAG]
    }

    /// R790 §5.38 §5.39 — Escape dismisses the open dialog as a cancel.
    /// `Enter` in the filename field commits the Save (the dialog's
    /// default action). Otherwise route by focus: the filename field's
    /// keys forward to its `TextField` substrate (R764.1 SSOT), the action
    /// buttons activate through the shared ARIA helper.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: pinion_core::Modifiers,
    ) -> bool {
        if key == "Escape" {
            if modal().is_open() {
                modal().close();
                return true;
            }
            return false;
        }
        match focused {
            Some(FILENAME_TAG) => {
                if key == "Enter" {
                    // Enter in the name field = the dialog's default
                    // action (Save). `confirm` self-guards a blank name,
                    // and a single-line field has no use for a newline.
                    confirm();
                    return true;
                }
                tf_paint::forward_key_to_field(scene, FILENAME_TAG, key, modifiers)
            }
            Some(SAVE_TAG) => apply_aria_activate(scene, focused, key, SAVE_TAG),
            Some(CANCEL_TAG) => apply_aria_activate(scene, focused, key, CANCEL_TAG),
            _ => false,
        }
    }

    /// R790 §5.13 §5.38 — route IME composition to the filename field
    /// while it owns focus (the platform IME path + the AI-RPC path funnel
    /// through one `TextField` invoke).
    fn apply_composition(
        scene: &mut Scene,
        focused: Option<&str>,
        event: &pinion_core::CompositionEvent,
    ) -> bool {
        if focused != Some(FILENAME_TAG) {
            return false;
        }
        tf_paint::forward_composition_to_field(scene, FILENAME_TAG, event)
    }

    /// R790 §5.22 — X11 / Wayland canonical middle-click PRIMARY paste
    /// into the focused filename field. Funnels through the `TextField`
    /// `paste-primary` invoke (the lifted clipboard now forwards the
    /// selection-aware read, so PRIMARY actually reaches the field).
    fn apply_middle_click(
        scene: &mut Scene,
        focused: Option<&str>,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(FILENAME_TAG) {
            return false;
        }
        let Some(node) = scene.find_external_with_tag_mut(FILENAME_TAG) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        match intro.invoke("paste-primary", IntrospectValue::Null) {
            Ok(IntrospectValue::Bool(handled)) => handled,
            _ => false,
        }
    }

    /// R790 §5.39 — bridge the action buttons' `"<tag>.click"` intents
    /// into the [`ModalState`] + the `Directory` write. The browser's row
    /// clicks route to the [`DirectoryExternal`] directly, the filename
    /// field's keys to its `TextField`; only the trigger / Save / Cancel
    /// clicks reach here. Side-effect-only.
    fn update(
        _state: FileSaveViewState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            "save_file.click" => open_dialog(),
            "savefile_save.click" => confirm(),
            "savefile_cancel.click" => modal().close(),
            _ => {}
        }
        Vec::new()
    }

    fn fmt_state_log(state: &FileSaveViewState) -> String {
        format!(
            "trigger={:?} save={:?} cancel={:?} name={}",
            state.0,
            state.1,
            state.2,
            state.4.as_name(),
        )
    }
}

impl WidgetA11y for FileSaveView {
    /// R790 §5.40 — WAI-ARIA dialog tree. Closed: just the trigger
    /// [`AriaRole::Button`]. Open: the `aria-modal` [`AriaRole::Dialog`]
    /// root owning the file `list` (`windowed_list_nodes_selected`, R776),
    /// the filename [`AriaRole::TextInput`] (live value), and the two
    /// action buttons. Save carries `aria-disabled` until a name is typed,
    /// matching the painted gate. Names enrich from the paint scene.
    fn access_node(state: &FileSaveViewState, focused: Option<&str>) -> Vec<AccessNode> {
        if !modal().is_open() {
            return vec![AccessNode::new(TRIGGER_TAG, AriaRole::Button)
                .with_state(button_a11y_state(state.0, focused == Some(TRIGGER_TAG)))];
        }
        let dir = directory();
        let scroll = use_scroll_state(SCROLL_KEY);
        let count = dir.entries().len();
        let window = compute_visible_range(scroll.offset_y(), LIST_H, count, ROW_PITCH, OVERSCAN);
        let name = filename_state().text();
        let has_name = !name.trim().is_empty();

        let dialog = AccessNode::new(PANEL_TAG, AriaRole::Dialog)
            .with_modal()
            .with_child(DIR_TAG)
            .with_child(FILENAME_TAG)
            .with_child(CANCEL_TAG)
            .with_child(SAVE_TAG);
        let mut nodes = vec![dialog];
        nodes.extend(windowed_list_nodes_selected(
            DIR_TAG,
            "Files",
            u32::try_from(count).unwrap_or(u32::MAX),
            &window,
            dir.selected_index(),
        ));
        // The filename textbox (live value mirrors the field content) via
        // the R790 lifted SSOT shared by every text-field binding.
        nodes.push(tf_paint::text_field_a11y_node(
            FILENAME_TAG,
            name,
            state.4,
            focused == Some(FILENAME_TAG),
        ));
        // Save reflects the name gate (aria-disabled until a name typed).
        let save_posture = if has_name { state.1 } else { ButtonState::Disabled };
        nodes.push(
            AccessNode::new(CANCEL_TAG, AriaRole::Button)
                .with_state(button_a11y_state(state.2, focused == Some(CANCEL_TAG)))
                .with_position_in_set(1)
                .with_size_of_set(2),
        );
        nodes.push(
            AccessNode::new(SAVE_TAG, AriaRole::Button)
                .with_state(button_a11y_state(save_posture, focused == Some(SAVE_TAG)))
                .with_position_in_set(2)
                .with_size_of_set(2),
        );
        nodes
    }
}

impl WidgetView for FileSaveView {
    type Renderer = HelloFileSaveDialogRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }

    /// R790 §5.13 — publish the filename field's caret rect to the
    /// platform IME (so the candidate window tracks the caret) while the
    /// field owns focus. Mirrors hello-textfield's hook, scoped to the
    /// filename tag.
    fn ime_caret_rect(
        state: &FileSaveViewState,
        scene: &Scene,
        focused: Option<&str>,
    ) -> Option<CaretRect> {
        if focused != Some(FILENAME_TAG) {
            return None;
        }
        let (interaction, caret_byte) = (state.4, state.5);
        let field_rect = pinion_shell::rect_for_tag(scene, FILENAME_TAG)?;
        let theme = use_theme(THEME_TAG).theme_animated();
        Some(tf_paint::ime_caret_rect_for(
            FILENAME_TAG,
            interaction,
            caret_byte,
            field_rect,
            &theme,
            &filename_style(),
        ))
    }
}

fn main() {
    pinion_shell::run::<FileSaveView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AccessValue;
    use pinion_core::modal_scope_request::{self, ModalRequest};

    fn idle() -> FileSaveViewState {
        (
            ButtonState::Idle,
            ButtonState::Idle,
            ButtonState::Idle,
            [false; 3],
            TextFieldState::Idle,
            0,
        )
    }

    fn intent(tag: &str) -> pinion_core::Intent {
        pinion_core::Intent::new_owned(tag.to_string(), pinion_core::external::IntrospectValue::Null)
    }

    // ----- reducer + signals -----

    #[test]
    fn r790_trigger_opens_resets_browser_prefills_name_and_requests_modal() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            // Pre-navigate + dirty the field so we can prove open() resets.
            let dir = directory();
            dir.navigate("src");
            filename_state().set_text("stale".to_owned());
            let _ = FileSaveView::update(idle(), &intent("save_file.click"));
            assert!(modal().is_open(), "trigger opens the dialog");
            assert_eq!(dir.cwd(), ROOT_DIR, "open resets the browser to the root");
            assert_eq!(filename_state().text(), DEFAULT_NAME, "open pre-fills the default name");
            assert_eq!(
                filename_state().caret(),
                DEFAULT_NAME.len(),
                "caret sits at the end of the pre-filled name",
            );
            assert_eq!(
                modal_scope_request::drain(),
                Some(ModalRequest::Open { members: dialog_members() }),
                "modal trap requested over the filename field + action buttons",
            );
        });
    }

    #[test]
    fn r790_members_put_filename_first_for_autofocus() {
        assert_eq!(dialog_members(), vec![FILENAME_TAG, CANCEL_TAG, SAVE_TAG]);
    }

    #[test]
    fn r790_save_with_blank_name_is_a_noop() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            modal().open(dialog_members());
            let _ = modal_scope_request::drain();
            filename_state().set_text("   ".to_owned()); // whitespace = blank
            let before = directory().entries().len();
            let _ = FileSaveView::update(idle(), &intent("savefile_save.click"));
            assert!(modal().is_open(), "Save with a blank name keeps the dialog open");
            assert_eq!(use_saved().get(), None, "nothing saved");
            assert_eq!(directory().entries().len(), before, "no file created");
            assert_eq!(modal_scope_request::drain(), None, "no modal lifecycle change");
        });
    }

    #[test]
    fn r790_save_writes_file_records_path_and_closes() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            open_dialog();
            filename_state().set_text("notes.md".to_owned());
            let _ = modal_scope_request::drain();
            let _ = FileSaveView::update(idle(), &intent("savefile_save.click"));
            assert_eq!(
                use_saved().get(),
                Some("/proj/notes.md".to_string()),
                "Save records the written path",
            );
            assert!(
                directory().entries().iter().any(|e| e.name == "notes.md" && !e.is_dir),
                "the file was created in the current directory",
            );
            assert!(!modal().is_open(), "Save closes the dialog");
            assert_eq!(modal_scope_request::drain(), Some(ModalRequest::Close));
        });
    }

    #[test]
    fn r790_save_into_subdirectory_uses_current_cwd() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            open_dialog();
            directory().navigate("assets"); // cwd = /proj/assets
            filename_state().set_text("sprite.png".to_owned());
            let _ = FileSaveView::update(idle(), &intent("savefile_save.click"));
            assert_eq!(
                use_saved().get(),
                Some("/proj/assets/sprite.png".to_string()),
                "Save writes into the browsed-into folder, not the root",
            );
        });
    }

    #[test]
    fn r790_cancel_closes_without_writing() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            open_dialog();
            filename_state().set_text("scratch.txt".to_owned());
            let _ = modal_scope_request::drain();
            let _ = FileSaveView::update(idle(), &intent("savefile_cancel.click"));
            assert!(!modal().is_open(), "Cancel closes the dialog");
            assert_eq!(use_saved().get(), None, "Cancel does not save");
            assert!(
                !directory().entries().iter().any(|e| e.name == "scratch.txt"),
                "Cancel writes no file",
            );
            assert_eq!(modal_scope_request::drain(), Some(ModalRequest::Close));
        });
    }

    // ----- filename-selection sync Effect -----

    #[test]
    fn r790_selecting_a_file_populates_the_filename_field() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            // Install the reactive bridge, then open the dialog.
            let _sync = use_filename_sync();
            open_dialog();
            assert_eq!(filename_state().text(), DEFAULT_NAME, "starts at the default");
            // Clicking an existing file mirrors its basename into the field.
            directory().select("README.md");
            assert_eq!(
                filename_state().text(),
                "README.md",
                "selecting a file overwrites the name (Save As convention)",
            );
            assert_eq!(filename_state().caret(), "README.md".len(), "caret at end");
        });
    }

    #[test]
    fn r790_navigating_a_folder_leaves_the_typed_name_intact() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            let _sync = use_filename_sync();
            open_dialog();
            filename_state().set_text("my-name.txt".to_owned());
            // Navigating clears the selection (None) — the sync skips it.
            directory().navigate("src");
            assert_eq!(
                filename_state().text(),
                "my-name.txt",
                "a directory navigation must not clobber the typed name",
            );
        });
    }

    // ----- apply_key -----

    #[test]
    fn r790_enter_in_filename_field_saves() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            open_dialog();
            filename_state().set_text("via-enter.txt".to_owned());
            let _ = modal_scope_request::drain();
            let mut scene = boot_scene();
            let handled = FileSaveView::apply_key(
                &mut scene,
                Some(FILENAME_TAG),
                "Enter",
                pinion_core::Modifiers::empty(),
            );
            assert!(handled, "Enter in the name field is consumed");
            assert_eq!(use_saved().get(), Some("/proj/via-enter.txt".to_string()), "Enter saves");
            assert!(!modal().is_open(), "Enter closes the dialog");
        });
    }

    #[test]
    fn r790_escape_cancels_open_dialog() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            open_dialog();
            filename_state().set_text("draft.txt".to_owned());
            let mut scene = boot_scene();
            let handled = FileSaveView::apply_key(
                &mut scene,
                Some(FILENAME_TAG),
                "Escape",
                pinion_core::Modifiers::empty(),
            );
            assert!(handled);
            assert!(!modal().is_open(), "Escape dismisses");
            assert_eq!(use_saved().get(), None, "Escape does not save");
            assert_eq!(modal_scope_request::drain(), Some(ModalRequest::Close));
        });
    }

    #[test]
    fn r790_escape_ignored_when_closed() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert!(!FileSaveView::apply_key(
                &mut scene,
                None,
                "Escape",
                pinion_core::Modifiers::empty(),
            ));
        });
    }

    #[test]
    fn r790_focusable_tags_lists_only_trigger() {
        assert_eq!(FileSaveView::focusable_tags(), vec![TRIGGER_TAG]);
    }

    // ----- view / paint -----

    #[test]
    fn r790_closed_view_omits_scrim_and_panel() {
        Owner::new().run(|| {
            let scene = view(idle(), &Frame::new());
            assert!(!scene.contains_tag(SCRIM_TAG), "closed: no scrim");
            assert!(!scene.contains_tag(PANEL_TAG), "closed: no panel");
            assert!(scene.contains_tag(TRIGGER_TAG), "trigger always painted");
        });
    }

    #[test]
    fn r790_open_view_paints_scrim_panel_browser_field_and_actions() {
        Owner::new().run(|| {
            open_dialog();
            let scene = view(idle(), &Frame::new());
            assert!(scene.contains_tag(SCRIM_TAG), "open: scrim painted");
            assert!(scene.contains_tag(PANEL_TAG), "open: panel painted");
            assert!(scene.contains_tag(SAVE_TAG), "Save action painted");
            assert!(scene.contains_tag(CANCEL_TAG), "Cancel action painted");
            assert!(scene.contains_tag(FILENAME_TAG), "filename field painted in body");
            assert!(scene.contains_tag(&format!("{DIR_TAG}#0")), "row 0 painted in body");
            assert!(scene.contains_tag(&format!("{DIR_TAG}#up")), "parent affordance painted");
        });
    }

    /// Walk for a tagged container's fill (the Save button's disabled vs
    /// accent ink).
    fn find_tagged<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(tag) {
                    return Some(c);
                }
                c.children.iter().find_map(|ch| find_tagged(ch, tag))
            }
            Scene::Scroll(s) => find_tagged(s.content.as_ref(), tag),
            _ => None,
        }
    }

    #[test]
    fn r790_save_disabled_until_named_then_accent() {
        Owner::new().run(|| {
            modal().open(dialog_members());
            filename_state().set_text(String::new()); // blank
            let accent = Theme::light().resolve(ColorRole::Accent);
            let scene = view(idle(), &Frame::new());
            let save_blank = find_tagged(&scene, SAVE_TAG).expect("save painted").style.fill;
            assert_ne!(save_blank, accent, "Save is not Accent with a blank name (gate)");
            filename_state().set_text("ok.txt".to_owned());
            let scene = view(idle(), &Frame::new());
            let save_named = find_tagged(&scene, SAVE_TAG).expect("save painted").style.fill;
            assert_eq!(save_named, accent, "Save paints Accent once a name is typed");
        });
    }

    // ----- a11y -----

    #[test]
    fn r790_closed_emits_only_trigger_button() {
        Owner::new().run(|| {
            let nodes = FileSaveView::access_node(&idle(), None);
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].role, AriaRole::Button);
            assert_eq!(nodes[0].tag, TRIGGER_TAG);
        });
    }

    #[test]
    fn r790_open_emits_modal_dialog_with_list_field_and_actions() {
        Owner::new().run(|| {
            open_dialog();
            let nodes = FileSaveView::access_node(&idle(), Some(FILENAME_TAG));
            assert_eq!(nodes[0].role, AriaRole::Dialog);
            assert!(nodes[0].modal, "dialog root carries aria-modal");
            assert_eq!(nodes[0].children, vec![DIR_TAG, FILENAME_TAG, CANCEL_TAG, SAVE_TAG]);
            let list = nodes.iter().find(|n| n.tag == DIR_TAG).expect("file list node");
            assert_eq!(list.role, AriaRole::List);
            assert_eq!(list.size_of_set, Some(4), "four entries in /proj");
            let field = nodes.iter().find(|n| n.tag == FILENAME_TAG).expect("filename node");
            assert_eq!(field.role, AriaRole::TextInput);
            assert_eq!(
                field.value,
                Some(AccessValue::Text(DEFAULT_NAME.to_string())),
                "the a11y value mirrors the field content",
            );
            // Save enabled (the default name is non-blank).
            let save = nodes.iter().find(|n| n.tag == SAVE_TAG).expect("save node");
            assert!(!save.state.disabled, "Save enabled with the pre-filled name");
        });
    }

    #[test]
    fn r790_a11y_save_disabled_when_name_blank() {
        Owner::new().run(|| {
            modal().open(dialog_members());
            filename_state().set_text(String::new());
            let nodes = FileSaveView::access_node(&idle(), None);
            let save = nodes.iter().find(|n| n.tag == SAVE_TAG).expect("save node");
            assert!(save.state.disabled, "Save is aria-disabled until a name is typed");
        });
    }

    #[test]
    fn r790_names_enriched_from_paint_not_hardcoded() {
        Owner::new().run(|| {
            open_dialog();
            let scene = view(idle(), &Frame::new());
            let mut nodes = FileSaveView::access_node(&idle(), Some(FILENAME_TAG));
            pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
            let name = |t: &str| {
                nodes.iter().find(|n| n.tag == t).and_then(|n| n.name.as_deref())
            };
            assert_eq!(name(PANEL_TAG), Some("Save file"), "dialog name from title");
            assert_eq!(name(CANCEL_TAG), Some("Cancel"));
            assert_eq!(name(SAVE_TAG), Some("Save"));
        });
    }

    #[test]
    fn r790_view_contains_trigger_paint_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<FileSaveView>(
            idle(),
            &Frame::default(),
        );
    }

    /// Build the multi-external state scene the way the shell does so
    /// `apply_key`'s forwarding has real externals to dispatch against.
    fn boot_scene() -> Scene {
        use pinion_core::scene::ExternalNode;
        let mut children = vec![Scene::External(
            ExternalNode::new(FileSaveView::create_external()).with_tag(TRIGGER_TAG),
        )];
        for extra in FileSaveView::create_extra_externals() {
            children.push(Scene::External(ExternalNode::new(extra.handle).with_tag(extra.tag)));
        }
        Scene::Container(ContainerNode::new(children))
    }
}
