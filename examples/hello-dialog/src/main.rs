// R693 §5.16 — example bindings tolerate looser doc-markdown lints
// than substrate crates; the narrative carries many proper-noun
// identifiers (FocusManager, WAI-ARIA, DialogStyle, …).
#![allow(clippy::doc_markdown)]

//! `hello-dialog` — R693 §5.16 §5.39 §5.40 §5.50 first consumer of the
//! modal-focus-trap substrate (`pinion_runtime::FocusManager`
//! `push_modal_scope` / `pop_modal_scope`, driven through
//! [`pinion_core::modal_scope_request`]) and the
//! [`pinion_widget_paint::dialog`] chrome.
//!
//! ## Why this binding exists
//!
//! Phase B widget-catalog entry. A destructive-confirm modal dialog —
//! the "Delete file?" / "Discard changes?" prompt every pro DCC / IDE /
//! CAD tool ships, and a direct step toward the northern-star
//! "Unreal-class editor self-hosted in pinion" (every editor command
//! that needs confirmation routes through one).
//!
//! ## Architecture (unidirectional, no reducer→widget back-channel)
//!
//! Three **real** focusable [`ButtonExternal`]s — the trigger
//! (`open_dialog`, the primary external) plus the dialog's `dialog_ok`
//! and `dialog_cancel` action buttons (extra externals). Each button is
//! driven by input (pointer router + ARIA Enter/Space) and emits a
//! `"<tag>.click"` intent; the [`DialogView::update`] reducer maps those
//! to the application's reactive `dialog_open` + `dialog_result`
//! [`Signal`]s and writes the modal-scope lifecycle request. The view
//! reads the signals back (auto-subscribed) and paints the scrim + panel
//! when open. Nothing reaches into a widget from the reducer — the data
//! flows input → External → intent → reducer → Signal → view, exactly
//! like the menu / toolbar / todomvc precedents.
//!
//! ## The modal focus trap (the substrate this round adds)
//!
//! Opening the dialog calls
//! [`modal_scope_request::open`](pinion_core::modal_scope_request::open)
//! with the action-button tags. The shell drains it in `handle_tail`
//! and pushes a `FocusManager` modal
//! scope: focus moves into the dialog (auto-focus `dialog_cancel`, the
//! safe default for a destructive prompt), Tab / Shift+Tab are confined
//! to `[dialog_cancel, dialog_ok]` (the trigger behind the scrim is
//! unreachable), and closing restores focus to the trigger. The action
//! tags are *not* marked `.with_focusable(true)` in the base view — they
//! are focusable only while the modal is up, the dynamic-focusable case a
//! scene that is always present cannot express (todomvc's R664
//! phantom-tab-stop note); the modal scope *is* the dynamic Tab-stop set
//! for the dialog's life.
//!
//! The scrim (`dialog_scrim`) is a full-window backdrop placed last in
//! the scene, so it hit-tests above the background and blocks pointer
//! input there; it carries no `External`, so a backdrop click is
//! swallowed (WAI-ARIA modal default — no light-dismiss). `Escape`
//! dismisses (routed to [`apply_key`](DialogView) by the shell only
//! while the trap is active).
//!
//! ## Known gap (pre-existing, honest carry)
//!
//! The buttons show pointer hover / pressed feedback but do **not**
//! paint a keyboard-focus ring: `ButtonState` has no focus posture and
//! the view fn does not receive the shell-focused tag (the
//! shell-focus-paint axis R690 Tabs / R692 Toolbar already deferred).
//! The trap itself is real and observable — `focus/get` over RPC shows
//! focus confined to the action members, Tab wraps inside them, and the
//! a11y tree marks the dialog `aria-modal`. The focus ring lands when
//! the shared shell-focus-paint axis does.

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
    SurfaceAction, button_a11y_state, read_button_focused, read_button_state, surface_action_scene,
};
use pinion_widget_paint::dialog::{DialogContent, DialogStyle, view_dialog};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloDialogRenderer, HelloDialogRendererError);

const WIN_W: u32 = 520;
const WIN_H: u32 = 360;
const THEME_TAG: &str = "app";

