// R788 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the narrative carries many proper-noun identifiers
// (FocusManager, WAI-ARIA, DirectoryExternal, ModalState, …).
#![allow(clippy::doc_markdown)]

//! `hello-file-open-dialog` — R788 §5.15 §5.16 §5.39 **modal file-open
//! dialog**: the scene-graph-native, AI-introspectable peer of an OS
//! file-open dialog, assembled from three substrates that already exist —
//!
//! - the R787 **own-rendered file browser** ([`DirectoryExternal`] over
//!   the [`Directory`] read trait): browse + select a file inside pinion's
//!   own scene tree, every entry a paint node, the whole flow driven over
//!   `scene/query` + `scene/invoke` (a native OS dialog is a window pinion
//!   does not own — invisible to `scene/query`, unverifiable on a headless
//!   Linux box — `[[native-menu-macos-windows-only-verify]]`);
//! - the R693 **modal dialog chrome** ([`view_dialog`]: scrim + centred
//!   elevated panel + trailing action row), now carrying the browser pane
//!   in its new R788 [`DialogContent::body`] content slot; and
//! - the R788 **lifted modal lifecycle** ([`ModalState`] / [`use_modal`]):
//!   the open flag + [`modal_scope_request`](pinion_core::modal_scope_request)
//!   focus trap, flipped together so the painted modal can never desync
//!   from the trap.
//!
//! ## Why this binding exists
//!
//! Phase B OS-native-integration entry. "Open a file" is the single most
//! common pro-tool gesture, and the textbook answer here is the
//! *own-renderer*, not native delegation (R787): the file picker is a
//! pinion scene, so an AI agent can drive the entire open flow — browse,
//! select, confirm — through the RPC channel and read the chosen path
//! back as data, no pixels required. A direct step toward the
//! northern-star "Unreal-class editor self-hosted in pinion" (every
//! editor `File → Open` routes through one).
//!
//! ## Architecture (unidirectional, no reducer→widget back-channel)
//!
//! The trigger (`open_file`, primary external) plus three extra externals:
//! the [`DirectoryExternal`] (`fb_dir`, rows `fb_dir#<i>`) that owns the
//! browse + select model, and two real focusable action
//! [`ButtonExternal`]s (`fileopen_ok` / `fileopen_cancel`). Row clicks
//! route to the directory external's `send` (navigate a folder / select a
//! file); the action buttons emit `"<tag>.click"` intents the
//! [`FileOpenView::update`] reducer maps onto the [`ModalState`] +
//! `chosen` [`Signal`]. The view reads them back and paints the scrim +
//! panel + browser pane when open. Data flows input → External → intent →
//! reducer → Signal → view, exactly like hello-dialog / hello-drawer.
//!
//! ## OK gated on a selection
//!
//! A file-open dialog's confirm button is disabled until a file is
//! picked. The selection lives in the [`DirectoryExternal`] (mutated by
//! row clicks, *not* the reducer), so the gate is expressed where the view
//! can see it: with no selection the OK button paints the
//! [`ButtonState::Disabled`] posture (muted, no hover overlay) and carries
//! `aria-disabled`, and the reducer no-ops an OK with no selection. Once a
//! file is selected, OK paints its real Accent posture and confirms. (A
//! fully disabled control would also drop out of the Tab order; that needs
//! a dynamic modal-member list and is an honest deferred axis — the trap
//! is `[Cancel, OK]` throughout.)
//!
//! ## Dismissal / confirm paths
//!
//! - **Open** — `"fileopen_ok.click"` with a selection: records the chosen
//!   path and closes;
//! - **Cancel** — `"fileopen_cancel.click"`: closes without choosing;
//! - **Escape** — routed to [`apply_key`](FileOpenView) while the trap is
//!   active: dismisses as a cancel.
//!
//! Each open resets the browser to the root with no selection (a file
//! dialog opens at a default location), keeping the flow deterministic.
//!
//! ## Known gap (pre-existing, honest carry)
//!
//! Like hello-dialog / hello-drawer, the action buttons do not paint a
//! keyboard-focus ring on the *file rows* (the browser list is
//! pointer/RPC-driven — keyboard roving over entries is the R787
//! deferred axis); the modal trap over the action buttons is real and
//! observable (`focus/get` shows it confined to `[Cancel, OK]`).

