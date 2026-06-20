//! `hello-file-dialog` — R761 §3 §5.15 consumer of the
//! [`FileDialog`](pinion_core::FileDialog) capability + the first
//! production consumer of the §5.22 [`Resource`](pinion_core::Resource)
//! async-reactive carrier.
//!
//! ## What this demonstrates
//!
//! Three buttons — **Open**, **Save**, **Pick folder** — each launch a
//! file dialog through the `FileDialog` trait. The binding drives the
//! deterministic [`ScriptedFileDialog`](pinion_core::ScriptedFileDialog)
//! mock (a native OS dialog cannot run headless — that trait-split *is*
//! the verification discipline + AI-first channel; the real
//! `RfdFileDialog` lives in `pinion-platform-file-dialog`). The chosen
//! path (or cancellation) flows back through a
//! `Resource<Option<DialogOutcome>, String>` and renders as a tagged
//! status line, observable through `scene/snapshot` (§2 #7
//! scene-as-data) — no pixels needed to verify a file-pick flow.
//!
//! ## Architecture (unidirectional, async-via-Resource)
//!
//! Three [`ButtonExternal`]s emit `"<tag>.click"` intents;
//! [`FileDialogView::update`] maps each onto a dialog call and feeds the
//! returned [`FileDialogFuture`](pinion_core::FileDialogFuture) into the
//! shared [`Resource`] via [`Resource::fetch_with`], driven by the
//! owner-scoped [`LocalTaskPump`](pinion_core::LocalTaskPump) the shell
//! polls each frame (R761.1). The scripted mock's future is immediately
//! ready (resolves on the first poll); a real native `RfdFileDialog`
//! future yields `Pending` until the portal replies and resolves over
//! subsequent frames — the *same* pump, no per-binding `block_on`.
//!
//! ## Verification
//!
//! `tools/demos/r761_file_dialog.py`: boot → "No file chosen yet";
//! Open → scripted path rendered; Open again → cancelled; Save →
//! scripted path; Pick → scripted path; keyboard activation (Enter on a
//! focused button); a11y button roles + status live region.

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::External;
use pinion_core::reactive::{Owner, Resource, ResourceState};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::{
    use_local_task_pump, DialogKind, FileDialog, FileDialogRequest, FileFilter, Frame, Scene,
    ScriptedFileDialog, WidgetCore,
};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::button::{
    button_a11y_state, read_button_focused, read_button_state, ButtonColors, ButtonStyle,
};
use serde::{Deserialize, Serialize};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloFileDialogRenderer, HelloFileDialogRendererError);

const WIN_W: u32 = 420;
const WIN_H: u32 = 220;
const THEME_TAG: &str = "app";

const OPEN_TAG: &str = "open_file";
const SAVE_TAG: &str = "save_file";
const PICK_TAG: &str = "pick_folder";
const STATUS_TAG: &str = "dialog_status";

const OPEN_HOVER_KEY: &str = "open_hover";
const SAVE_HOVER_KEY: &str = "save_hover";
const PICK_HOVER_KEY: &str = "pick_hover";

/// `Owner::cache` key for the scripted dialog backend.
const DIALOG_KEY: &str = "file_dialog.backend";
/// `Owner::cache` key for the async-result `Resource`.
const RESULT_KEY: &str = "file_dialog.result";

/// R761 — the resolved outcome of one dialog interaction, carried by the
/// [`Resource`]. `action` is the verb (`"open"` / `"save"` / `"pick"`);
/// `path` is the chosen path (`None` = the user cancelled). Derives serde
/// because [`Resource`]'s payload must be (de)serializable for the §5.22
/// snapshot/restore contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct DialogOutcome {
    action: String,
    path: Option<String>,
}

/// The scripted dialog backend for this view scope (seeded once with the
/// demo's deterministic choice sequence). Shared by the reducer (which
/// invokes it) and tests.
fn dialog() -> Rc<ScriptedFileDialog> {
    Owner::current()
        .expect("dialog() requires an active Owner scope")
        .cache(DIALOG_KEY, || {
            let scripted = ScriptedFileDialog::new();
            seed_demo_script(&scripted);
            scripted
        })
}

/// The async-result carrier. Starts `Ready(None)` — "no dialog used
/// yet". Each dialog call transitions it (via [`Resource::fetch_with`])
/// to the resolved [`DialogOutcome`].
fn result() -> Rc<Resource<Option<DialogOutcome>, String>> {
    Owner::current()
        .expect("result() requires an active Owner scope")
        .cache(RESULT_KEY, || Resource::ready(None))
}