/// Trigger button (the primary external). Opens the dialog.
const TRIGGER_TAG: &str = "open_dialog";
/// Affirmative (destructive) action button — extra external.
const OK_TAG: &str = "dialog_ok";
/// Cancel action button — extra external.
const CANCEL_TAG: &str = "dialog_cancel";
/// Backdrop tag (no external — blocks + swallows background clicks).
const SCRIM_TAG: &str = "dialog_scrim";
/// Dialog panel tag (queryable via `scene/snapshot`).
const PANEL_TAG: &str = "dialog_panel";
/// R795 — query-only modal-introspection tag. `scene/query`
/// `/dialog_state/external/open` reports the dialog's open flag whether the
/// panel is up or not (the first-class `open` query, uniform with `ContextMenu`).
const MODAL_STATE_TAG: &str = "dialog_state";

/// `Owner::cache` key for the shared [`ModalState`] open-lifecycle.
const MODAL_KEY: &str = "hello_dialog.modal";
/// `Owner::cache` key for the `Signal<Option<bool>>` last outcome.
const RESULT_KEY: &str = "hello_dialog.result";

const TRIGGER_HOVER_KEY: &str = "hello_dialog.trigger_hover";
const OK_HOVER_KEY: &str = "hello_dialog.ok_hover";
const CANCEL_HOVER_KEY: &str = "hello_dialog.cancel_hover";

const TRIGGER_W: u32 = 180;
const TRIGGER_H: u32 = 56;
const ACTION_W: u32 = 110;
const ACTION_H: u32 = 44;

/// The dialog's focusable controls, in Tab order. Cancel is first so it
/// is auto-focused on open (the safe default for a destructive prompt);
/// Tab moves Cancel → Delete and wraps. Passed to
/// [`modal_scope_request::open`](pinion_core::modal_scope_request::open).
fn dialog_members() -> Vec<String> {
    vec![CANCEL_TAG.to_string(), OK_TAG.to_string()]
}

/// The shared [`ModalState`] (R788 lifted open-lifecycle SSOT). One `Rc`
/// across the view fn (auto-subscribe via [`ModalState::is_open`]), the
/// reducer (open / close), and `access_node` (reads to switch the a11y
/// tree). The dialog's *outcome* (accepted / cancelled) stays in
/// [`use_dialog_result`] — that is per-binding policy, not the modal
/// mechanism.
fn modal() -> Rc<ModalState> {
    use_modal(MODAL_KEY)
}

/// `Owner::cache`-keyed hook: the last dialog outcome — `None` (no
/// action yet), `Some(true)` (confirmed the destructive action), or
/// `Some(false)` (cancelled). `Option<bool>` keeps the [`Signal`]'s
/// serde bound satisfied without a custom-enum derive.
fn use_dialog_result() -> Rc<Signal<Option<bool>>> {
    Owner::current()
        .expect("use_dialog_result requires an active Owner scope")
        .cache(RESULT_KEY, || Signal::new(None::<bool>))
}

/// Open the dialog: flip the signal and install the modal focus trap
/// over the action buttons. Runs inside the reducer (owner-scoped, and
/// the modal request drains in the same `handle_tail` so the trap is up
/// before the next paint).
fn open_dialog() {
    modal().open(dialog_members());
}

/// Close the dialog, recording `accepted` (`true` = confirmed,
/// `false` = cancelled): flip the open flag, store the outcome, and
/// lift the modal trap (restoring focus to the trigger). The two signal
/// writes are [`batch`]ed so the view re-renders once.
fn close_dialog(accepted: bool) {
    let m = modal();
    let res = use_dialog_result();
    // One batch: the outcome write + the modal close (flag flip) re-render
    // the view once. `ModalState::close` also pops the focus trap (a Cell
    // mailbox write, unaffected by the reactive batch).
    batch(|| {
        res.set(Some(accepted));
        m.close();
    });
}

/// Cached posture of the three buttons (trigger, ok, cancel) read back
/// from the state scene for the paint fn: each button's [`ButtonState`]
/// plus its R694 keyboard-focus flag (`[trigger, ok, cancel]`). `open` /
/// `result` are *not* here — they live in signals the owner-scoped view +
/// access_node read directly ([`read_state`](DialogView::read_state) is
/// not owner-wrapped).
type DialogViewState = (ButtonState, ButtonState, ButtonState, [bool; 3]);

