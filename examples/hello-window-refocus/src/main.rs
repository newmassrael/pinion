// R1463 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the narrative carries many proper-noun identifiers
// (FocusManager, WidgetView, ShellCore, …).
#![allow(clippy::doc_markdown)]

//! `hello-window-refocus` — R1463 §5.39 §5.16 an explicit focus request
//! reaches a node that appeared in a **secondary window**.
//!
//! ## Why this binding exists
//!
//! [`hello-modal-refocus`](../../../../examples/hello-modal-refocus) (R1462)
//! proved *which* mailbox wins when a modal restore and an explicit request
//! land in one dispatch. This one holds the winner fixed and varies the other
//! axis: **which window the named node lives in**.
//!
//! Scene-derived focus (R1020) enumerates focusable nodes from the painted
//! scene, so a node this dispatch just made paintable is not in the enumeration
//! yet — the paint that would enumerate it has not run. The shell handles that
//! by re-deriving the enumeration from a fresh, side-effect-free view run and
//! retrying the request once. That is what makes the everyday reducer work:
//!
//! ```ignore
//! editing().set(Editing::Title);                    // the editor becomes paintable
//! pinion_core::focus_request::request(TITLE_EDITOR); // ...and focus belongs in it
//! ```
//!
//! Until R1463 that retry re-derived **the primary window only**. A window that
//! has painted answers the enumeration from its harvested cache, so a secondary
//! window's contribution stayed frozen at its last paint: the identical reducer
//! landed in the main window and was silently dropped in the notes window — the
//! one-shot request consumed by the miss, focus left wherever it was. Same
//! binding, same intent, different window, different outcome.
//!
//! ## The two branches, both painted here
//!
//! Two OS windows share one binding. The **main** window carries both triggers,
//! so a single input path drives both branches and the only difference between
//! them is where the editor appears:
//!
//! - **Edit title** — the editor appears in the MAIN window. The control: this
//!   worked before R1463, because the retry re-derived exactly that window.
//! - **Edit note** — the editor appears in the NOTES window. The round's
//!   branch: pre-R1463 focus stayed on the trigger and the notes editor, though
//!   painted and focusable, never received it.
//!
//! Neither branch is visible in the picture — the editor paints either way. The
//! whole defect is the value of `focus/get`, which is what
//! `tools/demos/r1463_window_refocus.py` asserts on.
//!
//! ## §2 AI-first
//!
//! There is no bespoke introspection external here, deliberately: *which editor
//! is open* is scene data already (§2 #7 — a `{window}`-scoped
//! `scene/snapshot` shows the editor node in the window that paints it), and
//! *where focus is* is `focus/get`, whose `tab_order` is the binding-wide union
//! across both windows. The `hello-floating-chart` precedent: a window is
//! first-class introspectable data, so a multi-window binding adds no AT
//! scaffolding to be discoverable.

use pinion_a11y::WidgetA11y;
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
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, WindowSpec, vello_renderer_impl};
use pinion_widget_paint::button::{
    SurfaceAction, read_button_focused, read_button_state, surface_action_scene,
};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloWindowRefocusRenderer, HelloWindowRefocusRendererError);

const WIN_W: u32 = 420;
const WIN_H: u32 = 260;
const NOTES_W: u32 = 360;
const NOTES_H: u32 = 220;
const THEME_TAG: &str = "app";

/// The primary window: both triggers and the title editor.
const MAIN_WINDOW: &str = "main";
/// The secondary window: the notes pane and the note editor. A plain declared
/// window rather than a tear-off, because the tear-off machinery is orthogonal
/// — what matters is that this window has PAINTED, which is what makes its
/// focus enumeration a cache rather than a per-fold derivation.
const NOTES_WINDOW: &str = "notes";

/// Trigger — opens the editor in the window it is painted in (the control).
const EDIT_TITLE_TAG: &str = "edit_title";
/// Trigger — opens the editor in the OTHER window (the round's branch).
const EDIT_NOTE_TAG: &str = "edit_note";
/// The main window's inline editor; paintable only while [`Editing::Title`].
const TITLE_EDITOR_TAG: &str = "title_editor";
/// The notes window's inline editor; paintable only while [`Editing::Note`].
const NOTE_EDITOR_TAG: &str = "note_editor";
/// The notes window's permanent focusable — the pane the editor opens inside,
/// and the tag that proves the notes window contributes to the Tab order at all.
const NOTES_PANE_TAG: &str = "notes_pane";
/// Per-window status line; both windows paint their own copy.
const STATUS_TAG: &str = "refocus_status";

