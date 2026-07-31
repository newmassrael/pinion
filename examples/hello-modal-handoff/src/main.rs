// R1456 §5.16 — example bindings tolerate looser doc-markdown lints
// than substrate crates; the narrative carries many proper-noun
// identifiers (FocusManager, QMessageBox, ModalState, …).
#![allow(clippy::doc_markdown)]

//! `hello-modal-handoff` — R1456 §5.16 §5.39 §5.50 modal **handoff**.
//!
//! ## Why this binding exists
//!
//! [`hello-dialog`](../../../../examples/hello-dialog) proves one modal
//! opening and closing. This one proves the shape that has **two** modal
//! surfaces trading places inside a single user action: a modal command
//! menu whose destructive row dismisses the menu *and* raises a confirm
//! dialog. Qt spells it `QMenu` action → `QMessageBox::question`;
//! VS Code spells it palette command → confirmation; every toolkit ships
//! it, because "are you sure?" is not a separate click.
//!
//! The distinction that matters is **handoff vs nesting**:
//!
//! - *Nesting* stacks the confirm ON TOP of a still-open menu. Two user
//!   actions, two dispatch frames, two `open` requests.
//! - *Handoff* replaces the menu WITH the confirm. One user action, so
//!   one dispatch frame carrying `close` **and** `open` — the reducer
//!   below writes both from one intent.
//!
//! Until R1456 the [`modal_scope_request`](pinion_core::modal_scope_request)
//! mailbox was a single last-write-wins slot, so the `close` was
//! overwritten and the menu's focus scope stayed on `FocusManager`'s
//! modal stack forever. The visible damage was not in the dialog: it
//! appeared *after* the dialog was answered, when the background became
//! permanently unfocusable and every `focus/set` was refused. This
//! binding is the forcing consumer for that fix, and
//! `tools/demos/r1456_modal_handoff.py` drives the whole arc over RPC.
//!
//! ## Architecture
//!
//! Five real focusable [`ButtonExternal`]s — the trigger (`open_menu`,
//! the primary external) plus the menu's two rows and the confirm's two
//! actions (extra externals). Two [`ModalState`]s hold the open-lifecycle
//! of the two surfaces; each `open` / `close` moves its flag and its
//! focus-trap request in lockstep. Input → External → intent → reducer →
//! Signal → view, with no reducer→widget back-channel, exactly like
//! `hello-dialog`.
//!
//! ## §2 AI-first
//!
//! Both surfaces carry a query-only modal introspect
//! (`/menu_state/external/open`, `/confirm_state/external/open`), so an
//! agent can ask which modal is up without reading pixels, and
//! `focus/get` reports the active trap enumeration at every step.

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
vello_renderer_impl!(HelloModalHandoffRenderer, HelloModalHandoffRendererError);

const WIN_W: u32 = 560;
const WIN_H: u32 = 380;
const THEME_TAG: &str = "app";

/// Trigger button (the primary external). Opens the command menu.
const TRIGGER_TAG: &str = "open_menu";
/// Menu row — the safe command. Closes the menu and nothing else, so its
/// frame carries exactly ONE modal request (the pre-R1456 shape, kept as
/// the regression witness that the batch drain did not disturb it).
const RENAME_TAG: &str = "menu_rename";
/// Menu row — the destructive command. **The handoff**: closes the menu
/// and opens the confirm dialog from one intent.
const DELETE_TAG: &str = "menu_delete";
/// Confirm dialog's cancel action.
const CONFIRM_CANCEL_TAG: &str = "confirm_cancel";
/// Confirm dialog's affirmative (destructive) action.
const CONFIRM_OK_TAG: &str = "confirm_ok";

const MENU_SCRIM_TAG: &str = "menu_scrim";
const MENU_PANEL_TAG: &str = "menu_panel";
const CONFIRM_SCRIM_TAG: &str = "confirm_scrim";
const CONFIRM_PANEL_TAG: &str = "confirm_panel";

/// R795 query-only modal-introspection tags — `open` is readable whether
/// the surface is up or not, for both modals independently.
const MENU_STATE_TAG: &str = "menu_state";
const CONFIRM_STATE_TAG: &str = "confirm_state";

const MENU_KEY: &str = "hello_modal_handoff.menu";
const CONFIRM_KEY: &str = "hello_modal_handoff.confirm";
const ACTION_KEY: &str = "hello_modal_handoff.action";

const HOVER_KEYS: [&str; 5] = [
    "hello_modal_handoff.hover.trigger",
    "hello_modal_handoff.hover.rename",
    "hello_modal_handoff.hover.delete",
    "hello_modal_handoff.hover.cancel",
    "hello_modal_handoff.hover.ok",
];