/// view-fn (§6.3): pure sync mapping `(button postures) -> Scene`,
/// reading the reactive `dialog_open` + `dialog_result` signals. When
/// open, the dialog overlay is pushed **last** so it paints over (and
/// hit-tests above) the trigger content.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: DialogViewState, _frame: &Frame) -> Scene {
    let (trigger_state, ok_state, cancel_state, focus) = state;
    let [trigger_focused, ok_focused, cancel_focused] = focus;
    let theme = use_theme(THEME_TAG).theme_animated();
    let open = modal().is_open();
    let result = use_dialog_result().get();

    let trigger = surface_action_scene(
        &SurfaceAction {
            tag: TRIGGER_TAG,
            label: "Delete file\u{2026}",
            state: trigger_state,
            focused: trigger_focused,
            hover_key: TRIGGER_HOVER_KEY,
            size: Size::px(TRIGGER_W, TRIGGER_H),
            focusable: true,
        },
        &theme,
    );
    let status_label = match result {
        None => "No action taken yet.",
        Some(true) => "File deleted.",
        Some(false) => "Deletion cancelled.",
    };
    let status = Scene::Text(TextNode::styled(
        status_label,
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
    if open {
        // Action order matches `dialog_members()` (Cancel, then Delete)
        // so the left-to-right layout mirrors the Tab order.
        let cancel = surface_action_scene(
            &SurfaceAction {
                tag: CANCEL_TAG,
                label: "Cancel",
                state: cancel_state,
                focused: cancel_focused,
                hover_key: CANCEL_HOVER_KEY,
                size: Size::px(ACTION_W, ACTION_H),
                // Action tags are modal members: focusable only while
                // the trap is up, never in the base enumeration.
                focusable: false,
            },
            &theme,
        );
        let ok = surface_action_scene(
            &SurfaceAction {
                tag: OK_TAG,
                label: "Delete",
                state: ok_state,
                focused: ok_focused,
                hover_key: OK_HOVER_KEY,
                size: Size::px(ACTION_W, ACTION_H),
                focusable: false,
            },
            &theme,
        );
        children.push(view_dialog(
            SCRIM_TAG,
            PANEL_TAG,
            DialogContent {
                title: "Delete file?",
                message: "This permanently removes the file. This cannot be undone.",
                body: None,
            },
            vec![cancel, ok],
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

struct DialogView;

impl WidgetCore for DialogView {
    type State = DialogViewState;
    // Buttons are driven by the pointer router + ARIA Enter/Space; no
    // keybinding-channel events flow through `event_name` (mirrors
    // hello-menu / hello-toolbar).
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(OK_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(CANCEL_TAG, Box::new(ButtonExternal::new())),
            modal_introspection_extra(MODAL_STATE_TAG, modal()),
        ]
    }

    fn tag() -> &'static str {
        TRIGGER_TAG
    }

    fn read_state(scene: &Scene) -> DialogViewState {
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

    fn view(state: DialogViewState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-dialog (R693 §5.39 §5.40 §5.50)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// R693 §5.39 — Escape dismisses the open dialog (the shell routes
    /// Escape here only while the trap is active). Enter / Space on a
    /// focused button activate it through the shared ARIA helper, which
    /// emits the button's `"click"` intent into the reducer below.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if key == "Escape" {
            if modal().is_open() {
                close_dialog(false);
                return true;
            }
            return false;
        }
        // R693.A — Space/Enter on the focused button activate it through
        // the shared ARIA helper (now multi-External aware: it descends
        // the Container to the focused button's external). The chained-or
        // resolves to whichever tag holds focus.
        apply_aria_activate(scene, focused, key, TRIGGER_TAG)
            || apply_aria_activate(scene, focused, key, OK_TAG)
            || apply_aria_activate(scene, focused, key, CANCEL_TAG)
    }

    /// R693 §5.39 — bridge the buttons' `"<tag>.click"` intents into the
    /// `dialog_open` + `dialog_result` signals and the modal-scope
    /// lifecycle. Side-effect-only (empty `Vec<Command>` return); the
    /// `Signal::set` writes + modal request are the mutations.
    fn update(
        _state: DialogViewState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            "open_dialog.click" => open_dialog(),
            "dialog_ok.click" => close_dialog(true),
            "dialog_cancel.click" => close_dialog(false),
            _ => {}
        }
        Vec::new()
    }

    fn fmt_state_log(state: &DialogViewState) -> String {
        format!(
            "trigger={:?} ok={:?} cancel={:?}",
            state.0, state.1, state.2
        )
    }
}

impl WidgetA11y for DialogView {
    /// R693 §5.40 — WAI-ARIA dialog tree. Closed: just the trigger
    /// [`AriaRole::Button`]. Open: the `aria-modal` [`AriaRole::Dialog`]
    /// root owning the two action buttons (the trigger is omitted — the
    /// scrim makes the background inert, so AT should not announce it).
    ///
    /// R693.A §5.40 — accessible names are left `None` here and derived
    /// from the paint scene by `enrich_names_from_scene` (the shell runs
    /// it after `access_node`), so the button labels + dialog title have
    /// a single source of truth in the paint `TextNode`s — no parallel
    /// hardcoded copy that could drift (the hello-button / hello-menu
    /// precedent).
    fn access_node(state: &DialogViewState, focused: Option<&str>) -> Vec<AccessNode> {
        let open = modal().is_open();
        if !open {
            return vec![
                AccessNode::new(TRIGGER_TAG, AriaRole::Button)
                    .with_state(button_a11y_state(state.0, focused == Some(TRIGGER_TAG))),
            ];
        }
        let dialog = AccessNode::new(PANEL_TAG, AriaRole::Dialog)
            .with_modal()
            .with_child(CANCEL_TAG)
            .with_child(OK_TAG);
        vec![
            dialog,
            AccessNode::new(CANCEL_TAG, AriaRole::Button)
                .with_state(button_a11y_state(state.2, focused == Some(CANCEL_TAG)))
                .with_position_in_set(1)
                .with_size_of_set(2),
            AccessNode::new(OK_TAG, AriaRole::Button)
                .with_state(button_a11y_state(state.1, focused == Some(OK_TAG)))
                .with_position_in_set(2)
                .with_size_of_set(2),
        ]
    }
}

impl WidgetView for DialogView {
    type Renderer = HelloDialogRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<DialogView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> DialogViewState {
        (
            ButtonState::Idle,
            ButtonState::Idle,
            ButtonState::Idle,
            [false; 3],
        )
    }

    // ----- a11y -----

    #[test]
    fn r693_closed_emits_only_trigger_button() {
        Owner::new().run(|| {
            modal().close();
            let nodes = DialogView::access_node(&idle(), None);
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].role, AriaRole::Button);
            assert_eq!(nodes[0].tag, TRIGGER_TAG);
        });
    }

    #[test]
    fn r693_open_emits_modal_dialog_with_actions() {
        Owner::new().run(|| {
            modal().open(dialog_members());
            let nodes = DialogView::access_node(&idle(), Some(CANCEL_TAG));
            assert_eq!(nodes.len(), 3);
            assert_eq!(nodes[0].role, AriaRole::Dialog);
            assert!(nodes[0].modal, "dialog root carries aria-modal");
            assert_eq!(nodes[0].children, vec![CANCEL_TAG, OK_TAG]);
            assert_eq!(nodes[1].tag, CANCEL_TAG);
            assert!(nodes[1].state.focused, "auto-focused cancel marked focused");
            assert_eq!(nodes[2].tag, OK_TAG);
            assert!(!nodes[2].state.focused);
        });
    }

    #[test]
    fn r693a_names_enriched_from_paint_not_hardcoded() {
        // SSOT: access_node leaves names None; enrich_names_from_scene
        // derives them from the paint TextNodes (the button labels +
        // panel title), so there is no parallel hardcoded copy to drift.
        Owner::new().run(|| {
            modal().open(dialog_members());
            let scene = view(idle(), &Frame::new());
            let mut nodes = DialogView::access_node(&idle(), Some(CANCEL_TAG));
            assert!(
                nodes.iter().all(|n| n.name.is_none()),
                "access_node leaves names to enrich"
            );
            pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
            let name = |t: &str| {
                nodes
                    .iter()
                    .find(|n| n.tag == t)
                    .and_then(|n| n.name.as_deref())
            };
            assert_eq!(
                name(PANEL_TAG),
                Some("Delete file?"),
                "dialog name from title"
            );
            assert_eq!(name(CANCEL_TAG), Some("Cancel"));
            assert_eq!(name(OK_TAG), Some("Delete"));
        });
    }

    // ----- R694 focus ring -----

    /// Walk the paint scene for the tagged button container.
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

    #[test]
    fn r694_focused_action_button_paints_ring_others_do_not() {
        Owner::new().run(|| {
            modal().open(dialog_members());
            // Cancel focused (the auto-focus default); ok + trigger not.
            let state = (
                ButtonState::Idle,
                ButtonState::Idle,
                ButtonState::Idle,
                [false, false, true],
            );
            let scene = view(state, &Frame::new());
            let cancel = find_tagged(&scene, CANCEL_TAG).expect("cancel button painted");
            assert!(
                cancel.style.border.is_some(),
                "focused Cancel paints the keyboard focus ring",
            );
            let ok = find_tagged(&scene, OK_TAG).expect("ok button painted");
            assert!(ok.style.border.is_none(), "unfocused Delete has no ring");
        });
    }

    // ----- reducer + signals -----

    #[test]
    fn r693_open_click_sets_open_and_requests_modal() {
        Owner::new().run(|| {
            modal().close();
            let _ = pinion_core::modal_scope_request::drain();
            let _ = DialogView::update(idle(), &intent("open_dialog.click"));
            assert!(modal().is_open(), "open signal flipped true");
            assert_eq!(
                pinion_core::modal_scope_request::drain(),
                vec![pinion_core::modal_scope_request::ModalRequest::Open {
                    members: dialog_members(),
                }],
                "modal trap requested over the action buttons"
            );
        });
    }

    #[test]
    fn r693_ok_click_accepts_and_closes_modal() {
        Owner::new().run(|| {
            modal().open(dialog_members());
            let _ = pinion_core::modal_scope_request::drain();
            let _ = DialogView::update(idle(), &intent("dialog_ok.click"));
            assert!(!modal().is_open());
            assert_eq!(use_dialog_result().get(), Some(true));
            assert_eq!(
                pinion_core::modal_scope_request::drain(),
                vec![pinion_core::modal_scope_request::ModalRequest::Close]
            );
        });
    }

    #[test]
    fn r693_cancel_click_cancels_and_closes_modal() {
        Owner::new().run(|| {
            modal().open(dialog_members());
            let _ = pinion_core::modal_scope_request::drain();
            let _ = DialogView::update(idle(), &intent("dialog_cancel.click"));
            assert!(!modal().is_open());
            assert_eq!(use_dialog_result().get(), Some(false));
            assert_eq!(
                pinion_core::modal_scope_request::drain(),
                vec![pinion_core::modal_scope_request::ModalRequest::Close]
            );
        });
    }

    // ----- apply_key -----

    #[test]
    fn r693_escape_cancels_open_dialog() {
        Owner::new().run(|| {
            modal().open(dialog_members());
            let _ = pinion_core::modal_scope_request::drain();
            let mut scene = boot_scene();
            let handled = DialogView::apply_key(
                &mut scene,
                Some(CANCEL_TAG),
                "Escape",
                pinion_core::Modifiers::empty(),
            );
            assert!(handled);
            assert!(!modal().is_open());
            assert_eq!(use_dialog_result().get(), Some(false));
            assert_eq!(
                pinion_core::modal_scope_request::drain(),
                vec![pinion_core::modal_scope_request::ModalRequest::Close]
            );
        });
    }

    #[test]
    fn r693_escape_ignored_when_closed() {
        Owner::new().run(|| {
            modal().close();
            let mut scene = boot_scene();
            assert!(!DialogView::apply_key(
                &mut scene,
                None,
                "Escape",
                pinion_core::Modifiers::empty(),
            ));
        });
    }

    #[test]
    fn r693_focusable_tags_lists_only_trigger() {
        // §5.39: collected from the paint scene.
        let scene = Owner::new().run(|| view(idle(), &Frame::new()));
        assert_eq!(scene.collect_focusable_tags(), vec![TRIGGER_TAG.to_owned()]);
    }

    #[test]
    fn r693_view_contains_trigger_paint_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<DialogView>(
            idle(),
            &Frame::default(),
        );
    }

    fn intent(tag: &str) -> pinion_core::Intent {
        pinion_core::Intent::new_owned(
            tag.to_string(),
            pinion_core::external::IntrospectValue::Null,
        )
    }

    /// Build the multi-external state scene the way the shell does so
    /// `apply_key`'s ARIA-activate forwarding has real button externals
    /// to dispatch against.
    fn boot_scene() -> Scene {
        use pinion_core::scene::ExternalNode;
        let mut children = vec![Scene::External(
            ExternalNode::new(DialogView::create_external()).with_tag(TRIGGER_TAG),
        )];
        for extra in DialogView::create_extra_externals() {
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag),
            ));
        }
        Scene::Container(ContainerNode::new(children))
    }
}