/// R761 — the deterministic choice sequence the demo drives. The queue
/// is consumed FIFO across *all* dialog calls (it models "the sequence
/// of choices the user makes, in order"), so the demo clicks
/// Open → Open → Save → Pick to walk it: select, cancel, select, select.
fn seed_demo_script(scripted: &ScriptedFileDialog) {
    // 1: Open → selected, but *deferred* (resolves after 30 Pending
    // polls) — a deterministic stand-in for a native dialog future so the
    // demo can observe the shell `LocalTaskPump` keep-alive driving a
    // multi-frame future Loading → Ready (R761.1).
    scripted.queue_deferred_selection("/projects/alpha.pinion.xml", 30);
    scripted.queue_cancel(); //                               2: Open → cancelled
    scripted.queue_selection("/exports/diagram.svg"); //      3: Save → selected
    scripted.queue_selection("/home/user/assets"); //         4: Pick → selected
    scripted.queue_selection("/keyboard/activated.txt"); //   5: keyboard Enter → selected
}

/// Build the request for a given dialog kind — a distinct title +
/// filter / suggested name per kind so the scripted call log (and any
/// future native dialog) is meaningfully parameterised.
fn request_for(kind: DialogKind) -> FileDialogRequest {
    match kind {
        DialogKind::OpenFile => FileDialogRequest::new()
            .with_title("Open project")
            .with_filter(FileFilter::new("pinion scene", ["pinion.xml"])),
        DialogKind::SaveFile => FileDialogRequest::new()
            .with_title("Export as")
            .with_filter(FileFilter::new("SVG", ["svg"]))
            .with_suggested_name("untitled.svg"),
        DialogKind::PickFolder => FileDialogRequest::new().with_title("Choose asset folder"),
    }
}

/// Run one dialog: invoke the scripted backend, then feed the returned
/// future into the shared [`Resource`] so the view observes the outcome
/// reactively. Owner-scoped side effect — no back-channel into a widget.
fn run_dialog(kind: DialogKind, action: &'static str) {
    let backend = dialog();
    let request = request_for(kind);
    let future = match kind {
        DialogKind::OpenFile => backend.open_file(&request),
        DialogKind::SaveFile => backend.save_file(&request),
        DialogKind::PickFolder => backend.pick_folder(&request),
    };
    // R761.1 — drive the dialog future through the shared owner-scoped
    // `LocalTaskPump` (the shell polls it each frame). The scripted mock
    // resolves on the first poll; a native `RfdFileDialog` future (which
    // yields `Pending` until the portal replies) resolves over subsequent
    // frames — the same path, no per-binding `block_on`.
    result().fetch_with(&*use_local_task_pump(), async move {
        let chosen = future.await;
        Ok::<Option<DialogOutcome>, String>(Some(DialogOutcome {
            action: action.to_owned(),
            path: chosen.map(|p| p.display().to_string()),
        }))
    });
}

/// Render the human-readable status line for the current [`Resource`]
/// state. The text is the same string the `role=status` live region
/// announces (single SSOT → screen + AT cannot drift).
fn status_line() -> String {
    match result().state() {
        ResourceState::Loading => "Choosing a file…".to_owned(),
        ResourceState::Ready(None) => "No file chosen yet".to_owned(),
        ResourceState::Ready(Some(outcome)) => outcome_line(&outcome),
        ResourceState::Error(err) => format!("Error: {err}"),
    }
}

/// Map a resolved [`DialogOutcome`] to its display line.
fn outcome_line(outcome: &DialogOutcome) -> String {
    match (outcome.action.as_str(), &outcome.path) {
        ("open", Some(path)) => format!("Opened: {path}"),
        ("open", None) => "Open cancelled".to_owned(),
        ("save", Some(path)) => format!("Saving to: {path}"),
        ("save", None) => "Save cancelled".to_owned(),
        ("pick", Some(path)) => format!("Folder: {path}"),
        ("pick", None) => "Pick folder cancelled".to_owned(),
        _ => "—".to_owned(),
    }
}

/// Render one button through the [`pinion_widget_paint::button`]
/// substrate (R727 SSOT) with this binding's tonal tone + 15 px font.
fn button_scene(
    tag: &'static str,
    label: &str,
    state: ButtonState,
    focused: bool,
    hover_key: &'static str,
    theme: &pinion_core::theme::Theme,
) -> Scene {
    pinion_widget_paint::button::button_scene(
        label,
        state,
        focused,
        hover_key,
        &ButtonColors::filled_tonal(theme),
        &ButtonStyle::m3_default(tag)
            .with_size(Size::px(124, 40))
            .with_label_font_size_px(15)
            .with_focusable(true),
    )
}