const TRIGGER_W: u32 = 200;
const TRIGGER_H: u32 = 56;
const ROW_W: u32 = 160;
const ROW_H: u32 = 44;

/// The menu's focusable rows, in Tab order.
fn menu_members() -> Vec<String> {
    vec![RENAME_TAG.to_string(), DELETE_TAG.to_string()]
}

/// The confirm dialog's focusable actions, in Tab order. Cancel first so
/// it is auto-focused — the safe default for a destructive prompt.
fn confirm_members() -> Vec<String> {
    vec![CONFIRM_CANCEL_TAG.to_string(), CONFIRM_OK_TAG.to_string()]
}

fn menu() -> Rc<ModalState> {
    use_modal(MENU_KEY)
}

fn confirm() -> Rc<ModalState> {
    use_modal(CONFIRM_KEY)
}

/// The last completed command, for the status line. `None` until the
/// user finishes one.
fn use_last_action() -> Rc<Signal<Option<String>>> {
    Owner::current()
        .expect("use_last_action requires an active Owner scope")
        .cache(ACTION_KEY, || Signal::new(None::<String>))
}

fn record(action: &str) {
    use_last_action().set(Some(action.to_owned()));
}

/// Open the command menu over its rows.
fn open_menu() {
    menu().open(menu_members());
}

/// The safe row: dismiss the menu, record the command. ONE modal request
/// in this frame.
fn run_rename() {
    batch(|| {
        record("Renamed.");
        menu().close();
    });
}

/// **The handoff.** One intent, one dispatch frame, TWO modal-scope
/// edits: pop the menu's trap, push the confirm's. The order is the
/// meaning — reversing it would install the confirm's trap and then pop
/// it right back off.
///
/// Nothing here compensates for the substrate: no deferred signal, no
/// next-frame arming. A binding expresses the handoff by simply saying
/// what happened, and R1456's ordered mailbox carries both edits.
fn hand_off_to_confirm() {
    menu().close();
    confirm().open(confirm_members());
}

/// Answer the confirm dialog: record the outcome and pop its trap. Focus
/// returns to the trigger — the invoker captured when the *menu* opened,
/// which survived the handoff because the pop/push pair moved through it.
fn close_confirm(accepted: bool) {
    batch(|| {
        record(if accepted {
            "Deleted."
        } else {
            "Deletion cancelled."
        });
        confirm().close();
    });
}

/// `[trigger, rename, delete, cancel, ok]` postures + their R694
/// keyboard-focus flags, read back from the state scene.
type HandoffState = [ButtonState; 5];

const TAGS: [&str; 5] = [
    TRIGGER_TAG,
    RENAME_TAG,
    DELETE_TAG,
    CONFIRM_CANCEL_TAG,
    CONFIRM_OK_TAG,
];