use pinion_a11y::{AccessFocus, AccessNode, AriaRole, WidgetA11y, windowed_list_nodes_selected};
use pinion_core::directory::{Directory, InMemoryDirectory};
use pinion_core::external::External;
use pinion_core::reactive::{Owner, Signal, batch};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::widgets::file_browser::{
    DirectoryExternal, DirectoryState, dir_nav_key_selecting, use_directory_state,
};
use pinion_core::widgets::modal::{ModalState, modal_introspection_extra, use_modal};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{DirEntry, Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::button::{
    ButtonColors, ButtonStyle, button_a11y_state, button_scene, read_button_focused,
    read_button_state,
};
use pinion_widget_paint::dialog::{DialogContent, DialogStyle, view_dialog};
use pinion_widget_paint::file_browser::{FileBrowserMetrics, file_browser_pane};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(
    HelloFileOpenDialogRenderer,
    HelloFileOpenDialogRendererError
);

const WIN_W: u32 = 600;
const WIN_H: u32 = 600;
const THEME_TAG: &str = "app";

/// Trigger button (the primary external). Opens the file dialog.
const TRIGGER_TAG: &str = "open_file";
/// The [`DirectoryExternal`] / a11y `list` tag. Rows are `fb_dir#<i>`, the
/// parent affordance is `fb_dir#up` — all route here (R51.42 composite).
const DIR_TAG: &str = "fb_dir";
/// Affirmative action button — extra external. Confirms the picked file.
const OK_TAG: &str = "fileopen_ok";
/// Cancel action button — extra external.
const CANCEL_TAG: &str = "fileopen_cancel";
/// Backdrop tag (no external — blocks + swallows background clicks).
const SCRIM_TAG: &str = "fileopen_scrim";
/// Dialog panel tag (queryable via `scene/snapshot`).
const PANEL_TAG: &str = "fileopen_panel";

/// `Owner::cache` key for the shared [`ModalState`] open-lifecycle.
const MODAL_KEY: &str = "hello_file_open.modal";
/// R795 — query-only modal-introspection tag. `scene/query`
/// `/fileopen_state/external/open` reports the picker's open flag.
const MODAL_STATE_TAG: &str = "fileopen_state";
/// `Owner::cache` key for the `Signal<Option<String>>` chosen file path.
const CHOSEN_KEY: &str = "hello_file_open.chosen";
/// Vertical body scroll for the virtualized entry list.
const SCROLL_KEY: &str = "fileopen_scroll";
/// Where the picker opens each time.
const ROOT_DIR: &str = "/proj";

const TRIGGER_HOVER_KEY: &str = "hello_file_open.trigger_hover";
const OK_HOVER_KEY: &str = "hello_file_open.ok_hover";
const CANCEL_HOVER_KEY: &str = "hello_file_open.cancel_hover";

const TRIGGER_W: u32 = 200;
const TRIGGER_H: u32 = 56;
const ACTION_W: u32 = 110;
const ACTION_H: u32 = 44;

/// Dialog panel width; the browser list sits inside its padding.
const PANEL_W: u32 = 460;
/// Per-row vertical slot (px) + the entry-list viewport height (8 rows).
const ROW_PITCH: u32 = 32;
const LIST_W: u32 = PANEL_W - 48; // panel_width − 2 × m3 panel_padding (24)
const LIST_H: u32 = 8 * ROW_PITCH;
const OVERSCAN: usize = 3;

/// The dialog's focusable controls, in Tab order. R802 makes the **file
/// list** (`DIR_TAG`) the first member, so opening the dialog auto-focuses
/// the list — the native open-panel default, where the user can immediately
/// arrow through entries (selection-follows-focus). Tab then moves
/// list → Cancel → Open and wraps. The list is a single Tab stop with an
/// internal roving cursor (`aria-activedescendant`), not one stop per row.
fn dialog_members() -> Vec<String> {
    vec![
        DIR_TAG.to_string(),
        CANCEL_TAG.to_string(),
        OK_TAG.to_string(),
    ]
}

/// The R788 modal panel style: the M3 default widened to hold the browser.
fn dialog_style() -> DialogStyle {
    DialogStyle {
        panel_width: PANEL_W,
        ..DialogStyle::m3_default()
    }
}

/// The shared [`ModalState`] (R788 lifted open-lifecycle SSOT). One `Rc`
/// across the view fn, the reducer, and `access_node`. The chosen path is
/// per-binding policy ([`use_chosen`]), not the modal mechanism.
fn modal() -> Rc<ModalState> {
    use_modal(MODAL_KEY)
}

/// `Owner::cache`-keyed hook: the chosen file path — `None` until the user
/// confirms a selection with OK, then its full `'/'`-path.
fn use_chosen() -> Rc<Signal<Option<String>>> {
    Owner::current()
        .expect("use_chosen requires an active Owner scope")
        .cache(CHOSEN_KEY, || Signal::new(None::<String>))
}

/// The shared [`DirectoryState`] the browser pane reads and the
/// [`DirectoryExternal`] mutates (one `Rc` via the cache key `DIR_TAG`).
fn directory() -> Rc<DirectoryState> {
    use_directory_state(DIR_TAG, seed_directory, || ROOT_DIR.to_string())
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
            DirEntry::file(".gitignore"),
        ],
    );
    d.insert(
        "/proj/src",
        vec![
            DirEntry::dir("ui"),
            DirEntry::file("main.rs"),
            DirEntry::file("lib.rs"),
        ],
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

/// Open the dialog: reset the browser to the root with no selection (a
/// file dialog opens at a default location) and install the modal trap.
fn open_dialog() {
    directory().open_dir(ROOT_DIR);
    modal().open(dialog_members());
}

/// Confirm the picked file: record the selected path as `chosen` and close
/// the dialog. A no-op when nothing is selected (the OK gate — the button
/// paints disabled, but a keyboard activation is guarded here too). The
/// outcome write + modal close [`batch`] so the view re-renders once.
fn confirm() {
    let Some(path) = directory().selected() else {
        return;
    };
    let m = modal();
    batch(|| {
        use_chosen().set(Some(path));
        m.close();
    });
}

/// The browser pane that fills the dialog body: the lifted R789.1
/// [`file_browser_pane`] (breadcrumb + virtualized [`file_row`] list) sized
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
        FileBrowserMetrics {
            list_width: LIST_W,
            list_height: LIST_H,
            row_pitch: ROW_PITCH,
            overscan: OVERSCAN,
            focusable: false,
        },
        None,
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

/// Cached posture for the paint fn: each button's [`ButtonState`] plus its
/// keyboard-focus flag (`[trigger, ok, cancel]`). `open` / `chosen` /
/// browse state live in signals the owner-scoped view + access_node read
/// directly.
type FileOpenViewState = (ButtonState, ButtonState, ButtonState, [bool; 3]);

/// view-fn (§6.3): pure sync mapping `(button postures) -> Scene`, reading
/// the reactive `modal` open flag, `chosen` path, and the browse state.
/// When open, the dialog overlay (scrim + panel + browser pane) is pushed
/// **last** so it paints over (and hit-tests above) the trigger content.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: FileOpenViewState, _frame: &Frame) -> Scene {
    let (trigger_state, ok_state, cancel_state, focus) = state;
    let [trigger_focused, ok_focused, cancel_focused] = focus;
    let theme = use_theme(THEME_TAG).theme_animated();

    let trigger = button_scene(
        "Open file\u{2026}",
        trigger_state,
        trigger_focused,
        TRIGGER_HOVER_KEY,
        &ButtonColors::filled_tonal(&theme),
        &ButtonStyle::m3_default(TRIGGER_TAG)
            .with_size(Size::px(TRIGGER_W, TRIGGER_H))
            .with_label_font_size_px(16),
    );
    let chosen = use_chosen().get();
    let status = Scene::Text(TextNode::styled(
        match &chosen {
            Some(p) => format!("Chosen: {p}"),
            None => "Chosen: (none)".to_string(),
        },
        Rect::default(),
        TextStyle::new()
            .with_size_px(14)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
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
        let has_selection = dir.selected().is_some();
        let pane = browser_pane(&dir, &scroll, &theme);

        // Action order matches `dialog_members()` (Cancel, then Open).
        let cancel = action_button(
            CANCEL_TAG,
            "Cancel",
            cancel_state,
            cancel_focused,
            CANCEL_HOVER_KEY,
            &ButtonColors::filled_tonal(&theme),
        );
        // OK gated on a selection: with none, paint the Disabled posture.
        let ok_posture = if has_selection {
            ok_state
        } else {
            ButtonState::Disabled
        };
        let ok = action_button(
            OK_TAG,
            "Open",
            ok_posture,
            ok_focused,
            OK_HOVER_KEY,
            &ButtonColors::accent(&theme),
        );

        children.push(view_dialog(
            SCRIM_TAG,
            PANEL_TAG,
            DialogContent {
                title: "Open file",
                message: "",
                body: Some(pane),
            },
            vec![cancel, ok],
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

struct FileOpenView;

impl WidgetCore for FileOpenView {
    type State = FileOpenViewState;
    // Buttons are driven by the pointer router + ARIA Enter/Space; no
    // keybinding-channel events flow through `event_name`.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    /// The [`DirectoryExternal`] over the same shared [`DirectoryState`]
    /// the view reads (so `fb_dir#<i>` row clicks + `fb_dir#up` route to
    /// its `send`), plus the two action [`ButtonExternal`]s.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(DIR_TAG, Box::new(DirectoryExternal::new(directory()))),
            ExtraExternal::new(OK_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(CANCEL_TAG, Box::new(ButtonExternal::new())),
            modal_introspection_extra(MODAL_STATE_TAG, modal()),
        ]
    }

    fn tag() -> &'static str {
        TRIGGER_TAG
    }

    fn read_state(scene: &Scene) -> FileOpenViewState {
        (
            read_button_state(scene, TRIGGER_TAG),
            read_button_state(scene, OK_TAG),
            read_button_state(scene, CANCEL_TAG),
            [
                read_button_focused(scene, TRIGGER_TAG),
                read_button_focused(scene, OK_TAG),
                read_button_focused(scene, CANCEL_TAG),
            ],
        )
    }

    fn view(state: FileOpenViewState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-file-open-dialog (R788 §5.15 §5.16 §5.39 modal file picker)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// R788 §5.39 — Escape dismisses the open dialog as a cancel (the shell
    /// routes Escape here only while the trap is active). Enter / Space on a
    /// focused action button activate it through the shared ARIA helper.
    ///
    /// R802 §5.15 §5.40 — when the **file list** holds focus, the arrow keys
    /// drive the selection-follows-focus picker
    /// ([`dir_nav_key_selecting`]): each cursor move syncs `selected` to the
    /// focused file (enabling the OK gate) or clears it on a directory row.
    /// `Enter` on a *file* confirms the dialog (the native open-on-Enter);
    /// `Enter` on a *directory* descends into it (handled inside
    /// `dir_nav_key_selecting`). Runs in the shell root-owner scope, so
    /// `directory()` / `use_scroll_state` resolve.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if key == "Escape" {
            if modal().is_open() {
                modal().close();
                return true;
            }
            return false;
        }
        if focused == Some(DIR_TAG) {
            let dir = directory();
            // `Enter` on a focused *file* confirms (it is already selected,
            // selection-follows-focus); a *directory* descends via the nav body.
            if key == "Enter" {
                if let Some(idx) = dir.cursor() {
                    if dir.entries().get(idx).is_some_and(|e| !e.is_dir) {
                        confirm();
                        return true;
                    }
                }
            }
            return dir_nav_key_selecting(
                &dir,
                &use_scroll_state(SCROLL_KEY),
                focused,
                DIR_TAG,
                key,
                ROW_PITCH,
            );
        }
        apply_aria_activate(scene, focused, key, TRIGGER_TAG)
            || apply_aria_activate(scene, focused, key, OK_TAG)
            || apply_aria_activate(scene, focused, key, CANCEL_TAG)
    }

    /// R788 §5.39 — bridge the action buttons' `"<tag>.click"` intents into
    /// the [`ModalState`] + `chosen` signal. The browser's row clicks route
    /// to the [`DirectoryExternal`] directly, not here. Side-effect-only.
    fn update(
        _state: FileOpenViewState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            "open_file.click" => open_dialog(),
            "fileopen_ok.click" => confirm(),
            "fileopen_cancel.click" => modal().close(),
            _ => {}
        }
        Vec::new()
    }

    fn fmt_state_log(state: &FileOpenViewState) -> String {
        format!(
            "trigger={:?} ok={:?} cancel={:?}",
            state.0, state.1, state.2
        )
    }
}

impl WidgetA11y for FileOpenView {
    /// R788 §5.40 — WAI-ARIA dialog tree. Closed: just the trigger
    /// [`AriaRole::Button`]. Open: the `aria-modal` [`AriaRole::Dialog`]
    /// root owning the file `list` (the shared
    /// `windowed_list_nodes_selected`, R776 — the Nth consumer of the
    /// windowed-list a11y SSOT, with `aria-selected` on the picked file)
    /// plus the two action buttons. OK carries `aria-disabled` until a file
    /// is selected, matching the painted gate. Names are enriched from the
    /// paint scene (`enrich_names_from_scene`), so the labels + title have a
    /// single source of truth.
    fn access_node(state: &FileOpenViewState, focused: Option<&str>) -> Vec<AccessNode> {
        if !modal().is_open() {
            return vec![
                AccessNode::new(TRIGGER_TAG, AriaRole::Button)
                    .with_state(button_a11y_state(state.0, focused == Some(TRIGGER_TAG))),
            ];
        }
        let dir = directory();
        let scroll = use_scroll_state(SCROLL_KEY);
        let count = dir.entries().len();
        let window = compute_visible_range(scroll.offset_y(), LIST_H, count, ROW_PITCH, OVERSCAN);

        let dialog = AccessNode::new(PANEL_TAG, AriaRole::Dialog)
            .with_modal()
            .with_child(DIR_TAG)
            .with_child(CANCEL_TAG)
            .with_child(OK_TAG);
        let mut nodes = vec![dialog];
        nodes.extend(windowed_list_nodes_selected(
            DIR_TAG,
            "Files",
            u32::try_from(count).unwrap_or(u32::MAX),
            &window,
            dir.selected_index(),
        ));
        // OK reflects the selection gate (aria-disabled until a pick).
        let ok_posture = if dir.selected().is_some() {
            state.1
        } else {
            ButtonState::Disabled
        };
        nodes.push(
            AccessNode::new(CANCEL_TAG, AriaRole::Button)
                .with_state(button_a11y_state(state.2, focused == Some(CANCEL_TAG)))
                .with_position_in_set(1)
                .with_size_of_set(2),
        );
        nodes.push(
            AccessNode::new(OK_TAG, AriaRole::Button)
                .with_state(button_a11y_state(ok_posture, focused == Some(OK_TAG)))
                .with_position_in_set(2)
                .with_size_of_set(2),
        );
        nodes
    }

    /// R802 §5.40 — while the file list holds focus, the focus ring tracks
    /// the roving keyboard cursor (`aria-activedescendant`): the shell rings
    /// the cursor row's composite tag (`fb_dir#<i>`) rather than the whole
    /// list container, so the painted ring follows the arrow keys. Mirrors
    /// the file-manager / file-browser roving-cursor wiring (R792). Any other
    /// focus is the atomic focused tag.
    fn access_focus_target(
        _state: &FileOpenViewState,
        focused: Option<&str>,
    ) -> Option<AccessFocus> {
        if focused == Some(DIR_TAG) && modal().is_open() {
            if let Some(cursor) = directory().cursor() {
                return Some(AccessFocus::composite(
                    DIR_TAG,
                    format!("{DIR_TAG}#{cursor}"),
                ));
            }
        }
        focused.map(AccessFocus::atomic)
    }
}

impl WidgetView for FileOpenView {
    type Renderer = HelloFileOpenDialogRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<FileOpenView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::modal_scope_request::{self, ModalRequest};

    fn idle() -> FileOpenViewState {
        (
            ButtonState::Idle,
            ButtonState::Idle,
            ButtonState::Idle,
            [false; 3],
        )
    }

    fn intent(tag: &str) -> pinion_core::Intent {
        pinion_core::Intent::new_owned(
            tag.to_string(),
            pinion_core::external::IntrospectValue::Null,
        )
    }

    // ----- reducer + signals -----

    #[test]
    fn r788_trigger_opens_resets_browser_and_requests_modal() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            // Pre-navigate + select so we can prove open() resets to root.
            let dir = directory();
            dir.navigate("src");
            dir.select("main.rs");
            let _ = FileOpenView::update(idle(), &intent("open_file.click"));
            assert!(modal().is_open(), "trigger opens the dialog");
            assert_eq!(dir.cwd(), ROOT_DIR, "open resets the browser to the root");
            assert_eq!(dir.selected(), None, "open clears any prior selection");
            assert_eq!(
                modal_scope_request::drain(),
                Some(ModalRequest::Open {
                    members: dialog_members()
                }),
                "modal trap requested over the action buttons",
            );
        });
    }

    #[test]
    fn r788_ok_without_selection_is_a_noop() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            modal().open(dialog_members());
            let _ = modal_scope_request::drain();
            // No file picked: OK must not close or choose.
            let _ = FileOpenView::update(idle(), &intent("fileopen_ok.click"));
            assert!(
                modal().is_open(),
                "OK with no selection keeps the dialog open"
            );
            assert_eq!(use_chosen().get(), None, "nothing chosen");
            assert_eq!(
                modal_scope_request::drain(),
                None,
                "no modal lifecycle change"
            );
        });
    }

    #[test]
    fn r788_ok_with_selection_confirms_chosen_and_closes() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            open_dialog();
            directory().select("Cargo.toml");
            let _ = modal_scope_request::drain();
            let _ = FileOpenView::update(idle(), &intent("fileopen_ok.click"));
            assert_eq!(
                use_chosen().get(),
                Some("/proj/Cargo.toml".to_string()),
                "OK records the selected path as chosen",
            );
            assert!(!modal().is_open(), "OK closes the dialog");
            assert_eq!(modal_scope_request::drain(), Some(ModalRequest::Close));
        });
    }

    #[test]
    fn r788_cancel_closes_without_choosing() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            open_dialog();
            directory().select("README.md");
            let _ = modal_scope_request::drain();
            let _ = FileOpenView::update(idle(), &intent("fileopen_cancel.click"));
            assert!(!modal().is_open(), "Cancel closes the dialog");
            assert_eq!(use_chosen().get(), None, "Cancel does not choose");
            assert_eq!(modal_scope_request::drain(), Some(ModalRequest::Close));
        });
    }

    // ----- apply_key -----

    #[test]
    fn r788_escape_cancels_open_dialog() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            open_dialog();
            directory().select("README.md");
            let mut scene = boot_scene();
            let handled = FileOpenView::apply_key(
                &mut scene,
                Some(CANCEL_TAG),
                "Escape",
                pinion_core::Modifiers::empty(),
            );
            assert!(handled);
            assert!(!modal().is_open(), "Escape dismisses");
            assert_eq!(use_chosen().get(), None, "Escape does not choose");
            assert_eq!(modal_scope_request::drain(), Some(ModalRequest::Close));
        });
    }

    #[test]
    fn r788_escape_ignored_when_closed() {
        Owner::new().run(|| {
            let mut scene = boot_scene();
            assert!(!FileOpenView::apply_key(
                &mut scene,
                None,
                "Escape",
                pinion_core::Modifiers::empty(),
            ));
        });
    }

    #[test]
    fn r788_focusable_tags_lists_only_trigger() {
        // §5.39: collected from the paint scene.
        let scene = Owner::new().run(|| view(idle(), &Frame::new()));
        assert_eq!(scene.collect_focusable_tags(), vec![TRIGGER_TAG.to_owned()]);
    }

    // ----- view / paint -----

    #[test]
    fn r788_closed_view_omits_scrim_and_panel() {
        Owner::new().run(|| {
            let scene = view(idle(), &Frame::new());
            assert!(!scene.contains_tag(SCRIM_TAG), "closed: no scrim");
            assert!(!scene.contains_tag(PANEL_TAG), "closed: no panel");
            assert!(scene.contains_tag(TRIGGER_TAG), "trigger always painted");
        });
    }

    #[test]
    fn r788_open_view_paints_scrim_panel_browser_and_actions() {
        Owner::new().run(|| {
            open_dialog();
            let scene = view(idle(), &Frame::new());
            assert!(scene.contains_tag(SCRIM_TAG), "open: scrim painted");
            assert!(scene.contains_tag(PANEL_TAG), "open: panel painted");
            assert!(scene.contains_tag(OK_TAG), "Open action painted");
            assert!(scene.contains_tag(CANCEL_TAG), "Cancel action painted");
            // The browser pane's rows live in the dialog body.
            assert!(
                scene.contains_tag(&format!("{DIR_TAG}#0")),
                "row 0 painted in body"
            );
            assert!(
                scene.contains_tag(&format!("{DIR_TAG}#up")),
                "parent affordance painted"
            );
        });
    }

    /// Walk for the OK button container's fill (its disabled vs accent ink).
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
    fn r788_ok_disabled_until_selection_then_accent() {
        Owner::new().run(|| {
            open_dialog();
            // No selection: OK paints the disabled fill (not Accent).
            let accent = Theme::light().resolve(ColorRole::Accent);
            let scene = view(idle(), &Frame::new());
            let ok_no_sel = find_tagged(&scene, OK_TAG).expect("ok painted").style.fill;
            assert_ne!(
                ok_no_sel, accent,
                "OK is not Accent with no selection (disabled gate)"
            );
            // Select a file: OK paints its Accent (enabled) fill.
            directory().select("Cargo.toml");
            let scene = view(idle(), &Frame::new());
            let ok_sel = find_tagged(&scene, OK_TAG).expect("ok painted").style.fill;
            assert_eq!(ok_sel, accent, "OK paints Accent once a file is selected");
        });
    }

    // ----- a11y -----

    #[test]
    fn r788_closed_emits_only_trigger_button() {
        Owner::new().run(|| {
            let nodes = FileOpenView::access_node(&idle(), None);
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].role, AriaRole::Button);
            assert_eq!(nodes[0].tag, TRIGGER_TAG);
        });
    }

    #[test]
    fn r788_open_emits_modal_dialog_with_list_and_actions() {
        Owner::new().run(|| {
            open_dialog();
            let nodes = FileOpenView::access_node(&idle(), Some(CANCEL_TAG));
            assert_eq!(nodes[0].role, AriaRole::Dialog);
            assert!(nodes[0].modal, "dialog root carries aria-modal");
            assert_eq!(nodes[0].children, vec![DIR_TAG, CANCEL_TAG, OK_TAG]);
            // The file list (shared windowed_list_nodes_selected) is present.
            let list = nodes
                .iter()
                .find(|n| n.tag == DIR_TAG)
                .expect("file list node");
            assert_eq!(list.role, AriaRole::List);
            assert_eq!(list.size_of_set, Some(5), "five entries in /proj");
            // OK carries aria-disabled with no selection.
            let ok = nodes.iter().find(|n| n.tag == OK_TAG).expect("ok node");
            assert!(
                ok.state.disabled,
                "OK is aria-disabled until a file is picked"
            );
        });
    }

    #[test]
    fn r788_a11y_ok_enabled_and_file_marked_selected_after_pick() {
        Owner::new().run(|| {
            open_dialog();
            directory().select("Cargo.toml"); // row 3 (assets, src, .gitignore, Cargo.toml)
            let nodes = FileOpenView::access_node(&idle(), Some(OK_TAG));
            let ok = nodes.iter().find(|n| n.tag == OK_TAG).expect("ok node");
            assert!(!ok.state.disabled, "OK enabled once a file is picked");
            let item = nodes
                .iter()
                .find(|n| n.position_in_set == Some(4) && n.role == AriaRole::ListItem)
                .expect("listitem for the picked row");
            assert_eq!(
                item.selected,
                Some(true),
                "picked file carries aria-selected"
            );
        });
    }

    #[test]
    fn r788_names_enriched_from_paint_not_hardcoded() {
        Owner::new().run(|| {
            open_dialog();
            let scene = view(idle(), &Frame::new());
            let mut nodes = FileOpenView::access_node(&idle(), Some(CANCEL_TAG));
            pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
            let name = |t: &str| {
                nodes
                    .iter()
                    .find(|n| n.tag == t)
                    .and_then(|n| n.name.as_deref())
            };
            assert_eq!(name(PANEL_TAG), Some("Open file"), "dialog name from title");
            assert_eq!(name(CANCEL_TAG), Some("Cancel"));
            assert_eq!(name(OK_TAG), Some("Open"));
        });
    }

    #[test]
    fn r788_view_contains_trigger_paint_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<FileOpenView>(
            idle(),
            &Frame::default(),
        );
    }

    /// Build the multi-external state scene the way the shell does so
    /// `apply_key`'s ARIA-activate forwarding has real externals to dispatch
    /// against.
    fn boot_scene() -> Scene {
        use pinion_core::scene::ExternalNode;
        let mut children = vec![Scene::External(
            ExternalNode::new(FileOpenView::create_external()).with_tag(TRIGGER_TAG),
        )];
        for extra in FileOpenView::create_extra_externals() {
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag),
            ));
        }
        Scene::Container(ContainerNode::new(children))
    }
}
