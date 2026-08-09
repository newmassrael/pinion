// R1462 §5.16 — example bindings tolerate looser doc-markdown lints
// than substrate crates; the narrative carries many proper-noun
// identifiers (FocusManager, message box, ModalState, …).
#![allow(clippy::doc_markdown)]

//! `hello-modal-refocus` — R1462 §5.16 §5.39 §5.50 a modal that hands
//! focus somewhere the binding names.
//!
//! ## Why this binding exists
//!
//! [`hello-dialog`](../../../../examples/hello-dialog) proves one modal
//! opening and closing.
//! [`hello-modal-handoff`](../../../../examples/hello-modal-handoff)
//! proves two modal surfaces trading places in one user action. This one
//! proves the third shape, and the one every command palette is made of:
//! a modal row that **dismisses itself, runs a command, and then says
//! where focus belongs** — because the command changed what is on screen
//! and the automatic restore no longer points anywhere useful.
//!
//! Closing a modal focus trap restores the invoker: WAI-ARIA requires it,
//! and `FocusManager::pop_modal_scope` does it unconditionally. That is
//! the right *default* and the wrong *verdict*. The two mailboxes carry
//! different kinds of thing:
//!
//! - [`modal_scope_request`](pinion_core::modal_scope_request) carries
//!   **automatic policy** — a restore target the substrate snapshotted on
//!   its own, which the binding never named.
//! - [`focus_request`](pinion_core::focus_request) carries **explicit
//!   intent** — the binding naming a tag.
//!
//! Until R1462 the shell drained intent first and policy second, so the
//! restore always won. A palette row could not put focus on what it
//! produced. Reported live from a downstream consumer (PINION-PR78); this
//! binding is the forcing consumer for the fix, and
//! `tools/demos/r1462_modal_refocus.py` drives both of its branches over
//! RPC.
//!
//! ## The two branches, both painted here
//!
//! The palette has two rows, chosen so the invoker survives one and dies
//! in the other — the field report's two table rows, made clickable:
//!
//! - **Focus the field** — a command that changes nothing structural. The
//!   invoker (`open_palette`) is still on screen, so the automatic
//!   restore has a valid target and lands on it. Pre-R1462 that beat the
//!   row's request and focus went to the *palette trigger* instead of the
//!   field the command was about.
//! - **Show results** — a command that replaces the view. The invoker
//!   leaves the scene with it, so the restore resolves to nothing and
//!   commits `None`: **no keyboard focus anywhere**. This is the branch
//!   the report calls the worse one, and it is the common one — a command
//!   that changes the view is exactly the command whose invoker vanishes.
//!
//! Both rows write `close()` and then `focus_request::request(...)` from
//! one intent. Nothing compensates for the substrate: no deferred signal,
//! no next-frame re-request, no paint-time backstop. The binding says
//! what happened and the tail resolves it.
//!
//! ## §2 AI-first
//!
//! The palette carries a query-only modal introspect
//! (`/palette_state/external/open`), so an agent can ask whether the
//! surface is up without reading pixels; which *view* is painted is read
//! from the scene tree itself (§2 #7), because that is where a view swap
//! actually lives — it has no lifecycle object to query. `focus/get`
//! reports the landing tag and the active enumeration at every step,
//! which is the whole observable this round is about: both branches of
//! the defect are a wrong `focused` value, not a wrong picture.

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::External;
use pinion_core::reactive::{Owner, Signal, batch};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::widgets::modal::{ModalState, modal_introspection_extra, use_modal};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::button::{
    SurfaceAction, button_a11y_state, read_button_state, surface_action_scene,
};
use pinion_widget_paint::dialog::{DialogContent, DialogStyle, view_dialog};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloModalRefocusRenderer, HelloModalRefocusRendererError);

const WIN_W: u32 = 560;
const WIN_H: u32 = 380;
const THEME_TAG: &str = "app";

/// The palette trigger and **the invoker** — the tag `push_modal_scope`
/// snapshots for restore. Painted in the editor view only, so the
/// view-switching row destroys it.
const TRIGGER_TAG: &str = "open_palette";
/// The editor view's content control. Target of the row whose command
/// leaves the invoker alive.
const FIELD_TAG: &str = "editor_field";
/// The results view's content control. Target of the row whose command
/// takes the invoker with it.
const RESULT_TAG: &str = "result_row";
/// The results view's way back to the editor. Keeps the second view from
/// being a dead end (and lets the demo re-run both branches).
const BACK_TAG: &str = "back_to_editor";
/// Palette row — the command that changes nothing structural.
const ROW_FIELD_TAG: &str = "row_focus_field";
/// Palette row — the command that replaces the view.
const ROW_RESULTS_TAG: &str = "row_show_results";