const EDITING_KEY: &str = "hello_window_refocus.editing";
const HOVER_KEYS: [&str; 2] = [
    "hello_window_refocus.hover.title",
    "hello_window_refocus.hover.note",
];
const TAGS: [&str; 2] = [EDIT_TITLE_TAG, EDIT_NOTE_TAG];

const BTN_W: u32 = 240;
const BTN_H: u32 = 48;
const EDITOR_W: u32 = 260;
const EDITOR_H: u32 = 44;

/// Which inline editor is open — at most one, since a binding editing two
/// things at once would confuse *which* request the demo is measuring.
/// `Serialize`/`Deserialize` because every [`Signal`] value is snapshot-
/// restorable by contract, not because this binding persists anything.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
enum Editing {
    None,
    Title,
    Note,
}

impl Editing {
    /// The status line each window paints, so the open editor is readable as
    /// text as well as structure.
    fn label(self) -> &'static str {
        match self {
            Self::None => "No editor open.",
            Self::Title => "Editing the title (main window).",
            Self::Note => "Editing the note (notes window).",
        }
    }
}

/// The reducer-owned gate that makes an editor paintable. `Owner::cache`d so
/// the reducer, both `view_for_window` arms and the shell's re-derive all
/// resolve the SAME signal.
fn editing() -> Rc<Signal<Editing>> {
    Owner::current()
        .expect("hello-window-refocus: runs inside the substrate owner scope")
        .cache(EDITING_KEY, || Signal::new(Editing::None))
}

/// Open the main window's editor and name it. The control branch.
fn edit_title() {
    batch(|| {
        editing().set(Editing::Title);
        pinion_core::focus_request::request(TITLE_EDITOR_TAG);
    });
}

/// Open the NOTES window's editor and name it — byte-for-byte the same shape
/// as [`edit_title`], differing only in which window paints the named node.
/// The binding compensates for nothing: no deferred re-request, no waiting a
/// frame for the other window to paint, no `window` parameter. It sets state
/// and names a tag, exactly as the single-window case does.
fn edit_note() {
    batch(|| {
        editing().set(Editing::Note);
        pinion_core::focus_request::request(NOTE_EDITOR_TAG);
    });
}

/// Close whichever editor is open and return focus to the trigger that opened
/// it — the same idiom once more, so the return path is not a special case.
fn close_editor() {
    let back = match editing().get() {
        Editing::Title => EDIT_TITLE_TAG,
        Editing::Note => EDIT_NOTE_TAG,
        Editing::None => return,
    };
    batch(|| {
        editing().set(Editing::None);
        pinion_core::focus_request::request(back);
    });
}

/// A focusable inline editor panel. A real binding would put a `TextField`
/// here; what the round is about is not text editing but whether the focus
/// enumeration can SEE the node, so the panel stays a bare focusable surface
/// with a caret glyph. Focusability is the view fn's own
/// `LayoutStyle::focusable` mark — the thing `Scene::collect_focusable_tags`
/// reads — so this node joins the Tab order by being painted, in whichever
/// window paints it.
fn editor_panel(tag: &'static str, label: &str, theme: &Theme) -> Scene {
    let text = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new()
            .with_size_px(14)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![text])
            .with_tag(tag)
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerHighest),
            ))
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(EDITOR_W, EDITOR_H))
                    .with_focusable(true)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center),
            ),
    )
}

/// `[edit_title, edit_note]` postures plus their R694 keyboard-focus flags,
/// read back from the state scene. Both triggers live in the main window, so
/// the single-window state read covers them.
type RefocusState = ([ButtonState; 2], [bool; 2]);

fn status_line(editing_now: Editing, window_id: &str, theme: &Theme) -> Scene {
    Scene::Text(
        TextNode::styled(
            format!("{window_id}: {}", editing_now.label()),
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag(STATUS_TAG),
    )
}

fn column(children: Vec<Scene>, theme: &Theme) -> Scene {
    Scene::Container(
        ContainerNode::new(children)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_gap(14),
            ),
    )
}

/// The main window: the two triggers, and the title editor while it is open.
fn view_main(state: RefocusState, theme: &Theme) -> Scene {
    let (postures, _focused) = state;
    let editing_now = editing().get();
    let trigger = |i: usize, label: &str| {
        surface_action_scene(
            &SurfaceAction {
                tag: TAGS[i],
                label,
                state: postures[i],
                hover_key: HOVER_KEYS[i],
                size: Size::px(BTN_W, BTN_H),
                focusable: true,
            },
            theme,
        )
    };

    let mut children = vec![
        trigger(0, "Edit title (this window)"),
        trigger(1, "Edit note (notes window)"),
    ];
    if editing_now == Editing::Title {
        children.push(editor_panel(TITLE_EDITOR_TAG, "title: \u{2588}", theme));
    }
    children.push(status_line(editing_now, MAIN_WINDOW, theme));
    column(children, theme)
}