/// Cached posture of the three buttons + their focus flags
/// (`[open, save, pick]`). The dialog result lives in the [`Resource`]
/// the owner-scoped view + `access_node` read directly.
type FileDialogViewState = (ButtonState, ButtonState, ButtonState, [bool; 3]);

/// view-fn (§6.3): pure sync mapping `(button postures) -> Scene`,
/// reading the reactive [`Resource`] for the status line.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: FileDialogViewState, _frame: &Frame) -> Scene {
    let (open_state, save_state, pick_state, [open_focused, save_focused, pick_focused]) = state;
    let theme = use_theme(THEME_TAG).theme_animated();

    let title = Scene::Text(TextNode::styled(
        "File dialog (scripted)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(16)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let buttons = Scene::Container(
        ContainerNode::new(vec![
            button_scene(OPEN_TAG, "Open", open_state, open_focused, OPEN_HOVER_KEY, &theme),
            button_scene(SAVE_TAG, "Save", save_state, save_focused, SAVE_HOVER_KEY, &theme),
            button_scene(PICK_TAG, "Pick folder", pick_state, pick_focused, PICK_HOVER_KEY, &theme),
        ])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                .with_gap(12),
        ),
    );

    let status = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            status_line(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))])
        .with_tag(STATUS_TAG)
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center),
        ),
    );

    Scene::Container(
        ContainerNode::new(vec![title, buttons, status])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_size(Size::px(WIN_W, WIN_H))
                    .with_gap(20),
            ),
    )
}

struct FileDialogView;

impl WidgetCore for FileDialogView {
    type State = FileDialogViewState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(SAVE_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(PICK_TAG, Box::new(ButtonExternal::new())),
        ]
    }

    fn tag() -> &'static str {
        OPEN_TAG
    }

    fn read_state(scene: &Scene) -> FileDialogViewState {
        (
            read_button_state(scene, OPEN_TAG),
            read_button_state(scene, SAVE_TAG),
            read_button_state(scene, PICK_TAG),
            [
                read_button_focused(scene, OPEN_TAG),
                read_button_focused(scene, SAVE_TAG),
                read_button_focused(scene, PICK_TAG),
            ],
        )
    }

    fn view(state: FileDialogViewState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-file-dialog (R761 §3 §5.15)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }


    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        // Space / Enter activates whichever button holds focus; the
        // per-tag call is a no-op unless `focused` matches.
        [OPEN_TAG, SAVE_TAG, PICK_TAG]
            .into_iter()
            .any(|tag| apply_aria_activate(scene, focused, key, tag))
    }

    /// R761 reducer: each button click runs the matching dialog and
    /// feeds the result into the shared [`Resource`]. Owner-scoped side
    /// effect — no back-channel into a widget.
    fn update(
        _state: FileDialogViewState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            "open_file.click" => run_dialog(DialogKind::OpenFile, "open"),
            "save_file.click" => run_dialog(DialogKind::SaveFile, "save"),
            "pick_folder.click" => run_dialog(DialogKind::PickFolder, "pick"),
            _ => {}
        }
        Vec::new()
    }
}

impl WidgetA11y for FileDialogView {
    /// Three `role=button`s plus a passive `role=status` live region
    /// whose accessible name is the status line (derived from the
    /// `STATUS_TAG` container's `TextNode` by `enrich_names_from_scene`).
    fn access_node(state: &FileDialogViewState, focused: Option<&str>) -> Vec<AccessNode> {
        // The `[bool; 3]` focus array drives the painted ring in `view`;
        // a11y focus comes from the `focused` param (mirrors hello-snackbar).
        let (open_state, save_state, pick_state, _focus) = *state;
        vec![
            AccessNode::new(OPEN_TAG, AriaRole::Button)
                .with_state(button_a11y_state(open_state, focused == Some(OPEN_TAG))),
            AccessNode::new(SAVE_TAG, AriaRole::Button)
                .with_state(button_a11y_state(save_state, focused == Some(SAVE_TAG))),
            AccessNode::new(PICK_TAG, AriaRole::Button)
                .with_state(button_a11y_state(pick_state, focused == Some(PICK_TAG))),
            AccessNode::new(STATUS_TAG, AriaRole::Status),
        ]
    }
}

