// R693 §5.16 — example bindings tolerate looser doc-markdown lints
// than substrate crates; the narrative carries many proper-noun
// identifiers (FocusManager, WAI-ARIA, DialogStyle, …).
#![allow(clippy::doc_markdown)]

//! `hello-dialog` — R693 §5.16 §5.39 §5.40 §5.50 first consumer of the
//! modal-focus-trap substrate ([`pinion_runtime::FocusManager`]
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
//! and pushes a [`FocusManager`](pinion_runtime::FocusManager) modal
//! scope: focus moves into the dialog (auto-focus `dialog_cancel`, the
//! safe default for a destructive prompt), Tab / Shift+Tab are confined
//! to `[dialog_cancel, dialog_ok]` (the trigger behind the scrim is
//! unreachable), and closing restores focus to the trigger. The action
//! tags are *not* in [`focusable_tags`](DialogView) — they are focusable
//! only while the modal is up, the dynamic-focusable case the static
//! enumeration cannot express (todomvc's R664 phantom-tab-stop note);
//! the modal scope *is* the dynamic enumeration for the dialog's life.
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
use pinion_core::reactive::{batch, Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::button::{use_hover_progress, view_button, ButtonColors, ButtonStyle};
use pinion_widget_paint::dialog::{view_dialog, DialogContent, DialogStyle};
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

/// `Owner::cache` key for the `Signal<bool>` "is the dialog open".
const OPEN_KEY: &str = "hello_dialog.open";
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

/// `Owner::cache`-keyed hook: the shared `Rc<Signal<bool>>` open flag.
/// One `Rc` across the view fn (auto-subscribe via `.get()`), the
/// reducer (writes), and `access_node` (reads to switch the a11y tree).
fn use_dialog_open() -> Rc<Signal<bool>> {
    Owner::current()
        .expect("use_dialog_open requires an active Owner scope")
        .cache(OPEN_KEY, || Signal::new(false))
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
    use_dialog_open().set(true);
    pinion_core::modal_scope_request::open(dialog_members());
}

/// Close the dialog, recording `accepted` (`true` = confirmed,
/// `false` = cancelled): flip the open flag, store the outcome, and
/// lift the modal trap (restoring focus to the trigger). The two signal
/// writes are [`batch`]ed so the view re-renders once.
fn close_dialog(accepted: bool) {
    let open = use_dialog_open();
    let res = use_dialog_result();
    batch(|| {
        open.set(false);
        res.set(Some(accepted));
    });
    pinion_core::modal_scope_request::close();
}

/// Cached posture of the three buttons (trigger, ok, cancel) read back
/// from the state scene for the paint fn. `open` / `result` are *not*
/// here — they live in signals the owner-scoped view + access_node read
/// directly ([`read_state`](DialogView::read_state) is not owner-wrapped).
type DialogViewState = (ButtonState, ButtonState, ButtonState);

/// Read a [`ButtonExternal`]'s [`ButtonState`] out of the state scene by
/// tag. Defaults to [`ButtonState::Idle`] when the tag is absent.
fn read_button_state(scene: &Scene, tag: &str) -> ButtonState {
    scene
        .find_external_with_tag(tag)
        .and_then(|node| node.handle.introspect())
        .and_then(|intro| intro.query("state"))
        .map_or(ButtonState::Idle, |v| match v {
            pinion_core::external::IntrospectValue::Text(s) => {
                ButtonState::from_name_or_default(&s)
            }
            _ => ButtonState::Idle,
        })
}

/// Render one button via the [`pinion_widget_paint::button`] substrate.
fn button_scene(
    tag: &'static str,
    label: &str,
    state: ButtonState,
    hover_key: &'static str,
    size: Size,
    theme: &pinion_core::theme::Theme,
) -> Scene {
    let hover = use_hover_progress(matches!(state, ButtonState::Hover), hover_key);
    view_button(
        label,
        state,
        hover,
        &ButtonColors::filled_tonal(theme),
        &ButtonStyle::m3_default(tag)
            .with_size(size)
            .with_label_font_size_px(16),
    )
}

/// view-fn (§6.3): pure sync mapping `(button postures) -> Scene`,
/// reading the reactive `dialog_open` + `dialog_result` signals. When
/// open, the dialog overlay is pushed **last** so it paints over (and
/// hit-tests above) the trigger content.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: DialogViewState, _frame: &Frame) -> Scene {
    let (trigger_state, ok_state, cancel_state) = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let open = use_dialog_open().get();
    let result = use_dialog_result().get();

    let trigger = button_scene(
        TRIGGER_TAG,
        "Delete file\u{2026}",
        trigger_state,
        TRIGGER_HOVER_KEY,
        Size::px(TRIGGER_W, TRIGGER_H),
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
        let cancel = button_scene(
            CANCEL_TAG,
            "Cancel",
            cancel_state,
            CANCEL_HOVER_KEY,
            Size::px(ACTION_W, ACTION_H),
            &theme,
        );
        let ok = button_scene(
            OK_TAG,
            "Delete",
            ok_state,
            OK_HOVER_KEY,
            Size::px(ACTION_W, ACTION_H),
            &theme,
        );
        children.push(view_dialog(
            SCRIM_TAG,
            PANEL_TAG,
            DialogContent {
                title: "Delete file?",
                message: "This permanently removes the file. This cannot be undone.",
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

    /// Only the trigger is a *static* tab stop. The dialog's action
    /// buttons become focusable solely while the modal scope is up
    /// (`dialog_members()`), which is the dynamic-focusable case the
    /// static enumeration cannot express — see the module docs.
    fn focusable_tags() -> Vec<&'static str> {
        vec![TRIGGER_TAG]
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
            if use_dialog_open().get() {
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
        let open = use_dialog_open().get();
        if !open {
            return vec![AccessNode::new(TRIGGER_TAG, AriaRole::Button).with_state(
                button_a11y_state(state.0, focused == Some(TRIGGER_TAG)),
            )];
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

/// Map a [`ButtonState`] + shell-focus flag onto the a11y interaction
/// flags (mirrors the `#[widget(role = Button, state_flags(...))]`
/// derive hello-button uses).
fn button_a11y_state(state: ButtonState, focused: bool) -> pinion_a11y::AccessState {
    pinion_a11y::AccessState {
        focused,
        hovered: matches!(state, ButtonState::Hover),
        pressed: matches!(state, ButtonState::Pressed),
        disabled: matches!(state, ButtonState::Disabled),
        checked: None,
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
        (ButtonState::Idle, ButtonState::Idle, ButtonState::Idle)
    }

    // ----- a11y -----

    #[test]
    fn r693_closed_emits_only_trigger_button() {
        Owner::new().run(|| {
            use_dialog_open().set(false);
            let nodes = DialogView::access_node(&idle(), None);
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].role, AriaRole::Button);
            assert_eq!(nodes[0].tag, TRIGGER_TAG);
        });
    }

    #[test]
    fn r693_open_emits_modal_dialog_with_actions() {
        Owner::new().run(|| {
            use_dialog_open().set(true);
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
            use_dialog_open().set(true);
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
            assert_eq!(name(PANEL_TAG), Some("Delete file?"), "dialog name from title");
            assert_eq!(name(CANCEL_TAG), Some("Cancel"));
            assert_eq!(name(OK_TAG), Some("Delete"));
        });
    }

    // ----- reducer + signals -----

    #[test]
    fn r693_open_click_sets_open_and_requests_modal() {
        Owner::new().run(|| {
            let _ = pinion_core::modal_scope_request::drain();
            use_dialog_open().set(false);
            let _ = DialogView::update(idle(), &intent("open_dialog.click"));
            assert!(use_dialog_open().get(), "open signal flipped true");
            assert_eq!(
                pinion_core::modal_scope_request::drain(),
                Some(pinion_core::modal_scope_request::ModalRequest::Open {
                    members: dialog_members(),
                }),
                "modal trap requested over the action buttons"
            );
        });
    }

    #[test]
    fn r693_ok_click_accepts_and_closes_modal() {
        Owner::new().run(|| {
            let _ = pinion_core::modal_scope_request::drain();
            use_dialog_open().set(true);
            let _ = DialogView::update(idle(), &intent("dialog_ok.click"));
            assert!(!use_dialog_open().get());
            assert_eq!(use_dialog_result().get(), Some(true));
            assert_eq!(
                pinion_core::modal_scope_request::drain(),
                Some(pinion_core::modal_scope_request::ModalRequest::Close)
            );
        });
    }

    #[test]
    fn r693_cancel_click_cancels_and_closes_modal() {
        Owner::new().run(|| {
            let _ = pinion_core::modal_scope_request::drain();
            use_dialog_open().set(true);
            let _ = DialogView::update(idle(), &intent("dialog_cancel.click"));
            assert!(!use_dialog_open().get());
            assert_eq!(use_dialog_result().get(), Some(false));
            assert_eq!(
                pinion_core::modal_scope_request::drain(),
                Some(pinion_core::modal_scope_request::ModalRequest::Close)
            );
        });
    }

    // ----- apply_key -----

    #[test]
    fn r693_escape_cancels_open_dialog() {
        Owner::new().run(|| {
            let _ = pinion_core::modal_scope_request::drain();
            use_dialog_open().set(true);
            let mut scene = boot_scene();
            let handled = DialogView::apply_key(
                &mut scene,
                Some(CANCEL_TAG),
                "Escape",
                pinion_core::Modifiers::empty(),
            );
            assert!(handled);
            assert!(!use_dialog_open().get());
            assert_eq!(use_dialog_result().get(), Some(false));
            assert_eq!(
                pinion_core::modal_scope_request::drain(),
                Some(pinion_core::modal_scope_request::ModalRequest::Close)
            );
        });
    }

    #[test]
    fn r693_escape_ignored_when_closed() {
        Owner::new().run(|| {
            use_dialog_open().set(false);
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
        assert_eq!(DialogView::focusable_tags(), vec![TRIGGER_TAG]);
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
        let mut children = vec![
            Scene::External(ExternalNode::new(DialogView::create_external()).with_tag(TRIGGER_TAG)),
        ];
        for extra in DialogView::create_extra_externals() {
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag),
            ));
        }
        Scene::Container(ContainerNode::new(children))
    }
}