/// The notes window: its permanent pane, and the note editor while it is open.
fn view_notes(theme: &Theme) -> Scene {
    let editing_now = editing().get();
    let pane = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            "Notes",
            Rect::default(),
            TextStyle::new()
                .with_size_px(15)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))])
        .with_tag(NOTES_PANE_TAG)
        .with_style(BoxStyle::filled(
            theme.resolve(ColorRole::SurfaceContainerLow),
        ))
        .with_layout(
            LayoutStyle::new()
                .with_size(Size::px(EDITOR_W, EDITOR_H))
                .with_focusable(true)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center),
        ),
    );

    let mut children = vec![pane];
    if editing_now == Editing::Note {
        children.push(editor_panel(NOTE_EDITOR_TAG, "note: \u{2588}", theme));
    }
    children.push(status_line(editing_now, NOTES_WINDOW, theme));
    column(children, theme)
}

struct WindowRefocusView;

impl WidgetCore for WindowRefocusView {
    type State = RefocusState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![ExtraExternal::new(
            EDIT_NOTE_TAG,
            Box::new(ButtonExternal::new()),
        )]
    }

    fn tag() -> &'static str {
        EDIT_TITLE_TAG
    }

    fn read_state(scene: &Scene) -> RefocusState {
        (
            TAGS.map(|t| read_button_state(scene, t)),
            TAGS.map(|t| read_button_focused(scene, t)),
        )
    }

    fn view(state: RefocusState, _frame: &Frame) -> Scene {
        // Single-window fallback (tests / unscoped RPC): the live loop always
        // calls `view_for_window`, and the shell's focus re-derive routes the
        // primary through here.
        view_main(state, &use_theme(THEME_TAG).theme_animated())
    }

    fn event_name((): ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-window-refocus (R1463 §5.39)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// Escape closes the open editor and names where focus returns; Enter /
    /// Space activate a focused trigger.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if key == "Escape" {
            if editing().get() == Editing::None {
                return false;
            }
            close_editor();
            return true;
        }
        TAGS.iter()
            .any(|t| apply_aria_activate(scene, focused, key, t))
    }

    /// Bridge the triggers' `"<tag>.click"` intents. Both arms open an editor
    /// and name it in ONE dispatch — the shape the round is about.
    fn update(
        _state: RefocusState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            "edit_title.click" => edit_title(),
            "edit_note.click" => edit_note(),
            _ => {}
        }
        Vec::new()
    }

    fn fmt_state_log(state: &RefocusState) -> String {
        format!("triggers={:?}", state.0)
    }
}

// Default a11y surface: §2 #7 discovery here is per-window scene data (an
// editor node inside the window that paints it) plus `focus/get`, the
// `hello-floating-chart` precedent — a window is already first-class
// introspectable, so this binding adds no bespoke AccessNodes.
impl WidgetA11y for WindowRefocusView {}

impl WidgetView for WindowRefocusView {
    type Renderer = HelloWindowRefocusRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }

    fn windows() -> Vec<WindowSpec> {
        vec![
            WindowSpec::new(
                MAIN_WINDOW,
                "hello-window-refocus — Main",
                SizeStrategy::Fixed {
                    width: WIN_W,
                    height: WIN_H,
                },
            ),
            WindowSpec::new(
                NOTES_WINDOW,
                "hello-window-refocus — Notes",
                SizeStrategy::Fixed {
                    width: NOTES_W,
                    height: NOTES_H,
                },
            ),
        ]
    }

    fn view_for_window(window_id: &str, state: Self::State, frame: &Frame) -> Scene {
        if window_id == NOTES_WINDOW {
            view_notes(&use_theme(THEME_TAG).theme_animated())
        } else {
            Self::view(state, frame)
        }
    }
}