const PALETTE_SCRIM_TAG: &str = "palette_scrim";
const PALETTE_PANEL_TAG: &str = "palette_panel";

/// R795 query-only introspect — is the palette up?
const PALETTE_STATE_TAG: &str = "palette_state";

const PALETTE_KEY: &str = "hello_modal_refocus.palette";
const RESULTS_KEY: &str = "hello_modal_refocus.results";
const ACTION_KEY: &str = "hello_modal_refocus.action";

const HOVER_KEYS: [&str; 6] = [
    "hello_modal_refocus.hover.trigger",
    "hello_modal_refocus.hover.field",
    "hello_modal_refocus.hover.result",
    "hello_modal_refocus.hover.back",
    "hello_modal_refocus.hover.row_field",
    "hello_modal_refocus.hover.row_results",
];

const CONTROL_W: u32 = 220;
const CONTROL_H: u32 = 52;
const ROW_W: u32 = 200;
const ROW_H: u32 = 44;

/// The palette's focusable rows, in Tab order.
fn palette_members() -> Vec<String> {
    vec![ROW_FIELD_TAG.to_string(), ROW_RESULTS_TAG.to_string()]
}

fn palette() -> Rc<ModalState> {
    use_modal(PALETTE_KEY)
}

/// Which view is painted: `true` = results, `false` = editor.
///
/// Deliberately a bare [`Signal`] and **not** a [`ModalState`]. The view
/// swap is not a modal: it pushes no focus scope and traps nothing.
/// `ModalState::open` raises its flag *and* writes
/// [`modal_scope_request::open`](pinion_core::modal_scope_request::open)
/// — they are inseparable by construction, which is the entire reason
/// that type exists — so reusing it here for a plain boolean would push
/// an empty modal scope and clear focus on every view change. An agent
/// reads the current view from the painted scene instead (§2 #7), which
/// is where the view actually lives.
fn results_open() -> Rc<Signal<bool>> {
    Owner::current()
        .expect("results_open requires an active Owner scope")
        .cache(RESULTS_KEY, || Signal::new(false))
}

/// The last completed command, for the status line.
fn use_last_action() -> Rc<Signal<Option<String>>> {
    Owner::current()
        .expect("use_last_action requires an active Owner scope")
        .cache(ACTION_KEY, || Signal::new(None::<String>))
}

fn record(action: &str) {
    use_last_action().set(Some(action.to_owned()));
}

/// Open the palette over its rows. The invoker is whatever holds focus
/// now — in practice the trigger the user just activated.
fn open_palette() {
    palette().open(palette_members());
}

/// Row 1 — a command that changes nothing structural, then names its
/// target. The invoker survives, so the automatic restore has a valid
/// target and is a genuine competitor: pre-R1462 focus landed on
/// [`TRIGGER_TAG`] instead of [`FIELD_TAG`].
fn run_focus_field() {
    batch(|| {
        record("Focused the field.");
        palette().close();
        pinion_core::focus_request::request(FIELD_TAG);
    });
}

/// Row 2 — a command that replaces the view, then names its target. The
/// invoker leaves the scene with the editor view, so the automatic
/// restore resolves to nothing and commits `None`: pre-R1462 the window
/// was left with no keyboard focus at all.
///
/// Note what is NOT here: the binding does not check whether the invoker
/// survived, does not re-request on the next frame, does not consult the
/// focus manager. It closes, it switches, it names a tag.
fn run_show_results() {
    batch(|| {
        record("Showed results.");
        palette().close();
        results_open().set(true);
        pinion_core::focus_request::request(RESULT_TAG);
    });
}

/// Return to the editor view. Not a modal path — a plain view swap whose
/// focus target is named the same way, so the binding has one idiom for
/// "the view changed, focus goes here".
fn back_to_editor() {
    batch(|| {
        record("Back to the editor.");
        results_open().set(false);
        pinion_core::focus_request::request(TRIGGER_TAG);
    });
}

/// `[trigger, field, result, back, row_field, row_results]` postures plus
/// their R694 keyboard-focus flags, read back from the state scene.
type RefocusState = [ButtonState; 6];

const TAGS: [&str; 6] = [
    TRIGGER_TAG,
    FIELD_TAG,
    RESULT_TAG,
    BACK_TAG,
    ROW_FIELD_TAG,
    ROW_RESULTS_TAG,
];