impl WidgetView for FileDialogView {
    type Renderer = HelloFileDialogRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<FileDialogView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> FileDialogViewState {
        (
            ButtonState::Idle,
            ButtonState::Idle,
            ButtonState::Idle,
            [false, false, false],
        )
    }

    fn find_container<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        match scene {
            Scene::Container(c) if c.tag.as_deref() == Some(tag) => Some(c),
            Scene::Container(c) => c.children.iter().find_map(|ch| find_container(ch, tag)),
            _ => None,
        }
    }

    fn status_text(scene: &Scene) -> Option<String> {
        let container = find_container(scene, STATUS_TAG)?;
        container.children.iter().find_map(|ch| match ch {
            Scene::Text(t) => Some(t.content.clone()),
            _ => None,
        })
    }

    #[test]
    fn r761_boot_shows_no_selection() {
        let owner = Owner::new();
        let scene = owner.run(|| view(idle(), &Frame::new()));
        assert_eq!(status_text(&scene).as_deref(), Some("No file chosen yet"));
    }

    /// Drive the owner-scoped `LocalTaskPump` to completion — what the
    /// shell does once per frame (R761.1). The demo's first scripted
    /// outcome is deferred (30 Pending polls), so loop generously.
    fn drain_pump() {
        for _ in 0..64 {
            if !use_local_task_pump().poll() {
                break;
            }
        }
    }

    #[test]
    fn r761_open_renders_scripted_path() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            run_dialog(DialogKind::OpenFile, "open");
            drain_pump();
            view(idle(), &Frame::new())
        });
        assert_eq!(
            status_text(&scene).as_deref(),
            Some("Opened: /projects/alpha.pinion.xml"),
        );
    }

    #[test]
    fn r761_second_open_is_cancelled_per_fifo() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            run_dialog(DialogKind::OpenFile, "open"); // consumes selection (deferred)
            run_dialog(DialogKind::OpenFile, "open"); // consumes cancel
            drain_pump();
            view(idle(), &Frame::new())
        });
        assert_eq!(status_text(&scene).as_deref(), Some("Open cancelled"));
    }

    #[test]
    fn r761_full_sequence_walks_the_script() {
        let owner = Owner::new();
        let lines: Vec<String> = owner.run(|| {
            let mut out = Vec::new();
            for (kind, action) in [
                (DialogKind::OpenFile, "open"),
                (DialogKind::OpenFile, "open"),
                (DialogKind::SaveFile, "save"),
                (DialogKind::PickFolder, "pick"),
            ] {
                run_dialog(kind, action);
                drain_pump();
                out.push(status_line());
            }
            out
        });
        assert_eq!(
            lines,
            vec![
                "Opened: /projects/alpha.pinion.xml".to_owned(),
                "Open cancelled".to_owned(),
                "Saving to: /exports/diagram.svg".to_owned(),
                "Folder: /home/user/assets".to_owned(),
            ],
        );
    }

    #[test]
    fn r761_call_log_records_each_kind() {
        let owner = Owner::new();
        owner.run(|| {
            run_dialog(DialogKind::OpenFile, "open");
            run_dialog(DialogKind::SaveFile, "save");
            run_dialog(DialogKind::PickFolder, "pick");
            let calls = dialog().calls();
            assert_eq!(calls.len(), 3);
            assert_eq!(calls[0].kind, DialogKind::OpenFile);
            assert_eq!(calls[1].kind, DialogKind::SaveFile);
            assert_eq!(calls[2].kind, DialogKind::PickFolder);
            // Save dialog carries the suggested name.
            assert_eq!(calls[1].request.suggested_name.as_deref(), Some("untitled.svg"));
        });
    }

    #[test]
    fn r761_three_buttons_are_focusable() {
        // §5.39: tree order IS tab order — collected from the paint scene.
        let scene = Owner::new().run(|| view(idle(), &Frame::new()));
        assert_eq!(
            scene.collect_focusable_tags(),
            vec![OPEN_TAG.to_owned(), SAVE_TAG.to_owned(), PICK_TAG.to_owned()],
        );
    }

    #[test]
    fn r761_a11y_exposes_three_buttons_and_status_region() {
        let owner = Owner::new();
        let nodes = owner.run(|| <FileDialogView as WidgetA11y>::access_node(&idle(), None));
        assert_eq!(nodes.len(), 4);
        assert!(nodes
            .iter()
            .filter(|n| n.role == AriaRole::Button)
            .count()
            .eq(&3));
        assert!(nodes
            .iter()
            .any(|n| n.tag == STATUS_TAG && n.role == AriaRole::Status));
    }
}