fn main() {
    pinion_shell::run::<WindowRefocusView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Intent;
    use pinion_core::external::IntrospectValue;
    use pinion_shell::ShellCore;

    fn idle() -> RefocusState {
        ([ButtonState::Idle; 2], [false; 2])
    }

    /// Drive an intent the way a real click does — through
    /// `ShellCore::dispatch_intent`, which runs the reducer AND the dispatch
    /// tail that drains the focus mailbox. Calling the reducer fn directly
    /// would skip the very step under test.
    fn click(core: &mut ShellCore<WindowRefocusView>, tag: &'static str) {
        core.dispatch_intent(&Intent::new_static(tag, IntrospectValue::Null));
    }

    fn tags_of(scene: &Scene) -> Vec<String> {
        scene.collect_focusable_tags()
    }

    /// Each window paints its own editor and only its own — the precondition
    /// for the round: the named node is enumerable ONLY from the window that
    /// paints it, so a primary-only re-derive cannot see the notes editor.
    #[test]
    fn each_window_paints_only_its_own_editor() {
        let owner = Owner::new();
        owner.run(|| {
            editing().set(Editing::Note);
            let main = view_main(idle(), &use_theme(THEME_TAG).theme_animated());
            let notes = view_notes(&use_theme(THEME_TAG).theme_animated());
            assert!(
                !tags_of(&main).contains(&NOTE_EDITOR_TAG.to_owned()),
                "the note editor is not in the main window's scene",
            );
            assert!(
                tags_of(&notes).contains(&NOTE_EDITOR_TAG.to_owned()),
                "the note editor is in the notes window's scene",
            );

            editing().set(Editing::Title);
            let main = view_main(idle(), &use_theme(THEME_TAG).theme_animated());
            let notes = view_notes(&use_theme(THEME_TAG).theme_animated());
            assert!(tags_of(&main).contains(&TITLE_EDITOR_TAG.to_owned()));
            assert!(!tags_of(&notes).contains(&TITLE_EDITOR_TAG.to_owned()));
        });
    }

    /// The round, through the real `ShellCore` per-window paint pipeline: both
    /// windows paint, then ONE dispatch opens the notes editor and names it.
    /// Pre-R1463 the retry re-derived the primary only, the notes window
    /// answered from its stale harvest, and focus stayed on the trigger.
    #[test]
    fn a_request_lands_in_the_secondary_window_that_painted() {
        let mut core = ShellCore::<WindowRefocusView>::new();
        core.compute_paint_scene(WIN_W, WIN_H);
        core.compute_paint_scene_for_window(NOTES_WINDOW, NOTES_W, NOTES_H);

        // The notes pane joined the Tab order by being painted in ITS window.
        assert!(
            core.focus().tab_order().iter().any(|t| t == NOTES_PANE_TAG),
            "the secondary window contributes to the binding-wide enumeration",
        );
        assert!(
            !core
                .focus()
                .tab_order()
                .iter()
                .any(|t| t == NOTE_EDITOR_TAG),
            "precondition: the editor is not enumerated before the reducer runs",
        );

        click(&mut core, "edit_note.click");

        assert_eq!(
            core.focus().focused(),
            Some(NOTE_EDITOR_TAG),
            "the request names a node the same dispatch made paintable in the \
             notes window, and it lands",
        );
    }

    /// The control: the identical reducer shape targeting the PRIMARY window.
    /// It passed before R1463 too — which is the point, since the defect was
    /// the asymmetry between the two, not the mechanism.
    #[test]
    fn the_same_shape_lands_in_the_primary_window() {
        let mut core = ShellCore::<WindowRefocusView>::new();
        core.compute_paint_scene(WIN_W, WIN_H);
        core.compute_paint_scene_for_window(NOTES_WINDOW, NOTES_W, NOTES_H);

        click(&mut core, "edit_title.click");

        assert_eq!(core.focus().focused(), Some(TITLE_EDITOR_TAG));
    }

    /// Closing names the trigger it came from, so the return path is the same
    /// idiom — and the closed editor leaves the enumeration with the paint.
    #[test]
    fn closing_the_notes_editor_returns_focus_to_its_trigger() {
        let mut core = ShellCore::<WindowRefocusView>::new();
        core.compute_paint_scene(WIN_W, WIN_H);
        core.compute_paint_scene_for_window(NOTES_WINDOW, NOTES_W, NOTES_H);

        click(&mut core, "edit_note.click");
        assert_eq!(core.focus().focused(), Some(NOTE_EDITOR_TAG));

        // Escape is the close path; it names the trigger it came from.
        core.apply_key("Escape");
        assert_eq!(
            core.focus().focused(),
            Some(EDIT_NOTE_TAG),
            "focus returns to the trigger the binding named",
        );

        // The next paint of each window drops the editor from the enumeration.
        core.compute_paint_scene(WIN_W, WIN_H);
        core.compute_paint_scene_for_window(NOTES_WINDOW, NOTES_W, NOTES_H);
        assert!(
            !core
                .focus()
                .tab_order()
                .iter()
                .any(|t| t == NOTE_EDITOR_TAG),
            "the closed editor leaves the Tab order with the paint",
        );
    }
}
