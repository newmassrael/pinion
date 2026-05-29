// R702 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the narrative carries many proper-noun identifiers
// (FocusManager, WAI-ARIA, DrawerStyle, …).
#![allow(clippy::doc_markdown)]

//! `hello-drawer` — R702 §5.16 §5.39 §5.40 §5.50 modal **navigation
//! drawer** (side sheet): the 2nd consumer of the R693 modal
//! focus-trap substrate ([`pinion_runtime::FocusManager`]
//! `push_modal_scope` / `pop_modal_scope`, driven through
//! [`pinion_core::modal_scope_request`]) and the
//! [`pinion_widget_paint::drawer`] chrome.
//!
//! ## Why this binding exists
//!
//! Phase B widget-catalog entry. The edge-anchored navigation sheet
//! every pro DCC / IDE / CAD tool ships (a left nav rail, a right
//! inspector panel) — a direct step toward the northern-star
//! "Unreal-class editor self-hosted in pinion". It validates the R693
//! modal substrate as reusable across more than one widget
//! (`[[abstraction-needs-second-consumer]]`): Dialog *centres* a panel,
//! Drawer *edge-anchors* one, but both share the scrim + Tab trap +
//! Escape dismissal verbatim.
//!
//! ## The carry this round clears — scrim light-dismiss
//!
//! R693 deferred backdrop-click light-dismiss ("register the scrim tag
//! as an `External`"). A modal navigation drawer is exactly that
//! consumer: Material's modal drawer dismisses on scrim tap. So the
//! scrim (`drawer_scrim`) is a **real, click-only**
//! [`ButtonExternal`] — a backdrop click emits its
//! `"drawer_scrim.click"` intent and the reducer closes the drawer.
//! Reusing [`ButtonExternal`] (rather than minting a one-off
//! `ScrimExternal`) is the disciplined choice per
//! `[[abstraction-needs-second-consumer]]`: it already emits the
//! activation intent, and the scrim's non-button concerns are handled
//! by the binding (omitted from the a11y tree, painted flat, never in
//! `focusable_tags`). Dialog keeps the WAI-ARIA modal default (Esc
//! only) by *not* binding an External to its scrim; the mechanism is
//! now available, opt-in per widget.
//!
//! ## Architecture (unidirectional, no reducer→widget back-channel)
//!
//! Real focusable [`ButtonExternal`]s — the trigger (`drawer_trigger`,
//! the primary external) plus N navigation items + the click-only
//! scrim (extra externals). Input drives each button; it emits a
//! `"<tag>.click"` intent; the [`DrawerView::update`] reducer maps
//! those to the application's reactive `drawer_open` + `drawer_active`
//! [`Signal`]s and writes the modal-scope lifecycle. The view reads the
//! signals back and paints the scrim + panel when open. Data flows
//! input → External → intent → reducer → Signal → view, exactly like
//! hello-dialog.
//!
//! ## Dismissal paths (all converge on `close_drawer`)
//!
//! - **Scrim tap** — `"drawer_scrim.click"` (the new light-dismiss);
//! - **Escape** — routed to [`apply_key`](DrawerView) while the trap is
//!   active;
//! - **Selecting a destination** — `"drawer_item_<i>.click"` sets the
//!   active destination *and* closes (the modal nav-drawer convention).
//!
//! ## Known gap (pre-existing, honest carry)
//!
//! Like hello-dialog, the buttons do not paint a keyboard-focus ring
//! (the shared shell-focus-paint axis R690 deferred); the trap itself
//! is real and observable — `focus/get` shows focus confined to the
//! nav items, Tab wraps inside them, and the a11y tree marks the drawer
//! `aria-modal`.

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::External;
use pinion_core::reactive::{batch, Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::button::{use_hover_progress, view_button, ButtonColors, ButtonStyle};
use pinion_widget_paint::drawer::{view_drawer, DrawerStyle};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloDrawerRenderer, HelloDrawerRendererError);

const WIN_W: u32 = 560;
const WIN_H: u32 = 380;
const THEME_TAG: &str = "app";

/// Number of navigation destinations in the drawer.
const NAV_N: usize = 3;