/// view-fn (§6.3): pure sync `(button postures) -> Scene`. At most one
/// overlay is ever pushed — the handoff *replaces* one surface with the
/// other, so the menu and the confirm are never both painted.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: HandoffState, _frame: &Frame) -> Scene {
    let postures = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let menu_open = menu().is_open();
    let confirm_open = confirm().is_open();

    // Index 0 is the background trigger — the only *static* Tab stop.
    // Every other index is a modal member, focusable only while its trap
    // is up, so the base enumeration must not list it.
    let row = |i: usize, label: &str, size: Size| {
        surface_action_scene(
            &SurfaceAction {
                tag: TAGS[i],
                label,
                state: postures[i],
                hover_key: HOVER_KEYS[i],
                size,
                focusable: i == 0,
            },
            &theme,
        )
    };

    let trigger = row(0, "Commands\u{2026}", Size::px(TRIGGER_W, TRIGGER_H));
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
    if menu_open {
        children.push(view_dialog(
            MENU_SCRIM_TAG,
            MENU_PANEL_TAG,
            DialogContent {
                title: "Commands",
                message: "Pick a command to run on the selected file.",
                body: None,
            },
            vec![
                row(1, "Rename", Size::px(ROW_W, ROW_H)),
                row(2, "Delete", Size::px(ROW_W, ROW_H)),
            ],
            (WIN_W, WIN_H),
            &theme,
            &DialogStyle::m3_default(),
        ));
    } else if confirm_open {
        children.push(view_dialog(
            CONFIRM_SCRIM_TAG,
            CONFIRM_PANEL_TAG,
            DialogContent {
                title: "Delete file?",
                message: "This permanently removes the file. This cannot be undone.",
                body: None,
            },
            vec![
                row(3, "Cancel", Size::px(ROW_W, ROW_H)),
                row(4, "Delete", Size::px(ROW_W, ROW_H)),
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

struct HandoffView;

impl WidgetCore for HandoffView {
    type State = HandoffState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(RENAME_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(DELETE_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(CONFIRM_CANCEL_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(CONFIRM_OK_TAG, Box::new(ButtonExternal::new())),
            modal_introspection_extra(MENU_STATE_TAG, menu()),
            modal_introspection_extra(CONFIRM_STATE_TAG, confirm()),
        ]
    }

    fn tag() -> &'static str {
        TRIGGER_TAG
    }

    fn read_state(scene: &Scene) -> HandoffState {
        TAGS.map(|t| read_button_state(scene, t))
    }

    fn view(state: HandoffState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name((): ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-modal-handoff (R1456 §5.39)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// Escape dismisses whichever surface is up (the shell routes Escape
    /// here only while a trap is active). Enter / Space activate the
    /// focused button through the shared ARIA helper.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if key == "Escape" {
            if confirm().is_open() {
                close_confirm(false);
                return true;
            }
            if menu().is_open() {
                menu().close();
                return true;
            }
            return false;
        }
        TAGS.iter()
            .any(|t| apply_aria_activate(scene, focused, key, t))
    }

    /// Bridge the buttons' `"<tag>.click"` intents into the two modal
    /// lifecycles. `menu_delete.click` is the round's subject: one intent
    /// writing two modal-scope edits.
    fn update(
        _state: HandoffState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            "open_menu.click" => open_menu(),
            "menu_rename.click" => run_rename(),
            "menu_delete.click" => hand_off_to_confirm(),
            "confirm_ok.click" => close_confirm(true),
            "confirm_cancel.click" => close_confirm(false),
            _ => {}
        }
        Vec::new()
    }

    fn fmt_state_log(state: &HandoffState) -> String {
        format!("postures={state:?}")
    }
}

impl WidgetA11y for HandoffView {
    /// WAI-ARIA tree. Closed: the trigger button. Menu up / confirm up:
    /// the `aria-modal` dialog root owning that surface's two controls.
    /// The two never coexist, which is the handoff's whole point — an AT
    /// user is never told two modals are open at once.
    fn access_node(state: &HandoffState, focused: Option<&str>) -> Vec<AccessNode> {
        let postures = state;
        let surface = if menu().is_open() {
            Some((MENU_PANEL_TAG, [1_usize, 2_usize]))
        } else if confirm().is_open() {
            Some((CONFIRM_PANEL_TAG, [3, 4]))
        } else {
            None
        };
        let Some((panel, [first, second])) = surface else {
            return vec![
                AccessNode::new(TRIGGER_TAG, AriaRole::Button)
                    .with_state(button_a11y_state(postures[0], focused == Some(TRIGGER_TAG))),
            ];
        };
        let dialog = AccessNode::new(panel, AriaRole::Dialog)
            .with_modal()
            .with_child(TAGS[first])
            .with_child(TAGS[second]);
        vec![
            dialog,
            AccessNode::new(TAGS[first], AriaRole::Button)
                .with_state(button_a11y_state(
                    postures[first],
                    focused == Some(TAGS[first]),
                ))
                .with_position_in_set(1)
                .with_size_of_set(2),
            AccessNode::new(TAGS[second], AriaRole::Button)
                .with_state(button_a11y_state(
                    postures[second],
                    focused == Some(TAGS[second]),
                ))
                .with_position_in_set(2)
                .with_size_of_set(2),
        ]
    }
}

impl WidgetView for HandoffView {
    type Renderer = HelloModalHandoffRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<HandoffView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::modal_scope_request::{self, ModalRequest};

    fn idle() -> HandoffState {
        [ButtonState::Idle; 5]
    }

    fn intent(tag: &str) -> pinion_core::Intent {
        pinion_core::Intent::new_owned(
            tag.to_string(),
            pinion_core::external::IntrospectValue::Null,
        )
    }

    /// The round's subject, at the binding boundary: ONE intent emits a
    /// `Close` **then** an `Open`, in that order. Before R1456 the
    /// mailbox kept only the `Open`.
    #[test]
    fn r1456_destructive_row_emits_close_then_open_from_one_intent() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            menu().open(menu_members());
            let _ = modal_scope_request::drain();

            let _ = HandoffView::update(idle(), &intent("menu_delete.click"));

            assert!(!menu().is_open(), "the menu dismissed itself");
            assert!(confirm().is_open(), "the confirm took its place");
            assert_eq!(
                modal_scope_request::drain(),
                vec![
                    ModalRequest::Close,
                    ModalRequest::Open {
                        members: confirm_members(),
                    },
                ],
                "one dispatch frame carries BOTH modal-scope edits, in the \
                 order the reducer wrote them",
            );
        });
    }

    /// The safe row stays a one-request frame — the batch drain widened
    /// what a frame *may* carry without changing what this one does.
    #[test]
    fn r1456_safe_row_emits_a_single_close() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            menu().open(menu_members());
            let _ = modal_scope_request::drain();

            let _ = HandoffView::update(idle(), &intent("menu_rename.click"));

            assert!(!menu().is_open());
            assert!(!confirm().is_open(), "the safe row opens no dialog");
            assert_eq!(modal_scope_request::drain(), vec![ModalRequest::Close]);
            assert_eq!(use_last_action().get().as_deref(), Some("Renamed."));
        });
    }

    #[test]
    fn r1456_answering_the_confirm_pops_exactly_one_scope() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            confirm().open(confirm_members());
            let _ = modal_scope_request::drain();

            let _ = HandoffView::update(idle(), &intent("confirm_ok.click"));

            assert!(!confirm().is_open());
            assert_eq!(use_last_action().get().as_deref(), Some("Deleted."));
            assert_eq!(modal_scope_request::drain(), vec![ModalRequest::Close]);
        });
    }

    #[test]
    fn r1456_escape_dismisses_the_confirm_not_the_menu_underneath() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            menu().close();
            confirm().open(confirm_members());
            let _ = modal_scope_request::drain();
            let mut scene = boot_scene();

            assert!(HandoffView::apply_key(
                &mut scene,
                Some(CONFIRM_CANCEL_TAG),
                "Escape",
                pinion_core::Modifiers::empty(),
            ));
            assert!(!confirm().is_open());
            assert_eq!(
                use_last_action().get().as_deref(),
                Some("Deletion cancelled.")
            );
            assert_eq!(modal_scope_request::drain(), vec![ModalRequest::Close]);
        });
    }

    /// Only one surface is ever painted: the handoff replaces, it does
    /// not stack. Two scrims would mean the menu never left.
    #[test]
    fn r1456_menu_and_confirm_are_never_painted_together() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            menu().open(menu_members());
            let _ = HandoffView::update(idle(), &intent("menu_delete.click"));
            let scene = view(idle(), &Frame::new());
            assert!(
                find_tagged(&scene, CONFIRM_PANEL_TAG).is_some(),
                "the confirm is up"
            );
            assert!(
                find_tagged(&scene, MENU_PANEL_TAG).is_none(),
                "the menu is gone from the paint scene"
            );
        });
    }

    #[test]
    fn r1456_a11y_reports_one_modal_dialog_at_a_time() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            menu().close();
            confirm().close();
            assert_eq!(HandoffView::access_node(&idle(), None).len(), 1);

            menu().open(menu_members());
            let nodes = HandoffView::access_node(&idle(), Some(RENAME_TAG));
            assert_eq!(nodes.len(), 3);
            assert_eq!(nodes[0].tag, MENU_PANEL_TAG);
            assert!(nodes[0].modal);
            assert_eq!(nodes[0].children, vec![RENAME_TAG, DELETE_TAG]);

            let _ = HandoffView::update(idle(), &intent("menu_delete.click"));
            let nodes = HandoffView::access_node(&idle(), Some(CONFIRM_CANCEL_TAG));
            assert_eq!(nodes.len(), 3, "still exactly one dialog, not two");
            assert_eq!(nodes[0].tag, CONFIRM_PANEL_TAG);
            assert_eq!(nodes[0].children, vec![CONFIRM_CANCEL_TAG, CONFIRM_OK_TAG]);
        });
    }

    #[test]
    fn r1456_base_enumeration_lists_only_the_trigger() {
        let scene = Owner::new().run(|| {
            menu().close();
            confirm().close();
            view(idle(), &Frame::new())
        });
        assert_eq!(scene.collect_focusable_tags(), vec![TRIGGER_TAG.to_owned()]);
    }

    #[test]
    fn r1456_view_contains_trigger_paint_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<HandoffView>(
            idle(),
            &Frame::default(),
        );
    }

    fn find_tagged<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        if let Scene::Container(c) = scene {
            if c.tag.as_deref() == Some(tag) {
                return Some(c);
            }
            for child in &c.children {
                if let Some(found) = find_tagged(child, tag) {
                    return Some(found);
                }
            }
        }
        None
    }

    fn boot_scene() -> Scene {
        use pinion_core::scene::ExternalNode;
        let mut children = vec![Scene::External(
            ExternalNode::new(HandoffView::create_external()).with_tag(TRIGGER_TAG),
        )];
        for extra in HandoffView::create_extra_externals() {
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag),
            ));
        }
        Scene::Container(ContainerNode::new(children))
    }
}