/// view-fn (§6.3): pure sync `(button postures) -> Scene`. Exactly one of
/// the two views is painted, and the palette overlays whichever is up.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: RefocusState, _frame: &Frame) -> Scene {
    let postures = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let palette_open = palette().is_open();
    let results = results_open().get();

    // Indices 0..=3 are background controls, focusable while their view is
    // painted. Indices 4/5 are palette rows — focusable only while the
    // trap is up, so the base enumeration must not list them.
    let control = |i: usize, label: &str, size: Size, focusable: bool| {
        surface_action_scene(
            &SurfaceAction {
                tag: TAGS[i],
                label,
                state: postures[i],
                hover_key: HOVER_KEYS[i],
                size,
                focusable,
            },
            &theme,
        )
    };

    let body = if results {
        vec![
            control(2, "Result row", Size::px(CONTROL_W, CONTROL_H), true),
            control(3, "Back to editor", Size::px(CONTROL_W, CONTROL_H), true),
        ]
    } else {
        vec![
            control(0, "Commands\u{2026}", Size::px(CONTROL_W, CONTROL_H), true),
            control(1, "Editor field", Size::px(CONTROL_W, CONTROL_H), true),
        ]
    };

    let status_label = use_last_action()
        .get()
        .unwrap_or_else(|| "No command run yet.".to_owned());
    let status = Scene::Text(TextNode::styled(
        &status_label,
        Rect::default(),
        TextStyle::new()
            .with_size_px(14)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    let mut column = body;
    column.push(status);
    let content = Scene::Container(
        ContainerNode::new(column).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                .with_flex_grow(1.0)
                .with_gap(16),
        ),
    );

    let mut children = vec![content];
    if palette_open {
        children.push(view_dialog(
            PALETTE_SCRIM_TAG,
            PALETTE_PANEL_TAG,
            DialogContent {
                title: "Commands",
                message: "Run a command. Each one says where focus goes next.",
                body: None,
            },
            vec![
                control(4, "Focus the field", Size::px(ROW_W, ROW_H), true),
                control(5, "Show results", Size::px(ROW_W, ROW_H), true),
            ],
            (WIN_W, WIN_H),
            &theme,
            &DialogStyle::m3_default(),
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

struct RefocusView;

impl WidgetCore for RefocusView {
    type State = RefocusState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(FIELD_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(RESULT_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(BACK_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(ROW_FIELD_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(ROW_RESULTS_TAG, Box::new(ButtonExternal::new())),
            modal_introspection_extra(PALETTE_STATE_TAG, palette()),
        ]
    }

    fn tag() -> &'static str {
        TRIGGER_TAG
    }

    fn read_state(scene: &Scene) -> RefocusState {
        TAGS.map(|t| read_button_state(scene, t))
    }

    fn view(state: RefocusState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name((): ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-modal-refocus (R1462 §5.39)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// Escape dismisses the palette (the shell routes Escape here only
    /// while a trap is active) WITHOUT naming a focus target — the plain
    /// dismiss path, where the automatic restore is the whole answer and
    /// must still work. Enter / Space activate the focused button.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if key == "Escape" {
            if palette().is_open() {
                record("Dismissed.");
                palette().close();
                return true;
            }
            return false;
        }
        TAGS.iter()
            .any(|t| apply_aria_activate(scene, focused, key, t))
    }

    /// Bridge the buttons' `"<tag>.click"` intents into the palette
    /// lifecycle. The two row arms are the round's subject: one intent
    /// writing a modal edit AND a focus request.
    fn update(
        _state: RefocusState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            "open_palette.click" => open_palette(),
            "row_focus_field.click" => run_focus_field(),
            "row_show_results.click" => run_show_results(),
            "back_to_editor.click" => back_to_editor(),
            _ => {}
        }
        Vec::new()
    }

    fn fmt_state_log(state: &RefocusState) -> String {
        format!("postures={state:?}")
    }
}

impl WidgetA11y for RefocusView {
    /// WAI-ARIA tree. Palette up: the `aria-modal` dialog root owning its
    /// two rows. Palette down: whichever view's two controls are painted
    /// — an AT user is never told about a control the current view does
    /// not show.
    fn access_node(state: &RefocusState, focused: Option<&str>) -> Vec<AccessNode> {
        let postures = state;
        let button = |i: usize| {
            AccessNode::new(TAGS[i], AriaRole::Button)
                .with_state(button_a11y_state(postures[i], focused == Some(TAGS[i])))
        };
        if palette().is_open() {
            return vec![
                AccessNode::new(PALETTE_PANEL_TAG, AriaRole::Dialog)
                    .with_modal()
                    .with_child(ROW_FIELD_TAG)
                    .with_child(ROW_RESULTS_TAG),
                button(4).with_position_in_set(1).with_size_of_set(2),
                button(5).with_position_in_set(2).with_size_of_set(2),
            ];
        }
        if results_open().get() {
            return vec![button(2), button(3)];
        }
        vec![button(0), button(1)]
    }
}

impl WidgetView for RefocusView {
    type Renderer = HelloModalRefocusRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<RefocusView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::modal_scope_request::{self, ModalRequest};

    fn idle() -> RefocusState {
        [ButtonState::Idle; 6]
    }

    fn intent(tag: &str) -> pinion_core::Intent {
        pinion_core::Intent::new_owned(
            tag.to_string(),
            pinion_core::external::IntrospectValue::Null,
        )
    }

    /// A fresh Owner scope with both mailboxes drained, so each test reads
    /// only what its own reducer call wrote.
    fn scope() -> Owner {
        let owner = Owner::new();
        owner.run(|| {
            let _ = modal_scope_request::drain();
            let _ = pinion_core::focus_request::drain();
        });
        owner
    }

    /// The round's shape, at the mailbox level: ONE intent writes a modal
    /// edit AND a focus request. What the shell does with the pair is the
    /// substrate's business (`pinion_shell::substrate::
    /// modal_tail_focus_tests`); what the binding must do is write both
    /// from one frame, without arbitrating between them itself.
    #[test]
    fn a_row_writes_both_a_modal_edit_and_a_focus_request() {
        let owner = scope();
        owner.run(|| {
            let _ = RefocusView::update(idle(), &intent("open_palette.click"));
            assert_eq!(
                modal_scope_request::drain(),
                vec![ModalRequest::Open {
                    members: palette_members()
                }],
            );
            assert_eq!(
                pinion_core::focus_request::drain(),
                None,
                "opening names no target — the trap auto-focuses its first row",
            );

            let _ = RefocusView::update(idle(), &intent("row_focus_field.click"));
            assert_eq!(
                modal_scope_request::drain(),
                vec![ModalRequest::Close],
                "the row dismisses the palette",
            );
            assert_eq!(
                pinion_core::focus_request::drain().as_deref(),
                Some(FIELD_TAG),
                "and names where focus belongs, in the SAME frame",
            );
        });
    }

    /// The view-replacing row destroys the invoker in the same frame it
    /// names its target — the branch where the automatic restore has
    /// nothing to land on.
    #[test]
    fn the_view_switching_row_takes_the_invoker_with_it() {
        let owner = scope();
        owner.run(|| {
            let _ = RefocusView::update(idle(), &intent("open_palette.click"));
            let _ = modal_scope_request::drain();

            let _ = RefocusView::update(idle(), &intent("row_show_results.click"));
            assert_eq!(modal_scope_request::drain(), vec![ModalRequest::Close]);
            assert_eq!(
                pinion_core::focus_request::drain().as_deref(),
                Some(RESULT_TAG),
            );
            assert!(results_open().get(), "the view really did swap");

            // The invoker is painted by the editor view only, so it is gone
            // from the post-command enumeration — which is precisely what
            // makes `pop_modal_scope`'s restore resolve to nothing. Asserted
            // against `collect_focusable_tags`, the same walk the shell's
            // focus enumeration is derived from, rather than against paint
            // presence in general.
            let scene = RefocusView::view(idle(), &Frame::with_dt(0.0));
            let focusable = scene.collect_focusable_tags();
            assert!(
                !focusable.iter().any(|t| t == TRIGGER_TAG),
                "the palette trigger left the enumeration with the editor \
                 view; got {focusable:?}",
            );
            assert!(
                focusable.iter().any(|t| t == RESULT_TAG),
                "the results view's control is enumerable; got {focusable:?}",
            );
        });
    }

    /// Escape is the control: dismissing without naming a target writes a
    /// modal edit and NO request, so the automatic restore is the entire
    /// answer and must keep working.
    #[test]
    fn a_plain_dismiss_names_no_target() {
        let owner = scope();
        owner.run(|| {
            let _ = RefocusView::update(idle(), &intent("open_palette.click"));
            let _ = modal_scope_request::drain();

            let mut scene = RefocusView::view(idle(), &Frame::with_dt(0.0));
            assert!(RefocusView::apply_key(
                &mut scene,
                Some(ROW_FIELD_TAG),
                "Escape",
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(modal_scope_request::drain(), vec![ModalRequest::Close]);
            assert_eq!(
                pinion_core::focus_request::drain(),
                None,
                "a plain dismiss must not override the invoker restore",
            );
        });
    }
}