/// Trigger button (the primary external). Opens the drawer.
const TRIGGER_TAG: &str = "drawer_trigger";
/// Backdrop tag — a click-only [`ButtonExternal`] (light-dismiss).
const SCRIM_TAG: &str = "drawer_scrim";
/// Drawer panel tag (queryable via `scene/snapshot`).
const PANEL_TAG: &str = "drawer_panel";
/// Active-destination status text tag (read by the RPC demo).
const STATUS_TAG: &str = "drawer_status";

/// Per-destination navigation-item button tags, in Tab order. Static
/// `&'static str` (the fixed-cluster convention) so they double as the
/// modal-scope members and the a11y child list.
const NAV_TAGS: [&str; NAV_N] = ["drawer_item_0", "drawer_item_1", "drawer_item_2"];

/// Destination labels — double as each item's AT accessible name
/// (name-from-contents enrichment of the button label) and the status
/// line's "Current: <name>" copy.
const DESTINATIONS: [&str; NAV_N] = ["Home", "Search", "Settings"];

/// `Owner::cache` key for the `Signal<bool>` "is the drawer open".
const OPEN_KEY: &str = "hello_drawer.open";
/// `Owner::cache` key for the `Signal<usize>` active destination index.
const ACTIVE_KEY: &str = "hello_drawer.active";

const TRIGGER_HOVER_KEY: &str = "hello_drawer.trigger_hover";
const ITEM_HOVER_KEYS: [&str; NAV_N] = [
    "hello_drawer.item0_hover",
    "hello_drawer.item1_hover",
    "hello_drawer.item2_hover",
];

const TRIGGER_W: u32 = 200;
const TRIGGER_H: u32 = 56;
const ITEM_H: u32 = 48;

/// The drawer's focusable controls, in Tab order. Passed to
/// [`modal_scope_request::open`](pinion_core::modal_scope_request::open);
/// the first (`drawer_item_0`) is auto-focused on open.
fn nav_members() -> Vec<String> {
    NAV_TAGS.iter().map(|t| (*t).to_string()).collect()
}

/// `Owner::cache`-keyed hook: the shared `Rc<Signal<bool>>` open flag.
fn use_drawer_open() -> Rc<Signal<bool>> {
    Owner::current()
        .expect("use_drawer_open requires an active Owner scope")
        .cache(OPEN_KEY, || Signal::new(false))
}

/// `Owner::cache`-keyed hook: the active destination index (default 0).
fn use_drawer_active() -> Rc<Signal<usize>> {
    Owner::current()
        .expect("use_drawer_active requires an active Owner scope")
        .cache(ACTIVE_KEY, || Signal::new(0usize))
}

/// Open the drawer: flip the signal and install the modal focus trap
/// over the navigation items. Runs inside the reducer (owner-scoped);
/// the modal request drains in the same `handle_tail` so the trap is up
/// before the next paint.
fn open_drawer() {
    use_drawer_open().set(true);
    pinion_core::modal_scope_request::open(nav_members());
}

/// Close the drawer and lift the modal trap (restoring focus to the
/// trigger), without changing the active destination — the Escape /
/// scrim-tap path.
fn close_drawer() {
    use_drawer_open().set(false);
    pinion_core::modal_scope_request::close();
}

/// Select destination `index`: record it as active and close the drawer
/// (the modal nav-drawer convention — choosing a destination navigates
/// and dismisses). The two signal writes are [`batch`]ed so the view
/// re-renders once; the modal trap is lifted afterwards.
fn select(index: usize) {
    let open = use_drawer_open();
    let active = use_drawer_active();
    batch(|| {
        active.set(index);
        open.set(false);
    });
    pinion_core::modal_scope_request::close();
}

/// Cached posture for the paint fn: the trigger's [`ButtonState`], each
/// nav item's [`ButtonState`], and the R694 keyboard-focus flag for the
/// trigger + each item. `open` / `active` are *not* here — they live in
/// signals the owner-scoped view + access_node read directly.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct DrawerViewState {
    trigger: ButtonState,
    items: [ButtonState; NAV_N],
    trigger_focused: bool,
    items_focused: [bool; NAV_N],
}

/// Read a [`ButtonExternal`]'s [`ButtonState`] out of the state scene by
/// tag. Defaults to [`ButtonState::Idle`] when the tag is absent.
fn read_button_state(scene: &Scene, tag: &str) -> ButtonState {
    scene
        .find_external_with_tag(tag)
        .and_then(|node| node.handle.introspect())
        .and_then(|intro| intro.query("state"))
        .map_or(ButtonState::Idle, |v| match v {
            pinion_core::external::IntrospectValue::Text(s) => ButtonState::from_name_or_default(&s),
            _ => ButtonState::Idle,
        })
}

/// R694 §5.39 — read a [`ButtonExternal`]'s keyboard-focus posture (the
/// `focused` introspect slot the shell mirrors via `on_focus_change`).
fn read_button_focused(scene: &Scene, tag: &str) -> bool {
    scene
        .find_external_with_tag(tag)
        .and_then(|node| node.handle.introspect())
        .and_then(|intro| intro.query("focused"))
        .is_some_and(|v| matches!(v, pinion_core::external::IntrospectValue::Bool(true)))
}

/// Render one button via the [`pinion_widget_paint::button`] substrate.
fn button_scene(
    tag: &'static str,
    label: &str,
    state: ButtonState,
    focused: bool,
    hover_key: &'static str,
    size: Size,
    theme: &pinion_core::theme::Theme,
) -> Scene {
    let hover = use_hover_progress(matches!(state, ButtonState::Hover), hover_key);
    view_button(
        label,
        state,
        hover,
        focused,
        &ButtonColors::filled_tonal(theme),
        &ButtonStyle::m3_default(tag)
            .with_size(size)
            .with_label_font_size_px(16),
    )
}

/// view-fn (§6.3): pure sync mapping `(button postures) -> Scene`,
/// reading the reactive `drawer_open` + `drawer_active` signals. When
/// open, the drawer overlay is pushed **last** so it paints over (and
/// hit-tests above) the trigger content.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &DrawerViewState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let open = use_drawer_open().get();
    let active = use_drawer_active().get().min(NAV_N - 1);

    let trigger = button_scene(
        TRIGGER_TAG,
        "\u{2630} Open navigation",
        state.trigger,
        state.trigger_focused,
        TRIGGER_HOVER_KEY,
        Size::px(TRIGGER_W, TRIGGER_H),
        &theme,
    );
    let status = Scene::Text(
        TextNode::styled(
            // Concatenated at runtime so the active destination is a
            // single SSOT (`DESTINATIONS`), readable by the RPC demo via
            // `scene/snapshot` of the `drawer_status` text node.
            format!("Current: {}", DESTINATIONS[active]),
            Rect::default(),
            TextStyle::new()
                .with_size_px(16)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_tag(STATUS_TAG),
    );
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
        let items: Vec<Scene> = (0..NAV_N)
            .map(|i| {
                button_scene(
                    NAV_TAGS[i],
                    DESTINATIONS[i],
                    state.items[i],
                    state.items_focused[i],
                    ITEM_HOVER_KEYS[i],
                    Size::px(DrawerStyle::m3_default().panel_width - 24, ITEM_H),
                    &theme,
                )
            })
            .collect();
        children.push(view_drawer(
            SCRIM_TAG,
            PANEL_TAG,
            "Navigation",
            items,
            (WIN_W, WIN_H),
            &theme,
            &DrawerStyle::m3_default(),
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

struct DrawerView;

impl WidgetCore for DrawerView {
    type State = DrawerViewState;
    // Buttons are driven by the pointer router + ARIA Enter/Space; no
    // keybinding-channel events flow through `event_name`.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    /// The click-only scrim plus the N navigation items, as extra
    /// externals. The scrim is registered as a real `External` so a
    /// backdrop click light-dismisses (the R693 carry this round clears).
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let mut extras = vec![ExtraExternal::new(SCRIM_TAG, Box::new(ButtonExternal::new()))];
        for tag in NAV_TAGS {
            extras.push(ExtraExternal::new(tag, Box::new(ButtonExternal::new())));
        }
        extras
    }

    fn tag() -> &'static str {
        TRIGGER_TAG
    }

    fn read_state(scene: &Scene) -> DrawerViewState {
        let mut items = [ButtonState::Idle; NAV_N];
        let mut items_focused = [false; NAV_N];
        for (i, tag) in NAV_TAGS.iter().enumerate() {
            items[i] = read_button_state(scene, tag);
            items_focused[i] = read_button_focused(scene, tag);
        }
        DrawerViewState {
            trigger: read_button_state(scene, TRIGGER_TAG),
            items,
            trigger_focused: read_button_focused(scene, TRIGGER_TAG),
            items_focused,
        }
    }

    fn view(state: DrawerViewState, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-drawer (R702 §5.39 §5.40 modal navigation drawer)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// Only the trigger is a *static* tab stop. The nav items become
    /// focusable solely while the modal scope is up (`nav_members()`),
    /// the dynamic-focusable case the static enumeration cannot express.
    fn focusable_tags() -> Vec<&'static str> {
        vec![TRIGGER_TAG]
    }

    /// R702 §5.39 — Escape dismisses the open drawer (the shell routes
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
            if use_drawer_open().get() {
                close_drawer();
                return true;
            }
            return false;
        }
        // Space/Enter on the focused control activate it through the
        // shared multi-External ARIA helper (descends the Container to
        // the focused button). The chained-or resolves to whichever tag
        // holds focus — the trigger or one of the nav items.
        if apply_aria_activate(scene, focused, key, TRIGGER_TAG) {
            return true;
        }
        NAV_TAGS
            .iter()
            .any(|tag| apply_aria_activate(scene, focused, key, tag))
    }

    /// R702 §5.39 — bridge the buttons' `"<tag>.click"` intents into the
    /// `drawer_open` + `drawer_active` signals and the modal-scope
    /// lifecycle. Side-effect-only (empty `Vec<Command>` return).
    fn update(
        _state: DrawerViewState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        let tag = intent.tag_str();
        if tag == "drawer_trigger.click" {
            open_drawer();
        } else if tag == "drawer_scrim.click" {
            // Light-dismiss: a backdrop click closes without navigating.
            close_drawer();
        } else if let Some(item_tag) = tag.strip_suffix(".click") {
            if let Some(idx) = NAV_TAGS.iter().position(|t| *t == item_tag) {
                select(idx);
            }
        }
        Vec::new()
    }

    fn fmt_state_log(state: &DrawerViewState) -> String {
        // `open` / `active` live in signals (owner-scoped); keep the
        // stderr trace to the button postures `read_state` captured.
        format!("trigger={:?} items={:?}", state.trigger, state.items)
    }
}

impl WidgetA11y for DrawerView {
    /// R702 §5.40 — WAI-ARIA modal-drawer tree. Closed: just the trigger
    /// [`AriaRole::Button`]. Open: the `aria-modal` [`AriaRole::Dialog`]
    /// root owning the N navigation buttons (the trigger is omitted — the
    /// scrim makes the background inert, so AT should not announce it;
    /// the scrim itself is never an a11y node, only a click target).
    ///
    /// Accessible names are left `None` and derived from the paint scene
    /// by `enrich_names_from_scene` (the button labels), so there is no
    /// parallel hardcoded copy to drift (the hello-dialog precedent).
    fn access_node(state: &DrawerViewState, focused: Option<&str>) -> Vec<AccessNode> {
        let open = use_drawer_open().get();
        if !open {
            return vec![AccessNode::new(TRIGGER_TAG, AriaRole::Button)
                .with_state(button_a11y_state(state.trigger, focused == Some(TRIGGER_TAG)))];
        }
        let mut dialog = AccessNode::new(PANEL_TAG, AriaRole::Dialog).with_modal();
        for tag in NAV_TAGS {
            dialog = dialog.with_child(tag);
        }
        let mut nodes = vec![dialog];
        let set_size = u32::try_from(NAV_N).expect("NAV_N fits in u32");
        for (i, tag) in NAV_TAGS.iter().enumerate() {
            let posinset = u32::try_from(i + 1).expect("nav index fits in u32");
            nodes.push(
                AccessNode::new(*tag, AriaRole::Button)
                    .with_state(button_a11y_state(state.items[i], focused == Some(*tag)))
                    .with_position_in_set(posinset)
                    .with_size_of_set(set_size),
            );
        }
        nodes
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

impl WidgetView for DrawerView {
    type Renderer = HelloDrawerRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<DrawerView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> DrawerViewState {
        DrawerViewState {
            trigger: ButtonState::Idle,
            items: [ButtonState::Idle; NAV_N],
            trigger_focused: false,
            items_focused: [false; NAV_N],
        }
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
        let mut children =
            vec![Scene::External(ExternalNode::new(DrawerView::create_external()).with_tag(TRIGGER_TAG))];
        for extra in DrawerView::create_extra_externals() {
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag),
            ));
        }
        Scene::Container(ContainerNode::new(children))
    }

    // ----- reducer + signals -----

    #[test]
    fn r702_trigger_click_opens_and_requests_modal() {
        Owner::new().run(|| {
            let _ = pinion_core::modal_scope_request::drain();
            use_drawer_open().set(false);
            let _ = DrawerView::update(idle(), &intent("drawer_trigger.click"));
            assert!(use_drawer_open().get(), "open signal flipped true");
            assert_eq!(
                pinion_core::modal_scope_request::drain(),
                Some(pinion_core::modal_scope_request::ModalRequest::Open {
                    members: nav_members(),
                }),
                "modal trap requested over the nav items"
            );
        });
    }

    #[test]
    fn r702_scrim_click_light_dismisses_without_navigating() {
        Owner::new().run(|| {
            let _ = pinion_core::modal_scope_request::drain();
            use_drawer_open().set(true);
            use_drawer_active().set(2);
            let _ = DrawerView::update(idle(), &intent("drawer_scrim.click"));
            assert!(!use_drawer_open().get(), "scrim tap closes the drawer");
            assert_eq!(use_drawer_active().get(), 2, "scrim tap does not change active");
            assert_eq!(
                pinion_core::modal_scope_request::drain(),
                Some(pinion_core::modal_scope_request::ModalRequest::Close)
            );
        });
    }

    #[test]
    fn r702_item_click_selects_and_closes() {
        Owner::new().run(|| {
            let _ = pinion_core::modal_scope_request::drain();
            use_drawer_open().set(true);
            use_drawer_active().set(0);
            let _ = DrawerView::update(idle(), &intent("drawer_item_2.click"));
            assert_eq!(use_drawer_active().get(), 2, "selecting item 2 sets active");
            assert!(!use_drawer_open().get(), "selection closes the modal drawer");
            assert_eq!(
                pinion_core::modal_scope_request::drain(),
                Some(pinion_core::modal_scope_request::ModalRequest::Close)
            );
        });
    }

    // ----- apply_key -----

    #[test]
    fn r702_escape_closes_open_drawer() {
        Owner::new().run(|| {
            let _ = pinion_core::modal_scope_request::drain();
            use_drawer_open().set(true);
            use_drawer_active().set(1);
            let mut scene = boot_scene();
            let handled = DrawerView::apply_key(
                &mut scene,
                Some(NAV_TAGS[0]),
                "Escape",
                pinion_core::Modifiers::empty(),
            );
            assert!(handled);
            assert!(!use_drawer_open().get());
            assert_eq!(use_drawer_active().get(), 1, "Escape does not navigate");
            assert_eq!(
                pinion_core::modal_scope_request::drain(),
                Some(pinion_core::modal_scope_request::ModalRequest::Close)
            );
        });
    }

    #[test]
    fn r702_escape_ignored_when_closed() {
        Owner::new().run(|| {
            use_drawer_open().set(false);
            let mut scene = boot_scene();
            assert!(!DrawerView::apply_key(
                &mut scene,
                None,
                "Escape",
                pinion_core::Modifiers::empty(),
            ));
        });
    }

    #[test]
    fn r702_focusable_tags_lists_only_trigger() {
        assert_eq!(DrawerView::focusable_tags(), vec![TRIGGER_TAG]);
    }

    // ----- a11y -----

    #[test]
    fn r702_closed_emits_only_trigger_button() {
        Owner::new().run(|| {
            use_drawer_open().set(false);
            let nodes = DrawerView::access_node(&idle(), None);
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].role, AriaRole::Button);
            assert_eq!(nodes[0].tag, TRIGGER_TAG);
        });
    }

    #[test]
    fn r702_open_emits_modal_dialog_with_nav_items() {
        Owner::new().run(|| {
            use_drawer_open().set(true);
            let nodes = DrawerView::access_node(&idle(), Some(NAV_TAGS[0]));
            assert_eq!(nodes.len(), NAV_N + 1);
            assert_eq!(nodes[0].role, AriaRole::Dialog);
            assert!(nodes[0].modal, "drawer root carries aria-modal");
            assert_eq!(nodes[0].children, NAV_TAGS.to_vec());
            assert!(nodes[1].state.focused, "auto-focused first item marked focused");
            let set_size = u32::try_from(NAV_N).expect("fits");
            for (i, node) in nodes.iter().skip(1).enumerate() {
                assert_eq!(node.role, AriaRole::Button);
                assert_eq!(node.position_in_set, Some(u32::try_from(i + 1).expect("fits")));
                assert_eq!(node.size_of_set, Some(set_size));
            }
        });
    }

    #[test]
    fn r702_names_enriched_from_paint_not_hardcoded() {
        Owner::new().run(|| {
            use_drawer_open().set(true);
            let scene = view(&idle(), &Frame::new());
            let mut nodes = DrawerView::access_node(&idle(), Some(NAV_TAGS[0]));
            assert!(
                nodes.iter().all(|n| n.name.is_none()),
                "access_node leaves names to enrich"
            );
            pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
            for (i, tag) in NAV_TAGS.iter().enumerate() {
                let name = nodes
                    .iter()
                    .find(|n| &n.tag == tag)
                    .and_then(|n| n.name.as_deref());
                assert_eq!(name, Some(DESTINATIONS[i]), "item name from paint label");
            }
        });
    }

    // ----- view / paint -----

    #[test]
    fn r702_closed_view_omits_scrim_and_panel() {
        Owner::new().run(|| {
            use_drawer_open().set(false);
            let scene = view(&idle(), &Frame::new());
            assert!(!scene.contains_tag(SCRIM_TAG), "closed: no scrim painted");
            assert!(!scene.contains_tag(PANEL_TAG), "closed: no panel painted");
            assert!(scene.contains_tag(TRIGGER_TAG), "trigger always painted");
        });
    }

    #[test]
    fn r702_open_view_paints_scrim_panel_and_items() {
        Owner::new().run(|| {
            use_drawer_open().set(true);
            let scene = view(&idle(), &Frame::new());
            assert!(scene.contains_tag(SCRIM_TAG), "open: scrim painted");
            assert!(scene.contains_tag(PANEL_TAG), "open: panel painted");
            for tag in NAV_TAGS {
                assert!(scene.contains_tag(tag), "nav item {tag} painted");
            }
        });
    }

    /// Walk the paint scene for the `drawer_status` text node content.
    fn status_text(scene: &Scene) -> Option<String> {
        match scene {
            Scene::Text(t) if t.tag.as_deref() == Some(STATUS_TAG) => Some(t.content.clone()),
            Scene::Container(c) => c.children.iter().find_map(status_text),
            _ => None,
        }
    }

    #[test]
    fn r702_status_text_reflects_active_destination() {
        Owner::new().run(|| {
            use_drawer_open().set(false);
            use_drawer_active().set(2);
            let scene = view(&idle(), &Frame::new());
            assert_eq!(
                status_text(&scene).as_deref(),
                Some("Current: Settings"),
                "status line shows the active destination",
            );
        });
    }

    #[test]
    fn r702_view_contains_trigger_paint_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<DrawerView>(
            idle(),
            &Frame::default(),
        );
    }
}
